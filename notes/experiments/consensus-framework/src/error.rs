//! Error types for the consensus framework

use thiserror::Error;

/// Consensus-related errors
#[derive(Error, Debug)]
pub enum ConsensusError {
    #[error("Failed to submit transaction: {0}")]
    SubmitError(String),

    #[error("Consensus node not ready")]
    NotReady,

    #[error("Invalid transaction: {0}")]
    InvalidTransaction(String),

    #[error("Timeout waiting for commit")]
    Timeout,

    #[error("Internal consensus error: {0}")]
    Internal(String),

    #[error("Configuration error: {0}")]
    Configuration(String),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// Execution engine errors
#[derive(Error, Debug)]
pub enum ExecutionError {
    #[error("Transaction execution failed: {0}")]
    ExecutionFailed(String),

    #[error("Invalid state transition: {0}")]
    InvalidStateTransition(String),

    #[error("Insufficient resources: {0}")]
    InsufficientResources(String),

    #[error("State error: {0}")]
    StateError(String),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// State manager errors
#[derive(Error, Debug)]
pub enum StateError {
    #[error("Failed to create checkpoint: {0}")]
    CheckpointCreationFailed(String),

    #[error("Failed to restore checkpoint: {0}")]
    CheckpointRestoreFailed(String),

    #[error("Checkpoint not found: {0}")]
    CheckpointNotFound(String),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
