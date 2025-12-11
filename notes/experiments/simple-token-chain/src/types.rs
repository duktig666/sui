//! Core type definitions for the Token Chain

use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

/// Account address (32 bytes)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Address(pub [u8; 32]);

impl Address {
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn from_slice(slice: &[u8]) -> Result<Self, String> {
        if slice.len() != 32 {
            return Err(format!("Invalid address length: {}", slice.len()));
        }
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(slice);
        Ok(Self(bytes))
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Create address from a string (for testing)
    pub fn from_string(s: &str) -> Self {
        let mut bytes = [0u8; 32];
        let input_bytes = s.as_bytes();
        let len = input_bytes.len().min(32);
        bytes[..len].copy_from_slice(&input_bytes[..len]);
        Self(bytes)
    }
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{}", hex::encode(&self.0[..8]))
    }
}

/// Transaction types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Transaction {
    /// Transfer tokens from one account to another
    Transfer {
        from: Address,
        to: Address,
        amount: u64,
        nonce: u64,
    },
    /// Mint new tokens (for testing/initialization)
    Mint { to: Address, amount: u64 },
}

impl Transaction {
    /// Get the transaction hash
    pub fn hash(&self) -> TxHash {
        let bytes = bincode::serialize(self).unwrap();
        let hash = blake3::hash(&bytes);
        TxHash(hash.as_bytes().to_vec())
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> Bytes {
        Bytes::from(bincode::serialize(self).unwrap())
    }

    /// Deserialize from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        bincode::deserialize(bytes).map_err(|e| e.to_string())
    }
}

impl From<Bytes> for Transaction {
    fn from(bytes: Bytes) -> Self {
        bincode::deserialize(&bytes).expect("Failed to deserialize transaction")
    }
}

impl From<Transaction> for Bytes {
    fn from(tx: Transaction) -> Self {
        tx.to_bytes()
    }
}

/// Transaction hash
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TxHash(pub Vec<u8>);

impl TxHash {
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Display for TxHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{}", hex::encode(&self.0[..8]))
    }
}

/// Account state
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Account {
    pub balance: u64,
    pub nonce: u64,
}

impl Account {
    pub fn new(balance: u64) -> Self {
        Self { balance, nonce: 0 }
    }
}

/// Chain state (all accounts)
pub type State = HashMap<Address, Account>;

/// State change record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateChange {
    pub address: Address,
    pub old_balance: u64,
    pub new_balance: u64,
    pub old_nonce: u64,
    pub new_nonce: u64,
}

/// Transaction execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    pub tx_hash: TxHash,
    pub success: bool,
    pub error: Option<String>,
    pub state_changes: Vec<StateChange>,
    pub gas_used: u64,
}

impl ExecutionResult {
    pub fn success(tx_hash: TxHash, state_changes: Vec<StateChange>) -> Self {
        Self {
            tx_hash,
            success: true,
            error: None,
            state_changes,
            gas_used: 0, // Simplified: no gas accounting yet
        }
    }

    pub fn failure(tx_hash: TxHash, error: String) -> Self {
        Self {
            tx_hash,
            success: false,
            error: Some(error),
            state_changes: Vec::new(),
            gas_used: 0,
        }
    }
}

/// Batch execution output
#[derive(Debug, Clone)]
pub struct BatchOutput {
    pub results: Vec<ExecutionResult>,
    pub total_gas_used: u64,
}

impl BatchOutput {
    pub fn new(results: Vec<ExecutionResult>) -> Self {
        let total_gas_used = results.iter().map(|r| r.gas_used).sum();
        Self {
            results,
            total_gas_used,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_address_creation() {
        let addr = Address::from_string("alice");
        assert_eq!(addr.0[0], b'a');
        assert_eq!(addr.0[1], b'l');
    }

    #[test]
    fn test_transaction_hash() {
        let tx = Transaction::Mint {
            to: Address::from_string("alice"),
            amount: 1000,
        };

        let hash1 = tx.hash();
        let hash2 = tx.hash();

        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_transaction_serialization() {
        let tx = Transaction::Transfer {
            from: Address::from_string("alice"),
            to: Address::from_string("bob"),
            amount: 100,
            nonce: 1,
        };

        let bytes = tx.to_bytes();
        let tx2 = Transaction::from_bytes(&bytes).unwrap();

        assert_eq!(tx, tx2);
    }

    #[test]
    fn test_account() {
        let mut account = Account::new(1000);
        assert_eq!(account.balance, 1000);
        assert_eq!(account.nonce, 0);

        account.nonce += 1;
        assert_eq!(account.nonce, 1);
    }
}
