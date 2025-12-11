//! Integration tests for Token Chain
//!
//! These tests verify the complete system behavior including:
//! - Multi-transaction workflows
//! - State consistency
//! - Error handling
//! - Concurrent operations

use simple_token_chain::{Address, NodeConfig, TokenChainNode, Transaction};
use tokio::time::{sleep, Duration};

/// Helper to create a test node
async fn create_test_node(node_id: u32) -> TokenChainNode {
    let config = NodeConfig::default_for_node(node_id);
    let node = TokenChainNode::new(config).expect("Failed to create node");
    node.start().await.expect("Failed to start node");
    node
}

/// Helper to create test addresses
fn test_addresses() -> (Address, Address, Address) {
    (
        Address::from_string("alice"),
        Address::from_string("bob"),
        Address::from_string("charlie"),
    )
}

#[tokio::test]
async fn test_complete_token_workflow() {
    let node = create_test_node(0).await;
    let (alice, bob, charlie) = test_addresses();

    // Step 1: Mint tokens to Alice
    let mint_tx = Transaction::Mint {
        to: alice,
        amount: 1000,
    };
    node.submit_transaction(mint_tx)
        .await
        .expect("Mint failed");

    sleep(Duration::from_millis(100)).await;

    let alice_balance = node.get_balance(alice).await.unwrap();
    assert_eq!(alice_balance, 1000, "Alice should have 1000 tokens");

    // Step 2: Transfer from Alice to Bob
    let transfer_tx = Transaction::Transfer {
        from: alice,
        to: bob,
        amount: 300,
        nonce: 0,
    };
    node.submit_transaction(transfer_tx)
        .await
        .expect("Transfer failed");

    sleep(Duration::from_millis(100)).await;

    let alice_balance = node.get_balance(alice).await.unwrap();
    let bob_balance = node.get_balance(bob).await.unwrap();
    assert_eq!(alice_balance, 700);
    assert_eq!(bob_balance, 300);

    // Step 3: Transfer from Alice to Charlie
    let transfer_tx = Transaction::Transfer {
        from: alice,
        to: charlie,
        amount: 200,
        nonce: 1,
    };
    node.submit_transaction(transfer_tx)
        .await
        .expect("Transfer failed");

    sleep(Duration::from_millis(100)).await;

    let alice_balance = node.get_balance(alice).await.unwrap();
    let charlie_balance = node.get_balance(charlie).await.unwrap();
    assert_eq!(alice_balance, 500);
    assert_eq!(charlie_balance, 200);

    // Verify total supply
    let total = alice_balance + bob_balance + charlie_balance;
    assert_eq!(total, 1000, "Total supply should be conserved");

    node.stop().await.unwrap();
}

#[tokio::test]
async fn test_nonce_validation() {
    let node = create_test_node(0).await;
    let (alice, bob, _) = test_addresses();

    // Mint tokens
    node.submit_transaction(Transaction::Mint {
        to: alice,
        amount: 1000,
    })
    .await
    .unwrap();
    sleep(Duration::from_millis(100)).await;

    // Valid transfer with nonce 0
    node.submit_transaction(Transaction::Transfer {
        from: alice,
        to: bob,
        amount: 100,
        nonce: 0,
    })
    .await
    .unwrap();
    sleep(Duration::from_millis(100)).await;

    // Invalid: reuse nonce 0
    let result = node
        .submit_transaction(Transaction::Transfer {
            from: alice,
            to: bob,
            amount: 100,
            nonce: 0,
        })
        .await;
    assert!(result.is_err(), "Should reject duplicate nonce");

    // Invalid: skip nonce (use 5 instead of 1)
    let result = node
        .submit_transaction(Transaction::Transfer {
            from: alice,
            to: bob,
            amount: 100,
            nonce: 5,
        })
        .await;
    assert!(result.is_err(), "Should reject out-of-order nonce");

    // Valid: correct nonce 1
    node.submit_transaction(Transaction::Transfer {
        from: alice,
        to: bob,
        amount: 100,
        nonce: 1,
    })
    .await
    .unwrap();

    sleep(Duration::from_millis(100)).await;

    // Verify final state
    let alice_nonce = node.get_nonce(alice).await.unwrap();
    assert_eq!(alice_nonce, 2, "Alice's nonce should be 2");

    let bob_balance = node.get_balance(bob).await.unwrap();
    assert_eq!(bob_balance, 200, "Bob should have 200 tokens");

    node.stop().await.unwrap();
}

