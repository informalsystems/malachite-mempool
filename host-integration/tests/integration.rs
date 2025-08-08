use crate::utils::create_nodes;
use host_integration::app::{TestCheckTxOutcome, TestTx};
use std::time::Duration;
use tokio::time::sleep;

pub mod utils;

#[tokio::test]
async fn test_mempool_error_handling() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .try_init();

    println!("Testing mempool error handling with transparent App variant...");

    let mut nodes = create_nodes(1, 9000).await;
    let node = &mut nodes[0];

    let good_tx = TestTx(1001);
    let bad_tx = TestTx(9999);

    node.rpc
        .add_tx(&node.rpc_actor, good_tx.clone())
        .await
        .unwrap();
    let state = node.rpc.get_state(&node.rpc_actor).await.unwrap();
    assert_eq!(state, Some(TestCheckTxOutcome::Success(good_tx.hash())));

    node.rpc
        .add_tx(&node.rpc_actor, bad_tx.clone())
        .await
        .unwrap();
    let state = node.rpc.get_state(&node.rpc_actor).await.unwrap();
    assert_eq!(
        state,
        Some(TestCheckTxOutcome::Error(
            bad_tx.hash(),
            "Transaction validation failed".to_string()
        ))
    );
}

#[tokio::test]
async fn test_three_node_gossip_and_removal() {
    println!("Starting three-node tx gossip and removal test with actors...");

    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .try_init();

    let mut nodes = create_nodes(3, 8000).await;

    // Wait a bit for actors to initialize and connect
    sleep(Duration::from_millis(300)).await;

    // Test `add_tx() and `Msg::Add`
    // Each node adds a unique transaction (should trigger gossip)
    println!("Adding transactions and trigger gossip...");
    let tx1 = TestTx(1001);
    let tx2 = TestTx(1002);
    let tx3 = TestTx(1003);

    nodes[0]
        .rpc
        .add_tx(&nodes[0].rpc_actor, tx1.clone())
        .await
        .unwrap();
    nodes[1]
        .rpc
        .add_tx(&nodes[1].rpc_actor, tx2.clone())
        .await
        .unwrap();
    nodes[2]
        .rpc
        .add_tx(&nodes[2].rpc_actor, tx3.clone())
        .await
        .unwrap();

    // Wait for transactions to be processed and gossiped
    sleep(Duration::from_millis(200)).await;

    // Test `get_transactions()` and `Msg::Take`
    // Check state - each node should have all transactions
    for (i, node) in nodes.iter().enumerate() {
        let txs = node.get_transactions().await;
        println!("Node {} has {} transactions", i, txs.len());
        assert_eq!(
            txs.len(),
            3,
            "Node {i} should have exactly 3 transactions initially"
        );
    }

    // Test `remove_tx()` and `Msg::Remove`
    // Remove tx1 from all nodes
    println!("Testing removal...");
    let tx_to_remove = tx1.clone();
    for node in &mut nodes {
        node.remove_tx(&tx_to_remove).await;
    }

    // Wait for removal to be processed
    sleep(Duration::from_millis(200)).await;

    // Check final state after removal
    for (i, node) in nodes.iter().enumerate() {
        let txs = node.get_transactions().await;
        println!("Node {} has {} transactions after removal", i, txs.len());
        assert_eq!(
            txs.len(),
            2,
            "Node {i} should have exactly 2 transactions initially"
        );
    }
}
