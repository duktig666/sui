use crate::error::{DexError, DexResult};
use crate::types::{Order, OrderId, OrderSide, OrderStatus, Price, Trade, TradingPair};
use dashmap::DashMap;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use sui_types::base_types::SuiAddress;

#[derive(Debug)]
pub struct OrderBook {
    pair: TradingPair,
    buy_orders: BTreeMap<Price, Vec<Order>>,
    sell_orders: BTreeMap<Price, Vec<Order>>,
    orders_by_id: DashMap<OrderId, Order>,
    next_order_id: AtomicU64,
}

impl OrderBook {
    pub fn new(pair: TradingPair) -> Self {
        Self {
            pair,
            buy_orders: BTreeMap::new(),
            sell_orders: BTreeMap::new(),
            orders_by_id: DashMap::new(),
            next_order_id: AtomicU64::new(1),
        }
    }

    pub fn next_order_id(&self) -> OrderId {
        self.next_order_id.fetch_add(1, Ordering::SeqCst)
    }

    pub fn add_order(&mut self, mut order: Order) -> DexResult<Vec<Trade>> {
        if order.pair != self.pair {
            return Err(DexError::UnsupportedTradingPair(format!(
                "{:?}",
                order.pair
            )));
        }

        if order.price == 0 {
            return Err(DexError::InvalidPrice(order.price));
        }

        if order.quantity == 0 {
            return Err(DexError::InvalidQuantity(order.quantity));
        }

        let mut trades = Vec::new();

        match order.side {
            OrderSide::Buy => {
                while order.remaining() > 0 {
                    if let Some(best_sell_price) = self.sell_orders.keys().next().copied() {
                        if best_sell_price <= order.price {
                            let sell_orders = self.sell_orders.get_mut(&best_sell_price).unwrap();
                            if let Some(mut maker_order) = sell_orders.first().cloned() {
                                let trade_quantity =
                                    order.remaining().min(maker_order.remaining());

                                order.filled += trade_quantity;
                                maker_order.filled += trade_quantity;

                                if maker_order.is_complete() {
                                    maker_order.status = OrderStatus::Filled;
                                    sell_orders.remove(0);
                                    if sell_orders.is_empty() {
                                        self.sell_orders.remove(&best_sell_price);
                                    }
                                } else {
                                    maker_order.status = OrderStatus::PartiallyFilled;
                                    sell_orders[0] = maker_order.clone();
                                }

                                self.orders_by_id.insert(maker_order.id, maker_order.clone());

                                let trade = Trade {
                                    taker_order_id: order.id,
                                    maker_order_id: maker_order.id,
                                    pair: self.pair.clone(),
                                    price: maker_order.price,
                                    quantity: trade_quantity,
                                    taker: order.user,
                                    maker: maker_order.user,
                                    timestamp: order.timestamp,
                                };

                                trades.push(trade);
                            } else {
                                break;
                            }
                        } else {
                            break;
                        }
                    } else {
                        break;
                    }
                }
            }
            OrderSide::Sell => {
                while order.remaining() > 0 {
                    if let Some(best_buy_price) = self.buy_orders.keys().next_back().copied() {
                        if best_buy_price >= order.price {
                            let buy_orders = self.buy_orders.get_mut(&best_buy_price).unwrap();
                            if let Some(mut maker_order) = buy_orders.first().cloned() {
                                let trade_quantity =
                                    order.remaining().min(maker_order.remaining());

                                order.filled += trade_quantity;
                                maker_order.filled += trade_quantity;

                                if maker_order.is_complete() {
                                    maker_order.status = OrderStatus::Filled;
                                    buy_orders.remove(0);
                                    if buy_orders.is_empty() {
                                        self.buy_orders.remove(&best_buy_price);
                                    }
                                } else {
                                    maker_order.status = OrderStatus::PartiallyFilled;
                                    buy_orders[0] = maker_order.clone();
                                }

                                self.orders_by_id.insert(maker_order.id, maker_order.clone());

                                let trade = Trade {
                                    taker_order_id: order.id,
                                    maker_order_id: maker_order.id,
                                    pair: self.pair.clone(),
                                    price: maker_order.price,
                                    quantity: trade_quantity,
                                    taker: order.user,
                                    maker: maker_order.user,
                                    timestamp: order.timestamp,
                                };

                                trades.push(trade);
                            } else {
                                break;
                            }
                        } else {
                            break;
                        }
                    } else {
                        break;
                    }
                }
            }
        }

        if order.is_complete() {
            order.status = OrderStatus::Filled;
        } else if order.filled > 0 {
            order.status = OrderStatus::PartiallyFilled;
        }

        if order.remaining() > 0 {
            match order.side {
                OrderSide::Buy => {
                    self.buy_orders
                        .entry(order.price)
                        .or_default()
                        .push(order.clone());
                }
                OrderSide::Sell => {
                    self.sell_orders
                        .entry(order.price)
                        .or_default()
                        .push(order.clone());
                }
            }
        }

        self.orders_by_id.insert(order.id, order);

        Ok(trades)
    }

