use crate::error::AppError;
use fifo_mempool::{
    ActorResult, AppResult, CheckTxOutcome, MempoolApp, MempoolEvent, MempoolMsg, RawTx, TxHash, ReapCursor
};

use libp2p_network::output_port::OutputPortSubscriberTrait;
use prost::bytes::Bytes;
use ractor::{async_trait, Actor, ActorRef, RpcReplyPort};
use std::sync::Arc;

pub type AppMsg = Msg;

pub enum Msg {
    MempoolEvent(MempoolEvent),
    Reap { cursor: ReapCursor, reply: RpcReplyPort<Vec<RawTx>> },
    Remove(Vec<TestTxHash>),
}

impl From<Arc<MempoolEvent>> for Msg {
    fn from(event: Arc<MempoolEvent>) -> Self {
        match Arc::try_unwrap(event) {
            Ok(event) => Msg::MempoolEvent(event),
            Err(event_arc) => Msg::MempoolEvent((*event_arc).clone())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TestTx(pub u64); // Unique transaction by id

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TestTxHash(pub u64);

impl From<TestTxHash> for TxHash {
    fn from(test_hash: TestTxHash) -> Self {
        TxHash(Bytes::from(test_hash.0.to_le_bytes().to_vec()))
    }
}

impl TestTx {
    pub fn serialize(&self) -> RawTx {
        RawTx(Bytes::from(self.0.to_le_bytes().to_vec()))
    }
    pub fn deserialize(bytes: &[u8]) -> Result<TestTx, AppError> {
        let bytes_array: [u8; 8] = bytes.try_into().map_err(|_| {
            AppError::deserialization_failed("Expected 8 bytes for TestTx deserialization")
        })?;
        Ok(TestTx(u64::from_le_bytes(bytes_array)))
    }

    pub fn hash(&self) -> TestTxHash {
        TestTxHash(self.0)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum TestCheckTxOutcome {
    Success(TestTxHash),
    Error(TestTxHash, String),
}

impl CheckTxOutcome for TestCheckTxOutcome {
    fn is_valid(&self) -> bool {
        matches!(self, TestCheckTxOutcome::Success(_))
    }
    fn hash(&self) -> TxHash {
        match self {
            TestCheckTxOutcome::Success(hash) => hash.clone().into(),
            TestCheckTxOutcome::Error(hash, _) => hash.clone().into(),
        }
    }
}

pub struct TestMempoolAppActor {
    app: Arc<TestMempoolApp>,
    mempool_actor: ActorRef<MempoolMsg>,
}

impl TestMempoolAppActor {
    pub async fn spawn(
        app: Arc<TestMempoolApp>,
        mempool_actor: ActorRef<MempoolMsg>,
    ) -> ActorRef<Msg> {
        let actor = Self { app, mempool_actor };
        let (actor_ref, _) = Actor::spawn(None, actor, ()).await.unwrap();
        actor_ref
    }
}

#[async_trait]
impl Actor for TestMempoolAppActor {
    type Arguments = ();
    type Msg = Msg;
    type State = ();

    async fn pre_start(
        &self,
        myself: ActorRef<Self::Msg>,
        _args: Self::Arguments,
    ) -> ActorResult<Self::State> {
        let subscriber: Box<dyn OutputPortSubscriberTrait<Arc<MempoolEvent>>> =
            Box::new(myself.clone());
        self.mempool_actor
            .cast(MempoolMsg::Subscribe(Box::new(subscriber)))?;

        Ok(())
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        msg: Self::Msg,
        _state: &mut Self::State,
    ) -> ActorResult<()> {
        match msg {
            Msg::MempoolEvent(MempoolEvent::CheckTx { tx, reply }) => {
                let result = self.app.check_tx(&tx);
                let response = MempoolMsg::CheckTxResult { tx, result, reply };
                self.mempool_actor.cast(response)?;
            }
            Msg::Remove(hashes) => {
                let hashes: Vec<TxHash> = hashes.iter().map(|h| h.clone().into()).collect();
                self.mempool_actor.cast(MempoolMsg::Remove(hashes))?;
            }
            Msg::Reap { cursor, reply } => {
                self.mempool_actor.cast(MempoolMsg::Reap { cursor,  reply })?;
            }
        }
        Ok(())
    }
}

pub async fn spawn_app_actor(
    app: Arc<TestMempoolApp>,
    mempool_actor: ActorRef<MempoolMsg>,
) -> ActorRef<Msg> {
    TestMempoolAppActor::spawn(app, mempool_actor).await
}

pub struct TestMempoolApp;
impl MempoolApp for TestMempoolApp {
    fn check_tx(&self, tx: &RawTx) -> AppResult<Box<dyn CheckTxOutcome>> {
        // Deserialize the transaction
        match TestTx::deserialize(&tx.0) {
            Ok(test_tx) => {
                let tx_hash = test_tx.hash();
                // Return error for transaction with value 9999
                if test_tx.0 == 9999 {
                    Ok(Box::new(TestCheckTxOutcome::Error(
                        tx_hash,
                        "Transaction 9999 is not allowed".to_string(),
                    )))
                } else {
                    Ok(Box::new(TestCheckTxOutcome::Success(tx_hash)))
                }
            }
            Err(e) => Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                e.to_string(),
            ))),
        }
    }
}
