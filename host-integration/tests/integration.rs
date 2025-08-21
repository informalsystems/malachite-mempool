use crate::utils::create_nodes;
use bytesize::ByteSize;
use fifo_mempool::{RawTx, ReapCursor};
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

    let mut nodes = create_nodes(1, 9000, ByteSize::mb(4)).await;
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
async fn test_duplicate_transaction_handling() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .try_init();

    println!("Testing duplicate transaction handling...");

    let mut nodes = create_nodes(1, 10000, ByteSize::mb(4)).await;
    let node = &mut nodes[0];

    let tx = TestTx(2001);

    // First addition should succeed
    node.rpc.add_tx(&node.rpc_actor, tx.clone()).await.unwrap();
    let state = node.rpc.get_state(&node.rpc_actor).await.unwrap();
    assert_eq!(state, Some(TestCheckTxOutcome::Success(tx.hash())));

    // Verify transaction is in mempool
    let txs = node.get_transactions().await;
    assert_eq!(txs.len(), 1, "Mempool should contain 1 transaction");

    // Second addition of same transaction should fail with duplicate error
    node.rpc.add_tx(&node.rpc_actor, tx.clone()).await.unwrap();
    let state = node.rpc.get_state(&node.rpc_actor).await.unwrap();

    // Should be an error containing the duplicate message
    match state {
        Some(TestCheckTxOutcome::Error(hash, error_msg)) => {
            assert_eq!(hash, tx.hash());
            assert!(
                error_msg.contains("Transaction already exists"),
                "Error message should indicate duplicate transaction, got: {}",
                error_msg
            );
            println!(
                "✅ Duplicate transaction correctly rejected with error: {}",
                error_msg
            );
        }
        other => {
            panic!("Expected duplicate error, got: {:?}", other);
        }
    }

    // Verify mempool still contains only 1 transaction
    let txs = node.get_transactions().await;
    assert_eq!(
        txs.len(),
        1,
        "Mempool should still contain only 1 transaction after duplicate attempt"
    );

    println!("✅ Duplicate transaction handling test passed!");
}

#[tokio::test]
async fn test_three_node_gossip_and_removal() {
    println!("Starting three-node tx gossip and removal test with actors...");

    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .try_init();

    let mut nodes = create_nodes(3, 8000, ByteSize::mb(4)).await;

    // Wait for actors to initialize and network connections to stabilize
    sleep(Duration::from_millis(1000)).await;

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
    sleep(Duration::from_millis(500)).await;

    // Test `get_transactions()` and `Msg::Reap`
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
    sleep(Duration::from_millis(300)).await;

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

fn assert_eq_transactions(expected_txs: Vec<TestTx>, actual_txs: Vec<RawTx>) {
    assert_eq!(expected_txs.len(), actual_txs.len());
    for (expected, actual) in expected_txs.iter().zip(actual_txs.iter()) {
        assert_eq!(expected.0.to_le_bytes().to_vec(), actual.0.to_vec());
    }
}

#[tokio::test]
async fn test_reap_transactions() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .try_init();

    println!("Testing reap transactions handling...");

    // a transaction contains simply an u64, and thus is the size of a transaction and a block contains at most 2 transactions
    let tx_size = size_of::<u64>();
    let max_txs_in_a_block  = 2;
    let max_txs_size = max_txs_in_a_block * tx_size;

    let mut nodes = create_nodes(1, 10000, ByteSize::b(max_txs_size as u64)).await;
    let node = &mut nodes[0];

    let txs = vec![TestTx(1001), TestTx(1002), TestTx(1003), TestTx(1004), TestTx(1005)];

    // add transactions to the mempool
    for tx in &txs {
        node.rpc.add_tx(&node.rpc_actor, tx.clone()).await.unwrap();
        let state = node.rpc.get_state(&node.rpc_actor).await.unwrap();
        assert_eq!(state, Some(TestCheckTxOutcome::Success(tx.hash())));
    }

    let reaped_txs = node.reap_transactions(ReapCursor::Fresh).await;
    assert_eq_transactions(vec![txs[0].clone(), txs[1].clone()], reaped_txs);

    // a second fresh reap returns the same transactions as the previous fresh reap
    let reaped_txs = node.reap_transactions(ReapCursor::Fresh).await;
    assert_eq_transactions(vec![txs[0].clone(), txs[1].clone()], reaped_txs);

    // when resuming the reap of transactions, we retrieve subsequent transacstions
    let reaped_txs = node.reap_transactions(ReapCursor::Resume).await;
    assert_eq_transactions(vec![txs[2].clone(), txs[3].clone()], reaped_txs);

    // when resuming the reap of transactions, we retrieve a subsequent transacstion
    let reaped_txs = node.reap_transactions(ReapCursor::Resume).await;
    assert_eq_transactions(vec![txs[4].clone()], reaped_txs);

    // remove the first transaction and perform a fresh reap
    node.remove_tx(&txs[0]).await;
    let reaped_txs = node.reap_transactions(ReapCursor::Fresh).await;
    assert_eq_transactions(vec![txs[1].clone(), txs[2].clone()], reaped_txs);
}
