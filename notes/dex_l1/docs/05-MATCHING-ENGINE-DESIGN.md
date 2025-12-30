# DEX L1 撮合引擎设计 / Matching Engine Design

> **版本**: v1.0
> **状态**: Draft
> **目标读者**: 技术评审 / 架构师

---

## 1. 概述 / Overview

### 1.1 设计目标 / Design Goals

1. **极致性能**: 单次撮合 < 10μs
2. **高吞吐**: 200,000 TPS
3. **确定性**: 相同输入产生相同结果
4. **无锁设计**: 最大化并发

### 1.2 核心算法 / Core Algorithm

**价格-时间优先 (Price-Time Priority)**:
- 买单：价格高者优先，同价则先到者优先
- 卖单：价格低者优先，同价则先到者优先

---

## 2. 订单簿数据结构 / Order Book Data Structure

### 2.1 数据结构选型 / Data Structure Selection

| 数据结构 | 插入 | 删除 | 查找最优 | 遍历 | 选择原因 |
|---------|------|------|---------|------|---------|
| BTreeMap | O(log n) | O(log n) | O(1)* | O(n) | **选用** |
| HashMap | O(1) | O(1) | O(n) | O(n) | 无序 |
| Skip List | O(log n) | O(log n) | O(1) | O(n) | 复杂 |
| B+ Tree | O(log n) | O(log n) | O(1) | O(n) | 过度 |

*使用 `first_key_value()` / `last_key_value()` 获取最优价格

### 2.2 订单簿结构 / Order Book Structure

```
┌─────────────────────────────────────────────────────────────┐
│                       Order Book                             │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌─────────────────────────────────────────────────────────┐│
│  │                      Asks (卖单)                         ││
│  │  ┌──────────┐                                           ││
│  │  │ Price    │ → [Order1, Order2, ...] (时间顺序)        ││
│  │  │ 50100    │                                           ││
│  │  ├──────────┤                                           ││
│  │  │ 50050    │ → [Order3]                                ││
│  │  ├──────────┤                                           ││
│  │  │ 50000    │ → [Order4, Order5] ← Best Ask             ││
│  │  └──────────┘                                           ││
│  └─────────────────────────────────────────────────────────┘│
│                         ↕ Spread                             │
│  ┌─────────────────────────────────────────────────────────┐│
│  │                      Bids (买单)                         ││
│  │  ┌──────────┐                                           ││
│  │  │ 49990    │ → [Order6] ← Best Bid                     ││
│  │  ├──────────┤                                           ││
│  │  │ 49950    │ → [Order7, Order8]                        ││
│  │  ├──────────┤                                           ││
│  │  │ 49900    │ → [Order9]                                ││
│  │  └──────────┘                                           ││
│  └─────────────────────────────────────────────────────────┘│
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

### 2.3 核心数据结构

```rust
/// 订单簿 / Order Book
pub struct OrderBook {
    /// 市场 ID
    market_id: MarketId,
    /// 买单 (价格降序)
    bids: BTreeMap<Reverse<Price>, PriceLevel>,
    /// 卖单 (价格升序)
    asks: BTreeMap<Price, PriceLevel>,
    /// 订单索引 (快速查找)
    orders: DashMap<OrderId, OrderLocation>,
}

/// 价格层级 / Price Level
pub struct PriceLevel {
    price: Price,
    orders: VecDeque<Order>,
    total_quantity: u64,
}

/// 订单位置索引 / Order Location
pub struct OrderLocation {
    side: Side,
    price: Price,
    index: usize,
}
```

---

## 3. 撮合算法 / Matching Algorithm

### 3.1 撮合流程 / Matching Flow

```
┌─────────────────────────────────────────────────────────────┐
│                     Matching Flow                            │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  Input: Order (side, price, quantity)                       │
│                                                              │
│  ┌────────────┐                                             │
│  │ Validate   │ → Check: balance, market status, params     │
│  └─────┬──────┘                                             │
│        ▼                                                    │
│  ┌────────────┐                                             │
│  │ Lock       │ → Lock required balance                     │
│  │ Balance    │                                             │
│  └─────┬──────┘                                             │
│        ▼                                                    │
│  ┌────────────┐     ┌────────────┐                         │
│  │ Can Match? │─Yes→│ Execute    │→ Generate trades        │
│  └─────┬──────┘     │ Match      │                         │
│        │ No         └─────┬──────┘                         │
│        ▼                  │                                 │
│  ┌────────────┐          │                                 │
│  │ Add to     │          │                                 │
│  │ OrderBook  │          │                                 │
│  └─────┬──────┘          │                                 │
│        │                  │                                 │
│        └──────────────────┴─────────────────────────────────│
│                           ▼                                 │
│  ┌────────────────────────────────────────────────────────┐│
│  │ Update Balances │ Emit Events │ Generate Effects       ││
│  └────────────────────────────────────────────────────────┘│
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

