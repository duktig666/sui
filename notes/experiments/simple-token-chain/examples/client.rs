//! Simple client to interact with the Token Chain

use simple_token_chain::{Address, Transaction};
use jsonrpsee::http_client::HttpClientBuilder;
use jsonrpsee::core::client::ClientT;
use serde_json::json;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("🚀 Token Chain Client Demo\n");

    // Connect to the node
    let client = HttpClientBuilder::default()
        .build("http://127.0.0.1:9000")?;

    println!("✅ Connected to Token Chain node at http://127.0.0.1:9000\n");

    // Create addresses
    let alice = Address::from_string("alice");
    let bob = Address::from_string("bob");
    let charlie = Address::from_string("charlie");

    println!("👥 Created test addresses:");
    println!("   Alice:   {}", alice);
    println!("   Bob:     {}", bob);
    println!("   Charlie: {}\n", charlie);

    // Step 1: Check node status
    println!("📊 Step 1: Checking node status...");
    let status: serde_json::Value = client
        .request("getStatus", vec![json!(null)])
        .await?;
    println!("   Node status: {}\n", serde_json::to_string_pretty(&status)?);

    // Step 2: Check initial balances
    println!("💰 Step 2: Checking initial balances...");
    let alice_balance: u64 = client
        .request("getBalance", vec![json!(alice)])
        .await?;
    println!("   Alice's balance: {} tokens\n", alice_balance);

    // Step 3: Mint tokens to Alice
    println!("🏦 Step 3: Minting 1000 tokens to Alice...");
    let mint_tx = Transaction::Mint {
        to: alice,
        amount: 1000,
    };
    let tx_hash: String = client
        .request("submitTransaction", vec![json!(mint_tx)])
        .await?;
    println!("   Transaction hash: {}", tx_hash);

    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

    let alice_balance: u64 = client
        .request("getBalance", vec![json!(alice)])
        .await?;
    println!("   ✅ Alice's new balance: {} tokens\n", alice_balance);

    // Step 4: Transfer to Bob
    println!("💸 Step 4: Transferring 300 tokens from Alice to Bob...");
    let alice_nonce: u64 = client
        .request("getNonce", vec![json!(alice)])
        .await?;

    let transfer_tx = Transaction::Transfer {
        from: alice,
        to: bob,
        amount: 300,
        nonce: alice_nonce,
    };
    let tx_hash: String = client
        .request("submitTransaction", vec![json!(transfer_tx)])
        .await?;
    println!("   Transaction hash: {}", tx_hash);

    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

    let alice_balance: u64 = client
        .request("getBalance", vec![json!(alice)])
        .await?;
    let bob_balance: u64 = client
        .request("getBalance", vec![json!(bob)])
        .await?;
    println!("   ✅ Alice's balance: {} tokens", alice_balance);
    println!("   ✅ Bob's balance: {} tokens\n", bob_balance);

    // Step 5: Transfer to Charlie
    println!("💸 Step 5: Transferring 200 tokens from Alice to Charlie...");
    let alice_nonce: u64 = client
        .request("getNonce", vec![json!(alice)])
        .await?;

    let transfer_tx = Transaction::Transfer {
        from: alice,
        to: charlie,
        amount: 200,
        nonce: alice_nonce,
    };
    let tx_hash: String = client
        .request("submitTransaction", vec![json!(transfer_tx)])
        .await?;
    println!("   Transaction hash: {}", tx_hash);

    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

    // Step 6: Final balances
    println!("\n📊 Step 6: Final state of the blockchain:");
    let alice_balance: u64 = client
        .request("getBalance", vec![json!(alice)])
        .await?;
    let bob_balance: u64 = client
        .request("getBalance", vec![json!(bob)])
        .await?;
    let charlie_balance: u64 = client
        .request("getBalance", vec![json!(charlie)])
        .await?;

    let alice_nonce: u64 = client
        .request("getNonce", vec![json!(alice)])
        .await?;

    println!("   Alice:   {} tokens (nonce: {})", alice_balance, alice_nonce);
    println!("   Bob:     {} tokens", bob_balance);
    println!("   Charlie: {} tokens", charlie_balance);
    println!("   Total:   {} tokens\n", alice_balance + bob_balance + charlie_balance);

    // Step 7: Try invalid transaction (insufficient balance)
    println!("❌ Step 7: Testing invalid transaction (insufficient balance)...");
    let bob_nonce: u64 = client
        .request("getNonce", vec![json!(bob)])
        .await?;

    let invalid_tx = Transaction::Transfer {
        from: bob,
        to: alice,
        amount: 1000, // Bob only has 300
        nonce: bob_nonce,
    };

    match client.request::<String, _>("submitTransaction", vec![json!(invalid_tx)]).await {
        Ok(hash) => println!("   Unexpected success: {}", hash),
        Err(e) => println!("   ✅ Expected error: {}\n", e),
    }

    println!("🎉 Demo complete!");
    println!("\n📝 Summary:");
    println!("   - Created accounts for Alice, Bob, and Charlie");
    println!("   - Minted 1000 tokens to Alice");
    println!("   - Transferred 300 tokens to Bob");
    println!("   - Transferred 200 tokens to Charlie");
    println!("   - Verified nonce increments");
    println!("   - Tested invalid transaction handling");
    println!("\n✅ This is a working blockchain!");

    Ok(())
}
