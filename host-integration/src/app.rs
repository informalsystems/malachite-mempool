use fifo_mempool::{
    ActorResult, AppResult, CheckTxOutcome, MempoolApp, MempoolAppMsg, RawTx, TxHash,
};
use prost::bytes::Bytes;
use ractor::{async_trait, Actor, ActorRef};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TestTx(pub u64); // Unique transaction by id

impl TestTx {
    pub fn serialize(&self) -> RawTx {
        RawTx(Bytes::from(self.0.to_le_bytes().to_vec()))
    }
    pub fn deserialize(bytes: &[u8]) -> Result<TestTx, anyhow::Error> {
        Ok(TestTx(u64::from_le_bytes(bytes.try_into().unwrap())))
    }
    pub fn hash(&self) -> TxHash {
        TxHash(Bytes::from(self.0.to_le_bytes().to_vec()))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum TestCheckTxOutcome {
    Success(TxHash),
    Error(TxHash, String),
}

impl CheckTxOutcome for TestCheckTxOutcome {
    fn is_valid(&self) -> bool {
        matches!(self, TestCheckTxOutcome::Success(_))
    }
    fn hash(&self) -> TxHash {
        match self {
            TestCheckTxOutcome::Success(hash) => hash.clone(),
            TestCheckTxOutcome::Error(hash, _) => hash.clone(),
        }
    }
}

pub struct TestMempoolAppActor {
    app: Arc<TestMempoolApp>,
}

impl TestMempoolAppActor {
    pub async fn spawn(app: Arc<TestMempoolApp>) -> ActorRef<MempoolAppMsg> {
        let actor = Self { app };
        let (actor_ref, _) = Actor::spawn(None, actor, ()).await.unwrap();
        actor_ref
    }
}

#[async_trait]
impl Actor for TestMempoolAppActor {
    type Arguments = ();
    type Msg = MempoolAppMsg;
    type State = ();

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _args: Self::Arguments,
    ) -> ActorResult<Self::State> {
        Ok(())
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        msg: Self::Msg,
        _state: &mut Self::State,
    ) -> ActorResult<()> {
        match msg {
            MempoolAppMsg::CheckTx { tx, reply } => {
                let result = self.app.check_tx(&tx);
                reply.send(result.map_err(|e| fifo_mempool::MempoolError::App(e.to_string())))?;
            }
        }
        Ok(())
    }
}

pub async fn spawn_app_actor(app: Arc<TestMempoolApp>) -> ActorRef<MempoolAppMsg> {
    TestMempoolAppActor::spawn(app).await
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
