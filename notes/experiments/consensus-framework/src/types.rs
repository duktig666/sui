//! Core type definitions for the consensus framework

use serde::{Deserialize, Serialize};
use std::fmt;

/// Transaction identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TxId(pub [u8; 32]);

impl TxId {
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for TxId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex::encode(self.0))
    }
}

/// Block identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlockId(pub [u8; 32]);

impl BlockId {
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for BlockId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex::encode(self.0))
    }
}

/// Represents the output of committed consensus operations
#[derive(Debug, Clone)]
pub struct CommittedOutput<T> {
    /// The committed data
    pub data: T,
    /// Block ID where this was committed
    pub block_id: BlockId,
    /// Commit index (monotonically increasing)
    pub commit_index: u64,
}

impl<T> CommittedOutput<T> {
    pub fn new(data: T, block_id: BlockId, commit_index: u64) -> Self {
        Self {
            data,
            block_id,
            commit_index,
        }
    }
}
