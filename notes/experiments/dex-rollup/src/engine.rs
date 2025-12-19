use crate::error::{DexError, DexResult};
use crate::sequencer::DexSequencer;
use crate::types::{BatchOutput, DexTransaction};
use async_trait::async_trait;
use consensus_framework::error::ExecutionError;
use consensus_framework::traits::ExecutionEngine;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DexState {
    pub last_batch_index: u64,
    pub last_state_root: [u8; 32],
    pub total_transactions: u64,
    pub total_trades: u64,
}

impl DexState {
    pub fn new() -> Self {
        Self {
            last_batch_index: 0,
            last_state_root: [0u8; 32],
            total_transactions: 0,
            total_trades: 0,
        }
    }
}

impl Default for DexState {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub struct RollupExecutionEngine {
    sequencer: Arc<DexSequencer>,
    state: Arc<RwLock<DexState>>,
    cached_state: parking_lot::RwLock<DexState>,
}

impl RollupExecutionEngine {
    pub fn new() -> Self {
        Self {
            sequencer: Arc::new(DexSequencer::new()),
            state: Arc::new(RwLock::new(DexState::new())),
            cached_state: parking_lot::RwLock::new(DexState::new()),
        }
    }

    pub fn sequencer(&self) -> &DexSequencer {
        &self.sequencer
    }

    pub async fn get_state(&self) -> DexState {
        self.state.read().await.clone()
    }

    fn update_cached_state(&self, new_state: DexState) {
        *self.cached_state.write() = new_state;
    }

    async fn execute_single_transaction(&self, tx: DexTransaction) -> DexResult<BatchOutput> {
        match &tx {
            DexTransaction::SubmitBatch {
                batch,
                sequencer_signature,
            } => self.verify_and_execute_batch(batch.clone(), sequencer_signature.clone()).await,
            DexTransaction::SubmitFraudProof { batch_index, proof } => {
                self.handle_fraud_proof(*batch_index, *proof.clone()).await
            }
            _ => {
                self.sequencer.submit_transaction(tx.clone()).await?;
                let output = self.sequencer.execute_batch().await?;

                let mut state = self.state.write().await;
                state.total_transactions += 1;
                state.total_trades += output.trades.len() as u64;
                self.update_cached_state(state.clone());

                Ok(output)
            }
        }
    }

    async fn verify_and_execute_batch(
        &self,
        batch: crate::types::ExecutionBatch,
        _sequencer_signature: Vec<u8>,
    ) -> DexResult<BatchOutput> {
        let state = self.state.read().await;
        let expected_index = state.last_batch_index + 1;

        if batch.index != expected_index {
            return Err(DexError::InvalidBatchIndex {
                expected: expected_index,
                got: batch.index,
            });
        }

        drop(state);

        for tx in &batch.transactions {
            self.sequencer.submit_transaction(tx.clone()).await?;
        }

        let output = self.sequencer.execute_batch().await?;

        let mut state = self.state.write().await;
        state.last_batch_index = batch.index;
        state.last_state_root = batch.state_root_after;
        state.total_transactions += batch.transactions.len() as u64;
        state.total_trades += batch.trades.len() as u64;

        self.update_cached_state(state.clone());

        Ok(output)
    }

    async fn handle_fraud_proof(
        &self,
        _batch_index: u64,
        _proof: crate::types::FraudProof,
    ) -> DexResult<BatchOutput> {
        Err(DexError::InternalError(
            "Fraud proof handling not yet implemented".to_string(),
        ))
    }
}

impl Default for RollupExecutionEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ExecutionEngine for RollupExecutionEngine {
    type Transaction = DexTransaction;
    type State = DexState;
    type Output = BatchOutput;

