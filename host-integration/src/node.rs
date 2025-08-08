use fifo_mempool::{
    mempool::{spawn_mempool_actor, MempoolConfig, Msg},
    RawTx,
};
use libp2p_identity::Keypair;
use libp2p_network::{network::spawn_mempool_network_actor, MempoolNetworkConfig};
use ractor::ActorRef;
use std::sync::Arc;

use crate::{
    app::{spawn_app_actor, TestMempoolApp, TestTx},
    config::HostMempoolConfig,
    rpc::{Rpc, RpcMsg},
};

pub struct TestNode {
    pub id: usize,
    pub rpc: Rpc,
    pub rpc_actor: ActorRef<RpcMsg>,
    pub mempool_actor: ActorRef<Msg>,
}

impl TestNode {
    pub async fn new(id: usize, config: HostMempoolConfig) -> Self {
        let app = Arc::new(TestMempoolApp);

        // Create app actor
        let app_actor = spawn_app_actor(app).await;

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
        };

        let mempool_actor = spawn_mempool_actor(
            network_actor.clone(),
            Some(app_actor),
            tracing::Span::current(),
            &mempool_config,
        )
        .await;

        let rpc = Rpc::new(mempool_actor.clone());
        let rpc_actor = Rpc::spawn(rpc.clone()).await.unwrap();

        Self {
            id,
            mempool_actor,
            rpc_actor,
            rpc,
        }
    }

    pub async fn remove_tx(&mut self, tx: &TestTx) {
        // Send remove message to the mempool actor using cast (non-RPC)
        let tx_hash = tx.hash();
        let result = self.mempool_actor.cast(Msg::Remove(vec![tx_hash.clone()]));
        if result.is_ok() {
            println!(
                "Node {} removed transaction {} with hash {}",
                self.id, tx.0, tx_hash
            );
        } else {
            println!(
                "Node {} failed to remove transaction {}: {:?}",
                self.id, tx.0, result
            );
        }
    }

    pub async fn get_transactions(&self) -> Vec<RawTx> {
        // Get transactions from the mempool actor using Take message
        let result = self
            .mempool_actor
            .call(|reply| Msg::Take { reply }, None)
            .await;
        match result {
            Ok(txs) => txs.unwrap_or(vec![]),
            Err(e) => {
                println!("Node {} failed to get transactions: {:?}", self.id, e);
                vec![]
            }
        }
    }
}
