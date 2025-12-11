//! RPC server implementation

use crate::node::TokenChainNode;
use crate::types::{Address, Transaction};
use jsonrpsee::core::RpcResult;
use jsonrpsee::proc_macros::rpc;
use std::sync::Arc;

/// RPC API interface
#[rpc(server)]
pub trait TokenChainRpc {
    /// Submit a transaction to the chain
    #[method(name = "submitTransaction")]
    async fn submit_transaction(&self, tx: Transaction) -> RpcResult<String>;

    /// Get account balance
    #[method(name = "getBalance")]
    async fn get_balance(&self, address: Address) -> RpcResult<u64>;

    /// Get account nonce
    #[method(name = "getNonce")]
    async fn get_nonce(&self, address: Address) -> RpcResult<u64>;

    /// Get node status
    #[method(name = "getStatus")]
    async fn get_status(&self) -> RpcResult<NodeStatus>;
}

/// Node status information
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct NodeStatus {
    pub node_id: u32,
    pub running: bool,
    pub rpc_addr: String,
}

/// RPC server implementation
pub struct RpcServerImpl {
    node: Arc<TokenChainNode>,
}

impl RpcServerImpl {
    pub fn new(node: Arc<TokenChainNode>) -> Self {
        Self { node }
    }
}

#[async_trait::async_trait]
impl TokenChainRpcServer for RpcServerImpl {
    async fn submit_transaction(&self, tx: Transaction) -> RpcResult<String> {
        let tx_hash = self
            .node
            .submit_transaction(tx)
            .await
            .map_err(|e| {
                jsonrpsee::types::error::ErrorObjectOwned::owned(
                    jsonrpsee::types::error::INTERNAL_ERROR_CODE,
                    e.to_string(),
                    None::<()>,
                )
            })?;

        Ok(tx_hash.to_string())
    }

    async fn get_balance(&self, address: Address) -> RpcResult<u64> {
        self.node
            .get_balance(address)
            .await
            .map_err(|e| {
                jsonrpsee::types::error::ErrorObjectOwned::owned(
                    jsonrpsee::types::error::INTERNAL_ERROR_CODE,
                    e.to_string(),
                    None::<()>,
                )
            })
    }

    async fn get_nonce(&self, address: Address) -> RpcResult<u64> {
        self.node
            .get_nonce(address)
            .await
            .map_err(|e| {
                jsonrpsee::types::error::ErrorObjectOwned::owned(
                    jsonrpsee::types::error::INTERNAL_ERROR_CODE,
                    e.to_string(),
                    None::<()>,
                )
            })
    }

    async fn get_status(&self) -> RpcResult<NodeStatus> {
        let config = self.node.config();
        Ok(NodeStatus {
            node_id: config.node_id,
            running: self.node.is_running().await,
            rpc_addr: config.rpc_addr.clone(),
        })
    }
}
