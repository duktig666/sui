//! Error types for the Token Chain

use thiserror::Error;

#[derive(Error, Debug)]
pub enum TokenChainError {
    #[error("Insufficient balance: {0}")]
    InsufficientBalance(String),

    #[error("Invalid nonce: expected {expected}, got {got}")]
    InvalidNonce { expected: u64, got: u64 },

    #[error("Account not found: {0}")]
    AccountNotFound(String),

    #[error("Invalid transaction: {0}")]
    InvalidTransaction(String),

    #[error("Execution error: {0}")]
    ExecutionError(String),

    #[error("RPC error: {0}")]
    RpcError(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Node error: {0}")]
    NodeError(String),

    #[error(transparent)]
    ConsensusError(#[from] consensus_framework::ConsensusError),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, TokenChainError>;
