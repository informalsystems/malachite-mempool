use {
    crate::{types::tx::TxHash, ActorResult, RawTx},
    ractor::{async_trait, Actor, ActorRef, RpcReplyPort},
    serde::Deserialize,
    serde::Serialize,
    std::{
        cmp::min,
        collections::{HashMap, HashSet, VecDeque},
        sync::Arc,
    },
    thiserror::Error,
    tracing::{debug, error, info, warn, Span},
};

// Placeholder types for external dependencies

// Events emitted by the gossip network
pub type GossipNetworkEvent = libp2p_network::Event;

// Messages included in GossipNetworkEvent::Message event
pub type GossipNetworkMsg = libp2p_network::NetworkMsg;

// Messages that can be sent to the gossip network
pub type MempoolNetworkMsg = libp2p_network::Msg;

// Transaction batch type, can be sent or received by the gossip network
pub type MempoolTransactionBatch = libp2p_network::types::MempoolTransactionBatch;

// Reference to the MempoolApp actor
pub type MempoolAppActorRef = ActorRef<MempoolAppMsg>;

// Actor reference to the gossip network
pub type MempoolNetworkActorRef = ActorRef<MempoolNetworkMsg>;

// MempoolConfig, used to configure the mempool
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MempoolConfig {
    pub max_txs_bytes: u64,
    pub max_txs_per_block: usize,
}

impl Default for MempoolConfig {
    fn default() -> Self {
        Self {
            max_txs_bytes: 4 * 1024 * 1024, // 4MB
            max_txs_per_block: 100,         // 100KB
        }
    }
}

pub type MempoolMsg = Msg;
pub type MempoolActorRef = ActorRef<Msg>;

#[derive(Default, Clone)]
pub struct State {
    pub txs: VecDeque<RawTx>,
    pub tx_hashes: HashMap<TxHash, usize>,
}

impl State {
    pub fn exists(&self, tx: &TxHash) -> bool {
        self.tx_hashes.contains_key(tx)
    }
}

pub enum MempoolAppMsg {
    CheckTx {
        tx: RawTx,
        reply: RpcReplyPort<Result<Box<dyn crate::CheckTxOutcome>, MempoolError>>,
    },
}

pub enum Msg {
    NetworkEvent(Arc<GossipNetworkEvent>),
    Add {
        tx: RawTx,
        reply: RpcReplyPort<Result<Box<dyn crate::CheckTxOutcome>, MempoolError>>,
    },
    // TODO: figure out how to properly handle messages outside consensus
    CheckTxResult {
        tx: RawTx,
        result: Result<Box<dyn crate::CheckTxOutcome>, Box<dyn std::error::Error + Send + Sync>>,
        reply: Option<RpcReplyPort<Result<Box<dyn crate::CheckTxOutcome>, MempoolError>>>,
    },
    Take {
        reply: RpcReplyPort<Vec<RawTx>>,
    },
    Remove(Vec<TxHash>),
}

impl From<Arc<GossipNetworkEvent>> for Msg {
    fn from(event: Arc<GossipNetworkEvent>) -> Self {
        Self::NetworkEvent(event)
    }
}

pub struct Mempool {
    mempool_network: MempoolNetworkActorRef,
    app: Option<MempoolAppActorRef>,
    span: Span,
    config: MempoolConfig,
}

impl Mempool {
    pub async fn spawn(
        mempool_network: MempoolNetworkActorRef,
        app: Option<MempoolAppActorRef>,
        span: Span,
        config: MempoolConfig,
    ) -> Result<MempoolActorRef, ractor::SpawnErr> {
        let node = Self {
            mempool_network,
            app,
            span,
            config,
        };

        let (actor_ref, _) = Actor::spawn(None, node, ()).await?;
        Ok(actor_ref)
    }

    async fn handle_msg(
        &self,
        myself: &MempoolActorRef,
        msg: Msg,
        state: &mut State,
    ) -> ActorResult<()> {
        match msg {
            Msg::NetworkEvent(event) => self.handle_network_event(myself, &event, state).await?,
            Msg::Add { tx, reply } => self.add_tx(myself, tx, Some(reply), state).await?,
            Msg::CheckTxResult { tx, result, reply } => {
                self.handle_check_tx_result(tx, result, reply, state)?
            }
            Msg::Take { reply } => self.take(state, reply)?,
            Msg::Remove(tx_hashes) => self.remove(tx_hashes, state)?,
        }

        Ok(())
    }

