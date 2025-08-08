use libp2p::PeerId;
use ractor::async_trait;
use tracing::Span;

use crate::{
    handle::CtrlHandle,
    output_port::{OutputPort, OutputPortSubscriber},
};

use crate::msg::NetworkMsg as GossipNetworkMsg;
use crate::types::MempoolTransactionBatch;
use crate::Event;
use {
    libp2p_identity::Keypair,
    ractor::{Actor, ActorProcessingErr, ActorRef},
    std::{collections::BTreeSet, sync::Arc},
    tokio::task::JoinHandle,
    tracing::error,
};

pub type MempoolNetworkMsg = Msg;
pub type MempoolNetworkActorRef = ActorRef<Msg>;
pub type MempoolNetworkConfig = crate::Config;

pub struct Args {
    pub keypair: Keypair,
    pub config: MempoolNetworkConfig,
}

pub enum State {
    Stopped,
    Running {
        peers: BTreeSet<PeerId>,
        output_port: OutputPort<Arc<Event>>,
        ctrl_handle: CtrlHandle,
        recv_task: JoinHandle<()>,
    },
}

pub enum Msg {
    /// Subscribe to gossip events
    Subscribe(OutputPortSubscriber<Arc<Event>>),

    /// Broadcast a message to all peers
    Broadcast(MempoolTransactionBatch),

    // Internal message
    #[doc(hidden)]
    NewEvent(Event),
}

pub struct MempoolNetwork {
    span: Span,
}

impl MempoolNetwork {
    pub async fn spawn(
        keypair: Keypair,
        config: MempoolNetworkConfig,
        span: Span,
    ) -> Result<ActorRef<Msg>, ractor::SpawnErr> {
        let args = Args { keypair, config };

        let node = Self { span: span.clone() };

        let (actor_ref, _) = Actor::spawn(None, node, args).await?;
        Ok(actor_ref)
    }
}

#[async_trait]
impl Actor for MempoolNetwork {
    type Arguments = Args;
    type Msg = Msg;
    type State = State;

    async fn pre_start(
        &self,
        myself: ActorRef<Msg>,
        args: Args,
    ) -> Result<State, ActorProcessingErr> {
        let handle = crate::spawn(args.keypair, args.config).await?;
        let (mut recv_handle, ctrl_handle) = handle.split();

        let recv_task = tokio::spawn(async move {
            while let Some(event) = recv_handle.recv().await {
                if let Err(e) = myself.cast(Msg::NewEvent(event)) {
                    error!("Actor has died, stopping gossip mempool: {e:?}");
                    break;
                }
            }
        });

        Ok(State::Running {
            peers: BTreeSet::new(),
            output_port: OutputPort::default(),
            ctrl_handle,
            recv_task,
        })
    }

    async fn post_start(
        &self,
        _myself: ActorRef<Msg>,
        _state: &mut State,
    ) -> Result<(), ActorProcessingErr> {
        Ok(())
    }

    #[tracing::instrument(name = "gossip.mempool", parent = &self.span, skip_all)]
    async fn handle(
        &self,
        _myself: ActorRef<Msg>,
        msg: Msg,
        state: &mut State,
    ) -> Result<(), ActorProcessingErr> {
        let State::Running {
            peers,
            output_port,
            ctrl_handle,
            ..
        } = state
        else {
            return Ok(());
        };

        match msg {
            Msg::Subscribe(subscriber) => subscriber.subscribe_to_port(output_port),

            Msg::Broadcast(batch) => {
                match GossipNetworkMsg::TransactionBatch(batch).to_network_bytes() {
                    Ok(bytes) => {
                        ctrl_handle
                            .broadcast(crate::Channel::Mempool, bytes)
                            .await?;
                    }
                    Err(e) => {
                        error!("Failed to serialize transaction batch: {e}");
                    }
                }
            }

            Msg::NewEvent(event) => {
                match event {
                    Event::PeerConnected(peer_id) => {
                        peers.insert(peer_id);
                    }
                    Event::PeerDisconnected(peer_id) => {
                        peers.remove(&peer_id);
                    }
                    _ => {}
                }

                let event = Arc::new(event);
                output_port.send(event);
            }
        }

        Ok(())
    }

    async fn post_stop(
        &self,
        _myself: ActorRef<Msg>,
        state: &mut State,
    ) -> Result<(), ActorProcessingErr> {
        let state = std::mem::replace(state, State::Stopped);

        if let State::Running {
            ctrl_handle,
            recv_task,
            ..
        } = state
        {
            ctrl_handle.wait_shutdown().await?;
            recv_task.await?;
        }

        Ok(())
    }
}

pub async fn spawn_mempool_network_actor(
    cfg: &MempoolNetworkConfig,
    private_key: &Keypair,
    span: Span,
) -> MempoolNetworkActorRef {
    MempoolNetwork::spawn(private_key.clone(), cfg.clone(), span)
        .await
        .unwrap()
}
