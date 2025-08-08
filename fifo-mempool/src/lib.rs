pub mod mempool;
pub mod types;

pub use mempool::*;
pub use types::tx::{RawTx, TxHash};

pub type ActorResult<T> = Result<T, ractor::ActorProcessingErr>;

// Generic error type - applications should define their own error types
pub type AppResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub trait CheckTxOutcome: Send + Sync + std::fmt::Debug {
    fn is_valid(&self) -> bool;
    fn hash(&self) -> TxHash;
}

pub trait MempoolApp: Send + Sync + 'static {
    fn check_tx(&self, tx: &RawTx) -> AppResult<Box<dyn CheckTxOutcome>>;
}