    async fn execute_batch(
        &mut self,
        txs: Vec<Self::Transaction>,
    ) -> Result<Self::Output, ExecutionError> {
        if txs.is_empty() {
            return Err(ExecutionError::ExecutionFailed(
                "Empty transaction batch".to_string(),
            ));
        }

        let mut final_output: Option<BatchOutput> = None;

        for tx in txs {
            let output = self
                .execute_single_transaction(tx)
                .await
                .map_err(|e| ExecutionError::ExecutionFailed(e.to_string()))?;

            final_output = Some(output);
        }

        final_output.ok_or_else(|| {
            ExecutionError::ExecutionFailed("No output produced".to_string())
        })
    }

    fn get_state(&self) -> &Self::State {
        unsafe { &*self.cached_state.data_ptr() }
    }

    fn get_state_mut(&mut self) -> &mut Self::State {
        self.cached_state.get_mut()
    }

    async fn validate(&self, tx: &Self::Transaction) -> Result<(), ExecutionError> {
        match tx {
            DexTransaction::Deposit { amount, .. } | DexTransaction::Withdrawal { amount, .. } => {
                if *amount == 0 {
                    return Err(ExecutionError::ExecutionFailed(
                        "Amount cannot be zero".to_string(),
                    ));
                }
            }
            DexTransaction::PlaceOrder {
                price, quantity, ..
            } => {
                if *price == 0 {
                    return Err(ExecutionError::ExecutionFailed(
                        "Price cannot be zero".to_string(),
                    ));
                }
                if *quantity == 0 {
                    return Err(ExecutionError::ExecutionFailed(
                        "Quantity cannot be zero".to_string(),
                    ));
                }
            }
            DexTransaction::CancelOrder { .. } => {}
            DexTransaction::SubmitBatch { .. } => {}
            DexTransaction::SubmitFraudProof { .. } => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{L1UserBalance, OrderSide};
    use sui_types::base_types::SuiAddress;

    fn test_address() -> SuiAddress {
        SuiAddress::random_for_testing_only()
    }

    #[tokio::test]
    async fn test_execute_deposit() {
        let mut engine = RollupExecutionEngine::new();
        let user = test_address();

        let mut l1_balance = L1UserBalance::new(user);
        l1_balance.available = 1000;
        engine.sequencer.balance_manager().set_l1_balance(l1_balance);

        let tx = DexTransaction::Deposit {
            user,
            amount: 500,
            nonce: 0,
        };

        let output = engine.execute_batch(vec![tx]).await.unwrap();
        assert_eq!(output.balance_updates.len(), 1);

        let state = engine.get_state().await;
        assert_eq!(state.total_transactions, 1);
    }

    #[tokio::test]
    async fn test_execute_place_order() {
        let mut engine = RollupExecutionEngine::new();
        let user = test_address();

        let mut l1_balance = L1UserBalance::new(user);
        l1_balance.available = 1000;
        l1_balance.locked_in_rollup = 1000;
        engine.sequencer.balance_manager().set_l1_balance(l1_balance);

        let mut rollup_balance = crate::types::RollupBalance::new(user);
        rollup_balance.trading = 1000;
        engine
            .sequencer
            .balance_manager()
            .set_rollup_balance(rollup_balance);

        let tx = DexTransaction::PlaceOrder {
            user,
            pair: ("SUI".to_string(), "USDC".to_string()),
            side: OrderSide::Buy,
            price: 100,
            quantity: 5,
            nonce: 0,
        };

        let output = engine.execute_batch(vec![tx]).await.unwrap();
        assert_eq!(output.new_orders.len(), 1);

        let state = engine.get_state().await;
        assert_eq!(state.total_transactions, 1);
    }

    #[tokio::test]
    async fn test_validate_transaction() {
        let engine = RollupExecutionEngine::new();
        let user = test_address();

        let valid_tx = DexTransaction::Deposit {
            user,
            amount: 500,
            nonce: 0,
        };
        assert!(engine.validate(&valid_tx).await.is_ok());

        let invalid_tx = DexTransaction::Deposit {
            user,
            amount: 0,
            nonce: 0,
        };
        assert!(engine.validate(&invalid_tx).await.is_err());
    }
}
