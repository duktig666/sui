use thiserror::Error;

#[derive(Error, Debug, Clone)]
pub enum DexError {
    #[error("Insufficient balance: available={available}, required={required}")]
    InsufficientBalance { available: u64, required: u64 },

    #[error("Order not found: {0}")]
    OrderNotFound(u64),

    #[error("Invalid order: {0}")]
    InvalidOrder(String),

    #[error("Invalid price: {0}")]
    InvalidPrice(u64),

    #[error("Invalid quantity: {0}")]
    InvalidQuantity(u64),

    #[error("Trading pair not supported: {0:?}")]
    UnsupportedTradingPair(String),

    #[error("User not found: {0}")]
    UserNotFound(String),

    #[error("Invalid nonce: expected={expected}, got={got}")]
    InvalidNonce { expected: u64, got: u64 },

    #[error("Sequencer signature verification failed")]
    InvalidSequencerSignature,

    #[error("Fraud proof verification failed: {0}")]
    InvalidFraudProof(String),

    #[error("State root mismatch: expected={expected}, got={got}")]
    StateRootMismatch { expected: String, got: String },

    #[error("Batch already processed: {0}")]
    BatchAlreadyProcessed(u64),

    #[error("Invalid batch index: expected={expected}, got={got}")]
    InvalidBatchIndex { expected: u64, got: u64 },

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Internal error: {0}")]
    InternalError(String),
}

impl From<bincode::Error> for DexError {
    fn from(e: bincode::Error) -> Self {
        DexError::SerializationError(e.to_string())
    }
}

pub type DexResult<T> = Result<T, DexError>;
