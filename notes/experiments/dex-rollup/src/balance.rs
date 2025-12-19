use crate::error::{DexError, DexResult};
use crate::types::{Balance, L1UserBalance, RollupBalance};
use dashmap::DashMap;
use std::sync::Arc;
use sui_types::base_types::SuiAddress;

#[derive(Debug, Clone)]
pub struct BalanceManager {
    l1_balances: Arc<DashMap<SuiAddress, L1UserBalance>>,
    rollup_balances: Arc<DashMap<SuiAddress, RollupBalance>>,
}

impl BalanceManager {
    pub fn new() -> Self {
        Self {
            l1_balances: Arc::new(DashMap::new()),
            rollup_balances: Arc::new(DashMap::new()),
        }
    }

    pub fn get_l1_balance(&self, user: &SuiAddress) -> L1UserBalance {
        self.l1_balances
            .get(user)
            .map(|b| b.clone())
            .unwrap_or_else(|| L1UserBalance::new(*user))
    }

    pub fn get_rollup_balance(&self, user: &SuiAddress) -> RollupBalance {
        self.rollup_balances
            .get(user)
            .map(|b| b.clone())
            .unwrap_or_else(|| RollupBalance::new(*user))
    }

    pub fn set_l1_balance(&self, balance: L1UserBalance) {
        self.l1_balances.insert(balance.user, balance);
    }

    pub fn set_rollup_balance(&self, balance: RollupBalance) {
        self.rollup_balances.insert(balance.user, balance);
    }

    pub fn deposit_to_rollup(&self, user: SuiAddress, amount: Balance) -> DexResult<()> {
        let mut l1_balance = self.get_l1_balance(&user);

        if l1_balance.available < amount {
            return Err(DexError::InsufficientBalance {
                available: l1_balance.available,
                required: amount,
            });
        }

        l1_balance.available = l1_balance.available.saturating_sub(amount);
        l1_balance.locked_in_rollup = l1_balance.locked_in_rollup.saturating_add(amount);

        let mut rollup_balance = self.get_rollup_balance(&user);
        rollup_balance.trading = rollup_balance.trading.saturating_add(amount);

        self.set_l1_balance(l1_balance);
        self.set_rollup_balance(rollup_balance);

        Ok(())
    }

    pub fn withdraw_from_rollup(&self, user: SuiAddress, amount: Balance) -> DexResult<()> {
        let mut rollup_balance = self.get_rollup_balance(&user);

        if rollup_balance.trading < amount {
            return Err(DexError::InsufficientBalance {
                available: rollup_balance.trading,
                required: amount,
            });
        }

        rollup_balance.trading = rollup_balance.trading.saturating_sub(amount);

        let mut l1_balance = self.get_l1_balance(&user);
        l1_balance.locked_in_rollup = l1_balance.locked_in_rollup.saturating_sub(amount);
        l1_balance.available = l1_balance.available.saturating_add(amount);

        self.set_rollup_balance(rollup_balance);
        self.set_l1_balance(l1_balance);

        Ok(())
    }

    pub fn freeze_balance(&self, user: SuiAddress, amount: Balance) -> DexResult<()> {
        let mut rollup_balance = self.get_rollup_balance(&user);

        if rollup_balance.trading < amount {
            return Err(DexError::InsufficientBalance {
                available: rollup_balance.trading,
                required: amount,
            });
        }

        rollup_balance.trading = rollup_balance.trading.saturating_sub(amount);
        rollup_balance.frozen_in_orders = rollup_balance.frozen_in_orders.saturating_add(amount);

        self.set_rollup_balance(rollup_balance);
        Ok(())
    }

    pub fn unfreeze_balance(&self, user: SuiAddress, amount: Balance) -> DexResult<()> {
        let mut rollup_balance = self.get_rollup_balance(&user);

        if rollup_balance.frozen_in_orders < amount {
            return Err(DexError::InsufficientBalance {
                available: rollup_balance.frozen_in_orders,
                required: amount,
            });
        }

        rollup_balance.frozen_in_orders = rollup_balance.frozen_in_orders.saturating_sub(amount);
        rollup_balance.trading = rollup_balance.trading.saturating_add(amount);

        self.set_rollup_balance(rollup_balance);
        Ok(())
    }