### 3.2 撮合伪代码 / Matching Pseudocode

```rust
pub fn match_order(&mut self, order: Order) -> MatchResult {
    let mut remaining = order.quantity;
    let mut trades = Vec::new();

    // 获取对手盘
    let opposite_book = match order.side {
        Side::Buy => &mut self.asks,
        Side::Sell => &mut self.bids,
    };

    // 遍历对手盘，尝试撮合
    while remaining > 0 {
        // 获取最优价格
        let best_price = match opposite_book.first_entry() {
            Some(entry) => *entry.key(),
            None => break, // 对手盘为空
        };

        // 检查价格是否匹配
        if !price_matches(order.side, order.price, best_price) {
            break; // 价格不匹配
        }

        // 获取该价格层级
        let level = opposite_book.get_mut(&best_price).unwrap();

        // 撮合该层级的订单
        while remaining > 0 && !level.orders.is_empty() {
            let maker = level.orders.front_mut().unwrap();
            let fill_qty = remaining.min(maker.remaining);

            // 生成成交记录
            trades.push(Trade {
                maker_order_id: maker.id,
                taker_order_id: order.id,
                price: best_price,
                quantity: fill_qty,
            });

            // 更新剩余数量
            remaining -= fill_qty;
            maker.remaining -= fill_qty;

            // 如果 maker 完全成交，移除
            if maker.remaining == 0 {
                level.orders.pop_front();
            }
        }

        // 如果该层级为空，移除
        if level.orders.is_empty() {
            opposite_book.remove(&best_price);
        }
    }

    MatchResult { trades, remaining }
}
```

---

## 4. 订单类型处理 / Order Type Handling

### 4.1 支持的订单类型 / Supported Order Types

| 类型 | 描述 | 处理逻辑 |
|-----|------|---------|
| **Limit** | 指定价格限价单 | 匹配或挂单 |
| **Market** | 市价单 | 立即以最优价成交 |
| **IOC** | Immediate-Or-Cancel | 匹配后取消剩余 |
| **FOK** | Fill-Or-Kill | 全部成交或全部取消 |
| **PostOnly** | 仅挂单 | 如果会立即成交则拒绝 |

### 4.2 订单类型处理逻辑

```rust
pub fn process_order(&mut self, order: Order) -> ProcessResult {
    match order.order_type {
        OrderType::Limit => {
            let result = self.match_order(order.clone());
            if result.remaining > 0 {
                self.add_to_book(order.with_remaining(result.remaining));
            }
            ProcessResult::new(result.trades, None)
        }

        OrderType::Market => {
            let result = self.match_order(order.clone());
            // 市价单不挂单，剩余部分取消
            ProcessResult::new(result.trades, Some(result.remaining))
        }

        OrderType::IOC => {
            let result = self.match_order(order.clone());
            // IOC 不挂单，剩余部分取消
            ProcessResult::new(result.trades, Some(result.remaining))
        }

        OrderType::FOK => {
            // 先检查是否能完全成交
            if !self.can_fill_completely(&order) {
                return ProcessResult::rejected("Cannot fill completely");
            }
            let result = self.match_order(order);
            ProcessResult::new(result.trades, None)
        }

        OrderType::PostOnly => {
            // 检查是否会立即成交
            if self.would_match(&order) {
                return ProcessResult::rejected("Would match immediately");
            }
            self.add_to_book(order);
            ProcessResult::new(vec![], None)
        }
    }
}
```

---

## 5. 余额管理 / Balance Management

### 5.1 余额结构 / Balance Structure

```rust
/// 账户余额 / Account Balance
pub struct Balance {
    /// 可用余额
    pub available: u64,
    /// 锁定余额 (挂单占用)
    pub locked: u64,
}

impl Balance {
    pub fn total(&self) -> u64 {
        self.available + self.locked
    }

    pub fn lock(&mut self, amount: u64) -> Result<()> {
        if self.available < amount {
            return Err(Error::InsufficientBalance);
        }
        self.available -= amount;
        self.locked += amount;
        Ok(())
    }

    pub fn unlock(&mut self, amount: u64) {
        self.locked -= amount;
        self.available += amount;
    }

    pub fn transfer_out(&mut self, amount: u64) -> Result<()> {
        if self.locked < amount {
            return Err(Error::InsufficientLocked);
        }
        self.locked -= amount;
        Ok(())
    }
}
```