    async fn handle_network_event(
        &self,
        myself: &MempoolActorRef,
        event: &GossipNetworkEvent,
        state: &mut State,
    ) -> ActorResult<()> {
        // Handle network events from the gossip network
        debug!("Received network event: {:?}", event);

        match event {
            GossipNetworkEvent::Message(.., GossipNetworkMsg::TransactionBatch(batch)) => {
                let tx = RawTx(prost::bytes::Bytes::from(
                    batch.transaction_batch.value.clone(),
                ));
                self.add_tx(myself, tx, None, state).await?
            }
            e => info!("Network event: {:?}", e),
        }

        Ok(())
    }

    #[tracing::instrument("add_tx", skip_all)]
    async fn add_tx(
        &self,
        myself: &MempoolActorRef,
        tx: RawTx,
        reply: Option<RpcReplyPort<Result<Box<dyn crate::CheckTxOutcome>, MempoolError>>>,
        _state: &mut State,
    ) -> ActorResult<()> {
        match &self.app {
            Some(app) => {
                let tx_clone = tx.clone();
                app.call_and_forward(
                    |reply| MempoolAppMsg::CheckTx { tx, reply },
                    myself,
                    move |outcome| Msg::CheckTxResult {
                        tx: tx_clone.clone(),
                        result: outcome.map_err(|e| e.into()),
                        reply,
                    },
                    None,
                )?;
            }
            None => {
                // TODO: this will be removed and self.app will be required
                // No app configured - cannot process transactions without hash function
                warn!("No app configured, cannot process transaction without hash function");
                if let Some(reply) = reply {
                    reply.send(Err(MempoolError::App(
                        "No app configured to provide transaction hash".to_string(),
                    )))?;
                }
            }
        }

        Ok(())
    }

    #[tracing::instrument("handle_check_tx_result", skip_all)]
    fn handle_check_tx_result(
        &self,
        tx: RawTx,
        result: Result<Box<dyn crate::CheckTxOutcome>, Box<dyn std::error::Error + Send + Sync>>,
        reply: Option<RpcReplyPort<Result<Box<dyn crate::CheckTxOutcome>, MempoolError>>>,
        state: &mut State,
    ) -> ActorResult<()> {
        match result {
            Ok(check_tx_outcome) => {
                let tx_hash = check_tx_outcome.hash();

                if check_tx_outcome.is_valid() {
                    // Check for duplicates using app-provided hash
                    if state.exists(&tx_hash) {
                        warn!("tx already exists in mempool, not adding duplicate");
                    } else {
                        debug!("check_tx successful, tx is valid, adding tx to mempool");
                        state.tx_hashes.insert(tx_hash.clone(), state.txs.len());
                        state.txs.push_back(tx.clone());
                        self.gossip_tx(tx)?;
                    }
                } else {
                    warn!(reason = ?check_tx_outcome, "check_tx successful, tx is invalid, not adding to mempool");
                }
                if let Some(reply) = reply {
                    debug!("MEMPOOL DEBUG: handle_check_tx_result() - sending success reply");
                    match reply.send(Ok(check_tx_outcome)) {
                        Ok(_) => {
                            debug!("MEMPOOL DEBUG: handle_check_tx_result() - success reply sent")
                        }
                        Err(e) => {
                            error!("🔍 MEMPOOL DEBUG: handle_check_tx_result() - failed to send success reply: {:?}", e);
                            return Err(e.into());
                        }
                    }
                }
            }

            Err(app_error) => {
                error!(reason = ?app_error, "check_tx failed!");
                if let Some(reply) = reply {
                    debug!("MEMPOOL DEBUG: handle_check_tx_result() - sending error reply");
                    match reply.send(Err(MempoolError::App(app_error.to_string()))) {
                        Ok(_) => {
                            debug!("MEMPOOL DEBUG: handle_check_tx_result() - error reply sent")
                        }
                        Err(e) => {
                            error!("MEMPOOL DEBUG: handle_check_tx_result() - failed to send error reply: {:?}", e);
                            return Err(e.into());
                        }
                    }
                }
            }
        }

        Ok(())
    }

