//! Token Chain execution engine

use crate::types::{
    Account, Address, BatchOutput, ExecutionResult, State, StateChange, Transaction, TxHash,
};
use async_trait::async_trait;
use consensus_framework::ExecutionEngine;
use std::collections::HashMap;
use tracing::{debug, info, warn};

/// Token Chain executor
///
/// This executor maintains the state of all accounts and executes transactions.
pub struct TokenExecutor {
    state: State,
    execution_history: Vec<ExecutionResult>,
}

impl TokenExecutor {
    pub fn new() -> Self {
        Self {
            state: HashMap::new(),
            execution_history: Vec::new(),
        }
    }

    /// Execute a transfer transaction
    fn execute_transfer(
        &mut self,
        from: Address,
        to: Address,
        amount: u64,
        nonce: u64,
    ) -> ExecutionResult {
        let tx_hash = Transaction::Transfer {
            from,
            to,
            amount,
            nonce,
        }
        .hash();

        // Get or create sender account
        let from_account = self.state.entry(from).or_default();

        // Validate nonce
        if from_account.nonce != nonce {
            let error = format!(
                "Invalid nonce for {}: expected {}, got {}",
                from, from_account.nonce, nonce
            );
            warn!("{}", error);
            return ExecutionResult::failure(tx_hash, error);
        }

        // Check balance
        if from_account.balance < amount {
            let error = format!(
                "Insufficient balance for {}: has {}, needs {}",
                from, from_account.balance, amount
            );
            warn!("{}", error);
            return ExecutionResult::failure(tx_hash, error);
        }

        // Record old state
        let from_old_balance = from_account.balance;
        let from_old_nonce = from_account.nonce;

        // Update sender
        from_account.balance -= amount;
        from_account.nonce += 1;

        let from_new_balance = from_account.balance;
        let from_new_nonce = from_account.nonce;

        // Get or create receiver account
        let to_account = self.state.entry(to).or_default();
        let to_old_balance = to_account.balance;
        let to_old_nonce = to_account.nonce;

        // Update receiver
        to_account.balance += amount;

        let to_new_balance = to_account.balance;
        let to_new_nonce = to_account.nonce;

        // Record state changes
        let state_changes = vec![
            StateChange {
                address: from,
                old_balance: from_old_balance,
                new_balance: from_new_balance,
                old_nonce: from_old_nonce,
                new_nonce: from_new_nonce,
            },
            StateChange {
                address: to,
                old_balance: to_old_balance,
                new_balance: to_new_balance,
                old_nonce: to_old_nonce,
                new_nonce: to_new_nonce,
            },
        ];

        info!(
            "Transfer: {} -> {}, amount: {}, nonce: {}",
            from, to, amount, nonce
        );

        ExecutionResult::success(tx_hash, state_changes)
    }

    /// Execute a mint transaction
    fn execute_mint(&mut self, to: Address, amount: u64) -> ExecutionResult {
        let tx_hash = Transaction::Mint { to, amount }.hash();

        // Get or create account
        let account = self.state.entry(to).or_default();

        let old_balance = account.balance;
        let old_nonce = account.nonce;

        // Mint tokens
        account.balance += amount;

        let new_balance = account.balance;
        let new_nonce = account.nonce;

        let state_changes = vec![StateChange {
            address: to,
            old_balance,
            new_balance,
            old_nonce,
            new_nonce,
        }];

        info!("Mint: {} tokens to {}", amount, to);

        ExecutionResult::success(tx_hash, state_changes)
    }

    /// Get account balance
    pub fn get_balance(&self, address: &Address) -> u64 {
        self.state.get(address).map(|a| a.balance).unwrap_or(0)
    }

    /// Get account nonce
    pub fn get_nonce(&self, address: &Address) -> u64 {
        self.state.get(address).map(|a| a.nonce).unwrap_or(0)
    }

    /// Get account
    pub fn get_account(&self, address: &Address) -> Option<&Account> {
        self.state.get(address)
    }

    /// Get execution history
    pub fn get_history(&self) -> &[ExecutionResult] {
        &self.execution_history
    }

    /// Get transaction by hash
    pub fn get_transaction(&self, hash: &TxHash) -> Option<&ExecutionResult> {
        self.execution_history.iter().find(|r| r.tx_hash == *hash)
    }
}

impl Default for TokenExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ExecutionEngine for TokenExecutor {
    type Transaction = Transaction;
    type State = State;
    type Output = BatchOutput;

    async fn execute_batch(
        &mut self,
        txs: Vec<Self::Transaction>,
    ) -> std::result::Result<Self::Output, consensus_framework::ExecutionError> {
        debug!("Executing batch of {} transactions", txs.len());

        let mut results = Vec::with_capacity(txs.len());

        for tx in txs {
            let result = match tx {
                Transaction::Transfer {
                    from,
                    to,
                    amount,
                    nonce,
                } => self.execute_transfer(from, to, amount, nonce),
                Transaction::Mint { to, amount } => self.execute_mint(to, amount),
            };

            // Store in history
            self.execution_history.push(result.clone());
            results.push(result);
        }

        let output = BatchOutput::new(results);
        debug!("Batch execution complete: {} transactions", output.results.len());

        Ok(output)
    }

