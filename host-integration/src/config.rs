use std::time::Duration;

use bytesize::ByteSize;
use malachitebft_config::P2pConfig;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HostMempoolConfig {
    pub p2p: P2pConfig,
    pub idle_connection_timeout: Duration,
    pub max_txs_bytes: ByteSize,
    pub avg_tx_bytes: ByteSize,
}

impl Default for HostMempoolConfig {
    fn default() -> Self {
        Self {
            p2p: P2pConfig::default(),
            idle_connection_timeout: Duration::from_secs(30),
            max_txs_bytes: ByteSize::mb(4),
            avg_tx_bytes: ByteSize::kb(100),
        }
    }
}
