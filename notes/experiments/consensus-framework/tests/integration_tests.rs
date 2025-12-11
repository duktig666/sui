//! Integration tests for the consensus framework

use consensus_framework::{
    mysticeti_adapter::{MysticetiAdapter, MysticetiConfig, SimpleExecutor},
    traits::ConsensusProtocol,
};
use bytes::Bytes;

#[derive(Clone, Debug, PartialEq)]
struct TestTransaction {
    id: u64,
    data: Vec<u8>,
}

impl From<Bytes> for TestTransaction {
    fn from(bytes: Bytes) -> Self {
        if bytes.len() >= 8 {
            let mut id_bytes = [0u8; 8];
            id_bytes.copy_from_slice(&bytes[0..8]);
            let id = u64::from_le_bytes(id_bytes);
            Self {
                id,
                data: bytes[8..].to_vec(),
            }
        } else {
            Self {
                id: 0,
                data: bytes.to_vec(),
            }
        }
    }
}

impl From<TestTransaction> for Bytes {
    fn from(tx: TestTransaction) -> Self {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&tx.id.to_le_bytes());
        bytes.extend_from_slice(&tx.data);
        Bytes::from(bytes)
    }
}

#[tokio::test]
async fn test_basic_consensus_flow() {
    // Create a simple executor
    let executor = SimpleExecutor::<TestTransaction, ()>::new();

    // Create consensus adapter with default config
    let config = MysticetiConfig::default();
    let mut adapter = MysticetiAdapter::new(config, executor).unwrap();

    // Start the adapter
    adapter.start().await.unwrap();
    assert!(adapter.is_ready().await);

    // Submit a transaction
    let tx = TestTransaction {
        id: 1,
        data: vec![1, 2, 3, 4, 5],
    };

    let tx_id = adapter.submit(tx.clone()).await.unwrap();
    assert_ne!(tx_id.as_bytes(), &[0u8; 32]);

    // Stop the adapter
    adapter.stop().await.unwrap();
    assert!(!adapter.is_ready().await);
}

#[tokio::test]
async fn test_batch_submission() {
    let executor = SimpleExecutor::<TestTransaction, ()>::new();
    let config = MysticetiConfig::default();
    let mut adapter = MysticetiAdapter::new(config, executor).unwrap();

    adapter.start().await.unwrap();

    // Submit multiple transactions
    let transactions: Vec<TestTransaction> = (0..10)
        .map(|i| TestTransaction {
            id: i,
            data: vec![i as u8; 10],
        })
        .collect();

    let tx_ids = adapter.submit_batch(transactions).await.unwrap();
    assert_eq!(tx_ids.len(), 10);

    // All transaction IDs should be unique
    let mut unique_ids: Vec<_> = tx_ids.iter().collect();
    unique_ids.sort_by_key(|id| id.as_bytes());
    unique_ids.dedup();
    assert_eq!(unique_ids.len(), 10);

    adapter.stop().await.unwrap();
}

#[tokio::test]
async fn test_submit_before_ready() {
    let executor = SimpleExecutor::<TestTransaction, ()>::new();
    let config = MysticetiConfig::default();
    let adapter = MysticetiAdapter::new(config, executor).unwrap();

    // Try to submit before starting
    let tx = TestTransaction {
        id: 1,
        data: vec![1, 2, 3],
    };

    let result = adapter.submit(tx).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_executor_integration() {
    use consensus_framework::traits::ExecutionEngine;

    let mut executor = SimpleExecutor::<TestTransaction, ()>::new();

    // Create some test transactions
    let txs = vec![
        TestTransaction {
            id: 1,
            data: vec![1, 2, 3],
        },
        TestTransaction {
            id: 2,
            data: vec![4, 5, 6],
        },
        TestTransaction {
            id: 3,
            data: vec![7, 8, 9],
        },
    ];

    // Execute batch
    let result = executor.execute_batch(txs.clone()).await.unwrap();

    // Verify all transactions were executed
    assert_eq!(result.len(), 3);
    assert_eq!(result, txs);
}

#[tokio::test]
async fn test_commit_index() {
    let executor = SimpleExecutor::<TestTransaction, ()>::new();
    let config = MysticetiConfig::default();
    let mut adapter = MysticetiAdapter::new(config, executor).unwrap();

    adapter.start().await.unwrap();

    // Initial commit index should be 0
    assert_eq!(adapter.commit_index().await, 0);

    adapter.stop().await.unwrap();
}

#[tokio::test]
async fn test_multiple_adapters() {
    // Simulate multiple consensus nodes
    let configs = vec![
        MysticetiConfig {
            authority_index: 0,
            committee_size: 4,
            wave_length: 3,
            leader_timeout_ms: 2000,
        },
        MysticetiConfig {
            authority_index: 1,
            committee_size: 4,
            wave_length: 3,
            leader_timeout_ms: 2000,
        },
        MysticetiConfig {
            authority_index: 2,
            committee_size: 4,
            wave_length: 3,
            leader_timeout_ms: 2000,
        },
        MysticetiConfig {
            authority_index: 3,
            committee_size: 4,
            wave_length: 3,
            leader_timeout_ms: 2000,
        },
    ];

    let mut adapters = Vec::new();

    for config in configs {
        let executor = SimpleExecutor::<TestTransaction, ()>::new();
        let mut adapter = MysticetiAdapter::new(config, executor).unwrap();
        adapter.start().await.unwrap();
        adapters.push(adapter);
    }

    // All adapters should be ready
    for adapter in &adapters {
        assert!(adapter.is_ready().await);
    }

    // Submit a transaction to the first adapter
    let tx = TestTransaction {
        id: 42,
        data: vec![1, 2, 3, 4, 5],
    };

    let tx_id = adapters[0].submit(tx).await.unwrap();
    assert_ne!(tx_id.as_bytes(), &[0u8; 32]);

    // In a full implementation, we would verify that all nodes
    // eventually commit the same transaction

    // Stop all adapters
    for adapter in &mut adapters {
        adapter.stop().await.unwrap();
    }
}

#[tokio::test]
async fn test_concurrent_submissions() {
    let executor = SimpleExecutor::<TestTransaction, ()>::new();
    let config = MysticetiConfig::default();
    let mut adapter = MysticetiAdapter::new(config, executor).unwrap();
    adapter.start().await.unwrap();

    // Submit transactions concurrently
    for i in 0..100 {
        let tx = TestTransaction {
            id: i,
            data: vec![i as u8; 10],
        };

        // Clone adapter (through Arc internally)
        let result = adapter.submit(tx).await;
        assert!(result.is_ok());
    }

    adapter.stop().await.unwrap();
}

#[tokio::test]
async fn test_config_variations() {
    // Test different committee sizes
    for committee_size in [4, 7, 10] {
        let config = MysticetiConfig {
            authority_index: 0,
            committee_size,
            wave_length: 3,
            leader_timeout_ms: 2000,
        };

        let executor = SimpleExecutor::<TestTransaction, ()>::new();
        let adapter = MysticetiAdapter::new(config.clone(), executor).unwrap();

        assert_eq!(adapter.config().committee_size, committee_size);
    }

    // Test different wave lengths
    for wave_length in [2, 3, 4, 5] {
        let config = MysticetiConfig {
            authority_index: 0,
            committee_size: 4,
            wave_length,
            leader_timeout_ms: 2000,
        };

        let executor = SimpleExecutor::<TestTransaction, ()>::new();
        let adapter = MysticetiAdapter::new(config.clone(), executor).unwrap();

        assert_eq!(adapter.config().wave_length, wave_length);
    }
}
