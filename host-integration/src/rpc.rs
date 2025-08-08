use eyre::eyre;

use fifo_mempool::{ActorResult, Msg as MempoolMsg};
use ractor::{async_trait, Actor, ActorProcessingErr, ActorRef};

use crate::app::{TestCheckTxOutcome, TestTx};

pub enum RpcMsg {
    AddTxReply(TestTx, TestCheckTxOutcome),
    GetState(ractor::RpcReplyPort<Option<TestCheckTxOutcome>>),
}
#[derive(Clone)]
pub struct Rpc {
    pub mempool_actor: ActorRef<MempoolMsg>,
}

impl Rpc {
    pub fn new(mempool_actor: ActorRef<MempoolMsg>) -> Self {
        Self { mempool_actor }
    }

    pub async fn spawn(rpc: Rpc) -> Result<ActorRef<RpcMsg>, ractor::SpawnErr> {
        let (actor_ref, _) = Actor::spawn(None, rpc, ()).await?;
        Ok(actor_ref)
    }
    pub async fn add_tx(
        &self,
        actor_ref: &ActorRef<RpcMsg>,
        tx: TestTx,
    ) -> Result<(), ActorProcessingErr> {
        let raw_tx = tx.serialize();
        let tx_hash = tx.hash(); // Use TestTx::hash() instead of raw_tx.hash()
                                 // Send add message to the mempool actor using RPC call
        self.mempool_actor
            .call_and_forward(
                |reply| MempoolMsg::Add { tx: raw_tx, reply },
                actor_ref,
                move |outcome| match outcome {
                    Ok(check_tx_outcome) => {
                        if check_tx_outcome.is_valid() {
                            RpcMsg::AddTxReply(tx, TestCheckTxOutcome::Success(tx_hash))
                        } else {
                            RpcMsg::AddTxReply(
                                tx,
                                TestCheckTxOutcome::Error(
                                    tx_hash,
                                    "Transaction validation failed".to_string(),
                                ),
                            )
                        }
                    }
                    Err(mempool_error) => RpcMsg::AddTxReply(
                        tx,
                        TestCheckTxOutcome::Error(tx_hash, mempool_error.to_string()),
                    ),
                },
                None,
            )
            .map_err(|e| eyre!("Error when sending decided value to host: {e:?}"))?
            .await
            .map_err(|e| eyre!("Error waiting for result: {e:?}"))?;
        Ok(())
    }

    pub async fn get_state(
        &self,
        actor_ref: &ActorRef<RpcMsg>,
    ) -> Result<Option<TestCheckTxOutcome>, ActorProcessingErr> {
        let result = actor_ref.call(RpcMsg::GetState, None).await?;
        Ok(result.unwrap())
    }
}

#[async_trait]
impl Actor for Rpc {
    type Arguments = ();
    type Msg = RpcMsg;
    type State = Option<TestCheckTxOutcome>;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _args: Self::Arguments,
    ) -> ActorResult<Self::State> {
        Ok(None)
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        msg: Self::Msg,
        state: &mut Self::State,
    ) -> ActorResult<()> {
        match msg {
            RpcMsg::AddTxReply(_tx, outcome) => {
                *state = Some(outcome);
            }
            RpcMsg::GetState(reply) => {
                reply
                    .send(state.clone())
                    .map_err(|e| eyre!("Failed to send state: {e:?}"))?;
            }
        }
        Ok(())
    }
}