#[tokio::test]
async fn test_insufficient_balance() {
    let node = create_test_node(0).await;
    let (alice, bob, _) = test_addresses();

    // Mint 100 tokens to Alice
    node.submit_transaction(Transaction::Mint {
        to: alice,
        amount: 100,
    })
    .await
    .unwrap();
    sleep(Duration::from_millis(100)).await;

    // Try to transfer 200 (more than balance)
    let result = node
        .submit_transaction(Transaction::Transfer {
            from: alice,
            to: bob,
            amount: 200,
            nonce: 0,
        })
        .await;

    assert!(
        result.is_err(),
        "Should reject transfer with insufficient balance"
    );

    // Verify Alice still has 100
    let alice_balance = node.get_balance(alice).await.unwrap();
    assert_eq!(alice_balance, 100);

    // Verify Bob has 0
    let bob_balance = node.get_balance(bob).await.unwrap();
    assert_eq!(bob_balance, 0);

    node.stop().await.unwrap();
}

#[tokio::test]
async fn test_sequential_transactions() {
    let node = create_test_node(0).await;
    let (alice, bob, _) = test_addresses();

    // Mint tokens
    node.submit_transaction(Transaction::Mint {
        to: alice,
        amount: 1000,
    })
    .await
    .unwrap();
    sleep(Duration::from_millis(100)).await;

    // Submit 10 sequential transfers
    for i in 0..10 {
        node.submit_transaction(Transaction::Transfer {
            from: alice,
            to: bob,
            amount: 50,
            nonce: i,
        })
        .await
        .unwrap_or_else(|_| panic!("Transfer {} failed", i));
        sleep(Duration::from_millis(50)).await;
    }

    // Verify final state
    let alice_balance = node.get_balance(alice).await.unwrap();
    let bob_balance = node.get_balance(bob).await.unwrap();
    let alice_nonce = node.get_nonce(alice).await.unwrap();

    assert_eq!(alice_balance, 500, "Alice should have 500 left");
    assert_eq!(bob_balance, 500, "Bob should have received 500");
    assert_eq!(alice_nonce, 10, "Alice's nonce should be 10");

    node.stop().await.unwrap();
}

#[tokio::test]
async fn test_multiple_accounts() {
    let node = create_test_node(0).await;
    let alice = Address::from_string("alice");
    let bob = Address::from_string("bob");
    let charlie = Address::from_string("charlie");
    let dave = Address::from_string("dave");

    // Mint to multiple accounts
    for (addr, amount) in [(alice, 1000), (bob, 2000), (charlie, 1500)] {
        node.submit_transaction(Transaction::Mint { to: addr, amount })
            .await
            .unwrap();
        sleep(Duration::from_millis(50)).await;
    }

    // Create a transfer chain: Alice -> Dave, Bob -> Dave, Charlie -> Dave
    node.submit_transaction(Transaction::Transfer {
        from: alice,
        to: dave,
        amount: 300,
        nonce: 0,
    })
    .await
    .unwrap();

    node.submit_transaction(Transaction::Transfer {
        from: bob,
        to: dave,
        amount: 500,
        nonce: 0,
    })
    .await
    .unwrap();

    node.submit_transaction(Transaction::Transfer {
        from: charlie,
        to: dave,
        amount: 400,
        nonce: 0,
    })
    .await
    .unwrap();

    sleep(Duration::from_millis(200)).await;

    // Verify balances
    assert_eq!(node.get_balance(alice).await.unwrap(), 700);
    assert_eq!(node.get_balance(bob).await.unwrap(), 1500);
    assert_eq!(node.get_balance(charlie).await.unwrap(), 1100);
    assert_eq!(node.get_balance(dave).await.unwrap(), 1200);

    // Verify total supply
    let total = node.get_balance(alice).await.unwrap()
        + node.get_balance(bob).await.unwrap()
        + node.get_balance(charlie).await.unwrap()
        + node.get_balance(dave).await.unwrap();
    assert_eq!(total, 4500, "Total supply should be conserved");

    node.stop().await.unwrap();
}

