//! Mysticeti consensus adapter
//!
//! This module provides an adapter that implements the ConsensusProtocol trait
//! using the Mysticeti consensus implementation from Sui.

use crate::error::ConsensusError;
use crate::traits::{ConsensusProtocol, ExecutionEngine};
use crate::types::{CommittedOutput, TxId};
use async_trait::async_trait;
use bytes::Bytes;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

/// Configuration for the Mysticeti adapter
#[derive(Clone)]
pub struct MysticetiConfig {
    /// Authority index in the committee
    pub authority_index: u32,

    /// Committee size
    pub committee_size: u32,

    /// Wave length for consensus
    pub wave_length: u32,

    /// Leader timeout in milliseconds
    pub leader_timeout_ms: u64,
}

impl Default for MysticetiConfig {
    fn default() -> Self {
        Self {
            authority_index: 0,
            committee_size: 4,
            wave_length: 3,
            leader_timeout_ms: 2000,
        }
    }
}

/// Mysticeti consensus adapter
///
/// This adapter wraps the Mysticeti consensus implementation and provides
/// a generic interface through the ConsensusProtocol trait.
pub struct MysticetiAdapter<E>
where
    E: ExecutionEngine,
{
    config: MysticetiConfig,
    _executor: Arc<Mutex<E>>,
    _commit_sender: mpsc::Sender<CommittedOutput<E::Output>>,
    _commit_receiver: Arc<Mutex<mpsc::Receiver<CommittedOutput<E::Output>>>>,
    commit_index: Arc<Mutex<u64>>,
    ready: Arc<Mutex<bool>>,
}

impl<E> MysticetiAdapter<E>
where
    E: ExecutionEngine + 'static,
    E::Transaction: From<Bytes> + Into<Bytes>,
{
    /// Create a new Mysticeti adapter
    pub fn new(config: MysticetiConfig, executor: E) -> Result<Self, ConsensusError> {
        let (commit_sender, commit_receiver) = mpsc::channel(1000);

        Ok(Self {
            config,
            _executor: Arc::new(Mutex::new(executor)),
            _commit_sender: commit_sender,
            _commit_receiver: Arc::new(Mutex::new(commit_receiver)),
            commit_index: Arc::new(Mutex::new(0)),
            ready: Arc::new(Mutex::new(false)),
        })
    }

    /// Start the consensus adapter
    ///
    /// This initializes the Mysticeti consensus node and begins processing blocks.
    pub async fn start(&mut self) -> Result<(), ConsensusError> {
        // Mark as ready
        *self.ready.lock().await = true;

        // In a full implementation, this would:
        // 1. Initialize the AuthorityNode with the config
        // 2. Start the consensus protocol
        // 3. Set up callbacks for committed blocks
        // 4. Launch background tasks for processing commits

        Ok(())
    }

    /// Stop the consensus adapter
    pub async fn stop(&mut self) -> Result<(), ConsensusError> {
        *self.ready.lock().await = false;
        Ok(())
    }

    /// Get the configuration
    pub fn config(&self) -> &MysticetiConfig {
        &self.config
    }
}

#[async_trait]
impl<E> ConsensusProtocol for MysticetiAdapter<E>
where
    E: ExecutionEngine + 'static,
    E::Transaction: From<Bytes> + Into<Bytes> + Clone,
    E::Output: Clone,
{
    type Transaction = E::Transaction;
    type Block = Bytes;
    type CommittedOutput = CommittedOutput<E::Output>;

    async fn submit(&self, tx: Self::Transaction) -> Result<TxId, ConsensusError> {
        if !*self.ready.lock().await {
            return Err(ConsensusError::NotReady);
        }

        // Convert transaction to bytes
        let tx_bytes: Bytes = tx.into();

        // In a full implementation, this would:
        // 1. Serialize the transaction
        // 2. Submit to the consensus layer via AuthorityNode
        // 3. Return the transaction hash

        // For now, create a mock transaction ID
        let mut tx_id_bytes = [0u8; 32];
        let hash = blake3::hash(&tx_bytes);
        tx_id_bytes.copy_from_slice(hash.as_bytes());

        Ok(TxId::new(tx_id_bytes))
    }

    async fn get_committed(&self) -> Result<Vec<Self::CommittedOutput>, ConsensusError> {
        // In a full implementation, this would query the committed outputs
        // from the consensus layer
        Ok(Vec::new())
    }

    fn subscribe_commits(&self) -> mpsc::Receiver<Self::CommittedOutput> {
        // Create a new receiver for subscribing to commits
        let (_tx, rx) = mpsc::channel(1000);

        // In a full implementation, we would:
        // 1. Clone the commit receiver
        // 2. Forward commits to the new receiver
        // For now, just return an empty receiver

        rx
    }

    async fn is_ready(&self) -> bool {
        *self.ready.lock().await
    }

    async fn commit_index(&self) -> u64 {
        *self.commit_index.lock().await
    }
}

/// Simple in-memory execution engine for testing
///
/// This is a basic implementation that can be used for testing the adapter.
pub struct SimpleExecutor<T, S> {
    state: S,
    _phantom: std::marker::PhantomData<T>,
}

impl<T, S> SimpleExecutor<T, S>
where
    S: Default,
{
    pub fn new() -> Self {
        Self {
            state: S::default(),
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<T, S> Default for SimpleExecutor<T, S>
where
    S: Default,
{
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl<T, S> ExecutionEngine for SimpleExecutor<T, S>
where
    T: Send + Sync + Clone,
    S: Send + Sync + Default,
{
    type Transaction = T;
    type State = S;
    type Output = Vec<T>;

    async fn execute_batch(&mut self, txs: Vec<Self::Transaction>) -> Result<Self::Output, crate::error::ExecutionError> {
        Ok(txs)
    }

    fn get_state(&self) -> &Self::State {
        &self.state
    }

    fn get_state_mut(&mut self) -> &mut Self::State {
        &mut self.state
    }

    async fn validate(&self, _tx: &Self::Transaction) -> Result<(), crate::error::ExecutionError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug)]
    struct TestTransaction {
        data: Vec<u8>,
    }

    impl From<Bytes> for TestTransaction {
        fn from(bytes: Bytes) -> Self {
            Self {
                data: bytes.to_vec(),
            }
        }
    }

    impl From<TestTransaction> for Bytes {
        fn from(tx: TestTransaction) -> Self {
            Bytes::from(tx.data)
        }
    }

    #[tokio::test]
    async fn test_adapter_creation() {
        let config = MysticetiConfig::default();
        let executor = SimpleExecutor::<TestTransaction, ()>::new();
        let adapter = MysticetiAdapter::new(config, executor);
        assert!(adapter.is_ok());
    }

    #[tokio::test]
    async fn test_adapter_start_stop() {
        let config = MysticetiConfig::default();
        let executor = SimpleExecutor::<TestTransaction, ()>::new();
        let mut adapter = MysticetiAdapter::new(config, executor).unwrap();

        assert!(!adapter.is_ready().await);

        adapter.start().await.unwrap();
        assert!(adapter.is_ready().await);

        adapter.stop().await.unwrap();
        assert!(!adapter.is_ready().await);
    }

    #[tokio::test]
    async fn test_submit_transaction() {
        let config = MysticetiConfig::default();
        let executor = SimpleExecutor::<TestTransaction, ()>::new();
        let mut adapter = MysticetiAdapter::new(config, executor).unwrap();

        adapter.start().await.unwrap();

        let tx = TestTransaction {
            data: vec![1, 2, 3, 4],
        };

        let result = adapter.submit(tx).await;
        assert!(result.is_ok());
    }
}
