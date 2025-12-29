# DEX Layer 1 详细设计文档

> 基于 Sui Fork 的高性能原生 DEX 区块链

## 目录

1. [系统概述](#1-系统概述)
2. [核心模块设计](#2-核心模块设计)
3. [数据结构定义](#3-数据结构定义)
4. [Sequencer 详细设计](#4-sequencer-详细设计)
5. [撮合引擎详细设计](#5-撮合引擎详细设计)
6. [存储层详细设计](#6-存储层详细设计)
7. [永续合约详细设计](#7-永续合约详细设计)
8. [Sui 集成层设计](#8-sui-集成层设计)
9. [网络协议设计](#9-网络协议设计)
10. [API 接口设计](#10-api-接口设计)
11. [安全性设计](#11-安全性设计)
12. [性能优化策略](#12-性能优化策略)

---

## 1. 系统概述

### 1.1 设计目标

| 指标 | 目标值 | 说明 |
|-----|-------|------|
| 撮合延迟 (P99) | < 50ms | 从订单接收到撮合完成 |
| 吞吐量 | 100,000 TPS | 订单处理能力 |
| 软确认延迟 | < 50ms | Leader 本地确认 |
| 硬确认延迟 | < 100ms | 2f+1 节点确认 |
| 故障切换时间 | < 100ms | Sequencer Leader 切换 |

### 1.2 系统架构图

```
┌─────────────────────────────────────────────────────────────────────────────────────┐
│                              DEX L1 System Architecture                              │
├─────────────────────────────────────────────────────────────────────────────────────┤
│                                                                                      │
│  ┌─────────────────────────────────────────────────────────────────────────────┐    │
│  │                           External Interface Layer                           │    │
│  │  ┌───────────────┐  ┌───────────────┐  ┌───────────────┐  ┌──────────────┐  │    │
│  │  │   REST API    │  │  WebSocket    │  │   JSON-RPC    │  │   GraphQL    │  │    │
│  │  │  (Trading)    │  │  (Streaming)  │  │  (Sui Compat) │  │  (Query)     │  │    │
│  │  └───────┬───────┘  └───────┬───────┘  └───────┬───────┘  └──────┬───────┘  │    │
│  └──────────┼──────────────────┼──────────────────┼──────────────────┼──────────┘    │
│             │                  │                  │                  │               │
│  ┌──────────┴──────────────────┴──────────────────┴──────────────────┴──────────┐    │
│  │                            Gateway Layer                                      │    │
│  │  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────────────────┐   │    │
│  │  │  Rate Limiter   │  │  Auth Handler   │  │     Transaction Router      │   │    │
│  │  └─────────────────┘  └─────────────────┘  └─────────────┬───────────────┘   │    │
│  └──────────────────────────────────────────────────────────┼────────────────────┘    │
│                                                             │                         │
│             ┌───────────────────────────────────────────────┼─────────────────┐       │
│             │                                               ▼                 │       │
│             │  ┌────────────────────────────────────────────────────────┐    │       │
│             │  │                   Sequencer Layer                       │    │       │
│             │  │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  │    │       │
│             │  │  │ Tx Receiver  │─>│ Tx Orderer   │─>│ Tx Publisher │  │    │       │
│             │  │  │ (Lock-free)  │  │ (FIFO Queue) │  │ (Broadcast)  │  │    │       │
│             │  │  └──────────────┘  └──────────────┘  └──────────────┘  │    │       │
│             │  │                           │                             │    │       │
│             │  │  ┌──────────────┐  ┌──────┴───────┐  ┌──────────────┐  │    │       │
│             │  │  │ Leader Elect │  │ Seq Counter  │  │ HA Manager   │  │    │       │
│             │  │  │ (Rotation)   │  │ (Atomic U64) │  │ (Failover)   │  │    │       │
│             │  │  └──────────────┘  └──────────────┘  └──────────────┘  │    │       │
│             │  └────────────────────────────────────────────────────────┘    │       │
│             │                              │                                  │       │
│             │                              ▼                                  │       │
│  DEX Path   │  ┌────────────────────────────────────────────────────────┐    │       │
│             │  │                   DEX Engine Layer                      │    │       │
│             │  │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  │    │       │
│             │  │  │Order Manager │─>│Match Engine  │─>│ Risk Engine  │  │    │       │
│             │  │  │ (Validation) │  │ (BTreeMap)   │  │ (Margin/Liq) │  │    │       │
│             │  │  └──────────────┘  └──────────────┘  └──────────────┘  │    │       │
│             │  │                           │                             │    │       │
│             │  │  ┌──────────────┐  ┌──────┴───────┐  ┌──────────────┐  │    │       │
│             │  │  │Position Mgr  │  │ Fee Manager  │  │ Event Emitter│  │    │       │
│             │  │  │ (Perpetuals) │  │ (Maker/Taker)│  │ (Streaming)  │  │    │       │
│             │  │  └──────────────┘  └──────────────┘  └──────────────┘  │    │       │
│             │  └────────────────────────────────────────────────────────┘    │       │
│             │                              │                                  │       │
│             └──────────────────────────────┼──────────────────────────────────┘       │
│                                            │                                          │
│  ┌─────────────────────────────────────────┼────────────────────────────────────┐    │
│  │                          Execution Layer                                      │    │
│  │                                         │                                     │    │
│  │    ┌────────────────┐          ┌────────┴────────┐          ┌─────────────┐  │    │
│  │    │ DEX Precompile │<─────────│  Tx Dispatcher  │─────────>│   Move VM   │  │    │
│  │    │ (Native Rust)  │          │  (Route by Type)│          │ (Non-DEX)   │  │    │
│  │    └────────────────┘          └─────────────────┘          └─────────────┘  │    │
│  │            │                                                       │          │    │
│  │            ▼                                                       ▼          │    │
│  │    ┌────────────────┐                                      ┌─────────────┐   │    │
│  │    │ Effects Gen    │                                      │ Effects Gen │   │    │
│  │    │ (DEX Effects)  │                                      │ (Move Eff.) │   │    │
│  │    └────────────────┘                                      └─────────────┘   │    │
│  └───────────────────────────────────────────────────────────────────────────────┘    │
│                                            │                                          │
│  ┌─────────────────────────────────────────┼────────────────────────────────────┐    │
│  │                          Storage Layer                                        │    │
│  │                                         │                                     │    │
│  │    ┌────────────────┐   ┌───────────────┴───────────────┐   ┌─────────────┐  │    │
│  │    │ Orderbook State│   │       Balance Cache           │   │   RocksDB   │  │    │
│  │    │  (In-Memory)   │   │        (DashMap)              │   │ (Persist)   │  │    │
│  │    │                │   │                               │   │             │  │    │
│  │    │ ┌────────────┐ │   │ ┌───────────┐ ┌────────────┐ │   │ ┌─────────┐ │  │    │
│  │    │ │BTreeMap    │ │   │ │ Balances  │ │ Positions  │ │   │ │ Orders  │ │  │    │
│  │    │ │(Price Lvl) │ │   │ └───────────┘ └────────────┘ │   │ │ Trades  │ │  │    │
│  │    │ └────────────┘ │   │                               │   │ │ Events  │ │  │    │
│  │    │ ┌────────────┐ │   │ ┌───────────┐ ┌────────────┐ │   │ └─────────┘ │  │    │
│  │    │ │HashMap     │ │   │ │ Margins   │ │ PnL Cache  │ │   │ ┌─────────┐ │  │    │
│  │    │ │(Order Idx) │ │   │ └───────────┘ └────────────┘ │   │ │   WAL   │ │  │    │
│  │    │ └────────────┘ │   │                               │   │ └─────────┘ │  │    │
│  │    └────────────────┘   └───────────────────────────────┘   └─────────────┘  │    │
│  └───────────────────────────────────────────────────────────────────────────────┘    │
│                                                                                       │
└───────────────────────────────────────────────────────────────────────────────────────┘
```

### 1.3 Crate 依赖关系

```
┌─────────────────────────────────────────────────────────────────┐
│                      Crate Dependency Graph                      │
└─────────────────────────────────────────────────────────────────┘

                        ┌─────────────────┐
                        │   sui-node      │
                        │  (Entry Point)  │
                        └────────┬────────┘
                                 │
                    ┌────────────┼────────────┐
                    │            │            │
                    ▼            ▼            ▼
            ┌───────────┐ ┌───────────┐ ┌───────────────┐
            │ sui-core  │ │ consensus │ │ dex-sequencer │ (NEW)
            └─────┬─────┘ └───────────┘ └───────┬───────┘
                  │                             │
                  │         ┌───────────────────┤
                  │         │                   │
                  ▼         ▼                   ▼
            ┌───────────────────┐       ┌─────────────┐
            │   dex-engine      │       │ dex-storage │ (NEW)
            │   (NEW)           │       └──────┬──────┘
            └─────────┬─────────┘              │
                      │                        │
          ┌───────────┼───────────┐            │
          │           │           │            │
          ▼           ▼           ▼            │
    ┌───────────┐ ┌────────┐ ┌──────────┐     │
    │dex-perpet │ │dex-spot│ │dex-types │<────┘
    │(NEW)      │ │(NEW)   │ │(NEW)     │
    └───────────┘ └────────┘ └──────────┘
          │           │           │
          └───────────┴───────────┘
                      │
                      ▼
              ┌─────────────┐
              │  sui-types  │
              └─────────────┘
```

---

## 2. 核心模块设计

### 2.1 模块职责划分

| 模块 | 职责 | 核心数据结构 |
|-----|------|------------|
| `dex-types` | 类型定义、常量、错误码 | `Order`, `Trade`, `Market` |
| `dex-sequencer` | 交易排序、Leader 选举、HA | `Sequencer`, `SequenceBatch` |
| `dex-engine` | 撮合引擎、订单管理 | `MatchingEngine`, `Orderbook` |
| `dex-storage` | 内存状态、持久化 | `DexState`, `BalanceCache` |
| `dex-perpetuals` | 永续合约逻辑 | `Position`, `FundingRate` |
| `dex-spot` | 现货交易逻辑 | `SpotOrder`, `SpotTrade` |

### 2.2 模块接口定义

```rust
// ==================== dex-types ====================

/// 核心 trait: 可序列化的 DEX 类型
pub trait DexSerializable: Serialize + DeserializeOwned + Send + Sync + 'static {}

/// 核心 trait: 订单类型
pub trait OrderTrait: DexSerializable {
    fn id(&self) -> OrderId;
    fn market(&self) -> MarketId;
    fn side(&self) -> Side;
    fn price(&self) -> Price;
    fn quantity(&self) -> Quantity;
    fn remaining(&self) -> Quantity;
    fn order_type(&self) -> OrderType;
    fn time_in_force(&self) -> TimeInForce;
}

// ==================== dex-sequencer ====================

/// Sequencer 核心接口
#[async_trait]
pub trait SequencerService: Send + Sync {
    /// 提交交易到 Sequencer
    async fn submit(&self, tx: DexTransaction) -> Result<SequenceReceipt>;

    /// 获取当前序列号
    fn current_sequence(&self) -> u64;

    /// 订阅序列批次
    fn subscribe(&self) -> broadcast::Receiver<SequenceBatch>;

    /// 检查是否为当前 Leader
    fn is_leader(&self) -> bool;
}

/// 高可用管理接口
#[async_trait]
pub trait HAManager: Send + Sync {
    /// 开始 Leader 选举
    async fn start_election(&self) -> Result<()>;

    /// 处理心跳
    async fn handle_heartbeat(&self, from: ValidatorId) -> Result<()>;

    /// 故障切换
    async fn failover(&self, failed_leader: ValidatorId) -> Result<ValidatorId>;
}

// ==================== dex-engine ====================

/// 撮合引擎接口
pub trait MatchingEngineService: Send + Sync {
    /// 处理订单
    fn process_order(&mut self, order: Order) -> Result<MatchResult>;

    /// 取消订单
    fn cancel_order(&mut self, order_id: OrderId) -> Result<CancelResult>;

    /// 获取订单簿快照
    fn orderbook_snapshot(&self, market: MarketId, depth: usize) -> OrderbookSnapshot;

    /// 获取订单状态
    fn get_order(&self, order_id: OrderId) -> Option<OrderStatus>;
}

/// 风险引擎接口
pub trait RiskEngineService: Send + Sync {
    /// 验证订单
    fn validate_order(&self, order: &Order, account: &Account) -> Result<()>;

    /// 检查保证金
    fn check_margin(&self, account: &Account, order: &Order) -> Result<MarginCheck>;

    /// 执行清算检查
    fn check_liquidations(&self, market: MarketId) -> Vec<Liquidation>;
}

// ==================== dex-storage ====================

/// DEX 状态存储接口
#[async_trait]
pub trait DexStateStore: Send + Sync {
    /// 获取余额
    fn get_balance(&self, account: &AccountId, asset: &AssetId) -> Option<Balance>;

    /// 更新余额
    fn update_balance(&self, account: &AccountId, asset: &AssetId, delta: i128) -> Result<Balance>;

    /// 获取持仓
    fn get_position(&self, account: &AccountId, market: &MarketId) -> Option<Position>;

    /// 更新持仓
    fn update_position(&self, account: &AccountId, market: &MarketId, position: Position) -> Result<()>;

    /// 持久化快照
    async fn snapshot(&self) -> Result<StateSnapshot>;

    /// 从快照恢复
    async fn restore(&self, snapshot: StateSnapshot) -> Result<()>;
}
```

---

## 3. 数据结构定义

### 3.1 基础类型

```rust
// ==================== crates/dex-types/src/primitives.rs ====================

use std::num::NonZeroU64;

/// 订单 ID (128-bit, 全局唯一)
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub struct OrderId(pub u128);

impl OrderId {
    /// 生成新的订单 ID
    /// 格式: [timestamp_ms: 48bit][validator_id: 16bit][counter: 64bit]
    pub fn new(validator_id: u16, counter: u64) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        let id = ((timestamp as u128) << 80)
               | ((validator_id as u128) << 64)
               | (counter as u128);
        Self(id)
    }

    pub fn timestamp_ms(&self) -> u64 {
        (self.0 >> 80) as u64
    }
}

/// 市场 ID
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub struct MarketId(pub u64);

/// 账户 ID (复用 Sui Address)
pub type AccountId = SuiAddress;

/// 资产 ID (复用 Sui TypeTag)
pub type AssetId = TypeTag;

/// 价格 (定点数, 8位小数)
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
pub struct Price(pub u64);

impl Price {
    pub const DECIMALS: u32 = 8;
    pub const SCALE: u64 = 100_000_000; // 10^8

    pub fn from_f64(value: f64) -> Self {
        Self((value * Self::SCALE as f64) as u64)
    }

    pub fn to_f64(&self) -> f64 {
        self.0 as f64 / Self::SCALE as f64
    }

    /// 价格乘以数量
    pub fn mul_quantity(&self, qty: Quantity) -> u128 {
        (self.0 as u128) * (qty.0 as u128) / Self::SCALE as u128
    }
}

/// 数量 (定点数, 8位小数)
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
pub struct Quantity(pub u64);

impl Quantity {
    pub const DECIMALS: u32 = 8;
    pub const SCALE: u64 = 100_000_000;

    pub fn from_f64(value: f64) -> Self {
        Self((value * Self::SCALE as f64) as u64)
    }

    pub fn saturating_sub(&self, other: Quantity) -> Quantity {
        Quantity(self.0.saturating_sub(other.0))
    }

    pub fn is_zero(&self) -> bool {
        self.0 == 0
    }
}

/// 余额 (u128 支持大额)
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Balance(pub u128);

/// 序列号
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
pub struct SequenceNumber(pub u64);

/// 时间戳 (纳秒精度)
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
pub struct Timestamp(pub u64);

impl Timestamp {
    pub fn now() -> Self {
        Self(SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64)
    }

    pub fn as_millis(&self) -> u64 {
        self.0 / 1_000_000
    }
}
```

### 3.2 订单与交易

```rust
// ==================== crates/dex-types/src/order.rs ====================

/// 订单方向
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[repr(u8)]
pub enum Side {
    Buy = 0,
    Sell = 1,
}

impl Side {
    pub fn opposite(&self) -> Self {
        match self {
            Side::Buy => Side::Sell,
            Side::Sell => Side::Buy,
        }
    }
}

/// 订单类型
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[repr(u8)]
pub enum OrderType {
    /// 限价单
    Limit = 0,
    /// 市价单
    Market = 1,
    /// 限价止损单
    StopLimit = 2,
    /// 市价止损单
    StopMarket = 3,
    /// 只做 Maker
    PostOnly = 4,
    /// 立即成交或取消
    ImmediateOrCancel = 5,
    /// 全部成交或取消
    FillOrKill = 6,
}

/// 订单有效期
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum TimeInForce {
    /// 一直有效直到取消
    GoodTillCancel,
    /// 立即成交或取消
    ImmediateOrCancel,
    /// 全部成交或取消
    FillOrKill,
    /// 有效期至指定时间
    GoodTillTime(Timestamp),
}

/// 订单状态
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum OrderStatus {
    /// 待处理
    Pending,
    /// 已开放 (在订单簿中)
    Open,
    /// 部分成交
    PartiallyFilled,
    /// 完全成交
    Filled,
    /// 已取消
    Cancelled,
    /// 已过期
    Expired,
    /// 已拒绝
    Rejected,
}

/// 订单结构
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Order {
    /// 订单 ID
    pub id: OrderId,

    /// 账户
    pub account: AccountId,

    /// 市场
    pub market: MarketId,

    /// 方向
    pub side: Side,

    /// 价格 (市价单为0)
    pub price: Price,

    /// 数量
    pub quantity: Quantity,

    /// 已成交数量
    pub filled_quantity: Quantity,

    /// 订单类型
    pub order_type: OrderType,

    /// 有效期
    pub time_in_force: TimeInForce,

    /// 是否为减仓单 (永续)
    pub reduce_only: bool,

    /// 客户端订单 ID
    pub client_order_id: Option<String>,

    /// 创建时间
    pub created_at: Timestamp,

    /// 更新时间
    pub updated_at: Timestamp,

    /// 序列号
    pub sequence: SequenceNumber,
}

impl Order {
    /// 剩余数量
    pub fn remaining(&self) -> Quantity {
        self.quantity.saturating_sub(self.filled_quantity)
    }

    /// 是否已完全成交
    pub fn is_filled(&self) -> bool {
        self.filled_quantity >= self.quantity
    }

    /// 价格是否匹配
    pub fn price_matches(&self, other_price: Price) -> bool {
        match self.side {
            Side::Buy => self.price >= other_price,
            Side::Sell => self.price <= other_price,
        }
    }
}

/// 成交记录
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Trade {
    /// 成交 ID
    pub id: u64,

    /// 市场
    pub market: MarketId,

    /// Taker 订单 ID
    pub taker_order_id: OrderId,

    /// Maker 订单 ID
    pub maker_order_id: OrderId,

    /// Taker 账户
    pub taker: AccountId,

    /// Maker 账户
    pub maker: AccountId,

    /// 成交价格
    pub price: Price,

    /// 成交数量
    pub quantity: Quantity,

    /// Taker 方向
    pub taker_side: Side,

    /// Taker 手续费
    pub taker_fee: u64,

    /// Maker 手续费 (可能为负, 即返佣)
    pub maker_fee: i64,

    /// 成交时间
    pub timestamp: Timestamp,

    /// 序列号
    pub sequence: SequenceNumber,
}
```

### 3.3 市场配置

```rust
// ==================== crates/dex-types/src/market.rs ====================

/// 市场类型
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum MarketType {
    /// 现货
    Spot,
    /// 永续合约
    Perpetual,
    /// 交割合约
    Futures { expiry: Timestamp },
}

/// 市场配置
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MarketConfig {
    /// 市场 ID
    pub id: MarketId,

    /// 市场类型
    pub market_type: MarketType,

    /// 基础资产 (如 BTC)
    pub base_asset: AssetId,

    /// 报价资产 (如 USDT)
    pub quote_asset: AssetId,

    /// 最小价格变动 (tick size)
    pub tick_size: Price,

    /// 最小数量变动 (lot size)
    pub lot_size: Quantity,

    /// 最小订单价值
    pub min_order_value: u64,

    /// 最大订单数量
    pub max_order_quantity: Quantity,

    /// Taker 费率 (bps, 1 bps = 0.01%)
    pub taker_fee_bps: u16,

    /// Maker 费率 (bps, 可为负表示返佣)
    pub maker_fee_bps: i16,

    /// 是否活跃
    pub is_active: bool,

    // ===== 永续合约特有 =====

    /// 初始保证金率 (bps)
    pub initial_margin_bps: Option<u16>,

    /// 维持保证金率 (bps)
    pub maintenance_margin_bps: Option<u16>,

    /// 最大杠杆倍数
    pub max_leverage: Option<u8>,

    /// 资金费率间隔 (秒)
    pub funding_interval_secs: Option<u32>,
}

impl MarketConfig {
    /// 验证价格是否符合 tick size
    pub fn validate_price(&self, price: Price) -> bool {
        price.0 % self.tick_size.0 == 0
    }

    /// 验证数量是否符合 lot size
    pub fn validate_quantity(&self, quantity: Quantity) -> bool {
        quantity.0 % self.lot_size.0 == 0
    }

    /// 计算 Taker 手续费
    pub fn calculate_taker_fee(&self, notional: u128) -> u64 {
        ((notional * self.taker_fee_bps as u128) / 10000) as u64
    }

    /// 计算 Maker 手续费
    pub fn calculate_maker_fee(&self, notional: u128) -> i64 {
        ((notional as i128 * self.maker_fee_bps as i128) / 10000) as i64
    }
}
```

### 3.4 账户与持仓

```rust
// ==================== crates/dex-types/src/account.rs ====================

/// 账户余额
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AccountBalances {
    /// 可用余额
    pub available: HashMap<AssetId, Balance>,

    /// 冻结余额 (订单占用)
    pub frozen: HashMap<AssetId, Balance>,

    /// 保证金 (永续合约)
    pub margin: HashMap<MarketId, Balance>,
}

impl AccountBalances {
    /// 获取可用余额
    pub fn available(&self, asset: &AssetId) -> Balance {
        self.available.get(asset).copied().unwrap_or(Balance(0))
    }

    /// 冻结余额
    pub fn freeze(&mut self, asset: &AssetId, amount: Balance) -> Result<()> {
        let available = self.available.get_mut(asset)
            .ok_or(DexError::InsufficientBalance)?;

        if available.0 < amount.0 {
            return Err(DexError::InsufficientBalance);
        }

        available.0 -= amount.0;
        *self.frozen.entry(asset.clone()).or_insert(Balance(0)).0 += amount.0;

        Ok(())
    }

    /// 解冻余额
    pub fn unfreeze(&mut self, asset: &AssetId, amount: Balance) -> Result<()> {
        let frozen = self.frozen.get_mut(asset)
            .ok_or(DexError::InsufficientFrozen)?;

        if frozen.0 < amount.0 {
            return Err(DexError::InsufficientFrozen);
        }

        frozen.0 -= amount.0;
        *self.available.entry(asset.clone()).or_insert(Balance(0)).0 += amount.0;

        Ok(())
    }
}

/// 永续合约持仓
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Position {
    /// 账户
    pub account: AccountId,

    /// 市场
    pub market: MarketId,

    /// 持仓方向
    pub side: Side,

    /// 持仓数量
    pub size: Quantity,

    /// 平均入场价格
    pub entry_price: Price,

    /// 已实现盈亏
    pub realized_pnl: i128,

    /// 累计资金费用
    pub cumulative_funding: i128,

    /// 保证金
    pub margin: Balance,

    /// 杠杆倍数
    pub leverage: u8,

    /// 最后更新时间
    pub updated_at: Timestamp,
}

impl Position {
    /// 计算未实现盈亏
    pub fn unrealized_pnl(&self, mark_price: Price) -> i128 {
        let size_value = self.size.0 as i128;
        let entry_value = (self.entry_price.0 as i128 * size_value) / Price::SCALE as i128;
        let current_value = (mark_price.0 as i128 * size_value) / Price::SCALE as i128;

        match self.side {
            Side::Buy => current_value - entry_value,   // 多仓: 价格上涨盈利
            Side::Sell => entry_value - current_value,  // 空仓: 价格下跌盈利
        }
    }

    /// 计算保证金率
    pub fn margin_ratio(&self, mark_price: Price) -> f64 {
        let unrealized_pnl = self.unrealized_pnl(mark_price);
        let account_value = self.margin.0 as i128 + unrealized_pnl;
        let notional = (mark_price.0 as i128 * self.size.0 as i128) / Price::SCALE as i128;

        if notional == 0 {
            return f64::MAX;
        }

        account_value as f64 / notional as f64
    }

    /// 计算强平价格
    pub fn liquidation_price(&self, maintenance_margin_bps: u16) -> Price {
        let mm_ratio = maintenance_margin_bps as f64 / 10000.0;
        let margin = self.margin.0 as f64;
        let size = self.size.0 as f64 / Quantity::SCALE as f64;
        let entry = self.entry_price.0 as f64 / Price::SCALE as f64;

        let liq_price = match self.side {
            Side::Buy => entry - (margin / size) + (entry * mm_ratio),
            Side::Sell => entry + (margin / size) - (entry * mm_ratio),
        };

        Price::from_f64(liq_price.max(0.0))
    }
}
```

---

## 4. Sequencer 详细设计

### 4.1 Sequencer 核心结构

```rust
// ==================== crates/dex-sequencer/src/sequencer.rs ====================

use std::sync::atomic::{AtomicU64, AtomicBool, Ordering};
use tokio::sync::{broadcast, mpsc, RwLock};
use dashmap::DashMap;

/// Sequencer 配置
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SequencerConfig {
    /// 批次大小上限
    pub max_batch_size: usize,

    /// 批次时间间隔 (微秒)
    pub batch_interval_us: u64,

    /// 心跳间隔 (毫秒)
    pub heartbeat_interval_ms: u64,

    /// 心跳超时 (毫秒)
    pub heartbeat_timeout_ms: u64,

    /// Sequencer epoch 时长 (毫秒)
    pub epoch_duration_ms: u64,

    /// 序列确认超时 (毫秒)
    pub confirmation_timeout_ms: u64,
}

impl Default for SequencerConfig {
    fn default() -> Self {
        Self {
            max_batch_size: 1000,
            batch_interval_us: 1000,  // 1ms
            heartbeat_interval_ms: 25,
            heartbeat_timeout_ms: 50,
            epoch_duration_ms: 60_000,  // 1 minute
            confirmation_timeout_ms: 100,
        }
    }
}

/// Sequencer 主结构
pub struct DexSequencer {
    /// 配置
    config: SequencerConfig,

    /// 当前序列号 (原子操作)
    sequence_counter: AtomicU64,

    /// 是否为 Leader
    is_leader: AtomicBool,

    /// 当前 epoch
    current_epoch: AtomicU64,

    /// 验证者 ID
    validator_id: ValidatorId,

    /// 委员会信息
    committee: Arc<RwLock<Committee>>,

    /// 待处理交易队列
    pending_tx: mpsc::Sender<DexTransaction>,
    pending_rx: Arc<Mutex<mpsc::Receiver<DexTransaction>>>,

    /// 序列广播通道
    sequence_broadcast: broadcast::Sender<SequenceBatch>,

    /// 确认收集器
    confirmations: Arc<DashMap<u64, ConfirmationCollector>>,

    /// P2P 网络
    network: Arc<dyn SequencerNetwork>,

    /// DA 层客户端
    da_client: Arc<dyn DAClient>,

    /// 指标收集
    metrics: Arc<SequencerMetrics>,
}

/// 确认收集器
struct ConfirmationCollector {
    sequence_range: (u64, u64),
    confirmations: HashSet<ValidatorId>,
    created_at: Instant,
}

impl DexSequencer {
    /// 创建新的 Sequencer
    pub fn new(
        config: SequencerConfig,
        validator_id: ValidatorId,
        committee: Committee,
        network: Arc<dyn SequencerNetwork>,
        da_client: Arc<dyn DAClient>,
    ) -> Self {
        let (pending_tx, pending_rx) = mpsc::channel(100_000);
        let (sequence_broadcast, _) = broadcast::channel(10_000);

        Self {
            config,
            sequence_counter: AtomicU64::new(0),
            is_leader: AtomicBool::new(false),
            current_epoch: AtomicU64::new(0),
            validator_id,
            committee: Arc::new(RwLock::new(committee)),
            pending_tx,
            pending_rx: Arc::new(Mutex::new(pending_rx)),
            sequence_broadcast,
            confirmations: Arc::new(DashMap::new()),
            network,
            da_client,
            metrics: Arc::new(SequencerMetrics::new()),
        }
    }

    /// 提交交易
    pub async fn submit(&self, tx: DexTransaction) -> Result<SequenceReceipt> {
        // 1. 验证交易签名
        tx.verify_signature()?;

        // 2. 如果是 Leader, 直接处理
        if self.is_leader.load(Ordering::Acquire) {
            return self.sequence_transaction(tx).await;
        }

        // 3. 否则转发给 Leader
        let leader = self.get_current_leader().await?;
        self.network.forward_to_leader(leader, tx).await
    }

    /// 对交易进行排序
    async fn sequence_transaction(&self, tx: DexTransaction) -> Result<SequenceReceipt> {
        let start = Instant::now();

        // 分配序列号
        let seq = self.sequence_counter.fetch_add(1, Ordering::SeqCst);

        let sequenced_tx = SequencedTransaction {
            sequence: SequenceNumber(seq),
            timestamp: Timestamp::now(),
            transaction: tx,
            validator_signature: self.sign_sequence(seq),
        };

        // 发送到处理队列
        self.pending_tx.send(sequenced_tx.transaction.clone()).await?;

        self.metrics.sequence_latency.observe(start.elapsed().as_micros() as f64);

        Ok(SequenceReceipt {
            sequence: SequenceNumber(seq),
            timestamp: sequenced_tx.timestamp,
            status: ReceiptStatus::Sequenced,
        })
    }

    /// 运行 Sequencer 主循环
    pub async fn run(&self) -> Result<()> {
        let mut interval = tokio::time::interval(
            Duration::from_micros(self.config.batch_interval_us)
        );

        loop {
            interval.tick().await;

            if !self.is_leader.load(Ordering::Acquire) {
                continue;
            }

            // 收集批次
            let batch = self.collect_batch().await;

            if batch.transactions.is_empty() {
                continue;
            }

            // 广播批次
            self.broadcast_batch(batch.clone()).await?;

            // 写入 DA 层 (异步)
            let da_client = self.da_client.clone();
            let batch_clone = batch.clone();
            tokio::spawn(async move {
                if let Err(e) = da_client.write(batch_clone).await {
                    tracing::error!("Failed to write to DA: {}", e);
                }
            });

            // 广播到本地订阅者
            let _ = self.sequence_broadcast.send(batch);
        }
    }

    /// 收集交易批次
    async fn collect_batch(&self) -> SequenceBatch {
        let mut transactions = Vec::with_capacity(self.config.max_batch_size);
        let mut rx = self.pending_rx.lock().await;

        let deadline = Instant::now() + Duration::from_micros(self.config.batch_interval_us);

        while transactions.len() < self.config.max_batch_size {
            match tokio::time::timeout_at(deadline.into(), rx.recv()).await {
                Ok(Some(tx)) => {
                    let seq = self.sequence_counter.fetch_add(1, Ordering::SeqCst);
                    transactions.push(SequencedTransaction {
                        sequence: SequenceNumber(seq),
                        timestamp: Timestamp::now(),
                        transaction: tx,
                        validator_signature: self.sign_sequence(seq),
                    });
                }
                Ok(None) => break, // Channel closed
                Err(_) => break,   // Timeout
            }
        }

        let start_seq = transactions.first()
            .map(|t| t.sequence.0)
            .unwrap_or(self.sequence_counter.load(Ordering::SeqCst));
        let end_seq = transactions.last()
            .map(|t| t.sequence.0)
            .unwrap_or(start_seq);

        SequenceBatch {
            epoch: self.current_epoch.load(Ordering::SeqCst),
            sequence_range: (start_seq, end_seq),
            transactions,
            leader: self.validator_id,
            leader_signature: self.sign_batch(start_seq, end_seq),
            timestamp: Timestamp::now(),
        }
    }

    /// 广播批次到所有验证者
    async fn broadcast_batch(&self, batch: SequenceBatch) -> Result<()> {
        let committee = self.committee.read().await;

        // 并行发送给所有验证者
        let futures: Vec<_> = committee.validators()
            .filter(|v| *v != &self.validator_id)
            .map(|validator| {
                let network = self.network.clone();
                let batch = batch.clone();
                async move {
                    network.send_batch(validator.clone(), batch).await
                }
            })
            .collect();

        // 等待多数确认
        let results = futures::future::join_all(futures).await;
        let success_count = results.iter().filter(|r| r.is_ok()).count();

        if success_count < committee.quorum_threshold() {
            tracing::warn!("Batch broadcast: only {}/{} succeeded",
                success_count, committee.size());
        }

        // 记录待确认
        self.confirmations.insert(batch.sequence_range.0, ConfirmationCollector {
            sequence_range: batch.sequence_range,
            confirmations: HashSet::new(),
            created_at: Instant::now(),
        });

        Ok(())
    }

    /// 处理收到的确认
    pub async fn handle_confirmation(&self, from: ValidatorId, seq_range: (u64, u64)) {
        if let Some(mut collector) = self.confirmations.get_mut(&seq_range.0) {
            collector.confirmations.insert(from);

            let committee = self.committee.read().await;
            if collector.confirmations.len() >= committee.quorum_threshold() {
                // 达到 2f+1 确认, 批次已最终确认
                self.metrics.hard_finality_count.inc();
                drop(collector);
                self.confirmations.remove(&seq_range.0);
            }
        }
    }

    /// 获取当前 Leader
    async fn get_current_leader(&self) -> Result<ValidatorId> {
        let committee = self.committee.read().await;
        let epoch = self.current_epoch.load(Ordering::SeqCst);
        Ok(committee.leader_for_epoch(epoch))
    }

    fn sign_sequence(&self, seq: u64) -> Signature {
        // 实现签名逻辑
        todo!()
    }

    fn sign_batch(&self, start: u64, end: u64) -> Signature {
        // 实现签名逻辑
        todo!()
    }
}

/// 序列批次
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SequenceBatch {
    pub epoch: u64,
    pub sequence_range: (u64, u64),
    pub transactions: Vec<SequencedTransaction>,
    pub leader: ValidatorId,
    pub leader_signature: Signature,
    pub timestamp: Timestamp,
}

/// 已排序的交易
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SequencedTransaction {
    pub sequence: SequenceNumber,
    pub timestamp: Timestamp,
    pub transaction: DexTransaction,
    pub validator_signature: Signature,
}

/// 序列回执
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SequenceReceipt {
    pub sequence: SequenceNumber,
    pub timestamp: Timestamp,
    pub status: ReceiptStatus,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ReceiptStatus {
    Sequenced,
    Confirmed,
    Finalized,
    Failed(String),
}
```

### 4.2 Leader 选举与故障切换

```rust
// ==================== crates/dex-sequencer/src/leader_election.rs ====================

/// Leader 选举管理器
pub struct LeaderElectionManager {
    /// 配置
    config: SequencerConfig,

    /// 验证者 ID
    validator_id: ValidatorId,

    /// 委员会
    committee: Arc<RwLock<Committee>>,

    /// 当前 Leader
    current_leader: Arc<RwLock<ValidatorId>>,

    /// Leader 状态
    leader_state: Arc<RwLock<LeaderState>>,

    /// 故障投票
    failure_votes: Arc<DashMap<ValidatorId, HashSet<ValidatorId>>>,

    /// 网络
    network: Arc<dyn SequencerNetwork>,

    /// DA 层客户端
    da_client: Arc<dyn DAClient>,
}

#[derive(Clone, Debug)]
pub struct LeaderState {
    pub leader: ValidatorId,
    pub epoch: u64,
    pub last_heartbeat: Instant,
    pub last_sequence: u64,
}

impl LeaderElectionManager {
    /// 运行心跳监控
    pub async fn run_heartbeat_monitor(&self) {
        let heartbeat_interval = Duration::from_millis(self.config.heartbeat_interval_ms);
        let heartbeat_timeout = Duration::from_millis(self.config.heartbeat_timeout_ms);

        let mut interval = tokio::time::interval(heartbeat_interval);

        loop {
            interval.tick().await;

            let state = self.leader_state.read().await;
            let current_leader = state.leader.clone();
            let last_heartbeat = state.last_heartbeat;
            drop(state);

            // 如果自己是 Leader, 发送心跳
            if current_leader == self.validator_id {
                self.broadcast_heartbeat().await;
                continue;
            }

            // 检查心跳超时
            if last_heartbeat.elapsed() > heartbeat_timeout {
                tracing::warn!("Leader heartbeat timeout: {:?}", current_leader);
                self.initiate_failover(current_leader).await;
            }
        }
    }

    /// 广播心跳
    async fn broadcast_heartbeat(&self) {
        let state = self.leader_state.read().await;
        let heartbeat = Heartbeat {
            leader: self.validator_id.clone(),
            epoch: state.epoch,
            last_sequence: state.last_sequence,
            timestamp: Timestamp::now(),
        };
        drop(state);

        let committee = self.committee.read().await;
        for validator in committee.validators() {
            if validator != &self.validator_id {
                let _ = self.network.send_heartbeat(validator.clone(), heartbeat.clone()).await;
            }
        }
    }

    /// 处理收到的心跳
    pub async fn handle_heartbeat(&self, heartbeat: Heartbeat) {
        let mut state = self.leader_state.write().await;

        if heartbeat.leader == state.leader && heartbeat.epoch == state.epoch {
            state.last_heartbeat = Instant::now();
            state.last_sequence = heartbeat.last_sequence;
        }
    }

    /// 发起故障切换
    async fn initiate_failover(&self, failed_leader: ValidatorId) {
        // 1. 广播故障检测
        let failure_vote = FailureVote {
            failed_leader: failed_leader.clone(),
            voter: self.validator_id.clone(),
            epoch: self.leader_state.read().await.epoch,
            timestamp: Timestamp::now(),
        };

        self.broadcast_failure_vote(failure_vote.clone()).await;

        // 2. 记录自己的投票
        self.failure_votes
            .entry(failed_leader.clone())
            .or_insert_with(HashSet::new)
            .insert(self.validator_id.clone());

        // 3. 检查是否达到 quorum
        self.check_failure_quorum(failed_leader).await;
    }

    /// 处理故障投票
    pub async fn handle_failure_vote(&self, vote: FailureVote) {
        // 验证投票签名
        if !vote.verify() {
            return;
        }

        // 记录投票
        self.failure_votes
            .entry(vote.failed_leader.clone())
            .or_insert_with(HashSet::new)
            .insert(vote.voter.clone());

        // 检查 quorum
        self.check_failure_quorum(vote.failed_leader).await;
    }

    /// 检查故障投票是否达到 quorum
    async fn check_failure_quorum(&self, failed_leader: ValidatorId) {
        let votes = match self.failure_votes.get(&failed_leader) {
            Some(v) => v.len(),
            None => return,
        };

        let committee = self.committee.read().await;
        if votes >= committee.quorum_threshold() {
            // 达到 2f+1, 执行 Leader 切换
            self.execute_leader_switch(failed_leader).await;
        }
    }

    /// 执行 Leader 切换
    async fn execute_leader_switch(&self, failed_leader: ValidatorId) {
        let committee = self.committee.read().await;

        // 1. 选择新 Leader (排除失败的 Leader)
        let new_leader = committee.next_leader_excluding(&failed_leader);

        // 2. 从 DA 层获取最后确认的序列
        let last_confirmed = self.da_client
            .get_last_confirmed_sequence()
            .await
            .unwrap_or(0);

        // 3. 更新 Leader 状态
        let mut state = self.leader_state.write().await;
        state.leader = new_leader.clone();
        state.epoch += 1;
        state.last_sequence = last_confirmed;
        state.last_heartbeat = Instant::now();
        drop(state);

        // 4. 广播 Leader 变更
        let change = LeaderChange {
            old_leader: failed_leader,
            new_leader: new_leader.clone(),
            epoch: state.epoch,
            resume_sequence: last_confirmed + 1,
            timestamp: Timestamp::now(),
        };
        self.broadcast_leader_change(change).await;

        // 5. 如果自己是新 Leader, 激活 Sequencer
        if new_leader == self.validator_id {
            tracing::info!("Becoming new leader at epoch {}", state.epoch);
            self.activate_as_leader(last_confirmed + 1).await;
        }

        // 6. 清理故障投票
        self.failure_votes.remove(&failed_leader);
    }

    /// 激活为 Leader
    async fn activate_as_leader(&self, resume_from: u64) {
        // 更新序列计数器
        // 通知 Sequencer 开始处理
        todo!()
    }

    async fn broadcast_failure_vote(&self, vote: FailureVote) {
        let committee = self.committee.read().await;
        for validator in committee.validators() {
            if validator != &self.validator_id {
                let _ = self.network.send_failure_vote(validator.clone(), vote.clone()).await;
            }
        }
    }

    async fn broadcast_leader_change(&self, change: LeaderChange) {
        let committee = self.committee.read().await;
        for validator in committee.validators() {
            if validator != &self.validator_id {
                let _ = self.network.send_leader_change(validator.clone(), change.clone()).await;
            }
        }
    }
}

/// 心跳消息
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Heartbeat {
    pub leader: ValidatorId,
    pub epoch: u64,
    pub last_sequence: u64,
    pub timestamp: Timestamp,
}

/// 故障投票
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FailureVote {
    pub failed_leader: ValidatorId,
    pub voter: ValidatorId,
    pub epoch: u64,
    pub timestamp: Timestamp,
}

/// Leader 变更通知
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LeaderChange {
    pub old_leader: ValidatorId,
    pub new_leader: ValidatorId,
    pub epoch: u64,
    pub resume_sequence: u64,
    pub timestamp: Timestamp,
}
```

---

## 5. 撮合引擎详细设计

### 5.1 订单簿结构

```rust
// ==================== crates/dex-engine/src/orderbook.rs ====================

use std::cmp::Reverse;
use std::collections::{BTreeMap, HashMap, VecDeque};

/// 订单簿
pub struct Orderbook {
    /// 市场配置
    config: MarketConfig,

    /// 买单 (价格降序)
    bids: BTreeMap<Reverse<Price>, PriceLevel>,

    /// 卖单 (价格升序)
    asks: BTreeMap<Price, PriceLevel>,

    /// 订单索引
    order_index: HashMap<OrderId, OrderLocation>,

    /// 最新成交价
    last_trade_price: Option<Price>,

    /// 订单计数器
    order_count: u64,

    /// 统计信息
    stats: OrderbookStats,
}

/// 价格档位
#[derive(Clone, Debug)]
pub struct PriceLevel {
    /// 价格
    pub price: Price,

    /// 订单队列 (时间优先)
    pub orders: VecDeque<OrderEntry>,

    /// 该档位总数量
    pub total_quantity: Quantity,

    /// 订单数量
    pub order_count: u32,
}

/// 订单条目 (精简版, 用于订单簿)
#[derive(Clone, Debug)]
pub struct OrderEntry {
    pub id: OrderId,
    pub account: AccountId,
    pub quantity: Quantity,
    pub filled: Quantity,
    pub timestamp: Timestamp,
}

impl OrderEntry {
    pub fn remaining(&self) -> Quantity {
        self.quantity.saturating_sub(self.filled)
    }
}

/// 订单位置索引
#[derive(Clone, Debug)]
struct OrderLocation {
    pub side: Side,
    pub price: Price,
}

/// 订单簿统计
#[derive(Clone, Debug, Default)]
pub struct OrderbookStats {
    pub bid_count: u64,
    pub ask_count: u64,
    pub bid_volume: u128,
    pub ask_volume: u128,
    pub trade_count: u64,
    pub trade_volume: u128,
}

impl Orderbook {
    /// 创建新的订单簿
    pub fn new(config: MarketConfig) -> Self {
        Self {
            config,
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            order_index: HashMap::new(),
            last_trade_price: None,
            order_count: 0,
            stats: OrderbookStats::default(),
        }
    }

    /// 获取最优买价
    pub fn best_bid(&self) -> Option<Price> {
        self.bids.first_key_value().map(|(Reverse(p), _)| *p)
    }

    /// 获取最优卖价
    pub fn best_ask(&self) -> Option<Price> {
        self.asks.first_key_value().map(|(p, _)| *p)
    }

    /// 获取买卖价差
    pub fn spread(&self) -> Option<Price> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => Some(Price(ask.0.saturating_sub(bid.0))),
            _ => None,
        }
    }

    /// 获取中间价
    pub fn mid_price(&self) -> Option<Price> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => Some(Price((bid.0 + ask.0) / 2)),
            _ => None,
        }
    }

    /// 添加订单到订单簿
    pub fn add_order(&mut self, order: &Order) -> Result<()> {
        // 验证价格和数量
        if !self.config.validate_price(order.price) {
            return Err(DexError::InvalidTickSize);
        }
        if !self.config.validate_quantity(order.quantity) {
            return Err(DexError::InvalidLotSize);
        }

        let entry = OrderEntry {
            id: order.id,
            account: order.account.clone(),
            quantity: order.quantity,
            filled: order.filled_quantity,
            timestamp: order.created_at,
        };

        match order.side {
            Side::Buy => {
                let level = self.bids.entry(Reverse(order.price)).or_insert_with(|| {
                    PriceLevel::new(order.price)
                });
                level.add_order(entry);
                self.stats.bid_count += 1;
                self.stats.bid_volume += order.remaining().0 as u128;
            }
            Side::Sell => {
                let level = self.asks.entry(order.price).or_insert_with(|| {
                    PriceLevel::new(order.price)
                });
                level.add_order(entry);
                self.stats.ask_count += 1;
                self.stats.ask_volume += order.remaining().0 as u128;
            }
        }

        self.order_index.insert(order.id, OrderLocation {
            side: order.side,
            price: order.price,
        });

        self.order_count += 1;
        Ok(())
    }

    /// 从订单簿移除订单
    pub fn remove_order(&mut self, order_id: OrderId) -> Option<OrderEntry> {
        let location = self.order_index.remove(&order_id)?;

        let entry = match location.side {
            Side::Buy => {
                let level = self.bids.get_mut(&Reverse(location.price))?;
                let entry = level.remove_order(order_id)?;

                self.stats.bid_count -= 1;
                self.stats.bid_volume -= entry.remaining().0 as u128;

                if level.is_empty() {
                    self.bids.remove(&Reverse(location.price));
                }
                entry
            }
            Side::Sell => {
                let level = self.asks.get_mut(&location.price)?;
                let entry = level.remove_order(order_id)?;

                self.stats.ask_count -= 1;
                self.stats.ask_volume -= entry.remaining().0 as u128;

                if level.is_empty() {
                    self.asks.remove(&location.price);
                }
                entry
            }
        };

        self.order_count -= 1;
        Some(entry)
    }

    /// 获取订单簿快照
    pub fn snapshot(&self, depth: usize) -> OrderbookSnapshot {
        let bids: Vec<_> = self.bids.iter()
            .take(depth)
            .map(|(Reverse(price), level)| (*price, level.total_quantity))
            .collect();

        let asks: Vec<_> = self.asks.iter()
            .take(depth)
            .map(|(price, level)| (*price, level.total_quantity))
            .collect();

        OrderbookSnapshot {
            market: self.config.id,
            bids,
            asks,
            last_trade_price: self.last_trade_price,
            timestamp: Timestamp::now(),
        }
    }
}

impl PriceLevel {
    pub fn new(price: Price) -> Self {
        Self {
            price,
            orders: VecDeque::new(),
            total_quantity: Quantity(0),
            order_count: 0,
        }
    }

    pub fn add_order(&mut self, entry: OrderEntry) {
        self.total_quantity.0 += entry.remaining().0;
        self.order_count += 1;
        self.orders.push_back(entry);
    }

    pub fn remove_order(&mut self, order_id: OrderId) -> Option<OrderEntry> {
        let pos = self.orders.iter().position(|o| o.id == order_id)?;
        let entry = self.orders.remove(pos)?;
        self.total_quantity.0 -= entry.remaining().0;
        self.order_count -= 1;
        Some(entry)
    }

    pub fn is_empty(&self) -> bool {
        self.orders.is_empty()
    }

    pub fn front(&self) -> Option<&OrderEntry> {
        self.orders.front()
    }

    pub fn front_mut(&mut self) -> Option<&mut OrderEntry> {
        self.orders.front_mut()
    }
}

/// 订单簿快照
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrderbookSnapshot {
    pub market: MarketId,
    pub bids: Vec<(Price, Quantity)>,
    pub asks: Vec<(Price, Quantity)>,
    pub last_trade_price: Option<Price>,
    pub timestamp: Timestamp,
}
```

### 5.2 撮合引擎

```rust
// ==================== crates/dex-engine/src/matching_engine.rs ====================

/// 撮合引擎
pub struct MatchingEngine {
    /// 订单簿 (按市场)
    orderbooks: HashMap<MarketId, Orderbook>,

    /// 市场配置
    markets: HashMap<MarketId, MarketConfig>,

    /// 订单 ID 生成器
    order_id_gen: OrderIdGenerator,

    /// 成交 ID 生成器
    trade_id_counter: AtomicU64,

    /// 当前序列号
    current_sequence: SequenceNumber,

    /// 指标
    metrics: Arc<MatchingMetrics>,
}

/// 撮合结果
#[derive(Clone, Debug)]
pub struct MatchResult {
    /// 订单 ID
    pub order_id: OrderId,

    /// 订单状态
    pub status: OrderStatus,

    /// 成交列表
    pub trades: Vec<Trade>,

    /// 剩余数量 (如果是限价单且未完全成交)
    pub remaining: Quantity,

    /// 是否已加入订单簿
    pub added_to_book: bool,
}

/// 取消结果
#[derive(Clone, Debug)]
pub struct CancelResult {
    pub order_id: OrderId,
    pub cancelled_quantity: Quantity,
    pub status: OrderStatus,
}

impl MatchingEngine {
    pub fn new() -> Self {
        Self {
            orderbooks: HashMap::new(),
            markets: HashMap::new(),
            order_id_gen: OrderIdGenerator::new(),
            trade_id_counter: AtomicU64::new(0),
            current_sequence: SequenceNumber(0),
            metrics: Arc::new(MatchingMetrics::new()),
        }
    }

    /// 添加市场
    pub fn add_market(&mut self, config: MarketConfig) -> Result<()> {
        if self.markets.contains_key(&config.id) {
            return Err(DexError::MarketAlreadyExists);
        }

        let orderbook = Orderbook::new(config.clone());
        self.orderbooks.insert(config.id, orderbook);
        self.markets.insert(config.id, config);

        Ok(())
    }

    /// 处理订单
    pub fn process_order(&mut self, mut order: Order) -> Result<MatchResult> {
        let start = Instant::now();

        // 1. 获取市场配置和订单簿
        let config = self.markets.get(&order.market)
            .ok_or(DexError::MarketNotFound)?;
        let orderbook = self.orderbooks.get_mut(&order.market)
            .ok_or(DexError::MarketNotFound)?;

        // 2. 验证订单
        self.validate_order(&order, config)?;

        // 3. 分配订单 ID
        if order.id == OrderId(0) {
            order.id = self.order_id_gen.next();
        }

        // 4. 执行撮合
        let trades = self.match_order(&mut order, orderbook, config)?;

        // 5. 处理剩余数量
        let remaining = order.remaining();
        let mut added_to_book = false;

        let status = if order.is_filled() {
            OrderStatus::Filled
        } else {
            match order.order_type {
                OrderType::Market | OrderType::ImmediateOrCancel => {
                    // 市价单或 IOC: 取消剩余
                    OrderStatus::Cancelled
                }
                OrderType::FillOrKill => {
                    // FOK: 如果有成交则回滚 (不应该发生, 在撮合时已检查)
                    OrderStatus::Cancelled
                }
                OrderType::Limit | OrderType::PostOnly => {
                    // 限价单: 加入订单簿
                    orderbook.add_order(&order)?;
                    added_to_book = true;
                    if trades.is_empty() {
                        OrderStatus::Open
                    } else {
                        OrderStatus::PartiallyFilled
                    }
                }
                _ => OrderStatus::Open,
            }
        };

        // 6. 更新指标
        self.metrics.match_latency.observe(start.elapsed().as_micros() as f64);
        self.metrics.order_count.inc();
        self.metrics.trade_count.add(trades.len() as u64);

        Ok(MatchResult {
            order_id: order.id,
            status,
            trades,
            remaining,
            added_to_book,
        })
    }

    /// 执行撮合
    fn match_order(
        &mut self,
        order: &mut Order,
        orderbook: &mut Orderbook,
        config: &MarketConfig,
    ) -> Result<Vec<Trade>> {
        // PostOnly 订单不应该与对手方成交
        if order.order_type == OrderType::PostOnly {
            if self.would_match(order, orderbook) {
                return Err(DexError::PostOnlyWouldMatch);
            }
            return Ok(vec![]);
        }

        // FOK 订单需要检查能否完全成交
        if order.order_type == OrderType::FillOrKill {
            if !self.can_fill_completely(order, orderbook) {
                return Err(DexError::FillOrKillCannotFill);
            }
        }

        let mut trades = Vec::new();

        // 获取对手方订单簿
        let opposite_book = match order.side {
            Side::Buy => &mut orderbook.asks,
            Side::Sell => &mut orderbook.bids,
        };

        // 撮合循环
        while order.remaining().0 > 0 {
            // 获取最优价格档位
            let best_price = match order.side {
                Side::Buy => opposite_book.first_entry(),
                Side::Sell => {
                    // 对于卖单, bids 是 BTreeMap<Reverse<Price>, _>
                    // 需要特殊处理
                    orderbook.bids.first_entry()
                }
            };

            let (price, level) = match order.side {
                Side::Buy => {
                    match orderbook.asks.first_entry() {
                        Some(entry) => (*entry.key(), entry.into_mut()),
                        None => break,
                    }
                }
                Side::Sell => {
                    match orderbook.bids.first_entry() {
                        Some(entry) => (entry.key().0, entry.into_mut()),
                        None => break,
                    }
                }
            };

            // 价格检查 (限价单)
            if order.order_type != OrderType::Market {
                let price_ok = match order.side {
                    Side::Buy => order.price >= price,
                    Side::Sell => order.price <= price,
                };
                if !price_ok {
                    break;
                }
            }

            // 在该价格档位撮合
            while order.remaining().0 > 0 && !level.is_empty() {
                let maker = level.front_mut().unwrap();
                let fill_qty = Quantity(order.remaining().0.min(maker.remaining().0));

                // 创建成交记录
                let trade = Trade {
                    id: self.trade_id_counter.fetch_add(1, Ordering::SeqCst),
                    market: order.market,
                    taker_order_id: order.id,
                    maker_order_id: maker.id,
                    taker: order.account.clone(),
                    maker: maker.account.clone(),
                    price,
                    quantity: fill_qty,
                    taker_side: order.side,
                    taker_fee: config.calculate_taker_fee(price.mul_quantity(fill_qty)),
                    maker_fee: config.calculate_maker_fee(price.mul_quantity(fill_qty)),
                    timestamp: Timestamp::now(),
                    sequence: self.current_sequence,
                };

                trades.push(trade);

                // 更新数量
                order.filled_quantity.0 += fill_qty.0;
                maker.filled.0 += fill_qty.0;
                level.total_quantity.0 -= fill_qty.0;

                // 移除已完全成交的 Maker 订单
                if maker.remaining().0 == 0 {
                    let maker_id = maker.id;
                    level.orders.pop_front();
                    orderbook.order_index.remove(&maker_id);
                }
            }

            // 移除空的价格档位
            if level.is_empty() {
                match order.side {
                    Side::Buy => { orderbook.asks.pop_first(); }
                    Side::Sell => { orderbook.bids.pop_first(); }
                }
            }
        }

        // 更新最新成交价
        if let Some(trade) = trades.last() {
            orderbook.last_trade_price = Some(trade.price);
        }

        Ok(trades)
    }

    /// 取消订单
    pub fn cancel_order(&mut self, order_id: OrderId) -> Result<CancelResult> {
        // 找到订单所在的市场
        for (market_id, orderbook) in &mut self.orderbooks {
            if let Some(entry) = orderbook.remove_order(order_id) {
                return Ok(CancelResult {
                    order_id,
                    cancelled_quantity: entry.remaining(),
                    status: OrderStatus::Cancelled,
                });
            }
        }

        Err(DexError::OrderNotFound)
    }

    /// 验证订单
    fn validate_order(&self, order: &Order, config: &MarketConfig) -> Result<()> {
        // 检查市场是否活跃
        if !config.is_active {
            return Err(DexError::MarketInactive);
        }

        // 验证价格
        if order.order_type != OrderType::Market {
            if !config.validate_price(order.price) {
                return Err(DexError::InvalidTickSize);
            }
            if order.price.0 == 0 {
                return Err(DexError::InvalidPrice);
            }
        }

        // 验证数量
        if !config.validate_quantity(order.quantity) {
            return Err(DexError::InvalidLotSize);
        }
        if order.quantity.0 == 0 {
            return Err(DexError::InvalidQuantity);
        }
        if order.quantity > config.max_order_quantity {
            return Err(DexError::QuantityTooLarge);
        }

        // 验证订单价值
        let order_value = order.price.mul_quantity(order.quantity);
        if order_value < config.min_order_value as u128 {
            return Err(DexError::OrderValueTooSmall);
        }

        Ok(())
    }

    /// 检查订单是否会立即成交
    fn would_match(&self, order: &Order, orderbook: &Orderbook) -> bool {
        match order.side {
            Side::Buy => {
                if let Some(best_ask) = orderbook.best_ask() {
                    return order.price >= best_ask;
                }
            }
            Side::Sell => {
                if let Some(best_bid) = orderbook.best_bid() {
                    return order.price <= best_bid;
                }
            }
        }
        false
    }

    /// 检查 FOK 订单能否完全成交
    fn can_fill_completely(&self, order: &Order, orderbook: &Orderbook) -> bool {
        let mut remaining = order.quantity.0;

        let levels = match order.side {
            Side::Buy => orderbook.asks.iter()
                .take_while(|(p, _)| order.price >= **p)
                .map(|(_, l)| l),
            Side::Sell => orderbook.bids.iter()
                .take_while(|(Reverse(p), _)| order.price <= *p)
                .map(|(_, l)| l),
        };

        for level in levels {
            if remaining <= level.total_quantity.0 {
                return true;
            }
            remaining -= level.total_quantity.0;
        }

        false
    }

    /// 获取订单簿快照
    pub fn orderbook_snapshot(&self, market: MarketId, depth: usize) -> Option<OrderbookSnapshot> {
        self.orderbooks.get(&market).map(|ob| ob.snapshot(depth))
    }

    /// 设置当前序列号
    pub fn set_sequence(&mut self, seq: SequenceNumber) {
        self.current_sequence = seq;
    }
}
```

---

## 6. 存储层详细设计

### 6.1 DEX 状态存储

```rust
// ==================== crates/dex-storage/src/state.rs ====================

use dashmap::DashMap;
use parking_lot::RwLock;

/// DEX 状态存储
pub struct DexState {
    /// 账户余额
    balances: Arc<DashMap<AccountId, AccountBalances>>,

    /// 持仓
    positions: Arc<DashMap<(AccountId, MarketId), Position>>,

    /// 订单状态
    orders: Arc<DashMap<OrderId, OrderState>>,

    /// WAL 写入器
    wal: Arc<WriteAheadLog>,

    /// RocksDB 存储 (用于持久化)
    persistent_store: Arc<RocksDBStore>,

    /// 最后快照序列号
    last_snapshot_seq: AtomicU64,

    /// 指标
    metrics: Arc<StorageMetrics>,
}

/// 订单状态
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrderState {
    pub order: Order,
    pub status: OrderStatus,
    pub trades: Vec<Trade>,
}

impl DexState {
    pub fn new(config: StorageConfig) -> Result<Self> {
        let wal = WriteAheadLog::open(&config.wal_path)?;
        let persistent_store = RocksDBStore::open(&config.db_path)?;

        Ok(Self {
            balances: Arc::new(DashMap::new()),
            positions: Arc::new(DashMap::new()),
            orders: Arc::new(DashMap::new()),
            wal: Arc::new(wal),
            persistent_store: Arc::new(persistent_store),
            last_snapshot_seq: AtomicU64::new(0),
            metrics: Arc::new(StorageMetrics::new()),
        })
    }

    /// 从快照恢复
    pub async fn restore(&self) -> Result<()> {
        // 1. 加载最新快照
        let snapshot = self.persistent_store.load_latest_snapshot()?;

        if let Some(snap) = snapshot {
            // 恢复余额
            for (account, balances) in snap.balances {
                self.balances.insert(account, balances);
            }

            // 恢复持仓
            for ((account, market), position) in snap.positions {
                self.positions.insert((account, market), position);
            }

            self.last_snapshot_seq.store(snap.sequence.0, Ordering::SeqCst);
        }

        // 2. 重放 WAL
        let last_seq = self.last_snapshot_seq.load(Ordering::SeqCst);
        let wal_entries = self.wal.read_from(SequenceNumber(last_seq))?;

        for entry in wal_entries {
            self.apply_wal_entry(entry)?;
        }

        Ok(())
    }

    /// 应用 WAL 条目
    fn apply_wal_entry(&self, entry: WalEntry) -> Result<()> {
        match entry {
            WalEntry::BalanceUpdate { account, asset, delta, .. } => {
                self.update_balance_internal(&account, &asset, delta)?;
            }
            WalEntry::PositionUpdate { account, market, position, .. } => {
                self.positions.insert((account, market), position);
            }
            WalEntry::OrderUpdate { order_id, state, .. } => {
                self.orders.insert(order_id, state);
            }
        }
        Ok(())
    }

    // ===== 余额操作 =====

    /// 获取余额
    pub fn get_balance(&self, account: &AccountId, asset: &AssetId) -> Balance {
        self.balances
            .get(account)
            .and_then(|b| b.available.get(asset).copied())
            .unwrap_or(Balance(0))
    }

    /// 获取账户所有余额
    pub fn get_account_balances(&self, account: &AccountId) -> Option<AccountBalances> {
        self.balances.get(account).map(|r| r.clone())
    }

    /// 更新余额 (带 WAL)
    pub fn update_balance(
        &self,
        account: &AccountId,
        asset: &AssetId,
        delta: i128,
        sequence: SequenceNumber,
    ) -> Result<Balance> {
        // 1. 写入 WAL
        self.wal.append(WalEntry::BalanceUpdate {
            account: account.clone(),
            asset: asset.clone(),
            delta,
            sequence,
            timestamp: Timestamp::now(),
        })?;

        // 2. 更新内存
        self.update_balance_internal(account, asset, delta)
    }

    fn update_balance_internal(
        &self,
        account: &AccountId,
        asset: &AssetId,
        delta: i128,
    ) -> Result<Balance> {
        let mut balances = self.balances
            .entry(account.clone())
            .or_insert_with(AccountBalances::default);

        let current = balances.available
            .entry(asset.clone())
            .or_insert(Balance(0));

        let new_value = if delta >= 0 {
            current.0.checked_add(delta as u128)
                .ok_or(DexError::BalanceOverflow)?
        } else {
            let abs_delta = (-delta) as u128;
            if current.0 < abs_delta {
                return Err(DexError::InsufficientBalance);
            }
            current.0 - abs_delta
        };

        current.0 = new_value;
        Ok(*current)
    }

    /// 冻结余额
    pub fn freeze_balance(
        &self,
        account: &AccountId,
        asset: &AssetId,
        amount: Balance,
    ) -> Result<()> {
        let mut balances = self.balances
            .get_mut(account)
            .ok_or(DexError::AccountNotFound)?;

        balances.freeze(asset, amount)
    }

    /// 解冻余额
    pub fn unfreeze_balance(
        &self,
        account: &AccountId,
        asset: &AssetId,
        amount: Balance,
    ) -> Result<()> {
        let mut balances = self.balances
            .get_mut(account)
            .ok_or(DexError::AccountNotFound)?;

        balances.unfreeze(asset, amount)
    }

    // ===== 持仓操作 =====

    /// 获取持仓
    pub fn get_position(&self, account: &AccountId, market: &MarketId) -> Option<Position> {
        self.positions.get(&(account.clone(), *market)).map(|r| r.clone())
    }

    /// 更新持仓
    pub fn update_position(
        &self,
        account: &AccountId,
        market: &MarketId,
        position: Position,
        sequence: SequenceNumber,
    ) -> Result<()> {
        // 1. 写入 WAL
        self.wal.append(WalEntry::PositionUpdate {
            account: account.clone(),
            market: *market,
            position: position.clone(),
            sequence,
            timestamp: Timestamp::now(),
        })?;

        // 2. 更新内存
        self.positions.insert((account.clone(), *market), position);

        Ok(())
    }

    // ===== 快照 =====

    /// 创建快照
    pub async fn create_snapshot(&self, sequence: SequenceNumber) -> Result<()> {
        let snapshot = StateSnapshot {
            sequence,
            balances: self.balances.iter()
                .map(|r| (r.key().clone(), r.value().clone()))
                .collect(),
            positions: self.positions.iter()
                .map(|r| (r.key().clone(), r.value().clone()))
                .collect(),
            timestamp: Timestamp::now(),
        };

        // 写入持久化存储
        self.persistent_store.write_snapshot(&snapshot).await?;

        // 更新最后快照序列号
        self.last_snapshot_seq.store(sequence.0, Ordering::SeqCst);

        // 截断 WAL
        self.wal.truncate_before(sequence)?;

        Ok(())
    }
}

/// 状态快照
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StateSnapshot {
    pub sequence: SequenceNumber,
    pub balances: HashMap<AccountId, AccountBalances>,
    pub positions: HashMap<(AccountId, MarketId), Position>,
    pub timestamp: Timestamp,
}

/// WAL 条目
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum WalEntry {
    BalanceUpdate {
        account: AccountId,
        asset: AssetId,
        delta: i128,
        sequence: SequenceNumber,
        timestamp: Timestamp,
    },
    PositionUpdate {
        account: AccountId,
        market: MarketId,
        position: Position,
        sequence: SequenceNumber,
        timestamp: Timestamp,
    },
    OrderUpdate {
        order_id: OrderId,
        state: OrderState,
        sequence: SequenceNumber,
        timestamp: Timestamp,
    },
}
```

### 6.2 Write-Ahead Log

```rust
// ==================== crates/dex-storage/src/wal.rs ====================

use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Write, Seek, SeekFrom};
use std::path::Path;

/// WAL 配置
pub struct WalConfig {
    /// 同步间隔 (毫秒)
    pub sync_interval_ms: u64,

    /// 最大文件大小 (字节)
    pub max_file_size: u64,

    /// 最大保留文件数
    pub max_files: usize,
}

/// Write-Ahead Log
pub struct WriteAheadLog {
    /// 配置
    config: WalConfig,

    /// 当前文件
    current_file: RwLock<WalFile>,

    /// 文件目录
    dir: PathBuf,

    /// 当前序列号
    current_seq: AtomicU64,

    /// 同步通道
    sync_tx: mpsc::Sender<()>,
}

struct WalFile {
    file: BufWriter<File>,
    path: PathBuf,
    size: u64,
    start_seq: u64,
}

impl WriteAheadLog {
    /// 打开 WAL
    pub fn open(dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir)?;

        // 找到最新的 WAL 文件
        let (current_file, start_seq) = Self::open_or_create_latest(dir)?;

        let (sync_tx, sync_rx) = mpsc::channel(100);

        let wal = Self {
            config: WalConfig::default(),
            current_file: RwLock::new(current_file),
            dir: dir.to_path_buf(),
            current_seq: AtomicU64::new(start_seq),
            sync_tx,
        };

        // 启动同步任务
        wal.start_sync_task(sync_rx);

        Ok(wal)
    }

    /// 追加条目
    pub fn append(&self, entry: WalEntry) -> Result<()> {
        let mut file = self.current_file.write();

        // 序列化条目
        let data = bincode::serialize(&entry)?;
        let len = data.len() as u32;

        // 写入长度 + 数据
        file.file.write_all(&len.to_le_bytes())?;
        file.file.write_all(&data)?;
        file.size += 4 + data.len() as u64;

        // 检查是否需要轮转
        if file.size >= self.config.max_file_size {
            drop(file);
            self.rotate()?;
        }

        // 触发异步同步
        let _ = self.sync_tx.try_send(());

        Ok(())
    }

    /// 从指定序列号开始读取
    pub fn read_from(&self, from_seq: SequenceNumber) -> Result<Vec<WalEntry>> {
        let mut entries = Vec::new();

        // 获取所有 WAL 文件
        let files = self.list_wal_files()?;

        for file_path in files {
            let file = File::open(&file_path)?;
            let mut reader = BufReader::new(file);

            while let Ok(entry) = Self::read_entry(&mut reader) {
                let entry_seq = match &entry {
                    WalEntry::BalanceUpdate { sequence, .. } => sequence.0,
                    WalEntry::PositionUpdate { sequence, .. } => sequence.0,
                    WalEntry::OrderUpdate { sequence, .. } => sequence.0,
                };

                if entry_seq >= from_seq.0 {
                    entries.push(entry);
                }
            }
        }

        Ok(entries)
    }

    /// 截断指定序列号之前的条目
    pub fn truncate_before(&self, seq: SequenceNumber) -> Result<()> {
        let files = self.list_wal_files()?;

        for file_path in files {
            // 解析文件名中的起始序列号
            let file_start_seq = Self::parse_file_seq(&file_path)?;

            // 如果整个文件都在截断点之前, 删除它
            if file_start_seq < seq.0 {
                // 检查文件是否完全在截断点之前
                // (简化: 直接删除旧文件)
                std::fs::remove_file(file_path)?;
            }
        }

        Ok(())
    }

    /// 轮转文件
    fn rotate(&self) -> Result<()> {
        let mut file = self.current_file.write();

        // 同步当前文件
        file.file.flush()?;
        file.file.get_ref().sync_all()?;

        // 创建新文件
        let seq = self.current_seq.load(Ordering::SeqCst);
        let new_path = self.dir.join(format!("wal_{:020}.log", seq));
        let new_file = OpenOptions::new()
            .create(true)
            .write(true)
            .open(&new_path)?;

        *file = WalFile {
            file: BufWriter::new(new_file),
            path: new_path,
            size: 0,
            start_seq: seq,
        };

        Ok(())
    }

    fn read_entry<R: Read>(reader: &mut R) -> Result<WalEntry> {
        let mut len_buf = [0u8; 4];
        reader.read_exact(&mut len_buf)?;
        let len = u32::from_le_bytes(len_buf) as usize;

        let mut data = vec![0u8; len];
        reader.read_exact(&mut data)?;

        Ok(bincode::deserialize(&data)?)
    }

    fn open_or_create_latest(dir: &Path) -> Result<(WalFile, u64)> {
        todo!()
    }

    fn list_wal_files(&self) -> Result<Vec<PathBuf>> {
        todo!()
    }

    fn parse_file_seq(path: &Path) -> Result<u64> {
        todo!()
    }

    fn start_sync_task(&self, rx: mpsc::Receiver<()>) {
        todo!()
    }
}
```

---

## 7. 永续合约详细设计

### 7.1 资金费率

```rust
// ==================== crates/dex-perpetuals/src/funding.rs ====================

/// 资金费率计算器
pub struct FundingRateCalculator {
    /// 资金费率间隔 (秒)
    interval_secs: u64,

    /// 年化利率
    interest_rate: f64,

    /// 溢价限制
    premium_cap: f64,

    /// 历史费率
    history: Vec<FundingRateRecord>,
}

/// 资金费率记录
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FundingRateRecord {
    pub market: MarketId,
    pub rate: f64,
    pub mark_price: Price,
    pub index_price: Price,
    pub timestamp: Timestamp,
}

impl FundingRateCalculator {
    pub fn new(interval_secs: u64) -> Self {
        Self {
            interval_secs,
            interest_rate: 0.0001,  // 0.01% per 8 hours = 0.0001 per hour
            premium_cap: 0.0005,    // 0.05%
            history: Vec::new(),
        }
    }

    /// 计算资金费率
    ///
    /// 公式:
    /// Premium = (Mark Price - Index Price) / Index Price
    /// Funding Rate = Premium + Clamp(Interest - Premium, -cap, cap)
    pub fn calculate(&self, mark_price: Price, index_price: Price) -> f64 {
        let mark = mark_price.to_f64();
        let index = index_price.to_f64();

        if index == 0.0 {
            return 0.0;
        }

        // 计算溢价
        let premium = (mark - index) / index;

        // 计算利率分量
        let interest_component = self.interest_rate;

        // Clamp 函数
        let clamped = (interest_component - premium)
            .clamp(-self.premium_cap, self.premium_cap);

        // 最终资金费率
        premium + clamped
    }

    /// 应用资金费率到所有持仓
    pub fn apply_funding(
        &self,
        state: &DexState,
        market: MarketId,
        rate: f64,
        mark_price: Price,
    ) -> Result<Vec<FundingPayment>> {
        let mut payments = Vec::new();

        // 遍历所有持仓
        for entry in state.positions.iter() {
            let ((account, pos_market), position) = entry.pair();

            if *pos_market != market {
                continue;
            }

            // 计算资金费用
            let notional = (mark_price.0 as f64 * position.size.0 as f64)
                / (Price::SCALE as f64 * Quantity::SCALE as f64);
            let payment = (notional * rate) as i128;

            // 多仓支付, 空仓收取 (当 rate > 0 时)
            let adjusted_payment = match position.side {
                Side::Buy => -payment,   // 多仓: 支付 (负数)
                Side::Sell => payment,   // 空仓: 收取 (正数)
            };

            payments.push(FundingPayment {
                account: account.clone(),
                market,
                payment: adjusted_payment,
                rate,
                mark_price,
                timestamp: Timestamp::now(),
            });
        }

        Ok(payments)
    }

    /// 记录历史费率
    pub fn record(&mut self, record: FundingRateRecord) {
        self.history.push(record);

        // 保留最近 30 天的记录
        let cutoff = Timestamp::now().0 - 30 * 24 * 3600 * 1_000_000_000;
        self.history.retain(|r| r.timestamp.0 >= cutoff);
    }

    /// 获取历史费率
    pub fn get_history(&self, market: MarketId, limit: usize) -> Vec<FundingRateRecord> {
        self.history.iter()
            .filter(|r| r.market == market)
            .rev()
            .take(limit)
            .cloned()
            .collect()
    }
}

/// 资金费用支付
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FundingPayment {
    pub account: AccountId,
    pub market: MarketId,
    pub payment: i128,  // 正数=收取, 负数=支付
    pub rate: f64,
    pub mark_price: Price,
    pub timestamp: Timestamp,
}
```

### 7.2 清算引擎

```rust
// ==================== crates/dex-perpetuals/src/liquidation.rs ====================

/// 清算引擎
pub struct LiquidationEngine {
    /// 维持保证金率 (bps)
    maintenance_margin_bps: u16,

    /// 清算惩罚率 (bps)
    liquidation_penalty_bps: u16,

    /// 保险基金
    insurance_fund: Arc<RwLock<InsuranceFund>>,

    /// 清算订单队列
    liquidation_queue: VecDeque<LiquidationOrder>,
}

/// 保险基金
pub struct InsuranceFund {
    /// 各资产余额
    balances: HashMap<AssetId, Balance>,
}

/// 清算订单
#[derive(Clone, Debug)]
pub struct LiquidationOrder {
    pub account: AccountId,
    pub market: MarketId,
    pub side: Side,
    pub size: Quantity,
    pub bankruptcy_price: Price,
    pub timestamp: Timestamp,
}

/// 清算结果
#[derive(Clone, Debug)]
pub struct LiquidationResult {
    pub account: AccountId,
    pub market: MarketId,
    pub liquidated_size: Quantity,
    pub liquidation_price: Price,
    pub penalty: u64,
    pub insurance_payout: i128,
    pub remaining_margin: i128,
}

impl LiquidationEngine {
    pub fn new(maintenance_margin_bps: u16, liquidation_penalty_bps: u16) -> Self {
        Self {
            maintenance_margin_bps,
            liquidation_penalty_bps,
            insurance_fund: Arc::new(RwLock::new(InsuranceFund {
                balances: HashMap::new(),
            })),
            liquidation_queue: VecDeque::new(),
        }
    }

    /// 检查账户是否需要清算
    pub fn check_liquidation(
        &self,
        position: &Position,
        mark_price: Price,
    ) -> Option<LiquidationOrder> {
        let margin_ratio = position.margin_ratio(mark_price);
        let mm_ratio = self.maintenance_margin_bps as f64 / 10000.0;

        if margin_ratio < mm_ratio {
            // 计算破产价格
            let bankruptcy_price = self.calculate_bankruptcy_price(position);

            Some(LiquidationOrder {
                account: position.account.clone(),
                market: position.market,
                side: position.side.opposite(),  // 清算方向与持仓相反
                size: position.size,
                bankruptcy_price,
                timestamp: Timestamp::now(),
            })
        } else {
            None
        }
    }

    /// 计算破产价格
    fn calculate_bankruptcy_price(&self, position: &Position) -> Price {
        let margin = position.margin.0 as f64;
        let size = position.size.0 as f64 / Quantity::SCALE as f64;
        let entry = position.entry_price.0 as f64 / Price::SCALE as f64;

        let bankruptcy_price = match position.side {
            Side::Buy => entry - (margin / size),
            Side::Sell => entry + (margin / size),
        };

        Price::from_f64(bankruptcy_price.max(0.0))
    }

    /// 执行清算
    pub fn execute_liquidation(
        &mut self,
        order: &LiquidationOrder,
        execution_price: Price,
        state: &DexState,
    ) -> Result<LiquidationResult> {
        // 获取持仓
        let position = state.get_position(&order.account, &order.market)
            .ok_or(DexError::PositionNotFound)?;

        // 计算清算价值
        let liquidation_value = execution_price.mul_quantity(order.size);

        // 计算惩罚
        let penalty = (liquidation_value * self.liquidation_penalty_bps as u128 / 10000) as u64;

        // 计算盈亏
        let pnl = position.unrealized_pnl(execution_price);

        // 剩余保证金
        let remaining_margin = position.margin.0 as i128 + pnl - penalty as i128;

        // 计算保险基金收支
        let insurance_payout = if remaining_margin < 0 {
            // 保证金不足, 保险基金赔付
            remaining_margin
        } else {
            // 有盈余, 充入保险基金
            penalty as i128
        };

        // 更新保险基金
        {
            let mut fund = self.insurance_fund.write();
            let quote_asset = self.get_quote_asset(order.market);
            let balance = fund.balances.entry(quote_asset).or_insert(Balance(0));

            if insurance_payout > 0 {
                balance.0 += insurance_payout as u128;
            } else {
                let payout = (-insurance_payout) as u128;
                if balance.0 >= payout {
                    balance.0 -= payout;
                } else {
                    // 保险基金不足, 社会化损失
                    tracing::warn!("Insurance fund insufficient for liquidation");
                    balance.0 = 0;
                }
            }
        }

        Ok(LiquidationResult {
            account: order.account.clone(),
            market: order.market,
            liquidated_size: order.size,
            liquidation_price: execution_price,
            penalty,
            insurance_payout,
            remaining_margin,
        })
    }

    /// 批量检查清算
    pub fn scan_liquidations(
        &self,
        state: &DexState,
        market: MarketId,
        mark_price: Price,
    ) -> Vec<LiquidationOrder> {
        let mut liquidations = Vec::new();

        for entry in state.positions.iter() {
            let ((account, pos_market), position) = entry.pair();

            if *pos_market != market {
                continue;
            }

            if let Some(order) = self.check_liquidation(position, mark_price) {
                liquidations.push(order);
            }
        }

        // 按保证金率排序 (最危险的优先)
        liquidations.sort_by(|a, b| {
            let pos_a = state.get_position(&a.account, &a.market).unwrap();
            let pos_b = state.get_position(&b.account, &b.market).unwrap();

            let ratio_a = pos_a.margin_ratio(mark_price);
            let ratio_b = pos_b.margin_ratio(mark_price);

            ratio_a.partial_cmp(&ratio_b).unwrap()
        });

        liquidations
    }

    fn get_quote_asset(&self, market: MarketId) -> AssetId {
        // TODO: 从市场配置获取
        todo!()
    }
}
```

### 7.3 保证金管理

```rust
// ==================== crates/dex-perpetuals/src/margin.rs ====================

/// 保证金管理器
pub struct MarginManager {
    /// 市场配置
    markets: HashMap<MarketId, MarketConfig>,

    /// 账户风险状态
    account_risks: Arc<DashMap<AccountId, AccountRisk>>,
}

/// 账户风险状态
#[derive(Clone, Debug)]
pub struct AccountRisk {
    /// 总权益
    pub equity: i128,

    /// 已用保证金
    pub used_margin: u128,

    /// 可用保证金
    pub available_margin: i128,

    /// 保证金率
    pub margin_ratio: f64,

    /// 未实现盈亏
    pub unrealized_pnl: i128,

    /// 最后更新时间
    pub updated_at: Timestamp,
}

/// 保证金检查结果
#[derive(Clone, Debug)]
pub struct MarginCheck {
    pub is_valid: bool,
    pub required_margin: u128,
    pub available_margin: i128,
    pub new_margin_ratio: f64,
    pub reason: Option<String>,
}

impl MarginManager {
    pub fn new() -> Self {
        Self {
            markets: HashMap::new(),
            account_risks: Arc::new(DashMap::new()),
        }
    }

    /// 添加市场配置
    pub fn add_market(&mut self, config: MarketConfig) {
        self.markets.insert(config.id, config);
    }

    /// 计算订单所需保证金
    pub fn calculate_order_margin(
        &self,
        order: &Order,
        mark_price: Price,
    ) -> Result<u128> {
        let config = self.markets.get(&order.market)
            .ok_or(DexError::MarketNotFound)?;

        let im_bps = config.initial_margin_bps
            .ok_or(DexError::NotPerpetualMarket)?;

        // 名义价值 = 价格 * 数量
        let notional = mark_price.mul_quantity(order.quantity);

        // 初始保证金 = 名义价值 * 初始保证金率
        let margin = notional * im_bps as u128 / 10000;

        Ok(margin)
    }

    /// 检查订单保证金
    pub fn check_order_margin(
        &self,
        account: &AccountId,
        order: &Order,
        state: &DexState,
        mark_price: Price,
    ) -> Result<MarginCheck> {
        // 获取账户余额
        let balances = state.get_account_balances(account)
            .unwrap_or_default();

        let config = self.markets.get(&order.market)
            .ok_or(DexError::MarketNotFound)?;

        // 获取报价资产余额
        let quote_asset = &config.quote_asset;
        let available = balances.available(quote_asset);

        // 计算所需保证金
        let required = self.calculate_order_margin(order, mark_price)?;

        // 获取现有持仓
        let existing_position = state.get_position(account, &order.market);

        // 计算新的保证金率
        let new_margin_ratio = self.calculate_new_margin_ratio(
            &existing_position,
            order,
            available,
            mark_price,
        );

        let im_ratio = config.initial_margin_bps.unwrap_or(100) as f64 / 10000.0;

        let is_valid = available.0 as i128 >= required as i128
            && new_margin_ratio >= im_ratio;

        Ok(MarginCheck {
            is_valid,
            required_margin: required,
            available_margin: available.0 as i128,
            new_margin_ratio,
            reason: if !is_valid {
                Some("Insufficient margin".to_string())
            } else {
                None
            },
        })
    }

    /// 计算新的保证金率
    fn calculate_new_margin_ratio(
        &self,
        existing: &Option<Position>,
        order: &Order,
        available: Balance,
        mark_price: Price,
    ) -> f64 {
        let mut total_margin = available.0 as i128;
        let mut total_notional = 0i128;

        // 加入现有持仓
        if let Some(pos) = existing {
            total_margin += pos.margin.0 as i128;
            total_margin += pos.unrealized_pnl(mark_price);

            let pos_notional = (mark_price.0 as i128 * pos.size.0 as i128)
                / (Price::SCALE as i128);

            // 同方向加仓或反方向减仓
            if pos.side == order.side {
                total_notional = pos_notional + self.order_notional(order, mark_price);
            } else {
                total_notional = (pos_notional - self.order_notional(order, mark_price)).abs();
            }
        } else {
            total_notional = self.order_notional(order, mark_price);
        }

        if total_notional == 0 {
            return f64::MAX;
        }

        total_margin as f64 / total_notional as f64
    }

    fn order_notional(&self, order: &Order, mark_price: Price) -> i128 {
        (mark_price.0 as i128 * order.quantity.0 as i128) / (Price::SCALE as i128)
    }

    /// 更新账户风险状态
    pub fn update_account_risk(
        &self,
        account: &AccountId,
        state: &DexState,
        mark_prices: &HashMap<MarketId, Price>,
    ) -> AccountRisk {
        let balances = state.get_account_balances(account)
            .unwrap_or_default();

        let mut total_equity = 0i128;
        let mut total_used_margin = 0u128;
        let mut total_unrealized_pnl = 0i128;

        // 计算所有资产的价值
        for (asset, balance) in &balances.available {
            // TODO: 获取资产价格并转换为 USD 价值
            total_equity += balance.0 as i128;
        }

        // 计算持仓的未实现盈亏和占用保证金
        for market_id in self.markets.keys() {
            if let Some(position) = state.get_position(account, market_id) {
                if let Some(mark_price) = mark_prices.get(market_id) {
                    let pnl = position.unrealized_pnl(*mark_price);
                    total_unrealized_pnl += pnl;
                    total_used_margin += position.margin.0;
                }
            }
        }

        total_equity += total_unrealized_pnl;
        let available_margin = total_equity - total_used_margin as i128;

        let margin_ratio = if total_used_margin > 0 {
            total_equity as f64 / total_used_margin as f64
        } else {
            f64::MAX
        };

        let risk = AccountRisk {
            equity: total_equity,
            used_margin: total_used_margin,
            available_margin,
            margin_ratio,
            unrealized_pnl: total_unrealized_pnl,
            updated_at: Timestamp::now(),
        };

        self.account_risks.insert(account.clone(), risk.clone());

        risk
    }
}
```

---

## 8. Sui 集成层设计

### 8.1 交易路由

```rust
// ==================== Modified: crates/sui-core/src/authority.rs ====================

/// DEX 交易类型识别
pub fn is_dex_transaction(tx: &VerifiedTransaction) -> bool {
    match tx.data().transaction_data().kind() {
        TransactionKind::ProgrammableTransaction(pt) => {
            pt.commands.iter().any(|cmd| {
                match cmd {
                    Command::MoveCall(call) => {
                        // 检查是否调用 DEX 模块
                        call.package == DEX_PACKAGE_ID
                            && DEX_MODULES.contains(&call.module.as_str())
                    }
                    _ => false,
                }
            })
        }
        _ => false,
    }
}

/// DEX 模块列表
const DEX_MODULES: &[&str] = &[
    "orderbook",
    "perpetuals",
    "margin",
    "account",
];

impl AuthorityState {
    /// 修改后的交易处理入口
    pub async fn handle_transaction(
        &self,
        tx: VerifiedTransaction,
    ) -> Result<TransactionEffects> {
        // 1. 检查是否为 DEX 交易
        if is_dex_transaction(&tx) {
            return self.handle_dex_transaction(tx).await;
        }

        // 2. 检查是否涉及共享对象
        if self.has_shared_objects(&tx) {
            return self.submit_to_consensus(tx).await;
        }

        // 3. 快速路径 (仅 owned objects)
        self.try_execute_immediately(tx).await
    }

    /// 处理 DEX 交易
    async fn handle_dex_transaction(
        &self,
        tx: VerifiedTransaction,
    ) -> Result<TransactionEffects> {
        // 1. 提交给 Sequencer
        let receipt = self.dex_sequencer.submit(tx.clone().into()).await?;

        // 2. 等待软确认
        // (Sequencer 会立即返回, 无需等待共识)

        // 3. 执行交易 (通过 DEX Precompile)
        let effects = self.execute_dex_transaction(tx, receipt.sequence).await?;

        Ok(effects)
    }

    /// 执行 DEX 交易
    async fn execute_dex_transaction(
        &self,
        tx: VerifiedTransaction,
        sequence: SequenceNumber,
    ) -> Result<TransactionEffects> {
        // 解析 DEX 操作
        let dex_op = self.parse_dex_operation(&tx)?;

        // 通过 Precompile 执行
        let result = self.dex_precompile.execute(dex_op, sequence).await?;

        // 生成 Sui 兼容的 Effects
        let effects = self.generate_dex_effects(&tx, &result)?;

        Ok(effects)
    }
}
```

### 8.2 DEX Precompile

```rust
// ==================== crates/dex-engine/src/precompile.rs ====================

use sui_types::effects::TransactionEffects;

/// DEX Precompile
pub struct DexPrecompile {
    /// 撮合引擎
    engine: Arc<RwLock<MatchingEngine>>,

    /// 状态存储
    state: Arc<DexState>,

    /// 永续合约模块
    perpetuals: Arc<PerpetualEngine>,

    /// 风险引擎
    risk: Arc<RiskEngine>,
}

/// DEX 操作类型
#[derive(Clone, Debug)]
pub enum DexOperation {
    /// 下单
    PlaceOrder {
        account: AccountId,
        market: MarketId,
        side: Side,
        price: Price,
        quantity: Quantity,
        order_type: OrderType,
        reduce_only: bool,
    },

    /// 取消订单
    CancelOrder {
        account: AccountId,
        order_id: OrderId,
    },

    /// 存款
    Deposit {
        account: AccountId,
        asset: AssetId,
        amount: Balance,
    },

    /// 提款
    Withdraw {
        account: AccountId,
        asset: AssetId,
        amount: Balance,
    },

    /// 调整保证金
    AdjustMargin {
        account: AccountId,
        market: MarketId,
        amount: i128,
    },
}

/// DEX 执行结果
#[derive(Clone, Debug)]
pub struct DexExecutionResult {
    pub sequence: SequenceNumber,
    pub operation: DexOperation,
    pub success: bool,
    pub match_result: Option<MatchResult>,
    pub balance_changes: Vec<BalanceChange>,
    pub position_changes: Vec<PositionChange>,
    pub events: Vec<DexEvent>,
    pub error: Option<DexError>,
}

/// 余额变更
#[derive(Clone, Debug)]
pub struct BalanceChange {
    pub account: AccountId,
    pub asset: AssetId,
    pub before: Balance,
    pub after: Balance,
    pub reason: ChangeReason,
}

/// 持仓变更
#[derive(Clone, Debug)]
pub struct PositionChange {
    pub account: AccountId,
    pub market: MarketId,
    pub before: Option<Position>,
    pub after: Option<Position>,
}

impl DexPrecompile {
    pub fn new(
        engine: Arc<RwLock<MatchingEngine>>,
        state: Arc<DexState>,
        perpetuals: Arc<PerpetualEngine>,
        risk: Arc<RiskEngine>,
    ) -> Self {
        Self {
            engine,
            state,
            perpetuals,
            risk,
        }
    }

    /// 执行 DEX 操作
    pub async fn execute(
        &self,
        op: DexOperation,
        sequence: SequenceNumber,
    ) -> Result<DexExecutionResult> {
        let start = Instant::now();

        let result = match &op {
            DexOperation::PlaceOrder { .. } => {
                self.execute_place_order(&op, sequence).await
            }
            DexOperation::CancelOrder { account, order_id } => {
                self.execute_cancel_order(account, *order_id, sequence).await
            }
            DexOperation::Deposit { account, asset, amount } => {
                self.execute_deposit(account, asset, *amount, sequence).await
            }
            DexOperation::Withdraw { account, asset, amount } => {
                self.execute_withdraw(account, asset, *amount, sequence).await
            }
            DexOperation::AdjustMargin { account, market, amount } => {
                self.execute_adjust_margin(account, market, *amount, sequence).await
            }
        };

        tracing::debug!(
            "DEX operation executed in {:?}: {:?}",
            start.elapsed(),
            result.as_ref().map(|r| r.success)
        );

        result
    }

    /// 执行下单
    async fn execute_place_order(
        &self,
        op: &DexOperation,
        sequence: SequenceNumber,
    ) -> Result<DexExecutionResult> {
        let DexOperation::PlaceOrder {
            account,
            market,
            side,
            price,
            quantity,
            order_type,
            reduce_only,
        } = op else {
            unreachable!()
        };

        // 1. 获取当前标记价格
        let mark_price = self.get_mark_price(market)?;

        // 2. 构建订单
        let mut order = Order {
            id: OrderId(0),  // 将由引擎分配
            account: account.clone(),
            market: *market,
            side: *side,
            price: *price,
            quantity: *quantity,
            filled_quantity: Quantity(0),
            order_type: *order_type,
            time_in_force: TimeInForce::GoodTillCancel,
            reduce_only: *reduce_only,
            client_order_id: None,
            created_at: Timestamp::now(),
            updated_at: Timestamp::now(),
            sequence,
        };

        // 3. 风险检查
        let margin_check = self.risk.check_order_margin(
            account,
            &order,
            &self.state,
            mark_price,
        )?;

        if !margin_check.is_valid {
            return Ok(DexExecutionResult {
                sequence,
                operation: op.clone(),
                success: false,
                match_result: None,
                balance_changes: vec![],
                position_changes: vec![],
                events: vec![],
                error: Some(DexError::InsufficientMargin),
            });
        }

        // 4. 冻结保证金
        if !reduce_only {
            self.state.freeze_balance(
                account,
                &self.get_quote_asset(market)?,
                Balance(margin_check.required_margin),
            )?;
        }

        // 5. 执行撮合
        let match_result = {
            let mut engine = self.engine.write();
            engine.set_sequence(sequence);
            engine.process_order(order.clone())?
        };

        // 6. 处理成交结果
        let (balance_changes, position_changes) = self.process_trades(
            &match_result.trades,
            sequence,
        ).await?;

        // 7. 生成事件
        let events = self.generate_order_events(&order, &match_result);

        Ok(DexExecutionResult {
            sequence,
            operation: op.clone(),
            success: true,
            match_result: Some(match_result),
            balance_changes,
            position_changes,
            events,
            error: None,
        })
    }

    /// 处理成交
    async fn process_trades(
        &self,
        trades: &[Trade],
        sequence: SequenceNumber,
    ) -> Result<(Vec<BalanceChange>, Vec<PositionChange>)> {
        let mut balance_changes = Vec::new();
        let mut position_changes = Vec::new();

        for trade in trades {
            // 获取市场配置
            let config = self.get_market_config(&trade.market)?;

            match config.market_type {
                MarketType::Spot => {
                    // 现货: 直接交换资产
                    let (taker_changes, maker_changes) =
                        self.process_spot_trade(trade, &config, sequence)?;
                    balance_changes.extend(taker_changes);
                    balance_changes.extend(maker_changes);
                }
                MarketType::Perpetual => {
                    // 永续: 更新持仓
                    let (taker_pos, maker_pos) =
                        self.process_perpetual_trade(trade, sequence)?;
                    position_changes.push(taker_pos);
                    position_changes.push(maker_pos);
                }
                _ => {}
            }
        }

        Ok((balance_changes, position_changes))
    }

    /// 处理现货成交
    fn process_spot_trade(
        &self,
        trade: &Trade,
        config: &MarketConfig,
        sequence: SequenceNumber,
    ) -> Result<(Vec<BalanceChange>, Vec<BalanceChange>)> {
        let base = &config.base_asset;
        let quote = &config.quote_asset;

        let base_amount = Balance(trade.quantity.0 as u128);
        let quote_amount = Balance(trade.price.mul_quantity(trade.quantity));

        let mut taker_changes = Vec::new();
        let mut maker_changes = Vec::new();

        match trade.taker_side {
            Side::Buy => {
                // Taker 买入: -quote, +base
                taker_changes.push(self.update_balance_with_change(
                    &trade.taker, quote, -(quote_amount.0 as i128), sequence,
                )?);
                taker_changes.push(self.update_balance_with_change(
                    &trade.taker, base, base_amount.0 as i128, sequence,
                )?);

                // Maker 卖出: +quote, -base
                maker_changes.push(self.update_balance_with_change(
                    &trade.maker, quote, quote_amount.0 as i128, sequence,
                )?);
                maker_changes.push(self.update_balance_with_change(
                    &trade.maker, base, -(base_amount.0 as i128), sequence,
                )?);
            }
            Side::Sell => {
                // Taker 卖出: +quote, -base
                taker_changes.push(self.update_balance_with_change(
                    &trade.taker, quote, quote_amount.0 as i128, sequence,
                )?);
                taker_changes.push(self.update_balance_with_change(
                    &trade.taker, base, -(base_amount.0 as i128), sequence,
                )?);

                // Maker 买入: -quote, +base
                maker_changes.push(self.update_balance_with_change(
                    &trade.maker, quote, -(quote_amount.0 as i128), sequence,
                )?);
                maker_changes.push(self.update_balance_with_change(
                    &trade.maker, base, base_amount.0 as i128, sequence,
                )?);
            }
        }

        // 扣除手续费
        taker_changes.push(self.update_balance_with_change(
            &trade.taker, quote, -(trade.taker_fee as i128), sequence,
        )?);

        if trade.maker_fee > 0 {
            maker_changes.push(self.update_balance_with_change(
                &trade.maker, quote, -(trade.maker_fee as i128), sequence,
            )?);
        } else {
            // 返佣
            maker_changes.push(self.update_balance_with_change(
                &trade.maker, quote, (-trade.maker_fee) as i128, sequence,
            )?);
        }

        Ok((taker_changes, maker_changes))
    }

    fn update_balance_with_change(
        &self,
        account: &AccountId,
        asset: &AssetId,
        delta: i128,
        sequence: SequenceNumber,
    ) -> Result<BalanceChange> {
        let before = self.state.get_balance(account, asset);
        let after = self.state.update_balance(account, asset, delta, sequence)?;

        Ok(BalanceChange {
            account: account.clone(),
            asset: asset.clone(),
            before,
            after,
            reason: ChangeReason::Trade,
        })
    }

    fn get_mark_price(&self, market: &MarketId) -> Result<Price> {
        // TODO: 从价格预言机获取
        Ok(Price(100 * Price::SCALE))
    }

    fn get_quote_asset(&self, market: &MarketId) -> Result<AssetId> {
        let config = self.get_market_config(market)?;
        Ok(config.quote_asset.clone())
    }

    fn get_market_config(&self, market: &MarketId) -> Result<MarketConfig> {
        // TODO: 从配置获取
        todo!()
    }

    fn generate_order_events(&self, order: &Order, result: &MatchResult) -> Vec<DexEvent> {
        // TODO: 生成事件
        vec![]
    }

    fn process_perpetual_trade(
        &self,
        trade: &Trade,
        sequence: SequenceNumber,
    ) -> Result<(PositionChange, PositionChange)> {
        // TODO: 处理永续合约成交
        todo!()
    }
}

#[derive(Clone, Debug)]
pub enum ChangeReason {
    Trade,
    Deposit,
    Withdraw,
    Fee,
    Funding,
    Liquidation,
    MarginAdjust,
}
```

---

## 9. 网络协议设计

### 9.1 Sequencer 网络消息

```rust
// ==================== crates/dex-sequencer/src/network.rs ====================

use serde::{Serialize, Deserialize};

/// Sequencer 网络消息
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SequencerMessage {
    /// 交易提交
    SubmitTransaction(DexTransaction),

    /// 序列批次
    SequenceBatch(SignedSequenceBatch),

    /// 批次确认
    BatchConfirmation(BatchConfirmation),

    /// 心跳
    Heartbeat(Heartbeat),

    /// 故障投票
    FailureVote(SignedFailureVote),

    /// Leader 变更
    LeaderChange(SignedLeaderChange),

    /// 同步请求
    SyncRequest(SyncRequest),

    /// 同步响应
    SyncResponse(SyncResponse),
}

/// 签名的序列批次
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SignedSequenceBatch {
    pub batch: SequenceBatch,
    pub signature: Signature,
}

/// 批次确认
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BatchConfirmation {
    pub validator: ValidatorId,
    pub sequence_range: (u64, u64),
    pub batch_hash: [u8; 32],
    pub signature: Signature,
}

/// 签名的故障投票
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SignedFailureVote {
    pub vote: FailureVote,
    pub signature: Signature,
}

/// 签名的 Leader 变更
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SignedLeaderChange {
    pub change: LeaderChange,
    pub signatures: Vec<(ValidatorId, Signature)>,
}

/// 同步请求
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SyncRequest {
    pub from_sequence: u64,
    pub to_sequence: Option<u64>,
    pub requester: ValidatorId,
}

/// 同步响应
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SyncResponse {
    pub batches: Vec<SignedSequenceBatch>,
    pub provider: ValidatorId,
}

/// Sequencer 网络接口
#[async_trait]
pub trait SequencerNetwork: Send + Sync {
    /// 发送序列批次
    async fn send_batch(
        &self,
        to: ValidatorId,
        batch: SignedSequenceBatch,
    ) -> Result<()>;

    /// 广播序列批次
    async fn broadcast_batch(&self, batch: SignedSequenceBatch) -> Result<()>;

    /// 发送心跳
    async fn send_heartbeat(&self, to: ValidatorId, heartbeat: Heartbeat) -> Result<()>;

    /// 发送故障投票
    async fn send_failure_vote(&self, to: ValidatorId, vote: SignedFailureVote) -> Result<()>;

    /// 发送 Leader 变更
    async fn send_leader_change(&self, to: ValidatorId, change: SignedLeaderChange) -> Result<()>;

    /// 转发交易给 Leader
    async fn forward_to_leader(&self, leader: ValidatorId, tx: DexTransaction) -> Result<SequenceReceipt>;

    /// 请求同步
    async fn request_sync(&self, from: ValidatorId, request: SyncRequest) -> Result<SyncResponse>;
}
```

---

## 10. API 接口设计

### 10.1 REST API

```yaml
# ==================== API Specification ====================

openapi: 3.0.0
info:
  title: DEX L1 Trading API
  version: 1.0.0

paths:
  /api/v1/orders:
    post:
      summary: Place a new order
      requestBody:
        content:
          application/json:
            schema:
              $ref: '#/components/schemas/PlaceOrderRequest'
      responses:
        '200':
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/OrderResponse'

    get:
      summary: List open orders
      parameters:
        - name: market
          in: query
          schema:
            type: string
        - name: side
          in: query
          schema:
            type: string
            enum: [buy, sell]
      responses:
        '200':
          content:
            application/json:
              schema:
                type: array
                items:
                  $ref: '#/components/schemas/Order'

  /api/v1/orders/{orderId}:
    delete:
      summary: Cancel an order
      parameters:
        - name: orderId
          in: path
          required: true
          schema:
            type: string
      responses:
        '200':
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/CancelResponse'

  /api/v1/orderbook/{market}:
    get:
      summary: Get orderbook snapshot
      parameters:
        - name: market
          in: path
          required: true
          schema:
            type: string
        - name: depth
          in: query
          schema:
            type: integer
            default: 20
      responses:
        '200':
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/OrderbookSnapshot'

  /api/v1/account/balances:
    get:
      summary: Get account balances
      responses:
        '200':
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/AccountBalances'

  /api/v1/account/positions:
    get:
      summary: Get account positions
      responses:
        '200':
          content:
            application/json:
              schema:
                type: array
                items:
                  $ref: '#/components/schemas/Position'

  /api/v1/trades:
    get:
      summary: Get trade history
      parameters:
        - name: market
          in: query
          schema:
            type: string
        - name: limit
          in: query
          schema:
            type: integer
            default: 100
      responses:
        '200':
          content:
            application/json:
              schema:
                type: array
                items:
                  $ref: '#/components/schemas/Trade'

components:
  schemas:
    PlaceOrderRequest:
      type: object
      required:
        - market
        - side
        - quantity
        - orderType
      properties:
        market:
          type: string
        side:
          type: string
          enum: [buy, sell]
        price:
          type: string
          description: Required for limit orders
        quantity:
          type: string
        orderType:
          type: string
          enum: [limit, market, postOnly, ioc, fok]
        reduceOnly:
          type: boolean
          default: false
        clientOrderId:
          type: string

    OrderResponse:
      type: object
      properties:
        orderId:
          type: string
        sequence:
          type: integer
        status:
          type: string
        timestamp:
          type: integer
```

### 10.2 WebSocket API

```rust
// ==================== WebSocket Subscriptions ====================

/// WebSocket 消息类型
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WsMessage {
    /// 订阅请求
    Subscribe {
        channel: Channel,
        market: Option<MarketId>,
    },

    /// 取消订阅
    Unsubscribe {
        channel: Channel,
        market: Option<MarketId>,
    },

    /// 订单簿更新
    OrderbookUpdate {
        market: MarketId,
        bids: Vec<(Price, Quantity)>,
        asks: Vec<(Price, Quantity)>,
        timestamp: u64,
    },

    /// 成交更新
    TradeUpdate {
        market: MarketId,
        trades: Vec<Trade>,
    },

    /// 订单更新
    OrderUpdate {
        order: Order,
        status: OrderStatus,
    },

    /// 持仓更新
    PositionUpdate {
        position: Position,
    },

    /// 余额更新
    BalanceUpdate {
        asset: AssetId,
        available: Balance,
        frozen: Balance,
    },
}

/// 订阅频道
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Channel {
    /// 订单簿 (市场级别)
    Orderbook,

    /// 成交 (市场级别)
    Trades,

    /// 订单 (账户级别)
    Orders,

    /// 持仓 (账户级别)
    Positions,

    /// 余额 (账户级别)
    Balances,
}
```

---

## 11. 安全性设计

### 11.1 签名验证

```rust
/// 交易签名验证
pub fn verify_transaction_signature(tx: &DexTransaction) -> Result<()> {
    let message = tx.signing_message();
    let signature = &tx.signature;
    let public_key = &tx.sender_public_key;

    if !signature.verify(&message, public_key) {
        return Err(DexError::InvalidSignature);
    }

    Ok(())
}

/// Sequencer 批次签名验证
pub fn verify_batch_signature(batch: &SignedSequenceBatch, committee: &Committee) -> Result<()> {
    let leader = &batch.batch.leader;
    let public_key = committee.get_public_key(leader)
        .ok_or(DexError::UnknownValidator)?;

    let message = batch.batch.signing_message();

    if !batch.signature.verify(&message, public_key) {
        return Err(DexError::InvalidBatchSignature);
    }

    Ok(())
}
```

### 11.2 速率限制

```rust
/// 速率限制器
pub struct RateLimiter {
    /// 每个账户的限制
    account_limits: DashMap<AccountId, TokenBucket>,

    /// IP 限制
    ip_limits: DashMap<IpAddr, TokenBucket>,

    /// 配置
    config: RateLimitConfig,
}

#[derive(Clone)]
pub struct RateLimitConfig {
    /// 每秒订单数
    pub orders_per_second: u32,

    /// 每秒取消数
    pub cancels_per_second: u32,

    /// 每秒请求数 (API)
    pub requests_per_second: u32,
}

impl RateLimiter {
    pub fn check_order_rate(&self, account: &AccountId) -> Result<()> {
        let mut bucket = self.account_limits
            .entry(account.clone())
            .or_insert_with(|| TokenBucket::new(self.config.orders_per_second));

        if !bucket.try_consume(1) {
            return Err(DexError::RateLimitExceeded);
        }

        Ok(())
    }
}
```

---

## 12. 性能优化策略

### 12.1 关键优化点

| 优化点 | 策略 | 预期效果 |
|-------|------|---------|
| 订单簿查询 | BTreeMap + HashMap 双索引 | O(log n) 插入, O(1) 查找 |
| 余额操作 | DashMap 分片锁 | 并行读写无锁竞争 |
| 序列号分配 | AtomicU64 | 无锁递增 |
| 批次广播 | 并行发送 | 减少网络延迟 |
| WAL 写入 | 异步 + 批量 | 减少磁盘 IO |
| 事件推送 | 广播通道 | 无阻塞推送 |

### 12.2 内存优化

```rust
/// 订单条目 (优化版)
#[repr(C, packed)]
pub struct CompactOrderEntry {
    pub id: u128,           // 16 bytes
    pub account: [u8; 32],  // 32 bytes
    pub quantity: u64,      // 8 bytes
    pub filled: u64,        // 8 bytes
    pub timestamp: u64,     // 8 bytes
}
// Total: 72 bytes (vs 原版 ~200+ bytes)

/// 价格档位 (优化版)
pub struct CompactPriceLevel {
    pub price: u64,
    pub orders: Vec<CompactOrderEntry>,  // 连续内存
    pub total_quantity: u64,
}
```

### 12.3 批处理优化

```rust
/// 批量处理配置
pub struct BatchConfig {
    /// 最大批次大小
    pub max_batch_size: usize,

    /// 批次超时 (微秒)
    pub batch_timeout_us: u64,

    /// 并行处理线程数
    pub parallel_workers: usize,
}

/// 批量订单处理
pub async fn process_order_batch(
    orders: Vec<Order>,
    engine: &mut MatchingEngine,
) -> Vec<Result<MatchResult>> {
    // 按市场分组
    let mut by_market: HashMap<MarketId, Vec<Order>> = HashMap::new();
    for order in orders {
        by_market.entry(order.market).or_default().push(order);
    }

    // 并行处理不同市场
    let results: Vec<_> = by_market.into_par_iter()
        .flat_map(|(market, orders)| {
            orders.into_iter()
                .map(|order| engine.process_order(order))
                .collect::<Vec<_>>()
        })
        .collect();

    results
}
```

---

## 附录 A: 错误码定义

```rust
/// DEX 错误类型
#[derive(Clone, Debug, thiserror::Error)]
pub enum DexError {
    // ===== 订单错误 =====
    #[error("Order not found")]
    OrderNotFound,

    #[error("Invalid price: does not match tick size")]
    InvalidTickSize,

    #[error("Invalid quantity: does not match lot size")]
    InvalidLotSize,

    #[error("Invalid price")]
    InvalidPrice,

    #[error("Invalid quantity")]
    InvalidQuantity,

    #[error("Quantity too large")]
    QuantityTooLarge,

    #[error("Order value too small")]
    OrderValueTooSmall,

    #[error("Post-only order would match")]
    PostOnlyWouldMatch,

    #[error("Fill-or-kill order cannot be fully filled")]
    FillOrKillCannotFill,

    // ===== 账户错误 =====
    #[error("Account not found")]
    AccountNotFound,

    #[error("Insufficient balance")]
    InsufficientBalance,

    #[error("Insufficient frozen balance")]
    InsufficientFrozen,

    #[error("Insufficient margin")]
    InsufficientMargin,

    #[error("Balance overflow")]
    BalanceOverflow,

    // ===== 市场错误 =====
    #[error("Market not found")]
    MarketNotFound,

    #[error("Market already exists")]
    MarketAlreadyExists,

    #[error("Market is inactive")]
    MarketInactive,

    #[error("Not a perpetual market")]
    NotPerpetualMarket,

    // ===== 持仓错误 =====
    #[error("Position not found")]
    PositionNotFound,

    #[error("Position would exceed limit")]
    PositionLimitExceeded,

    // ===== 签名错误 =====
    #[error("Invalid signature")]
    InvalidSignature,

    #[error("Invalid batch signature")]
    InvalidBatchSignature,

    #[error("Unknown validator")]
    UnknownValidator,

    // ===== 系统错误 =====
    #[error("Rate limit exceeded")]
    RateLimitExceeded,

    #[error("Sequencer not leader")]
    NotLeader,

    #[error("Internal error: {0}")]
    Internal(String),
}
```

---

## 附录 B: 配置文件示例

```yaml
# dex-config.yaml

# Sequencer 配置
sequencer:
  max_batch_size: 1000
  batch_interval_us: 1000
  heartbeat_interval_ms: 25
  heartbeat_timeout_ms: 50
  epoch_duration_ms: 60000
  confirmation_timeout_ms: 100

# 撮合引擎配置
matching:
  default_tick_size: 100000000  # 0.01 (8 decimals)
  default_lot_size: 100000000   # 0.01 (8 decimals)

# 存储配置
storage:
  wal_path: "/data/dex/wal"
  db_path: "/data/dex/rocksdb"
  wal_sync_interval_ms: 100
  wal_max_file_size: 1073741824  # 1GB
  snapshot_interval_secs: 3600

# 永续合约配置
perpetuals:
  funding_interval_secs: 3600
  default_initial_margin_bps: 100   # 1%
  default_maintenance_margin_bps: 50 # 0.5%
  default_max_leverage: 100
  liquidation_penalty_bps: 100      # 1%

# 风险配置
risk:
  max_position_size: 1000000000000  # 10000 (8 decimals)
  max_order_value: 100000000000000  # 1000000 (8 decimals)

# API 配置
api:
  rest_port: 8080
  ws_port: 8081
  rate_limit:
    orders_per_second: 100
    cancels_per_second: 200
    requests_per_second: 1000

# 网络配置
network:
  p2p_port: 9000
  max_connections: 100
```

---

**文档版本**: 1.0.0
**最后更新**: 2025-01-XX