#[tokio::test]
async fn test_zero_amount_transfer() {
    let node = create_test_node(0).await;
    let (alice, bob, _) = test_addresses();

    node.submit_transaction(Transaction::Mint {
        to: alice,
        amount: 1000,
    })
    .await
    .unwrap();
    sleep(Duration::from_millis(100)).await;

    // Transfer 0 tokens (should succeed but not change balances)
    node.submit_transaction(Transaction::Transfer {
        from: alice,
        to: bob,
        amount: 0,
        nonce: 0,
    })
    .await
    .unwrap();

    sleep(Duration::from_millis(100)).await;

    assert_eq!(node.get_balance(alice).await.unwrap(), 1000);
    assert_eq!(node.get_balance(bob).await.unwrap(), 0);
    assert_eq!(
        node.get_nonce(alice).await.unwrap(),
        1,
        "Nonce should still increment"
    );

    node.stop().await.unwrap();
}

#[tokio::test]
async fn test_self_transfer() {
    let node = create_test_node(0).await;
    let alice = Address::from_string("alice");

    node.submit_transaction(Transaction::Mint {
        to: alice,
        amount: 1000,
    })
    .await
    .unwrap();
    sleep(Duration::from_millis(100)).await;

    // Transfer to self
    node.submit_transaction(Transaction::Transfer {
        from: alice,
        to: alice,
        amount: 300,
        nonce: 0,
    })
    .await
    .unwrap();

    sleep(Duration::from_millis(100)).await;

    // Balance should remain the same
    assert_eq!(node.get_balance(alice).await.unwrap(), 1000);
    assert_eq!(node.get_nonce(alice).await.unwrap(), 1);

    node.stop().await.unwrap();
}

#[tokio::test]
async fn test_node_restart() {
    let config = NodeConfig::default_for_node(0);
    let node = TokenChainNode::new(config.clone()).unwrap();

    // Start and submit transactions
    node.start().await.unwrap();

    let alice = Address::from_string("alice");
    node.submit_transaction(Transaction::Mint {
        to: alice,
        amount: 1000,
    })
    .await
    .unwrap();
    sleep(Duration::from_millis(100)).await;

    // Stop the node
    node.stop().await.unwrap();

    // Note: In current implementation, state is not persisted
    // This test verifies that the node can be stopped and restarted
    // In a production system, we would verify state persistence

    // Start again
    node.start().await.unwrap();
    assert!(node.is_running().await);

    node.stop().await.unwrap();
}

#[tokio::test]
async fn test_large_transfer_amount() {
    let node = create_test_node(0).await;
    let (alice, bob, _) = test_addresses();

    // Mint large even amount to avoid rounding issues
    let large_amount = 1_000_000_000_000_000_000u64;
    node.submit_transaction(Transaction::Mint {
        to: alice,
        amount: large_amount,
    })
    .await
    .unwrap();
    sleep(Duration::from_millis(100)).await;

    // Transfer half the amount
    let transfer_amount = large_amount / 2;
    node.submit_transaction(Transaction::Transfer {
        from: alice,
        to: bob,
        amount: transfer_amount,
        nonce: 0,
    })
    .await
    .unwrap();
    sleep(Duration::from_millis(100)).await;

    let alice_balance = node.get_balance(alice).await.unwrap();
    let bob_balance = node.get_balance(bob).await.unwrap();

    assert_eq!(alice_balance, transfer_amount);
    assert_eq!(bob_balance, transfer_amount);
    assert_eq!(alice_balance + bob_balance, large_amount, "Supply conservation");

    node.stop().await.unwrap();
}
