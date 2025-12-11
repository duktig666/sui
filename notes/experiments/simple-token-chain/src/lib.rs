//! Simple Token Chain - A blockchain built on the Consensus Framework
//!
//! This crate implements a simple token chain using the Mysticeti consensus protocol.

pub mod error;
pub mod executor;
pub mod node;
pub mod rpc;
pub mod types;

pub use error::{Result, TokenChainError};
pub use executor::TokenExecutor;
pub use node::{NodeConfig, TokenChainNode};
pub use rpc::{RpcServerImpl, TokenChainRpcServer};
pub use types::{Address, Transaction};