    fn get_state(&self) -> &Self::State {
        &self.state
    }

    fn get_state_mut(&mut self) -> &mut Self::State {
        &mut self.state
    }

    async fn validate(
        &self,
        tx: &Self::Transaction,
    ) -> std::result::Result<(), consensus_framework::ExecutionError> {
        match tx {
            Transaction::Transfer {
                from,
                to: _,
                amount,
                nonce,
            } => {
                // Check if sender account exists
                if let Some(account) = self.state.get(from) {
                    // Validate nonce
                    if account.nonce != *nonce {
                        return Err(consensus_framework::ExecutionError::ExecutionFailed(
                            format!("Invalid nonce: expected {}, got {}", account.nonce, nonce),
                        ));
                    }

                    // Check balance
                    if account.balance < *amount {
                        return Err(consensus_framework::ExecutionError::InsufficientResources(
                            format!("Insufficient balance: has {}, needs {}", account.balance, amount),
                        ));
                    }
                } else if *nonce != 0 {
                    return Err(consensus_framework::ExecutionError::ExecutionFailed(
                        format!("Account not found and nonce is not 0: {}", from),
                    ));
                }
            }
            Transaction::Mint { to: _, amount: _ } => {
                // Mint transactions are always valid (in this simplified version)
                // In production, you would check authorization
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mint() {
        let mut executor = TokenExecutor::new();

        let alice = Address::from_string("alice");
        let tx = Transaction::Mint {
            to: alice,
            amount: 1000,
        };

        let output = executor.execute_batch(vec![tx]).await.unwrap();

        assert_eq!(output.results.len(), 1);
        assert!(output.results[0].success);
        assert_eq!(executor.get_balance(&alice), 1000);
    }

    #[tokio::test]
    async fn test_transfer() {
        let mut executor = TokenExecutor::new();

        let alice = Address::from_string("alice");
        let bob = Address::from_string("bob");

        // Mint to Alice
        let mint_tx = Transaction::Mint {
            to: alice,
            amount: 1000,
        };
        executor.execute_batch(vec![mint_tx]).await.unwrap();

        // Transfer to Bob
        let transfer_tx = Transaction::Transfer {
            from: alice,
            to: bob,
            amount: 300,
            nonce: 0,
        };
        let output = executor.execute_batch(vec![transfer_tx]).await.unwrap();

        assert!(output.results[0].success);
        assert_eq!(executor.get_balance(&alice), 700);
        assert_eq!(executor.get_balance(&bob), 300);
        assert_eq!(executor.get_nonce(&alice), 1);
    }

    #[tokio::test]
    async fn test_insufficient_balance() {
        let mut executor = TokenExecutor::new();

        let alice = Address::from_string("alice");
        let bob = Address::from_string("bob");

        // Try to transfer without balance
        let transfer_tx = Transaction::Transfer {
            from: alice,
            to: bob,
            amount: 100,
            nonce: 0,
        };
        let output = executor.execute_batch(vec![transfer_tx]).await.unwrap();

        assert!(!output.results[0].success);
        assert!(output.results[0]
            .error
            .as_ref()
            .unwrap()
            .contains("Insufficient balance"));
    }

    #[tokio::test]
    async fn test_invalid_nonce() {
        let mut executor = TokenExecutor::new();

        let alice = Address::from_string("alice");
        let bob = Address::from_string("bob");

        // Mint to Alice
        let mint_tx = Transaction::Mint {
            to: alice,
            amount: 1000,
        };
        executor.execute_batch(vec![mint_tx]).await.unwrap();

        // Try transfer with wrong nonce
        let transfer_tx = Transaction::Transfer {
            from: alice,
            to: bob,
            amount: 100,
            nonce: 5, // Wrong nonce
        };
        let output = executor.execute_batch(vec![transfer_tx]).await.unwrap();

        assert!(!output.results[0].success);
        assert!(output.results[0]
            .error
            .as_ref()
            .unwrap()
            .contains("Invalid nonce"));
    }

    #[tokio::test]
    async fn test_batch_execution() {
        let mut executor = TokenExecutor::new();

        let alice = Address::from_string("alice");
        let bob = Address::from_string("bob");
        let charlie = Address::from_string("charlie");

        let txs = vec![
            Transaction::Mint {
                to: alice,
                amount: 1000,
            },
            Transaction::Transfer {
                from: alice,
                to: bob,
                amount: 300,
                nonce: 0,
            },
            Transaction::Transfer {
                from: alice,
                to: charlie,
                amount: 200,
                nonce: 1,
            },
        ];

        let output = executor.execute_batch(txs).await.unwrap();

        assert_eq!(output.results.len(), 3);
        assert!(output.results.iter().all(|r| r.success));
        assert_eq!(executor.get_balance(&alice), 500);
        assert_eq!(executor.get_balance(&bob), 300);
        assert_eq!(executor.get_balance(&charlie), 200);
    }
}
