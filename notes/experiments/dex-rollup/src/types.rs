use serde::{Deserialize, Serialize};
use sui_types::base_types::SuiAddress;

pub type TradingPair = (String, String);
pub type Price = u64;
pub type Quantity = u64;
pub type Balance = u64;
pub type OrderId = u64;
pub type Signature = Vec<u8>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderSide {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderStatus {
    Open,
    PartiallyFilled,
    Filled,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    pub id: OrderId,
    pub user: SuiAddress,
    pub pair: TradingPair,
    pub side: OrderSide,
    pub price: Price,
    pub quantity: Quantity,
    pub filled: Quantity,
    pub status: OrderStatus,
    pub timestamp: u64,
}

impl Order {
    pub fn new(
        id: OrderId,
        user: SuiAddress,
        pair: TradingPair,
        side: OrderSide,
        price: Price,
        quantity: Quantity,
        timestamp: u64,
    ) -> Self {
        Self {
            id,
            user,
            pair,
            side,
            price,
            quantity,
            filled: 0,
            status: OrderStatus::Open,
            timestamp,
        }
    }

    pub fn remaining(&self) -> Quantity {
        self.quantity.saturating_sub(self.filled)
    }

    pub fn is_complete(&self) -> bool {
        self.filled >= self.quantity
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trade {
    pub taker_order_id: OrderId,
    pub maker_order_id: OrderId,
    pub pair: TradingPair,
    pub price: Price,
    pub quantity: Quantity,
    pub taker: SuiAddress,
    pub maker: SuiAddress,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct L1UserBalance {
    pub user: SuiAddress,
    pub available: Balance,
    pub locked_in_rollup: Balance,
}

impl L1UserBalance {
    pub fn new(user: SuiAddress) -> Self {
        Self {
            user,
            available: 0,
            locked_in_rollup: 0,
        }
    }

    pub fn total(&self) -> Balance {
        self.available.saturating_add(self.locked_in_rollup)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollupBalance {
    pub user: SuiAddress,
    pub trading: Balance,
    pub frozen_in_orders: Balance,
}

impl RollupBalance {
    pub fn new(user: SuiAddress) -> Self {
        Self {
            user,
            trading: 0,
            frozen_in_orders: 0,
        }
    }

    pub fn total(&self) -> Balance {
        self.trading.saturating_add(self.frozen_in_orders)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DexTransaction {
    Deposit {
        user: SuiAddress,
        amount: Balance,
        nonce: u64,
    },
    Withdrawal {
        user: SuiAddress,
        amount: Balance,
        nonce: u64,
    },
    PlaceOrder {
        user: SuiAddress,
        pair: TradingPair,
        side: OrderSide,
        price: Price,
        quantity: Quantity,
        nonce: u64,
    },
    CancelOrder {
        user: SuiAddress,
        order_id: OrderId,
        nonce: u64,
    },
    SubmitBatch {
        batch: ExecutionBatch,
        sequencer_signature: Signature,
    },
    SubmitFraudProof {
        batch_index: u64,
        proof: Box<FraudProof>,
    },
}

impl DexTransaction {
    pub fn user(&self) -> Option<SuiAddress> {
        match self {
            DexTransaction::Deposit { user, .. }
            | DexTransaction::Withdrawal { user, .. }
            | DexTransaction::PlaceOrder { user, .. }
            | DexTransaction::CancelOrder { user, .. } => Some(*user),
            _ => None,
        }
    }

    pub fn nonce(&self) -> Option<u64> {
        match self {
            DexTransaction::Deposit { nonce, .. }
            | DexTransaction::Withdrawal { nonce, .. }
            | DexTransaction::PlaceOrder { nonce, .. }
            | DexTransaction::CancelOrder { nonce, .. } => Some(*nonce),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionBatch {
    pub index: u64,
    pub transactions: Vec<DexTransaction>,
    pub trades: Vec<Trade>,
    pub state_root_before: [u8; 32],
    pub state_root_after: [u8; 32],
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FraudProof {
    pub batch_index: u64,
    pub claimed_state_root: [u8; 32],
    pub correct_state_root: [u8; 32],
    pub invalid_transaction: Box<DexTransaction>,
    pub proof_data: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchOutput {
    pub batch: ExecutionBatch,
    pub new_orders: Vec<Order>,
    pub updated_orders: Vec<Order>,
    pub cancelled_orders: Vec<OrderId>,
    pub trades: Vec<Trade>,
    pub balance_updates: Vec<(SuiAddress, RollupBalance)>,
}