    fn take(&self, state: &mut State, reply: RpcReplyPort<Vec<RawTx>>) -> ActorResult<()> {
        debug!(
            "MEMPOOL DEBUG: take() - current mempool size: {}",
            state.txs.len()
        );
        let mut txs = Vec::with_capacity(min(self.config.max_txs_per_block, state.txs.len()));

        let mut max_tx_bytes = self.config.max_txs_bytes as usize;

        for tx in state.txs.iter() {
            max_tx_bytes = max_tx_bytes.saturating_sub(tx.len());

            if max_tx_bytes == 0 {
                break;
            }

            txs.push(tx.clone());
        }

        reply.send(txs)?;

        Ok(())
    }

    #[tracing::instrument("remove", skip_all)]
    fn remove(&self, tx_hashes: Vec<TxHash>, state: &mut State) -> ActorResult<()> {
        let mut ignore = HashSet::new();
        for tx_hash in tx_hashes {
            if let Some(index) = state.tx_hashes.remove(&tx_hash) {
                ignore.insert(index);
            }
        }

        debug!("removed {} txs from mempool", ignore.len());

        let mut new_txs = VecDeque::with_capacity(state.txs.len() - ignore.len());
        let mut new_hashes = HashMap::with_capacity(state.txs.len() - ignore.len());

        let mut counter = 0;
        for (hash, index) in state.tx_hashes.iter() {
            if !ignore.contains(index) {
                new_hashes.insert(hash.clone(), counter);
                new_txs.push_back(state.txs[*index].clone());
                counter += 1;
            }
        }

        state.txs = new_txs;
        state.tx_hashes = new_hashes;

        Ok(())
    }

    #[tracing::instrument("gossip_tx", skip_all)]
    fn gossip_tx(&self, tx: RawTx) -> ActorResult<()> {
        // Create a transaction batch for gossiping
        // In a real implementation, you might want to batch multiple transactions
        let batch = MempoolTransactionBatch::new(prost_types::Any {
            type_url: "mempool.transaction".to_string(),
            value: tx.to_vec(),
        });

        // Send the batch to the network actor for broadcasting
        // Note: This is a cast since we don't need a reply for gossip
        if let Err(e) = self
            .mempool_network
            .cast(MempoolNetworkMsg::Broadcast(batch))
        {
            error!("Failed to gossip transaction: {:?}", e);
        } else {
            debug!("Successfully gossiped transaction");
        }

        Ok(())
    }
}

#[async_trait]
impl Actor for Mempool {
    type Arguments = ();
    type Msg = Msg;
    type State = State;

    async fn pre_start(
        &self,
        myself: ActorRef<Self::Msg>,
        _args: Self::Arguments,
    ) -> ActorResult<Self::State> {
        self.mempool_network.link(myself.get_cell());

        self.mempool_network
            .cast(MempoolNetworkMsg::Subscribe(Box::new(myself.clone())))?;

        Ok(State::default())
    }

    #[tracing::instrument("mempool", parent = &self.span, skip_all)]
    async fn handle(
        &self,
        myself: MempoolActorRef,
        msg: MempoolMsg,
        state: &mut State,
    ) -> ActorResult<()> {
        if let Err(e) = self.handle_msg(&myself, msg, state).await {
            error!("Error handling message: {:?}", e);
        }

        Ok(())
    }
}

#[derive(Clone, Debug, Error)]
pub enum MempoolError {
    #[error("Application error: {0}")]
    App(String),

    #[error("tx already exists in mempool: {0}")]
    TxAlreadyExists(TxHash),
}

pub async fn spawn_mempool_actor(
    mempool_network: MempoolNetworkActorRef,
    app: Option<MempoolAppActorRef>,
    span: Span,
    config: &MempoolConfig,
) -> MempoolActorRef {
    Mempool::spawn(mempool_network, app, span, config.clone())
        .await
        .unwrap()
}
