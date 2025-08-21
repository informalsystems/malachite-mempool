use fifo_mempool::{mempool::{spawn_mempool_actor, MempoolConfig, MempoolMsg}, RawTx, ReapCursor};
use libp2p_identity::Keypair;
use libp2p_network::{network::spawn_mempool_network_actor, MempoolNetworkConfig};
use ractor::ActorRef;
use std::sync::Arc;

use crate::{
    app::{spawn_app_actor, AppMsg, TestMempoolApp, TestTx},
    config::HostMempoolConfig,
    rpc::{Rpc, RpcMsg},
};

pub struct TestNode {
    pub id: usize,
    pub app_actor: ActorRef<AppMsg>,
    pub rpc: Rpc,
    pub rpc_actor: ActorRef<RpcMsg>,
    pub mempool_actor: ActorRef<MempoolMsg>,
}

impl TestNode {
    pub async fn new(id: usize, config: HostMempoolConfig) -> Self {
        let app = Arc::new(TestMempoolApp);

        let network_config = MempoolNetworkConfig {
            listen_addr: config.p2p.listen_addr,
            persistent_peers: config.p2p.persistent_peers,
            idle_connection_timeout: config.idle_connection_timeout,
        };

        // Create network actor
        let keypair = Keypair::generate_ed25519();
        let network_actor =
            spawn_mempool_network_actor(&network_config, &keypair, tracing::Span::current()).await;

        // Create mempool actor
        let mempool_config = MempoolConfig {
            max_txs_bytes: config.max_txs_bytes.as_u64(),
            max_txs_per_block: (config.max_txs_bytes.as_u64() / config.avg_tx_bytes.as_u64())
                as usize,
            max_pool_size: config.max_pool_size,
        };

        let mempool_actor = spawn_mempool_actor(
            network_actor.clone(),
            mempool_config,
            tracing::Span::current(),
        )
        .await;

        // Create app actor
        let app_actor = spawn_app_actor(app, mempool_actor.clone()).await;

        let rpc = Rpc::new(mempool_actor.clone());
        let rpc_actor = Rpc::spawn(rpc.clone()).await.unwrap();

        Self {
            id,
            app_actor,
            mempool_actor,
            rpc_actor,
            rpc,
        }
    }

    pub async fn remove_tx(&mut self, tx: &TestTx) {
        // Send remove message to the mempool actor using cast (non-RPC)
        let result = self.app_actor.cast(AppMsg::Remove(vec![tx.hash()]));
        if result.is_ok() {
            println!(
                "Node {} removed transaction {} with hash {:?}",
                self.id,
                tx.0,
                tx.hash()
            );
        } else {
            println!(
                "Node {} failed to remove transaction {}: {:?}",
                self.id, tx.0, result
            );
        }
    }

    pub async fn reap_transactions(&self, cursor: ReapCursor) -> Vec<RawTx> {
        // Get transactions from the mempool actor using the Reap message
        let result = self
            .app_actor
            .call(| reply| AppMsg::Reap { cursor, reply }, None)
            .await;
        match result {
            Ok(txs) => {
                let txs = txs.unwrap_or(vec![]);
                println!("Node {} got {} transactions", self.id, txs.len());
                txs
            }
            Err(e) => {
                println!("Node {} failed to get transactions: {:?}", self.id, e);
                vec![]
            }
        }
    }

    pub async fn get_transactions(&self) -> Vec<RawTx> {
        self.reap_transactions(ReapCursor::Fresh).await
    }
}
