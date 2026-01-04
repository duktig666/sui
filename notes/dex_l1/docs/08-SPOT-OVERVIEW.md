# 08-SPOT-OVERVIEW.md
# 现货交易概要设计 / Spot Trading Overview Design

> **文档状态 / Status**: 概要设计 / Overview Design
> **版本**: v1.0
> **最后更新 / Last Updated**: 2025-12-31
> **关联文档 / Related**: 05-MATCHING-ENGINE-DESIGN.md, 07-MOVE-INTEGRATION-DESIGN.md

---

## 目录 / Table of Contents

1. [概述 / Overview](#1-概述--overview)
2. [核心概念 / Core Concepts](#2-核心概念--core-concepts)
3. [订单类型 / Order Types](#3-订单类型--order-types)
4. [市场机制 / Market Mechanisms](#4-市场机制--market-mechanisms)
5. [手续费模型 / Fee Model](#5-手续费模型--fee-model)
6. [结算机制 / Settlement](#6-结算机制--settlement)
7. [市场数据 / Market Data](#7-市场数据--market-data)
8. [风控机制 / Risk Control](#8-风控机制--risk-control)
9. [与永续合约关系 / Perpetual Integration](#9-与永续合约关系--perpetual-integration)

---

## 1. 概述 / Overview

### 1.1 设计目标 / Design Goals

现货交易模块提供传统的资产买卖功能，支持用户在 DEX L1 上进行即时交易和结算。

| 目标 | 描述 | 约束 |
|-----|------|-----|
| **即时成交** | 订单提交后立即撮合 | < 50ms 端到端延迟 |
| **原子结算** | 成交即结算，无需等待 | T+0 交割 |
| **价格发现** | 公平透明的价格形成机制 | 价格-时间优先 |
| **高吞吐** | 支持高频交易场景 | ≥ 200,000 TPS |

### 1.2 范围界定 / Scope

```
┌─────────────────────────────────────────────────────────┐
│                    现货交易模块                          │
├─────────────────────────────────────────────────────────┤
│  ✅ 本文档覆盖                                           │
│  ├── 交易对管理 (Trading Pairs)                         │
│  ├── 订单生命周期 (Order Lifecycle)                     │
│  ├── 手续费计算 (Fee Calculation)                       │
│  ├── 结算流程 (Settlement Flow)                         │
│  └── 市场数据服务 (Market Data)                         │
├─────────────────────────────────────────────────────────┤
│  📄 详见其他文档                                         │
│  ├── 撮合算法 → 05-MATCHING-ENGINE-DESIGN.md           │
│  ├── 存储持久化 → 06-STORAGE-DESIGN.md                 │
│  └── Move 接口 → 07-MOVE-INTEGRATION-DESIGN.md         │
└─────────────────────────────────────────────────────────┘
```

---

## 2. 核心概念 / Core Concepts

### 2.1 交易对 / Trading Pairs

交易对定义了两种资产之间的交易市场。

```rust
pub struct TradingPair {
    /// 市场唯一标识
    pub market_id: MarketId,

    /// 基础资产 (被交易的资产)
    /// Base asset (the asset being traded)
    pub base_asset: AssetId,

    /// 报价资产 (计价的资产)
    /// Quote asset (the asset used for pricing)
    pub quote_asset: AssetId,

    /// 价格精度 (最小价格变动单位)
    /// Tick size (minimum price increment)
    pub tick_size: Decimal,

    /// 数量精度 (最小数量变动单位)
    /// Step size (minimum quantity increment)
    pub step_size: Decimal,

    /// 最小订单金额
    pub min_notional: Decimal,

    /// 市场状态
    pub status: MarketStatus,
}
```

**示例交易对**:

| 交易对 | Base | Quote | Tick Size | Step Size | Min Notional |
|-------|------|-------|-----------|-----------|--------------|
| BTC/USDC | BTC | USDC | 0.01 | 0.0001 | 10 USDC |
| ETH/USDC | ETH | USDC | 0.01 | 0.001 | 10 USDC |
| SUI/USDC | SUI | USDC | 0.0001 | 0.1 | 1 USDC |

### 2.2 精度设计 / Precision Design

DEX 采用定点数避免浮点精度问题:

```rust
/// 价格精度: 8 位小数
/// Price precision: 8 decimal places
pub const PRICE_DECIMALS: u8 = 8;

/// 数量精度: 8 位小数
/// Quantity precision: 8 decimal places
pub const QUANTITY_DECIMALS: u8 = 8;

/// 内部计算精度: 18 位小数
/// Internal calculation precision: 18 decimal places
pub const INTERNAL_DECIMALS: u8 = 18;
```

**精度转换规则**:
- 用户输入 → 内部精度 (向下取整)
- 内部计算 → 用户输出 (向下取整)
- 手续费计算 → 向上取整 (有利于协议)

### 2.3 订单标识 / Order Identification

```rust
/// 订单ID: 全局唯一
/// 格式: [market_id:4][timestamp:6][sequence:6]
pub struct OrderId(u128);

/// 成交ID: 全局唯一
/// 格式: [market_id:4][timestamp:6][match_seq:6]
pub struct TradeId(u128);
```

---

## 3. 订单类型 / Order Types

### 3.1 基础订单类型 / Basic Order Types

| 类型 | 描述 | 行为 |
|-----|------|-----|
| **Limit** | 限价单 | 指定价格挂单，等待成交 |
| **Market** | 市价单 | 按当前最优价格立即成交 |

### 3.2 执行策略 / Time-in-Force

| 策略 | 描述 | 适用场景 |
|-----|------|---------|
| **GTC** | Good-Till-Cancel | 持续有效直到成交或取消 |
| **IOC** | Immediate-Or-Cancel | 立即成交可成交部分，取消剩余 |
| **FOK** | Fill-Or-Kill | 完全成交或完全取消 |
| **PostOnly** | 仅做市商 | 只挂单不吃单，否则取消 |

### 3.3 订单结构 / Order Structure

```rust
pub struct Order {
    /// 订单ID
    pub id: OrderId,

    /// 用户账户
    pub account: AccountId,

    /// 市场ID
    pub market_id: MarketId,

    /// 买卖方向
    pub side: Side,  // Buy | Sell

    /// 订单类型
    pub order_type: OrderType,

    /// 执行策略
    pub time_in_force: TimeInForce,

    /// 价格 (限价单)
    pub price: Option<Decimal>,

    /// 原始数量
    pub quantity: Decimal,

    /// 已成交数量
    pub filled_quantity: Decimal,

    /// 订单状态
    pub status: OrderStatus,

    /// 创建时间
    pub created_at: Timestamp,

    /// 序列号 (Sequencer 分配)
    pub sequence: u64,
}
```

### 3.4 订单状态流转 / Order State Machine

```
                          ┌─────────────┐
                          │   Pending   │ (提交中)
                          └──────┬──────┘
                                 │ Sequencer 确认
                                 ▼
┌─────────────┐           ┌─────────────┐
│  Rejected   │◄──────────│    Open     │ (已挂单)
│   (拒绝)    │  验证失败  └──────┬──────┘
└─────────────┘                  │
                    ┌────────────┼────────────┐
                    │            │            │
                    ▼            ▼            ▼
             ┌───────────┐ ┌───────────┐ ┌───────────┐
             │ Partially │ │   Filled  │ │ Cancelled │
             │  Filled   │ │  (完全成交)│ │  (已取消) │
             │ (部分成交) │ └───────────┘ └───────────┘
             └─────┬─────┘
                   │ 继续成交
                   ▼
             ┌───────────┐
             │   Filled  │
             └───────────┘
```

### 3.5 高级订单类型 / Advanced Orders (Phase 2)

```
┌─────────────────────────────────────────────────────────┐
│              高级订单类型 (计划中)                        │
├─────────────────────────────────────────────────────────┤
│  Stop-Loss     │ 止损单: 触发价达到后转市价单            │
│  Take-Profit   │ 止盈单: 触发价达到后转限价单            │
│  OCO           │ 二择一: 一个成交自动取消另一个          │
│  Trailing-Stop │ 追踪止损: 价格追踪设定百分比            │
│  Iceberg       │ 冰山单: 只显示部分数量                  │
│  TWAP          │ 时间加权: 按时间段分批成交              │
└─────────────────────────────────────────────────────────┘
```

---

## 4. 市场机制 / Market Mechanisms

### 4.1 市场状态 / Market Status

```rust
pub enum MarketStatus {
    /// 预上线 (只读)
    PreLaunch,

    /// 正常交易
    Trading,

    /// 暂停交易 (可取消订单)
    Suspended,

    /// 结算中 (清理订单)
    Settling,

    /// 已下架
    Delisted,
}
```

### 4.2 市场生命周期 / Market Lifecycle

```
创建市场 → PreLaunch → Trading ←→ Suspended → Settling → Delisted
              │                        │
              │    ┌───────────────────┘
              │    │ (维护/风控触发)
              │    ▼
              └──► Trading
                   (恢复交易)
```

### 4.3 市场参数 / Market Parameters

```rust
pub struct MarketConfig {
    /// 基础参数
    pub tick_size: Decimal,
    pub step_size: Decimal,
    pub min_notional: Decimal,

    /// 限制参数
    pub max_order_quantity: Decimal,
    pub max_position: Decimal,

    /// 价格保护
    pub price_band_percent: Decimal,  // 如 10% = 0.1

    /// 费率
    pub maker_fee: Decimal,  // 如 0.02% = 0.0002
    pub taker_fee: Decimal,  // 如 0.04% = 0.0004
}
```

### 4.4 开盘/收盘机制 / Trading Sessions

DEX L1 支持 24/7 交易，但保留以下机制:

| 机制 | 描述 | 用途 |
|-----|------|-----|
| **集合竞价** | 开盘前收集订单统一撮合 | 新市场上线、重大事件后恢复 |
| **熔断机制** | 价格波动超阈值暂停交易 | 极端行情风控 |
| **维护窗口** | 计划性暂停 | 系统升级 |

---

## 5. 手续费模型 / Fee Model

### 5.1 费率结构 / Fee Structure

采用 Maker-Taker 模型:

| 角色 | 定义 | 费率范围 |
|-----|------|---------|
| **Maker** | 提供流动性 (挂单) | 0% ~ 0.02% |
| **Taker** | 消耗流动性 (吃单) | 0.02% ~ 0.05% |

### 5.2 VIP 等级 / VIP Tiers

```rust
pub struct VipTier {
    pub level: u8,
    pub volume_30d: Decimal,      // 30日交易量门槛
    pub maker_fee: Decimal,
    pub taker_fee: Decimal,
}
```

| 等级 | 30日交易量 | Maker | Taker |
|-----|-----------|-------|-------|
| VIP 0 | < $1M | 0.020% | 0.040% |
| VIP 1 | $1M+ | 0.016% | 0.036% |
| VIP 2 | $5M+ | 0.014% | 0.032% |
| VIP 3 | $20M+ | 0.012% | 0.028% |
| VIP 4 | $100M+ | 0.010% | 0.024% |
| VIP 5 | $500M+ | 0.008% | 0.020% |
| MM | 做市商协议 | 0% | 0.015% |

### 5.3 手续费计算 / Fee Calculation

```rust
/// 手续费计算
/// Fee = notional × fee_rate
///
/// notional = price × quantity (成交金额)
pub fn calculate_fee(
    price: Decimal,
    quantity: Decimal,
    fee_rate: Decimal,
) -> Decimal {
    let notional = price * quantity;
    // 向上取整到最小精度
    (notional * fee_rate).ceil_to(QUOTE_DECIMALS)
}
```

### 5.4 手续费分配 / Fee Distribution

```
总手续费收入
    │
    ├── 80% → 协议金库 (Protocol Treasury)
    │
    ├── 15% → 保险基金 (Insurance Fund)
    │
    └── 5%  → 推荐奖励 (Referral Program)
```

---

## 6. 结算机制 / Settlement

### 6.1 即时结算 / Instant Settlement

DEX L1 采用 T+0 即时结算模式:

```
┌─────────────────────────────────────────────────────────┐
│                    成交结算流程                          │
├─────────────────────────────────────────────────────────┤
│                                                         │
│   订单撮合成功                                           │
│        │                                                │
│        ▼                                                │
│   ┌─────────────────────────────────────────┐          │
│   │           原子结算操作                    │          │
│   │  1. 冻结资金 → 已用资金                  │          │
│   │  2. 买方: quote - → base +              │          │
│   │  3. 卖方: base - → quote +              │          │
│   │  4. 扣除手续费                           │          │
│   │  5. 更新余额                             │          │
│   └─────────────────────────────────────────┘          │
│        │                                                │
│        ▼                                                │
│   结算完成 (< 1μs)                                      │
│                                                         │
└─────────────────────────────────────────────────────────┘
```

### 6.2 余额更新原子性 / Balance Atomicity

```rust
/// 成交结算 (必须原子执行)
pub fn settle_trade(trade: &Trade) -> Result<()> {
    // 单线程内执行，无需锁
    // 但需要事务语义保证一致性

    // 1. 验证余额充足
    let buyer_quote = get_available(trade.buyer, trade.quote_asset)?;
    let seller_base = get_available(trade.seller, trade.base_asset)?;

    ensure!(buyer_quote >= trade.quote_amount);
    ensure!(seller_base >= trade.base_amount);

    // 2. 原子更新
    // Buyer: -quote, +base
    decrease_balance(trade.buyer, trade.quote_asset, trade.quote_amount)?;
    increase_balance(trade.buyer, trade.base_asset, trade.base_amount)?;

    // Seller: -base, +quote
    decrease_balance(trade.seller, trade.base_asset, trade.base_amount)?;
    increase_balance(trade.seller, trade.quote_asset, trade.seller_receives)?;

    // 3. 手续费记账
    record_fee(trade.buyer, trade.buyer_fee);
    record_fee(trade.seller, trade.seller_fee);

    Ok(())
}
```

### 6.3 冻结机制 / Locking Mechanism

订单提交时冻结资金，防止超额下单:

```rust
pub struct Balance {
    /// 可用余额
    pub available: Decimal,

    /// 冻结余额 (挂单占用)
    pub locked: Decimal,
}

impl Balance {
    /// 总余额 = 可用 + 冻结
    pub fn total(&self) -> Decimal {
        self.available + self.locked
    }
}
```

**冻结流程**:
1. 下单时: `available -= order_value; locked += order_value`
2. 成交时: `locked -= filled_value; (结算处理)`
3. 取消时: `locked -= remaining_value; available += remaining_value`

---

## 7. 市场数据 / Market Data

### 7.1 实时订单簿 / Order Book

```rust
pub struct OrderBookSnapshot {
    pub market_id: MarketId,
    pub timestamp: Timestamp,
    pub sequence: u64,

    /// 买单深度 (价格降序)
    pub bids: Vec<PriceLevel>,

    /// 卖单深度 (价格升序)
    pub asks: Vec<PriceLevel>,
}

pub struct PriceLevel {
    pub price: Decimal,
    pub quantity: Decimal,  // 该价位总数量
    pub orders: u32,        // 该价位订单数
}
```

### 7.2 实时成交 / Recent Trades

```rust
pub struct PublicTrade {
    pub id: TradeId,
    pub market_id: MarketId,
    pub price: Decimal,
    pub quantity: Decimal,
    pub side: Side,  // Taker 方向
    pub timestamp: Timestamp,
}
```

### 7.3 K线数据 / Candlesticks (OHLCV)

```rust
pub struct Candlestick {
    pub market_id: MarketId,
    pub interval: Interval,  // 1m, 5m, 15m, 1h, 4h, 1d
    pub open_time: Timestamp,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub volume: Decimal,        // Base 资产成交量
    pub quote_volume: Decimal,  // Quote 资产成交量
    pub trades: u64,            // 成交笔数
}
```

### 7.4 24小时统计 / 24h Statistics

```rust
pub struct MarketStats24h {
    pub market_id: MarketId,
    pub last_price: Decimal,
    pub price_change: Decimal,
    pub price_change_percent: Decimal,
    pub high_24h: Decimal,
    pub low_24h: Decimal,
    pub volume_24h: Decimal,
    pub quote_volume_24h: Decimal,
    pub open_24h: Decimal,
    pub trades_24h: u64,
}
```

### 7.5 数据推送 / Data Streaming

```
┌─────────────────────────────────────────────────────────┐
│                   市场数据服务                           │
├─────────────────────────────────────────────────────────┤
│                                                         │
│  WebSocket Channels:                                    │
│  ├── orderbook@{market}     订单簿增量更新              │
│  ├── orderbook@{market}@100ms  订单簿快照 (100ms)       │
│  ├── trades@{market}        实时成交流                  │
│  ├── kline@{market}@{interval}  K线更新                 │
│  └── ticker@{market}        24h统计更新                 │
│                                                         │
│  REST Endpoints:                                        │
│  ├── GET /api/v1/depth      订单簿快照                  │
│  ├── GET /api/v1/trades     最近成交                    │
│  ├── GET /api/v1/klines     K线历史                     │
│  └── GET /api/v1/ticker/24h 24h统计                     │
│                                                         │
└─────────────────────────────────────────────────────────┘
```

---

## 8. 风控机制 / Risk Control

### 8.1 价格保护 / Price Band

防止极端价格订单和闪崩:

```rust
pub struct PriceBand {
    /// 参考价格 (最新成交价或指数价)
    pub reference_price: Decimal,

    /// 允许偏离百分比
    pub band_percent: Decimal,  // 如 10%
}

impl PriceBand {
    pub fn is_valid(&self, order_price: Decimal, side: Side) -> bool {
        let upper = self.reference_price * (1 + self.band_percent);
        let lower = self.reference_price * (1 - self.band_percent);

        match side {
            Side::Buy => order_price <= upper,
            Side::Sell => order_price >= lower,
        }
    }
}
```

### 8.2 订单限制 / Order Limits

| 限制类型 | 描述 | 默认值 |
|---------|------|-------|
| 单笔最大数量 | 单个订单最大数量 | 因市场而异 |
| 单笔最大金额 | 单个订单最大价值 | $1,000,000 |
| 账户最大挂单 | 单账户最大活跃订单数 | 200 |
| 市场最大挂单 | 单账户单市场最大订单 | 50 |
| 频率限制 | 每秒最大订单数 | 10 |

### 8.3 异常检测 / Anomaly Detection

```rust
pub enum AnomalyType {
    /// 价格操纵
    PriceManipulation,

    /// 清洗交易 (自成交)
    WashTrading,

    /// 异常大单
    AbnormalSize,

    /// 高频刷单
    HighFrequency,

    /// 分层欺骗 (挂单后快速取消)
    Layering,
}
```

### 8.4 熔断机制 / Circuit Breaker

```rust
pub struct CircuitBreaker {
    /// 5分钟价格波动阈值
    pub threshold_5m: Decimal,  // 如 10%

    /// 1小时价格波动阈值
    pub threshold_1h: Decimal,  // 如 20%

    /// 熔断冷却时间
    pub cooldown: Duration,     // 如 5 分钟
}
```

---

## 9. 与永续合约关系 / Perpetual Integration

### 9.1 共享组件 / Shared Components

```
┌─────────────────────────────────────────────────────────┐
│                    DEX L1 共享架构                       │
├─────────────────────────────────────────────────────────┤
│                                                         │
│  ┌─────────────┐     ┌─────────────┐                   │
│  │  现货交易    │     │  永续合约    │                   │
│  └──────┬──────┘     └──────┬──────┘                   │
│         │                   │                          │
│         └────────┬──────────┘                          │
│                  │                                      │
│    ┌─────────────┼─────────────┐                       │
│    │             │             │                       │
│    ▼             ▼             ▼                       │
│ ┌──────┐   ┌──────────┐   ┌──────────┐               │
│ │ 撮合  │   │  余额系统  │   │  风控系统  │               │
│ │ 引擎  │   │ (统一账户) │   │          │               │
│ └──────┘   └──────────┘   └──────────┘               │
│                                                         │
└─────────────────────────────────────────────────────────┘
```

### 9.2 统一账户 / Unified Account

```rust
pub struct UnifiedAccount {
    pub account_id: AccountId,

    /// 现货余额
    pub spot_balances: HashMap<AssetId, Balance>,

    /// 永续保证金
    pub perp_margin: Decimal,

    /// 跨品种保证金 (Phase 2)
    pub cross_margin: bool,
}
```

### 9.3 价格索引 / Price Index

现货价格作为永续合约的价格指数输入:

```rust
/// 指数价格 (用于永续合约)
/// Index price = 加权平均现货价格
pub fn calculate_index_price(market_id: MarketId) -> Decimal {
    // 使用现货市场最新成交价
    // 可扩展为多来源加权
    get_last_price(market_id)
}

/// 标记价格 (用于永续合约结算)
/// Mark price = 指数价格 + 基差移动平均
pub fn calculate_mark_price(perp_market: MarketId) -> Decimal {
    let index = calculate_index_price(spot_market);
    let basis = get_funding_basis(perp_market);
    index + basis
}
```

### 9.4 跨品种联动 / Cross-Product

| 场景 | 现货影响 | 永续影响 |
|-----|---------|---------|
| 大额现货成交 | 价格变动 | 指数价更新、资金费率变化 |
| 现货流动性枯竭 | 滑点增加 | 标记价偏离、可能触发清算 |
| 现货暂停交易 | 无法交易 | 使用历史指数价，限制开仓 |

---

## 10. 附录 / Appendix

### 10.1 错误码 / Error Codes

| 错误码 | 描述 | 处理建议 |
|-------|------|---------|
| E1001 | 余额不足 | 检查可用余额 |
| E1002 | 订单不存在 | 确认订单ID |
| E1003 | 市场已暂停 | 等待市场恢复 |
| E1004 | 价格超出范围 | 调整价格在保护带内 |
| E1005 | 数量低于最小值 | 增加订单数量 |
| E1006 | 超过最大挂单数 | 取消部分订单 |
| E1007 | 自成交拒绝 | 检查是否有反向挂单 |
| E1008 | PostOnly 会立即成交 | 使用限价单或调整价格 |

### 10.2 术语表 / Glossary

| 术语 | 英文 | 定义 |
|-----|------|-----|
| 基础资产 | Base Asset | 交易对中被交易的资产 |
| 报价资产 | Quote Asset | 交易对中用于计价的资产 |
| 挂单 | Maker | 提供流动性的订单 |
| 吃单 | Taker | 消耗流动性的订单 |
| 深度 | Depth | 各价格层级的订单量 |
| 滑点 | Slippage | 预期价格与成交价格的差异 |
| 成交金额 | Notional | 价格 × 数量 |

---

## 变更历史 / Change History

| 版本 | 日期 | 变更内容 | 状态 |
|-----|------|---------|------|
| v1.0 | 2025-12-31 | 初始版本 | ✅ 有效 |

### 待对齐事项 / Alignment Notes

| 章节 | 状态 | 说明 |
|-----|------|------|
| 3. 订单类型 | ✅ 有效 | 与 05-MATCHING-ENGINE 对齐 |
| 5. 手续费模型 | ⚠️ 概要 | 费率参数待经济模型确认 |
| 7. 市场数据 | ⚠️ 概要 | 数据服务接口待 API 设计 |

---

> **下一步 / Next**: [09-PERPETUAL-OVERVIEW.md](./09-PERPETUAL-OVERVIEW.md) - 永续合约概要设计
