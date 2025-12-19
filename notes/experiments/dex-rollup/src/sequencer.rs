use crate::balance::BalanceManager;
use crate::error::{DexError, DexResult};
use crate::orderbook::OrderBookManager;
use crate::types::{
    BatchOutput, DexTransaction, ExecutionBatch, Order, OrderSide,
};
use dashmap::DashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use sui_types::base_types::SuiAddress;
use tokio::sync::RwLock;

#[derive(Debug, Clone)]
pub struct DexSequencer {
    balance_manager: Arc<BalanceManager>,
    orderbook_manager: Arc<OrderBookManager>,
    user_nonces: Arc<DashMap<SuiAddress, u64>>,
    batch_index: Arc<AtomicU64>,
    pending_transactions: Arc<RwLock<Vec<DexTransaction>>>,
}

impl DexSequencer {
    pub fn new() -> Self {
        Self {
            balance_manager: Arc::new(BalanceManager::new()),
            orderbook_manager: Arc::new(OrderBookManager::new()),
            user_nonces: Arc::new(DashMap::new()),
            batch_index: Arc::new(AtomicU64::new(0)),
            pending_transactions: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub fn balance_manager(&self) -> &BalanceManager {
        &self.balance_manager
    }

    pub fn orderbook_manager(&self) -> &OrderBookManager {
        &self.orderbook_manager
    }

    pub async fn submit_transaction(&self, tx: DexTransaction) -> DexResult<()> {
        if let Some(user) = tx.user()
            && let Some(nonce) = tx.nonce()
        {
            let expected_nonce = self.user_nonces.get(&user).map(|n| *n).unwrap_or(0);
            if nonce != expected_nonce {
                return Err(DexError::InvalidNonce {
                    expected: expected_nonce,
                    got: nonce,
                });
            }
        }

        let mut pending = self.pending_transactions.write().await;
        pending.push(tx);
        Ok(())
    }

    pub async fn execute_batch(&self) -> DexResult<BatchOutput> {
        let mut pending = self.pending_transactions.write().await;
        let transactions = std::mem::take(&mut *pending);
        drop(pending);

        let batch_index = self.batch_index.fetch_add(1, Ordering::SeqCst);
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let state_root_before = self.compute_state_root();

        let mut new_orders = Vec::new();
        let mut updated_orders = Vec::new();
        let mut cancelled_orders = Vec::new();
        let mut all_trades = Vec::new();
        let mut balance_updates = Vec::new();

        for tx in &transactions {
            match tx {
                DexTransaction::Deposit { user, amount, .. } => {
                    self.balance_manager
                        .deposit_to_rollup(*user, *amount)?;
                    balance_updates.push((*user, self.balance_manager.get_rollup_balance(user)));

                    if let Some(nonce) = tx.nonce() {
                        self.user_nonces.insert(*user, nonce + 1);
                    }
                }
                DexTransaction::Withdrawal { user, amount, .. } => {
                    self.balance_manager
                        .withdraw_from_rollup(*user, *amount)?;
                    balance_updates.push((*user, self.balance_manager.get_rollup_balance(user)));

                    if let Some(nonce) = tx.nonce() {
                        self.user_nonces.insert(*user, nonce + 1);
                    }
                }
                DexTransaction::PlaceOrder {
                    user,
                    pair,
                    side,
                    price,
                    quantity,
                    ..
                } => {
                    let order_id = self
                        .orderbook_manager
                        .get_or_create_orderbook(pair.clone())
                        .await
                        .read()
                        .await
                        .next_order_id();

                    let required_balance = match side {
                        OrderSide::Buy => price.saturating_mul(*quantity),
                        OrderSide::Sell => *quantity,
                    };

                    self.balance_manager
                        .freeze_balance(*user, required_balance)?;

                    let order = Order::new(
                        order_id,
                        *user,
                        pair.clone(),
                        *side,
                        *price,
                        *quantity,
                        timestamp,
                    );

                    let trades = self.orderbook_manager.add_order(order.clone()).await?;

                    for trade in &trades {
                        let buyer = if trade.taker == *user && *side == OrderSide::Buy {
                            *user
                        } else {
                            trade.maker
                        };
                        let seller = if trade.taker == *user && *side == OrderSide::Sell {
                            *user
                        } else {
                            trade.maker
                        };

                        let total_cost = trade.price.saturating_mul(trade.quantity);

                        self.balance_manager.transfer_frozen_to_trading(
                            buyer,
                            seller,
                            total_cost,
                            0,
                        )?;
                        self.balance_manager.transfer_frozen_to_trading(
                            seller,
                            buyer,
                            trade.quantity,
                            0,
                        )?;
                    }

                    if order.remaining() > 0 {
                        new_orders.push(order.clone());
                    } else if order.filled > 0 {
                        updated_orders.push(order.clone());
                    }

                    all_trades.extend(trades);
                    balance_updates.push((*user, self.balance_manager.get_rollup_balance(user)));

                    if let Some(nonce) = tx.nonce() {
                        self.user_nonces.insert(*user, nonce + 1);
                    }
                }
                DexTransaction::CancelOrder { user, order_id, .. } => {
                    let order = self
                        .orderbook_manager
                        .get_order(("".to_string(), "".to_string()), *order_id)
                        .await
                        .ok_or(DexError::OrderNotFound(*order_id))?;

                    if order.user != *user {
                        return Err(DexError::InvalidOrder(
                            "User does not own this order".to_string(),
                        ));
                    }

                    let cancelled_order = self
                        .orderbook_manager
                        .cancel_order(order.pair.clone(), *order_id)
                        .await?;

                    let remaining = cancelled_order.remaining();
                    if remaining > 0 {
                        let frozen_amount = match cancelled_order.side {
                            OrderSide::Buy => cancelled_order.price.saturating_mul(remaining),
                            OrderSide::Sell => remaining,
                        };
                        self.balance_manager.unfreeze_balance(*user, frozen_amount)?;
                    }

                    cancelled_orders.push(*order_id);
                    balance_updates.push((*user, self.balance_manager.get_rollup_balance(user)));

                    if let Some(nonce) = tx.nonce() {
                        self.user_nonces.insert(*user, nonce + 1);
                    }
                }
                DexTransaction::SubmitBatch { .. } | DexTransaction::SubmitFraudProof { .. } => {
                }
            }
        }

        let state_root_after = self.compute_state_root();

        let batch = ExecutionBatch {
            index: batch_index,
            transactions: transactions.clone(),
            trades: all_trades.clone(),
            state_root_before,
            state_root_after,
            timestamp,
        };

        Ok(BatchOutput {
            batch,
            new_orders,
            updated_orders,
            cancelled_orders,
            trades: all_trades,
            balance_updates,
        })
    }

    fn compute_state_root(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();

        let mut l1_balances = self.balance_manager.all_l1_balances();
        l1_balances.sort_by_key(|b| b.user);
        for balance in l1_balances {
            hasher.update(&bincode::serialize(&balance).unwrap());
        }

        let mut rollup_balances = self.balance_manager.all_rollup_balances();
        rollup_balances.sort_by_key(|b| b.user);
        for balance in rollup_balances {
            hasher.update(&bincode::serialize(&balance).unwrap());
        }

        *hasher.finalize().as_bytes()
    }

    pub async fn get_pending_count(&self) -> usize {
        self.pending_transactions.read().await.len()
    }
}

impl Default for DexSequencer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{L1UserBalance, OrderSide};

    fn test_address() -> SuiAddress {
        SuiAddress::random_for_testing_only()
    }

    fn test_pair() -> (String, String) {
        ("SUI".to_string(), "USDC".to_string())
    }

    #[tokio::test]
    async fn test_deposit() {
        let sequencer = DexSequencer::new();
        let user = test_address();

        let mut l1_balance = L1UserBalance::new(user);
        l1_balance.available = 1000;
        sequencer.balance_manager.set_l1_balance(l1_balance);

        let tx = DexTransaction::Deposit {
            user,
            amount: 500,
            nonce: 0,
        };

        sequencer.submit_transaction(tx).await.unwrap();
        let output = sequencer.execute_batch().await.unwrap();

        assert_eq!(output.balance_updates.len(), 1);
        let l1 = sequencer.balance_manager.get_l1_balance(&user);
        assert_eq!(l1.available, 500);
        assert_eq!(l1.locked_in_rollup, 500);
    }

    #[tokio::test]
    async fn test_place_order() {
        let sequencer = DexSequencer::new();
        let user = test_address();

        let mut l1_balance = L1UserBalance::new(user);
        l1_balance.available = 1000;
        l1_balance.locked_in_rollup = 1000;
        sequencer.balance_manager.set_l1_balance(l1_balance);

        let mut rollup_balance = crate::types::RollupBalance::new(user);
        rollup_balance.trading = 1000;
        sequencer.balance_manager.set_rollup_balance(rollup_balance);

        let tx = DexTransaction::PlaceOrder {
            user,
            pair: test_pair(),
            side: OrderSide::Buy,
            price: 100,
            quantity: 5,
            nonce: 0,
        };

        sequencer.submit_transaction(tx).await.unwrap();
        let output = sequencer.execute_batch().await.unwrap();

        assert_eq!(output.new_orders.len(), 1);
        let order = &output.new_orders[0];
        assert_eq!(order.price, 100);
        assert_eq!(order.quantity, 5);
    }
}