### 5.2 余额存储 / Balance Storage

```rust
/// 余额管理器 / Balance Manager
pub struct BalanceManager {
    /// 余额表 (DashMap 无锁)
    balances: DashMap<(AccountId, AssetId), Balance>,
}

impl BalanceManager {
    /// 获取余额 (无锁读取)
    pub fn get_balance(&self, account: AccountId, asset: AssetId) -> Balance {
        self.balances
            .get(&(account, asset))
            .map(|b| b.clone())
            .unwrap_or_default()
    }

    /// 锁定余额
    pub fn lock(&self, account: AccountId, asset: AssetId, amount: u64) -> Result<()> {
        let mut balance = self.balances.entry((account, asset)).or_default();
        balance.lock(amount)
    }

    /// 执行转账 (原子操作)
    pub fn transfer(&self, from: AccountId, to: AccountId, asset: AssetId, amount: u64) -> Result<()> {
        // 使用 DashMap 的多键锁定
        // 确保原子性
    }
}
```

---

## 6. 手续费计算 / Fee Calculation

### 6.1 费率结构 / Fee Structure

```rust
pub struct FeeConfig {
    /// Maker 费率 (bps, 1 bps = 0.01%)
    pub maker_fee_bps: u32,  // 默认: 2 (0.02%)
    /// Taker 费率 (bps)
    pub taker_fee_bps: u32,  // 默认: 5 (0.05%)
    /// VIP 折扣
    pub vip_discount: VipDiscount,
}

/// 手续费计算 / Fee Calculation
pub fn calculate_fee(
    trade: &Trade,
    is_maker: bool,
    fee_config: &FeeConfig,
) -> u64 {
    let fee_bps = if is_maker {
        fee_config.maker_fee_bps
    } else {
        fee_config.taker_fee_bps
    };

    // fee = quantity * price * fee_bps / 10000
    let notional = trade.quantity * trade.price;
    notional * fee_bps as u64 / 10000
}
```

### 6.2 手续费扣除 / Fee Deduction

```
成交流程中的手续费处理：

Buy Order (Taker) ←→ Sell Order (Maker)
        │                    │
        ▼                    ▼
  Pay: Quote Asset     Receive: Quote Asset
  (Price × Qty)        (Price × Qty - MakerFee)
        │                    │
        ▼                    ▼
  Receive: Base Asset  Pay: Base Asset
  (Qty - TakerFee*)    (Qty)

* 手续费通常从报价资产扣除
```

---

## 7. 并发控制 / Concurrency Control

### 7.1 并发模型 / Concurrency Model

```
┌─────────────────────────────────────────────────────────────┐
│                    Concurrency Model                         │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  市场级并发 (不同市场可并行处理):                            │
│                                                              │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐         │
│  │ BTC-USDT   │  │ ETH-USDT   │  │ SOL-USDT   │         │
│  │  Engine    │  │  Engine    │  │  Engine    │         │
│  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘         │
│         │                │                │                 │
│         ▼                ▼                ▼                 │
│  ┌─────────────────────────────────────────────────────────┐│
│  │              Shared Balance Manager                     ││
│  │                 (DashMap 分片)                          ││
│  └─────────────────────────────────────────────────────────┘│
│                                                              │
│  市场内顺序 (同市场订单顺序处理):                            │
│                                                              │
│  Order1 → Order2 → Order3 → ... (FIFO)                      │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

### 7.2 无锁设计 / Lock-Free Design

```rust
/// 撮合引擎 / Matching Engine
pub struct MatchingEngine {
    /// 订单簿 (每市场一个，市场间并行)
    orderbooks: DashMap<MarketId, OrderBook>,
    /// 余额管理 (分片 DashMap)
    balances: BalanceManager,
    /// 市场配置
    markets: DashMap<MarketId, MarketConfig>,
}