    pub fn transfer_frozen_to_trading(
        &self,
        from: SuiAddress,
        to: SuiAddress,
        from_amount: Balance,
        to_amount: Balance,
    ) -> DexResult<()> {
        let mut from_balance = self.get_rollup_balance(&from);
        if from_balance.frozen_in_orders < from_amount {
            return Err(DexError::InsufficientBalance {
                available: from_balance.frozen_in_orders,
                required: from_amount,
            });
        }
        from_balance.frozen_in_orders = from_balance.frozen_in_orders.saturating_sub(from_amount);
        from_balance.trading = from_balance.trading.saturating_add(to_amount);

        let mut to_balance = self.get_rollup_balance(&to);
        to_balance.trading = to_balance.trading.saturating_add(from_amount);

        self.set_rollup_balance(from_balance);
        self.set_rollup_balance(to_balance);

        Ok(())
    }

    pub fn verify_invariants(&self) -> DexResult<()> {
        for entry in self.l1_balances.iter() {
            let user = entry.key();
            let l1_balance = entry.value();
            let rollup_balance = self.get_rollup_balance(user);

            if l1_balance.locked_in_rollup != rollup_balance.total() {
                return Err(DexError::InternalError(format!(
                    "Balance invariant violation for user {}: L1.locked={}, Rollup.total={}",
                    user,
                    l1_balance.locked_in_rollup,
                    rollup_balance.total()
                )));
            }
        }
        Ok(())
    }

    pub fn all_l1_balances(&self) -> Vec<L1UserBalance> {
        self.l1_balances.iter().map(|e| e.value().clone()).collect()
    }

    pub fn all_rollup_balances(&self) -> Vec<RollupBalance> {
        self.rollup_balances
            .iter()
            .map(|e| e.value().clone())
            .collect()
    }
}

impl Default for BalanceManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sui_types::base_types::SuiAddress;

    fn test_address() -> SuiAddress {
        SuiAddress::random_for_testing_only()
    }

    #[test]
    fn test_deposit_to_rollup() {
        let manager = BalanceManager::new();
        let user = test_address();

        let mut l1_balance = L1UserBalance::new(user);
        l1_balance.available = 1000;
        manager.set_l1_balance(l1_balance);

        manager.deposit_to_rollup(user, 600).unwrap();

        let l1 = manager.get_l1_balance(&user);
        assert_eq!(l1.available, 400);
        assert_eq!(l1.locked_in_rollup, 600);

        let rollup = manager.get_rollup_balance(&user);
        assert_eq!(rollup.trading, 600);
        assert_eq!(rollup.frozen_in_orders, 0);
    }

    #[test]
    fn test_withdraw_from_rollup() {
        let manager = BalanceManager::new();
        let user = test_address();

        let mut l1_balance = L1UserBalance::new(user);
        l1_balance.available = 400;
        l1_balance.locked_in_rollup = 600;
        manager.set_l1_balance(l1_balance);

        let mut rollup_balance = RollupBalance::new(user);
        rollup_balance.trading = 600;
        manager.set_rollup_balance(rollup_balance);

        manager.withdraw_from_rollup(user, 300).unwrap();

        let l1 = manager.get_l1_balance(&user);
        assert_eq!(l1.available, 700);
        assert_eq!(l1.locked_in_rollup, 300);

        let rollup = manager.get_rollup_balance(&user);
        assert_eq!(rollup.trading, 300);
    }

    #[test]
    fn test_freeze_unfreeze() {
        let manager = BalanceManager::new();
        let user = test_address();

        let mut rollup_balance = RollupBalance::new(user);
        rollup_balance.trading = 1000;
        manager.set_rollup_balance(rollup_balance);

        manager.freeze_balance(user, 400).unwrap();

        let balance = manager.get_rollup_balance(&user);
        assert_eq!(balance.trading, 600);
        assert_eq!(balance.frozen_in_orders, 400);

        manager.unfreeze_balance(user, 200).unwrap();

        let balance = manager.get_rollup_balance(&user);
        assert_eq!(balance.trading, 800);
        assert_eq!(balance.frozen_in_orders, 200);
    }

    #[test]
    fn test_verify_invariants() {
        let manager = BalanceManager::new();
        let user = test_address();

        let mut l1_balance = L1UserBalance::new(user);
        l1_balance.available = 400;
        l1_balance.locked_in_rollup = 600;
        manager.set_l1_balance(l1_balance);

        let mut rollup_balance = RollupBalance::new(user);
        rollup_balance.trading = 400;
        rollup_balance.frozen_in_orders = 200;
        manager.set_rollup_balance(rollup_balance);

        assert!(manager.verify_invariants().is_ok());
    }
}
