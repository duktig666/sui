# 09-PERPETUAL-OVERVIEW.md
# 永续合约概要设计 / Perpetual Contracts Overview Design

> **文档状态 / Status**: 概要设计 / Overview Design (Phase 2)
> **最后更新 / Last Updated**: 2024-01
> **关联文档 / Related**: 05-MATCHING-ENGINE-DESIGN.md, 08-SPOT-OVERVIEW.md

---

## 目录 / Table of Contents

1. [概述 / Overview](#1-概述--overview)
2. [核心概念 / Core Concepts](#2-核心概念--core-concepts)
3. [保证金系统 / Margin System](#3-保证金系统--margin-system)
4. [资金费率 / Funding Rate](#4-资金费率--funding-rate)
5. [清算机制 / Liquidation](#5-清算机制--liquidation)
6. [保险基金与 ADL / Insurance & ADL](#6-保险基金与-adl--insurance--adl)
7. [风控体系 / Risk Management](#7-风控体系--risk-management)
8. [架构设计 / Architecture](#8-架构设计--architecture)
9. [待详细设计 / To Be Designed](#9-待详细设计--to-be-designed)

---

## 1. 概述 / Overview

### 1.1 设计目标 / Design Goals

永续合约是无到期日的衍生品合约，通过资金费率机制锚定现货价格。

| 目标 | 描述 | 优先级 |
|-----|------|-------|
| **价格锚定** | 合约价格紧跟现货指数 | P0 |
| **杠杆交易** | 支持多倍杠杆放大收益 | P0 |
| **双向交易** | 支持做多和做空 | P0 |
| **风险可控** | 完善的清算和保险机制 | P0 |
| **高性能** | 与现货共享撮合引擎 | P1 |

### 1.2 与现货区别 / vs Spot Trading

| 特性 | 现货 | 永续合约 |
|-----|------|---------|
| 资产 | 实际资产交割 | 仅结算盈亏 |
| 杠杆 | 1x (无杠杆) | 1x ~ 100x |
| 持有成本 | 无 | 资金费率 |
| 到期日 | 无 | 无 (永续) |
| 做空 | 需先持有资产 | 直接做空 |
| 结算 | 即时交割 | 持续盈亏结算 |

### 1.3 Phase 2 范围 / Scope

```
┌─────────────────────────────────────────────────────────┐
│                  永续合约 Phase 2                        │
├─────────────────────────────────────────────────────────┤
│  ✅ 本期实现                                             │
│  ├── USDC 正向合约 (BTCUSDC-PERP)                       │
│  ├── 逐仓保证金模式                                      │
│  ├── 资金费率机制                                        │
│  ├── 强制清算                                           │
│  └── 保险基金                                           │
├─────────────────────────────────────────────────────────┤
│  📅 后续计划                                             │
│  ├── 币本位合约 (BTC 结算)                               │
│  ├── 全仓保证金模式                                      │
│  ├── 组合保证金                                         │
│  └── 期权合约                                           │
└─────────────────────────────────────────────────────────┘
```

---

## 2. 核心概念 / Core Concepts

### 2.1 合约规格 / Contract Specification

```rust
pub struct PerpetualContract {
    /// 合约标识
    pub contract_id: ContractId,

    /// 标的资产 (如 BTC)
    pub underlying: AssetId,

    /// 结算资产 (如 USDC)
    pub settlement_asset: AssetId,

    /// 合约乘数 (1 张 = 多少标的)
    pub contract_size: Decimal,

    /// 最大杠杆倍数
    pub max_leverage: u8,

    /// 价格精度
    pub tick_size: Decimal,

    /// 最小交易数量
    pub min_quantity: Decimal,

    /// 维持保证金率
    pub maintenance_margin_rate: Decimal,

    /// 初始保证金率
    pub initial_margin_rate: Decimal,

    /// 资金费率间隔
    pub funding_interval: Duration,  // 8 hours
}
```

**示例合约规格**:

| 合约 | 标的 | 结算 | 乘数 | 最大杠杆 | 维持保证金 |
|-----|------|-----|------|---------|-----------|
| BTCUSDC-PERP | BTC | USDC | 0.001 BTC | 100x | 0.5% |
| ETHUSDC-PERP | ETH | USDC | 0.01 ETH | 50x | 1% |
| SUIUSDC-PERP | SUI | USDC | 10 SUI | 20x | 2.5% |

### 2.2 仓位 / Position

```rust
pub struct Position {
    /// 账户
    pub account: AccountId,

    /// 合约
    pub contract_id: ContractId,

    /// 方向
    pub side: PositionSide,  // Long | Short

    /// 持仓数量
    pub size: Decimal,

    /// 开仓均价
    pub entry_price: Decimal,

    /// 杠杆倍数
    pub leverage: u8,

    /// 已实现盈亏
    pub realized_pnl: Decimal,

    /// 累计资金费用
    pub accumulated_funding: Decimal,

    /// 初始保证金
    pub initial_margin: Decimal,

    /// 开仓时间
    pub opened_at: Timestamp,
}
```

### 2.3 盈亏计算 / PnL Calculation

```rust
impl Position {
    /// 未实现盈亏 (不含资金费)
    pub fn unrealized_pnl(&self, mark_price: Decimal) -> Decimal {
        let notional = self.size * self.entry_price;
        let current_value = self.size * mark_price;

        match self.side {
            PositionSide::Long => current_value - notional,
            PositionSide::Short => notional - current_value,
        }
    }

    /// 仓位价值
    pub fn position_value(&self, mark_price: Decimal) -> Decimal {
        self.size * mark_price
    }

    /// 保证金率
    pub fn margin_ratio(&self, mark_price: Decimal) -> Decimal {
        let equity = self.initial_margin + self.unrealized_pnl(mark_price);
        let position_value = self.position_value(mark_price);

        equity / position_value
    }
}
```

---

## 3. 保证金系统 / Margin System

### 3.1 保证金类型 / Margin Types

```
┌─────────────────────────────────────────────────────────┐
│                    保证金结构                            │
├─────────────────────────────────────────────────────────┤
│                                                         │
│  账户余额 (Account Balance)                              │
│      │                                                  │
│      ├── 可用保证金 (Available Margin)                  │
│      │       用于开新仓位                               │
│      │                                                  │
│      └── 已用保证金 (Used Margin)                       │
│              │                                          │
│              ├── 初始保证金 (Initial Margin)            │
│              │       开仓时锁定                          │
│              │                                          │
│              └── 维持保证金 (Maintenance Margin)        │
│                      保持仓位的最低要求                  │
│                                                         │
└─────────────────────────────────────────────────────────┘
```

### 3.2 保证金计算 / Margin Calculation

```rust
/// 初始保证金 = 仓位价值 / 杠杆
pub fn initial_margin(
    size: Decimal,
    price: Decimal,
    leverage: u8,
) -> Decimal {
    (size * price) / Decimal::from(leverage)
}

/// 维持保证金 = 仓位价值 × 维持保证金率
pub fn maintenance_margin(
    size: Decimal,
    mark_price: Decimal,
    mmr: Decimal,
) -> Decimal {
    size * mark_price * mmr
}
```

### 3.3 杠杆阶梯 / Leverage Tiers

高仓位限制最大杠杆:

| 仓位价值 | 最大杠杆 | 维持保证金率 |
|---------|---------|-------------|
| $0 - $50K | 100x | 0.50% |
| $50K - $250K | 50x | 1.00% |
| $250K - $1M | 25x | 2.00% |
| $1M - $5M | 10x | 5.00% |
| $5M+ | 5x | 10.00% |

### 3.4 保证金模式 / Margin Modes

**逐仓模式 (Isolated Margin)** - Phase 2 实现:
- 每个仓位独立保证金
- 清算只影响单个仓位
- 风险隔离

**全仓模式 (Cross Margin)** - 后续实现:
- 账户余额共享
- 盈利仓位可支撑亏损仓位
- 资金利用率高

---

## 4. 资金费率 / Funding Rate

### 4.1 机制原理 / Mechanism

资金费率使永续合约价格锚定现货指数:

```
┌─────────────────────────────────────────────────────────┐
│                   资金费率机制                           │
├─────────────────────────────────────────────────────────┤
│                                                         │
│  合约价格 > 指数价格 (溢价)                              │
│      → 资金费率为正                                     │
│      → 多头支付空头                                     │
│      → 激励做空，抑制做多                                │
│      → 价格回归                                         │
│                                                         │
│  合约价格 < 指数价格 (折价)                              │
│      → 资金费率为负                                     │
│      → 空头支付多头                                     │
│      → 激励做多，抑制做空                                │
│      → 价格回归                                         │
│                                                         │
└─────────────────────────────────────────────────────────┘
```

### 4.2 计算公式 / Formula

```rust
/// 资金费率 = 溢价指数 + clamp(利率差 - 溢价指数, -0.05%, 0.05%)
///
/// 溢价指数 = (合约价格 - 指数价格) / 指数价格
pub fn calculate_funding_rate(
    mark_price: Decimal,
    index_price: Decimal,
    interest_rate: Decimal,  // 通常 0.01%/8h
) -> Decimal {
    let premium_index = (mark_price - index_price) / index_price;

    let interest_diff = interest_rate - premium_index;
    let clamped = interest_diff.clamp(-0.0005, 0.0005);

    premium_index + clamped
}

/// 资金费用 = 仓位价值 × 资金费率
pub fn calculate_funding_payment(
    position_size: Decimal,
    mark_price: Decimal,
    funding_rate: Decimal,
    side: PositionSide,
) -> Decimal {
    let notional = position_size * mark_price;
    let payment = notional * funding_rate;

    match side {
        PositionSide::Long => -payment,  // 正费率多头付
        PositionSide::Short => payment,   // 正费率空头收
    }
}
```

### 4.3 结算周期 / Settlement Cycle

| 参数 | 值 | 说明 |
|-----|---|------|
| 结算间隔 | 8 小时 | 00:00, 08:00, 16:00 UTC |
| 费率上限 | ±0.75% | 单次结算最大费率 |
| 计算窗口 | 8 小时 | 费率计算时间窗口 |
| 预测显示 | 实时 | 下一周期预估费率 |

### 4.4 资金费率示例 / Example

```
假设:
- BTC 指数价格: $50,000
- BTC 合约价格: $50,100 (溢价 0.2%)
- 利率: 0.01%

计算:
- 溢价指数 = ($50,100 - $50,000) / $50,000 = 0.2%
- 利率差 = 0.01% - 0.2% = -0.19%
- clamp(-0.19%, -0.05%, 0.05%) = -0.05%
- 资金费率 = 0.2% + (-0.05%) = 0.15%

多头持有 1 BTC:
- 仓位价值 = $50,100
- 资金费用 = $50,100 × 0.15% = $75.15 (支付)

空头持有 1 BTC:
- 资金费用 = $75.15 (收取)
```

---

## 5. 清算机制 / Liquidation

### 5.1 清算触发 / Trigger Conditions

```rust
/// 当保证金率 ≤ 维持保证金率时触发清算
pub fn should_liquidate(position: &Position, mark_price: Decimal) -> bool {
    let margin_ratio = position.margin_ratio(mark_price);
    let mmr = get_maintenance_margin_rate(position.contract_id);

    margin_ratio <= mmr
}

/// 清算价格 (多头)
pub fn liquidation_price_long(position: &Position, mmr: Decimal) -> Decimal {
    let entry = position.entry_price;
    let leverage = Decimal::from(position.leverage);

    entry * (1.0 - (1.0 / leverage) + mmr)
}

/// 清算价格 (空头)
pub fn liquidation_price_short(position: &Position, mmr: Decimal) -> Decimal {
    let entry = position.entry_price;
    let leverage = Decimal::from(position.leverage);

    entry * (1.0 + (1.0 / leverage) - mmr)
}
```

### 5.2 清算流程 / Liquidation Process

```
┌─────────────────────────────────────────────────────────┐
│                    清算流程                              │
├─────────────────────────────────────────────────────────┤
│                                                         │
│  1. 触发检测                                             │
│     └── 保证金率 ≤ 维持保证金率                          │
│                                                         │
│  2. 清算接管                                             │
│     └── 系统接管仓位控制权                               │
│                                                         │
│  3. 仓位平仓                                             │
│     ├── 尝试市场平仓                                     │
│     │   └── 以破产价格限价单挂单                         │
│     │                                                   │
│     └── 若无法成交                                       │
│         └── 由保险基金接管                               │
│                                                         │
│  4. 盈亏结算                                             │
│     ├── 有剩余 → 返还用户                               │
│     └── 有缺口 → 保险基金补足                           │
│                                                         │
│  5. 若保险基金不足                                       │
│     └── 触发 ADL (自动减仓)                              │
│                                                         │
└─────────────────────────────────────────────────────────┘
```

### 5.3 清算罚金 / Liquidation Penalty

```rust
pub struct LiquidationFee {
    /// 清算手续费 (归协议)
    pub liquidation_fee_rate: Decimal,  // 如 0.5%

    /// 保险基金贡献
    pub insurance_fund_rate: Decimal,   // 如 0.25%
}
```

---

## 6. 保险基金与 ADL / Insurance & ADL

### 6.1 保险基金 / Insurance Fund

保险基金用于覆盖清算缺口:

```rust
pub struct InsuranceFund {
    /// 基金余额
    pub balance: Decimal,

    /// 资金来源
    sources: Vec<FundingSource>,
}

pub enum FundingSource {
    /// 清算罚金
    LiquidationPenalty,

    /// 手续费分成
    FeeShare,

    /// 资金费盈余
    FundingProfit,

    /// 初始注资
    InitialCapital,
}
```

**资金来源**:
- 清算罚金的 50%
- 交易手续费的 15%
- 资金费结算盈余

### 6.2 自动减仓 ADL / Auto-Deleveraging

当保险基金不足时，按对手方盈利排名强制减仓:

```rust
/// ADL 排名 = 盈利率 × 杠杆
pub fn adl_ranking(position: &Position, mark_price: Decimal) -> Decimal {
    let pnl_percent = position.unrealized_pnl(mark_price)
        / position.initial_margin;
    let leverage = Decimal::from(position.leverage);

    pnl_percent * leverage
}
```

**ADL 排序规则**:

```
高盈利 + 高杠杆 → 优先 ADL
     │
     ▼
低盈利 + 低杠杆 → 最后 ADL
```

### 6.3 ADL 指示灯 / ADL Indicator

用户界面显示 ADL 风险等级:

| 灯数 | 风险 | 说明 |
|-----|------|-----|
| 5 灯 | 极高 | 前 20% 排名 |
| 4 灯 | 高 | 20-40% 排名 |
| 3 灯 | 中 | 40-60% 排名 |
| 2 灯 | 低 | 60-80% 排名 |
| 1 灯 | 极低 | 后 20% 排名 |

---

## 7. 风控体系 / Risk Management

### 7.1 价格保护 / Price Protection

```rust
pub struct PriceProtection {
    /// 标记价格偏离阈值
    pub mark_price_deviation: Decimal,  // 如 3%

    /// 清算使用标记价格而非最新成交价
    pub use_mark_price: bool,

    /// 异常价格过滤
    pub outlier_filter: bool,
}
```

### 7.2 仓位限制 / Position Limits

| 限制类型 | 描述 | 目的 |
|---------|------|-----|
| 单账户上限 | 最大持仓价值 | 防止集中风险 |
| 全市场上限 | 总未平仓合约 | 控制系统风险 |
| 开仓限制 | 单边失衡时限制 | 防止单边行情 |

### 7.3 风险指标 / Risk Metrics

```rust
pub struct MarketRiskMetrics {
    /// 未平仓合约价值
    pub open_interest: Decimal,

    /// 多空比
    pub long_short_ratio: Decimal,

    /// 清算风险指数
    pub liquidation_risk_index: Decimal,

    /// 保险基金覆盖率
    pub insurance_coverage: Decimal,
}
```

### 7.4 熔断机制 / Circuit Breakers

| 触发条件 | 动作 | 冷却期 |
|---------|------|-------|
| 5分钟价格波动 > 10% | 暂停开仓 | 5 分钟 |
| 大规模清算 | 限制杠杆 | 30 分钟 |
| 保险基金耗尽 | 启动 ADL | 直到恢复 |

---

## 8. 架构设计 / Architecture

### 8.1 系统架构 / System Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    永续合约架构                          │
├─────────────────────────────────────────────────────────┤
│                                                         │
│  ┌─────────────────────────────────────────────────┐   │
│  │                   API 层                         │   │
│  │  订单接口 | 仓位查询 | 资金费率 | 风险查询       │   │
│  └─────────────────────────────────────────────────┘   │
│                         │                               │
│  ┌─────────────────────────────────────────────────┐   │
│  │                  业务逻辑层                      │   │
│  │  ┌─────────┐ ┌─────────┐ ┌─────────┐          │   │
│  │  │ 保证金   │ │ 资金费率 │ │  清算   │          │   │
│  │  │ Manager │ │ Engine  │ │ Engine  │          │   │
│  │  └─────────┘ └─────────┘ └─────────┘          │   │
│  └─────────────────────────────────────────────────┘   │
│                         │                               │
│  ┌─────────────────────────────────────────────────┐   │
│  │              共享撮合引擎 (复用现货)              │   │
│  └─────────────────────────────────────────────────┘   │
│                         │                               │
│  ┌─────────────────────────────────────────────────┐   │
│  │               共享存储层 (复用现货)               │   │
│  └─────────────────────────────────────────────────┘   │
│                                                         │
└─────────────────────────────────────────────────────────┘
```

### 8.2 与现货共享 / Shared with Spot

| 组件 | 复用方式 | 说明 |
|-----|---------|-----|
| 撮合引擎 | 完全复用 | 订单簿结构相同 |
| Sequencer | 完全复用 | 统一序列号 |
| 存储层 | 完全复用 | 扩展表结构 |
| 账户系统 | 部分复用 | 增加保证金账户 |
| 风控系统 | 扩展 | 增加清算逻辑 |

### 8.3 新增组件 / New Components

```rust
/// 保证金管理器
pub trait MarginManager {
    fn lock_margin(&self, account: AccountId, amount: Decimal) -> Result<()>;
    fn release_margin(&self, account: AccountId, amount: Decimal) -> Result<()>;
    fn calculate_available(&self, account: AccountId) -> Decimal;
}

/// 资金费率引擎
pub trait FundingEngine {
    fn calculate_rate(&self, contract: ContractId) -> Decimal;
    fn settle_funding(&self, timestamp: Timestamp) -> Vec<FundingSettlement>;
    fn get_next_funding_time(&self) -> Timestamp;
}

/// 清算引擎
pub trait LiquidationEngine {
    fn check_positions(&self, mark_prices: &HashMap<ContractId, Decimal>);
    fn liquidate(&self, position: Position) -> LiquidationResult;
    fn trigger_adl(&self, contract: ContractId, amount: Decimal);
}
```

---

## 9. 待详细设计 / To Be Designed

### 9.1 Phase 2 详细设计清单 / Detailed Design Checklist

| 模块 | 设计项 | 优先级 | 状态 |
|-----|-------|-------|------|
| 保证金 | 初始/维持保证金计算 | P0 | 待设计 |
| 保证金 | 杠杆阶梯表 | P0 | 待设计 |
| 资金费率 | 溢价指数计算 | P0 | 待设计 |
| 资金费率 | 结算调度器 | P0 | 待设计 |
| 清算 | 清算引擎架构 | P0 | 待设计 |
| 清算 | 破产价格计算 | P0 | 待设计 |
| 保险基金 | 资金管理策略 | P1 | 待设计 |
| ADL | 排名算法优化 | P1 | 待设计 |
| 风控 | 仓位限制规则 | P1 | 待设计 |

### 9.2 接口待定义 / Interfaces To Define

```rust
// Move 接口
module dex::perpetual {
    public entry fun open_position(...);
    public entry fun close_position(...);
    public entry fun add_margin(...);
    public entry fun remove_margin(...);
}

// RPC 接口
- GET /api/v1/perpetual/positions
- GET /api/v1/perpetual/funding_rate
- GET /api/v1/perpetual/liquidations
- POST /api/v1/perpetual/order
```

### 9.3 数据结构待定义 / Data Structures

```rust
// 需要详细设计的存储结构
pub struct PositionStore { ... }
pub struct FundingRateHistory { ... }
pub struct LiquidationLog { ... }
pub struct InsuranceFundLedger { ... }
```

### 9.4 关键决策待定 / Decisions Pending

| 决策项 | 选项 | 考量 |
|-------|------|-----|
| 保证金资产 | 仅 USDC / 多资产 | 复杂度 vs 灵活性 |
| 清算执行者 | 协议 / 外部清算者 | 去中心化程度 |
| 资金费率上限 | 固定 / 动态 | 极端行情保护 |
| ADL 触发阈值 | 保守 / 激进 | 用户体验 vs 安全 |

---

## 10. 附录 / Appendix

### 10.1 术语表 / Glossary

| 术语 | 英文 | 定义 |
|-----|------|-----|
| 永续合约 | Perpetual | 无到期日的衍生品合约 |
| 资金费率 | Funding Rate | 多空双方定期支付的费用 |
| 标记价格 | Mark Price | 用于计算盈亏和清算的公允价格 |
| 指数价格 | Index Price | 现货加权平均价格 |
| 初始保证金 | Initial Margin | 开仓所需最低保证金 |
| 维持保证金 | Maintenance Margin | 维持仓位所需最低保证金 |
| 清算 | Liquidation | 保证金不足时强制平仓 |
| ADL | Auto-Deleveraging | 自动减仓机制 |
| 保险基金 | Insurance Fund | 覆盖清算缺口的储备金 |

### 10.2 参考资料 / References

- Binance Futures Trading Rules
- dYdX Protocol Specification
- Perpetual Protocol Documentation
- FTX Risk Management (历史参考)

---

## 文档历史 / Document History

| 版本 | 日期 | 作者 | 变更 |
|-----|------|-----|------|
| 0.1 | 2024-01 | DEX Team | 概要设计初稿 |

---

> **下一步 / Next**: [10-PERFORMANCE-DESIGN.md](./10-PERFORMANCE-DESIGN.md) - 性能优化设计