impl MatchingEngine {
    /// 处理订单 (无锁，除非跨市场)
    pub fn process(&self, order: Order) -> Result<Effects> {
        // 1. 获取订单簿 (读取不需要锁)
        let mut orderbook = self.orderbooks.get_mut(&order.market_id)
            .ok_or(Error::MarketNotFound)?;

        // 2. 锁定余额 (DashMap 分片锁)
        self.balances.lock(order.account, order.asset, order.lock_amount)?;

        // 3. 撮合 (在订单簿上顺序执行)
        let result = orderbook.process_order(order);

        // 4. 更新余额 (DashMap 分片锁)
        self.apply_trades(&result.trades)?;

        Ok(result.into())
    }
}
```

---

## 8. 性能优化策略 / Performance Optimization

### 8.1 关键优化点 / Key Optimizations

| 优化 | 技术 | 效果 |
|-----|------|------|
| 无锁读取 | DashMap | 读取无竞争 |
| 批量处理 | 批次撮合 | 减少锁获取 |
| 内存预分配 | 对象池 | 减少分配 |
| Cache 优化 | 紧凑布局 | 减少缓存失效 |
| 分片锁 | 按账户分片 | 减少锁竞争 |

### 8.2 内存布局优化 / Memory Layout

```rust
/// Cache-friendly 订单结构 (64 字节对齐)
#[repr(C, align(64))]
pub struct Order {
    pub id: OrderId,           // 8 bytes
    pub account: AccountId,    // 8 bytes
    pub market_id: MarketId,   // 8 bytes
    pub side: Side,            // 1 byte
    pub order_type: OrderType, // 1 byte
    pub _pad1: [u8; 6],        // 6 bytes padding
    pub price: u64,            // 8 bytes
    pub quantity: u64,         // 8 bytes
    pub remaining: u64,        // 8 bytes
    pub timestamp: u64,        // 8 bytes
}
// Total: 64 bytes = 1 cache line
```

### 8.3 对象池 / Object Pool

```rust
/// 订单对象池 / Order Object Pool
pub struct OrderPool {
    pool: crossbeam_queue::ArrayQueue<Box<Order>>,
    capacity: usize,
}

impl OrderPool {
    pub fn acquire(&self) -> Box<Order> {
        self.pool.pop().unwrap_or_else(|| Box::new(Order::default()))
    }

    pub fn release(&self, order: Box<Order>) {
        let _ = self.pool.push(order);
    }
}
```

---

## 9. 关键数据结构总结 / Data Structure Summary

```rust
// 撮合引擎核心
pub struct MatchingEngine {
    orderbooks: DashMap<MarketId, OrderBook>,
    balances: BalanceManager,
    markets: DashMap<MarketId, MarketConfig>,
    order_pool: OrderPool,
}

// 订单簿
pub struct OrderBook {
    market_id: MarketId,
    bids: BTreeMap<Reverse<Price>, PriceLevel>,
    asks: BTreeMap<Price, PriceLevel>,
    orders: DashMap<OrderId, OrderLocation>,
}

// 价格层级
pub struct PriceLevel {
    price: Price,
    orders: VecDeque<Order>,
    total_quantity: u64,
}

// 成交记录
pub struct Trade {
    id: TradeId,
    market_id: MarketId,
    maker_order_id: OrderId,
    taker_order_id: OrderId,
    price: Price,
    quantity: u64,
    maker_fee: u64,
    taker_fee: u64,
    timestamp: u64,
}
```

---

## 10. 性能指标 / Performance Metrics

### 10.1 目标指标 / Target Metrics

| 指标 | 目标 | 测量方法 |
|-----|------|---------|
| 单次撮合 | < 10μs | Criterion benchmark |
| 订单插入 | < 1μs | Criterion benchmark |
| 订单取消 | < 1μs | Criterion benchmark |
| 吞吐量 | 200K TPS | 负载测试 |

### 10.2 监控指标 / Monitoring Metrics

```rust
lazy_static! {
    pub static ref MATCH_LATENCY: Histogram = register_histogram!(
        "dex_match_latency_seconds",
        "Order matching latency",
        vec![0.000001, 0.000005, 0.00001, 0.00005, 0.0001]
    ).unwrap();

    pub static ref ORDERBOOK_DEPTH: GaugeVec = register_gauge_vec!(
        "dex_orderbook_depth",
        "Order book depth",
        &["market", "side"]
    ).unwrap();

    pub static ref TRADES_TOTAL: CounterVec = register_counter_vec!(
        "dex_trades_total",
        "Total trades executed",
        &["market"]
    ).unwrap();
}
```

---

*文档版本: v1.0 | 最后更新: 2025-01-01*
