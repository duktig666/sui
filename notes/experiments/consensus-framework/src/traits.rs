//! Core trait definitions for the consensus framework

use crate::error::{ConsensusError, ExecutionError, StateError};
use crate::types::TxId;
use async_trait::async_trait;
use tokio::sync::mpsc::Receiver;

/// Generic consensus protocol interface
///
/// This trait abstracts the core functionality of a consensus protocol,
/// allowing different applications to use the same consensus mechanism.
#[async_trait]
pub trait ConsensusProtocol: Send + Sync {
    /// Application-specific transaction type
    type Transaction: Send + Sync + Clone;

    /// Block representation in the consensus layer
    type Block: Send + Sync;

    /// Output produced when transactions are committed
    type CommittedOutput: Send + Sync;

    /// Submit a transaction to the consensus layer
    ///
    /// Returns a transaction ID that can be used to track the transaction
    async fn submit(&self, tx: Self::Transaction) -> Result<TxId, ConsensusError>;

    /// Submit multiple transactions in a batch
    async fn submit_batch(&self, txs: Vec<Self::Transaction>) -> Result<Vec<TxId>, ConsensusError> {
        let mut ids = Vec::with_capacity(txs.len());
        for tx in txs {
            ids.push(self.submit(tx).await?);
        }
        Ok(ids)
    }

    /// Get all committed outputs
    ///
    /// Returns outputs that have been finalized by consensus
    async fn get_committed(&self) -> Result<Vec<Self::CommittedOutput>, ConsensusError>;

    /// Subscribe to commit notifications
    ///
    /// Returns a receiver that will be notified whenever new outputs are committed
    fn subscribe_commits(&self) -> Receiver<Self::CommittedOutput>;

    /// Check if the consensus node is ready to accept transactions
    async fn is_ready(&self) -> bool;

    /// Get the current commit index
    async fn commit_index(&self) -> u64;
}

/// Execution engine interface
///
/// This trait defines how transactions are executed and how state is managed.
/// Applications implement this trait to define their custom execution logic.
#[async_trait]
pub trait ExecutionEngine: Send + Sync {
    /// Application-specific transaction type
    type Transaction: Send + Sync;

    /// Application state representation
    type State: Send + Sync;

    /// Output produced by executing transactions
    type Output: Send + Sync;

    /// Execute a batch of transactions
    ///
    /// Transactions are executed in the order they appear in the vector.
    /// The execution should be deterministic across all nodes.
    async fn execute_batch(
        &mut self,
        txs: Vec<Self::Transaction>,
    ) -> Result<Self::Output, ExecutionError>;

    /// Execute a single transaction
    async fn execute(&mut self, tx: Self::Transaction) -> Result<Self::Output, ExecutionError> {
        self.execute_batch(vec![tx]).await
    }

    /// Get a reference to the current state
    fn get_state(&self) -> &Self::State;

    /// Get a mutable reference to the current state
    fn get_state_mut(&mut self) -> &mut Self::State;

    /// Validate a transaction without executing it
    ///
    /// This is useful for checking if a transaction is valid before submitting
    /// it to consensus.
    async fn validate(&self, tx: &Self::Transaction) -> Result<(), ExecutionError>;
}

/// State management interface
///
/// This trait defines how application state is checkpointed and restored.
/// This is useful for state synchronization and recovery.
#[async_trait]
pub trait StateManager: Send + Sync {
    /// Checkpoint representation
    type Checkpoint: Send + Sync + Clone;

    /// Create a checkpoint of the current state
    ///
    /// Returns a checkpoint that can be used to restore state later
    async fn create_checkpoint(&self) -> Result<Self::Checkpoint, StateError>;

    /// Restore state from a checkpoint
    ///
    /// This replaces the current state with the state from the checkpoint
    async fn restore_checkpoint(&mut self, checkpoint: Self::Checkpoint) -> Result<(), StateError>;

    /// Get the checkpoint at a specific commit index
    async fn get_checkpoint_at(&self, commit_index: u64) -> Result<Option<Self::Checkpoint>, StateError>;

    /// Prune old checkpoints
    ///
    /// Remove checkpoints older than the specified commit index
    async fn prune_checkpoints(&mut self, before_index: u64) -> Result<(), StateError>;
}

/// Combined trait for a full consensus node
///
/// This trait combines all the necessary functionality for running a
/// consensus-based application.
#[async_trait]
pub trait ConsensusNode: ConsensusProtocol + Send + Sync {
    /// Start the consensus node
    async fn start(&mut self) -> Result<(), ConsensusError>;

    /// Stop the consensus node gracefully
    async fn stop(&mut self) -> Result<(), ConsensusError>;

    /// Check if the node is running
    fn is_running(&self) -> bool;
}
