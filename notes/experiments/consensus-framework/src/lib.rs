//! Consensus Framework - A reusable consensus abstraction layer
//!
//! This crate provides a generic framework for building blockchain applications
//! using the Mysticeti consensus protocol, decoupled from Sui-specific logic.

pub mod traits;
pub mod types;
pub mod error;
pub mod mysticeti_adapter;

pub use traits::{ConsensusProtocol, ExecutionEngine, StateManager};
pub use types::{TxId, BlockId, CommittedOutput};
pub use error::{ConsensusError, ExecutionError};
