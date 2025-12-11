//! Token Chain node executable

use simple_token_chain::{NodeConfig, RpcServerImpl, TokenChainNode, TokenChainRpcServer};
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    info!("Starting Token Chain node...");

    // Parse command line arguments
    let args: Vec<String> = std::env::args().collect();
    let config = if args.len() > 2 && args[1] == "--config" {
        // Load from file
        let config_path = &args[2];
        info!("Loading configuration from: {}", config_path);
        NodeConfig::from_file(config_path)?
    } else {
        // Use default config for node 0
        info!("Using default configuration for node 0");
        NodeConfig::default_for_node(0)
    };

    info!("Node ID: {}", config.node_id);
    info!("RPC address: {}", config.rpc_addr);

    // Create node
    let node = Arc::new(TokenChainNode::new(config.clone())?);

    // Start the node
    node.start().await?;
    info!("Node started successfully");

    // Create RPC server
    let rpc_impl = RpcServerImpl::new(node.clone());

    // Parse RPC address
    let rpc_addr: SocketAddr = config.rpc_addr.parse()?;

    // Build and start RPC server
    let server = jsonrpsee::server::ServerBuilder::default()
        .build(rpc_addr)
        .await?;

    let handle = server.start(rpc_impl.into_rpc());

    info!(
        "Token Chain node running at http://{}",
        config.rpc_addr
    );
    info!("Press Ctrl+C to stop");

    // Wait for shutdown signal
    tokio::signal::ctrl_c().await?;

    info!("Shutting down...");

    // Stop the server
    handle.stop()?;
    handle.stopped().await;

    // Stop the node
    node.stop().await?;

    info!("Token Chain node stopped");

    Ok(())
}
