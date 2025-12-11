//! Token Chain node implementation

use crate::error::{Result, TokenChainError};
use crate::executor::TokenExecutor;
use crate::types::{Address, Transaction, TxHash};
use consensus_framework::mysticeti_adapter::{MysticetiAdapter, MysticetiConfig};
use consensus_framework::{ConsensusProtocol, ExecutionEngine};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, warn};

/// Node configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    /// Node ID
    pub node_id: u32,

    /// RPC server address
    pub rpc_addr: String,

    /// Consensus configuration
    pub consensus: ConsensusNodeConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusNodeConfig {
    /// Authority index
    pub authority_index: u32,

    /// Committee size
    pub committee_size: u32,

    /// Wave length
    pub wave_length: u32,

    /// Leader timeout (milliseconds)
    pub leader_timeout_ms: u64,
}

impl NodeConfig {
    pub fn default_for_node(node_id: u32) -> Self {
        Self {
            node_id,
            rpc_addr: format!("127.0.0.1:{}", 9000 + node_id),
            consensus: ConsensusNodeConfig {
                authority_index: node_id,
                committee_size: 4,
                wave_length: 3,
                leader_timeout_ms: 2000,
            },
        }
    }

    pub fn from_file(path: &str) -> Result<Self> {
        let contents = std::fs::read_to_string(path)
            .map_err(|e| TokenChainError::ConfigError(format!("Failed to read config: {}", e)))?;

        serde_yaml::from_str(&contents)
            .map_err(|e| TokenChainError::ConfigError(format!("Failed to parse config: {}", e)))
    }
}

/// Token Chain node
pub struct TokenChainNode {
    config: NodeConfig,
    executor: Arc<Mutex<TokenExecutor>>,
    consensus: Arc<Mutex<MysticetiAdapter<TokenExecutor>>>,
    running: Arc<Mutex<bool>>,
}

impl TokenChainNode {
    /// Create a new node
    pub fn new(config: NodeConfig) -> Result<Self> {
        info!("Creating node {}", config.node_id);

        let executor = TokenExecutor::new();
        let executor_arc = Arc::new(Mutex::new(executor));

        // Create consensus config
        let consensus_config = MysticetiConfig {
            authority_index: config.consensus.authority_index,
            committee_size: config.consensus.committee_size,
            wave_length: config.consensus.wave_length,
            leader_timeout_ms: config.consensus.leader_timeout_ms,
        };

        // Create a separate executor instance for consensus
        // (In a real implementation, we would share state properly)
        let consensus_executor = TokenExecutor::new();
        let consensus = MysticetiAdapter::new(consensus_config, consensus_executor)
            .map_err(|e| TokenChainError::NodeError(format!("Failed to create consensus: {}", e)))?;

        Ok(Self {
            config,
            executor: executor_arc,
            consensus: Arc::new(Mutex::new(consensus)),
            running: Arc::new(Mutex::new(false)),
        })
    }

    /// Start the node
    pub async fn start(&self) -> Result<()> {
        info!("Starting node {}", self.config.node_id);

        let mut running = self.running.lock().await;
        if *running {
            warn!("Node {} is already running", self.config.node_id);
            return Ok(());
        }

        // Start consensus
        let mut consensus = self.consensus.lock().await;
        consensus
            .start()
            .await
            .map_err(|e| TokenChainError::NodeError(format!("Failed to start consensus: {}", e)))?;

        *running = true;
        info!("Node {} started successfully", self.config.node_id);

        Ok(())
    }

    /// Stop the node
    pub async fn stop(&self) -> Result<()> {
        info!("Stopping node {}", self.config.node_id);

        let mut running = self.running.lock().await;
        if !*running {
            warn!("Node {} is not running", self.config.node_id);
            return Ok(());
        }

        // Stop consensus
        let mut consensus = self.consensus.lock().await;
        consensus
            .stop()
            .await
            .map_err(|e| TokenChainError::NodeError(format!("Failed to stop consensus: {}", e)))?;

        *running = false;
        info!("Node {} stopped", self.config.node_id);

        Ok(())
    }

    /// Check if node is running
    pub async fn is_running(&self) -> bool {
        *self.running.lock().await
    }

    /// Submit a transaction
    pub async fn submit_transaction(&self, tx: Transaction) -> Result<TxHash> {
        if !self.is_running().await {
            return Err(TokenChainError::NodeError(
                "Node is not running".to_string(),
            ));
        }

        // Validate transaction first
        {
            let executor = self.executor.lock().await;
            (*executor)
                .validate(&tx)
                .await
                .map_err(|e| TokenChainError::ExecutionError(e.to_string()))?;
        }

        // Submit to consensus
        let tx_id = {
            let consensus = self.consensus.lock().await;
            consensus
                .submit(tx.clone())
                .await
                .map_err(TokenChainError::ConsensusError)?
        };

        // Execute locally (in a real implementation, this would happen after consensus)
        {
            let mut executor = self.executor.lock().await;
            (*executor)
                .execute_batch(vec![tx])
                .await
                .map_err(|e| TokenChainError::ExecutionError(e.to_string()))?;
        }

        // Convert consensus TxId to our TxHash
        let tx_hash = TxHash(tx_id.as_bytes().to_vec());

        Ok(tx_hash)
    }

    /// Get account balance
    pub async fn get_balance(&self, address: Address) -> Result<u64> {
        let executor = self.executor.lock().await;
        Ok(executor.get_balance(&address))
    }

    /// Get account nonce
    pub async fn get_nonce(&self, address: Address) -> Result<u64> {
        let executor = self.executor.lock().await;
        Ok(executor.get_nonce(&address))
    }

    /// Get transaction by hash
    pub async fn get_transaction(&self, hash: TxHash) -> Result<Option<crate::types::ExecutionResult>> {
        let executor = self.executor.lock().await;
        Ok(executor.get_transaction(&hash).cloned())
    }

    /// Get node config
    pub fn config(&self) -> &NodeConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_node_creation() {
        let config = NodeConfig::default_for_node(0);
        let node = TokenChainNode::new(config);
        assert!(node.is_ok());
    }

    #[tokio::test]
    async fn test_node_start_stop() {
        let config = NodeConfig::default_for_node(0);
        let node = TokenChainNode::new(config).unwrap();

        assert!(!node.is_running().await);

        node.start().await.unwrap();
        assert!(node.is_running().await);

        node.stop().await.unwrap();
        assert!(!node.is_running().await);
    }

    #[tokio::test]
    async fn test_submit_transaction() {
        let config = NodeConfig::default_for_node(0);
        let node = TokenChainNode::new(config).unwrap();

        node.start().await.unwrap();

        let alice = Address::from_string("alice");
        let tx = Transaction::Mint {
            to: alice,
            amount: 1000,
        };

        let result = node.submit_transaction(tx).await;
        assert!(result.is_ok());

        let balance = node.get_balance(alice).await.unwrap();
        assert_eq!(balance, 1000);

        node.stop().await.unwrap();
    }
}