    pub fn cancel_order(&mut self, order_id: OrderId) -> DexResult<Order> {
        let order = self
            .orders_by_id
            .get(&order_id)
            .ok_or(DexError::OrderNotFound(order_id))?
            .clone();

        if order.status == OrderStatus::Filled || order.status == OrderStatus::Cancelled {
            return Err(DexError::InvalidOrder(format!(
                "Order {} is already {:?}",
                order_id, order.status
            )));
        }

        let orders = match order.side {
            OrderSide::Buy => self.buy_orders.get_mut(&order.price),
            OrderSide::Sell => self.sell_orders.get_mut(&order.price),
        };

        if let Some(orders) = orders
            && let Some(pos) = orders.iter().position(|o| o.id == order_id)
        {
            orders.remove(pos);
            if orders.is_empty() {
                match order.side {
                    OrderSide::Buy => {
                        self.buy_orders.remove(&order.price);
                    }
                    OrderSide::Sell => {
                        self.sell_orders.remove(&order.price);
                    }
                }
            }
        }

        let mut cancelled_order = order.clone();
        cancelled_order.status = OrderStatus::Cancelled;
        self.orders_by_id.insert(order_id, cancelled_order.clone());

        Ok(cancelled_order)
    }

    pub fn get_order(&self, order_id: OrderId) -> Option<Order> {
        self.orders_by_id.get(&order_id).map(|o| o.clone())
    }

    pub fn best_bid(&self) -> Option<Price> {
        self.buy_orders.keys().next_back().copied()
    }

    pub fn best_ask(&self) -> Option<Price> {
        self.sell_orders.keys().next().copied()
    }

    pub fn get_orders_by_user(&self, user: &SuiAddress) -> Vec<Order> {
        self.orders_by_id
            .iter()
            .filter(|entry| entry.value().user == *user)
            .map(|entry| entry.value().clone())
            .collect()
    }
}

#[derive(Debug)]
pub struct OrderBookManager {
    orderbooks: Arc<DashMap<TradingPair, Arc<tokio::sync::RwLock<OrderBook>>>>,
}

impl OrderBookManager {
    pub fn new() -> Self {
        Self {
            orderbooks: Arc::new(DashMap::new()),
        }
    }

    pub async fn get_or_create_orderbook(&self, pair: TradingPair) -> Arc<tokio::sync::RwLock<OrderBook>> {
        self.orderbooks
            .entry(pair.clone())
            .or_insert_with(|| Arc::new(tokio::sync::RwLock::new(OrderBook::new(pair))))
            .clone()
    }

    pub async fn add_order(&self, order: Order) -> DexResult<Vec<Trade>> {
        let orderbook = self.get_or_create_orderbook(order.pair.clone()).await;
        let mut ob = orderbook.write().await;
        ob.add_order(order)
    }

    pub async fn cancel_order(&self, pair: TradingPair, order_id: OrderId) -> DexResult<Order> {
        let orderbook = self.get_or_create_orderbook(pair).await;
        let mut ob = orderbook.write().await;
        ob.cancel_order(order_id)
    }

    pub async fn get_order(&self, pair: TradingPair, order_id: OrderId) -> Option<Order> {
        let orderbook = self.get_or_create_orderbook(pair).await;
        let ob = orderbook.read().await;
        ob.get_order(order_id)
    }
}

impl Default for OrderBookManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_pair() -> TradingPair {
        ("SUI".to_string(), "USDC".to_string())
    }

    fn test_address() -> SuiAddress {
        SuiAddress::random_for_testing_only()
    }

    #[test]
    fn test_add_buy_order() {
        let mut orderbook = OrderBook::new(test_pair());
        let order = Order::new(1, test_address(), test_pair(), OrderSide::Buy, 100, 10, 0);

        let trades = orderbook.add_order(order).unwrap();
        assert!(trades.is_empty());
        assert_eq!(orderbook.best_bid(), Some(100));
    }

    #[test]
    fn test_add_sell_order() {
        let mut orderbook = OrderBook::new(test_pair());
        let order = Order::new(1, test_address(), test_pair(), OrderSide::Sell, 100, 10, 0);

        let trades = orderbook.add_order(order).unwrap();
        assert!(trades.is_empty());
        assert_eq!(orderbook.best_ask(), Some(100));
    }

    #[test]
    fn test_match_orders() {
        let mut orderbook = OrderBook::new(test_pair());

        let sell_order = Order::new(
            1,
            test_address(),
            test_pair(),
            OrderSide::Sell,
            100,
            10,
            0,
        );
        orderbook.add_order(sell_order).unwrap();

        let buy_order = Order::new(2, test_address(), test_pair(), OrderSide::Buy, 100, 10, 1);
        let trades = orderbook.add_order(buy_order).unwrap();

        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].quantity, 10);
        assert_eq!(trades[0].price, 100);
        assert_eq!(orderbook.best_bid(), None);
        assert_eq!(orderbook.best_ask(), None);
    }

    #[test]
    fn test_partial_match() {
        let mut orderbook = OrderBook::new(test_pair());

        let sell_order = Order::new(
            1,
            test_address(),
            test_pair(),
            OrderSide::Sell,
            100,
            10,
            0,
        );
        orderbook.add_order(sell_order).unwrap();

        let buy_order = Order::new(2, test_address(), test_pair(), OrderSide::Buy, 100, 5, 1);
        let trades = orderbook.add_order(buy_order).unwrap();

        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].quantity, 5);
        assert_eq!(orderbook.best_ask(), Some(100));
    }

    #[test]
    fn test_cancel_order() {
        let mut orderbook = OrderBook::new(test_pair());

        let order = Order::new(1, test_address(), test_pair(), OrderSide::Buy, 100, 10, 0);
        orderbook.add_order(order).unwrap();

        let cancelled = orderbook.cancel_order(1).unwrap();
        assert_eq!(cancelled.status, OrderStatus::Cancelled);
        assert_eq!(orderbook.best_bid(), None);
    }
}
