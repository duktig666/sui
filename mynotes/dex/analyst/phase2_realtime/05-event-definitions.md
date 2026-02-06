# Phase 2 事件定义规范

## 概述

本文档定义 Phase 2 实时通道涉及的所有事件结构，包括新增事件（OrderPlacedEventV1、OrderRemovedEventV1）的详细定义，以及现有事件的重命名清单。

---

## 1. 事件命名规范

### 1.1 命名规则

| 规则 | 说明 | 示例 |
|------|------|------|
| 后缀 V1 | 版本标识，便于后续升级 | FillEventV1 |
| 驼峰命名 | 首字母大写 | OrderPlacedEventV1 |
| 业务语义 | 名称反映业务动作 | PositionUpdateEventV1 |

### 1.2 事件分类

| 分类 | 事件 | 用途 |
|------|------|------|
| **订单簿快照** | **OrderbookSnapshotEvent** | **链上订单簿全量推送（250ms）** |
| 订单事件 | OrderPlacedEventV1, OrderRemovedEventV1 | 订单簿维护 |
| 成交事件 | FillEventV1 | 成交记录、K 线聚合 |
| 持仓事件 | PositionUpdateEventV1 | 持仓变化通知 |
| 清算事件 | LiquidationEventV1 | 清算通知 |
| 余额事件 | BalanceUpdateEventV1, TransferEventV1 | 账户余额变化 |
| 结算事件 | FundingSettlementEventV1 | 资金费结算 |
| 系统事件 | PerpetualCreatedEventV1 | 市场创建 |

---

## 2. 新增事件定义

### 2.0 OrderbookSnapshotEvent（核心新增 2026-02-05）

**用途**：链上订单簿全量快照推送，每 250ms 发射一次。dex-realtime 直接使用此快照，无需本地构建订单簿。

**文件位置**：`sui-types/src/dex_events.rs`

```rust
/// 订单簿快照事件
///
/// 每 ~250ms 发射一次，提供链上订单簿的完整状态。
/// 设计理由（2026-02-05）：
/// - 简化 dex-realtime：无需本地构建/维护订单簿
/// - 保证一致性：链上是权威数据源
/// - 简化启动恢复：等待下一个快照即可
/// - 类似 Hyperliquid 的全量推送模式
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OrderbookSnapshotEvent {
    /// 永续合约 ID
    pub perpetual_id: u32,

    /// 快照序列号（单调递增）
    pub sequence_number: u64,

    /// 生成快照时的 Checkpoint 序列号
    pub checkpoint_sequence: u64,

    /// 时间戳（毫秒）
    pub timestamp_ms: u64,

    /// 买盘价格档位（按价格降序 - 最优买价在前）
    pub bids: Vec<PriceLevelSnapshot>,

    /// 卖盘价格档位（按价格升序 - 最优卖价在前）
    pub asks: Vec<PriceLevelSnapshot>,

    /// 最优买价（快速访问）
    pub best_bid: Option<u64>,

    /// 最优卖价（快速访问）
    pub best_ask: Option<u64>,

    /// 订单簿统计信息
    pub stats: OrderbookStats,
}

/// 价格档位快照
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PriceLevelSnapshot {
    /// 价格（subticks）
    pub price: u64,

    /// 该价格档位的总数量
    pub total_size: u64,

    /// 该价格档位的订单数量
    pub order_count: u32,
}

/// 订单簿统计信息
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct OrderbookStats {
    /// 买单总数
    pub bid_count: u32,

    /// 卖单总数
    pub ask_count: u32,

    /// 买盘总量
    pub total_bid_size: u64,

    /// 卖盘总量
    pub total_ask_size: u64,

    /// 中间价（最优买卖价平均值）
    pub mid_price: Option<u64>,

    /// 价差（最优卖价 - 最优买价）
    pub spread: Option<u64>,
}
```

**字段说明**：

| 字段 | 类型 | 说明 | 示例值 |
|------|------|------|--------|
| perpetual_id | u32 | 永续合约 ID | 0 (BTC-USD) |
| sequence_number | u64 | 快照序列号 | 12345 |
| checkpoint_sequence | u64 | Checkpoint 序列号 | 99999 |
| timestamp_ms | u64 | 时间戳 | 1707000000000 |
| bids | Vec<PriceLevelSnapshot> | 买盘（最多 100 档） | [...] |
| asks | Vec<PriceLevelSnapshot> | 卖盘（最多 100 档） | [...] |
| best_bid | Option<u64> | 最优买价 | Some(97000) |
| best_ask | Option<u64> | 最优卖价 | Some(97100) |
| stats | OrderbookStats | 统计信息 | {...} |

**配置参数**：

| 参数 | 值 | 说明 |
|------|-----|------|
| ORDERBOOK_SNAPSHOT_INTERVAL_MS | 250 | 快照发射间隔 |
| ORDERBOOK_SNAPSHOT_MAX_DEPTH | 100 | 最大档位数 |

**数据量估算**：
- 单个快照：~4-5 KB（100 档 × 2 边）
- 每秒带宽（单市场）：~16-20 KB/s

---

### 2.1 OrderPlacedEventV1

**用途**：订单进入订单簿时发射，用于实时更新订单簿状态。

**文件位置**：`sui-types/src/dex_events.rs`

```rust
use serde::{Deserialize, Serialize};

/// 订单进入订单簿事件
///
/// 当订单未完全成交，剩余部分进入订单簿时发射此事件。
/// 用于 dex-realtime 实时更新内存订单簿，以及 dex-indexer 持久化订单记录。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderPlacedEventV1 {
    /// 永续合约 ID
    pub perpetual_id: u32,

    /// 订单 ID（唯一标识）
    /// 格式：BCS 序列化的 OrderId 结构
    pub order_id: Vec<u8>,

    /// 子账户标识
    /// 格式：BCS 序列化的 SubaccountId (address + number)
    pub subaccount: Vec<u8>,

    /// 订单方向
    /// 0 = Buy, 1 = Sell
    pub side: u8,

    /// 订单价格（以 quote 单位计价）
    /// 使用定点数表示，精度由 perpetual 配置决定
    pub price: u64,

    /// 订单数量（以 base 单位计价）
    /// 使用定点数表示，精度由 perpetual 配置决定
    pub quantity: u64,

    /// 订单类型
    /// 0 = Limit, 1 = Market, 2 = LimitIOC, 3 = LimitFOK
    pub order_type: u8,

    /// 是否为 reduce-only 订单
    pub reduce_only: bool,

    /// 客户端订单 ID（可选，用于客户端追踪）
    pub client_order_id: Option<u64>,

    /// 事件时间戳（毫秒）
    pub timestamp_ms: u64,
}

/// 订单方向枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum OrderSide {
    Buy = 0,
    Sell = 1,
}

/// 订单类型枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum OrderType {
    /// 限价单（挂单）
    Limit = 0,
    /// 市价单（立即执行或取消）
    Market = 1,
    /// 限价 IOC（立即执行或取消，可部分成交）
    LimitIOC = 2,
    /// 限价 FOK（全部成交或取消）
    LimitFOK = 3,
}
```

**字段说明**：

| 字段 | 类型 | 说明 | 示例值 |
|------|------|------|--------|
| perpetual_id | u32 | 永续合约 ID | 1 (BTC-USDC) |
| order_id | Vec<u8> | 订单唯一标识 | BCS 序列化字节 |
| subaccount | Vec<u8> | 子账户标识 | BCS 序列化字节 |
| side | u8 | 买/卖方向 | 0=Buy, 1=Sell |
| price | u64 | 订单价格 | 9700000000 (97000.00) |
| quantity | u64 | 订单数量 | 100000000 (1.0 BTC) |
| order_type | u8 | 订单类型 | 0=Limit |
| reduce_only | bool | 是否仅减仓 | false |
| client_order_id | Option<u64> | 客户端订单 ID | Some(12345) |
| timestamp_ms | u64 | 事件时间戳 | 1707000000000 |

---

### 2.2 OrderRemovedEventV1

**用途**：订单从订单簿移除时发射，包括取消、过期、完全成交、清算等情况。

```rust
/// 订单移除事件
///
/// 当订单从订单簿移除时发射此事件。移除原因包括：
/// - 用户主动取消
/// - 订单过期
/// - 订单完全成交
/// - 强制清算
///
/// 用于 dex-realtime 更新内存订单簿，dex-indexer 更新订单状态。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderRemovedEventV1 {
    /// 永续合约 ID
    pub perpetual_id: u32,

    /// 订单 ID
    pub order_id: Vec<u8>,

    /// 子账户标识
    pub subaccount: Vec<u8>,

    /// 移除原因
    /// 0 = Cancel (用户取消)
    /// 1 = Expired (订单过期)
    /// 2 = Filled (完全成交)
    /// 3 = Liquidated (清算移除)
    /// 4 = PostOnlyReject (Post-only 订单被拒绝)
    /// 5 = ReduceOnlyReject (Reduce-only 订单无法减仓)
    /// 6 = SelfTrade (自成交取消)
    pub reason: u8,

    /// 移除时剩余数量
    /// 对于完全成交 (Filled) 的订单，此值为 0
    /// 对于取消的订单，此值为取消时的剩余量
    pub remaining_quantity: u64,

    /// 累计成交数量
    /// 订单生命周期内的总成交量
    pub total_filled_quantity: u64,

    /// 事件时间戳（毫秒）
    pub timestamp_ms: u64,
}

/// 订单移除原因枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum OrderRemovalReason {
    /// 用户主动取消
    Cancel = 0,
    /// 订单过期（GTT 订单超时）
    Expired = 1,
    /// 订单完全成交
    Filled = 2,
    /// 清算时强制移除
    Liquidated = 3,
    /// Post-only 订单会吃单，被拒绝
    PostOnlyReject = 4,
    /// Reduce-only 订单无法减仓（持仓方向不匹配）
    ReduceOnlyReject = 5,
    /// 自成交保护触发
    SelfTrade = 6,
}
```

**字段说明**：

| 字段 | 类型 | 说明 | 示例值 |
|------|------|------|--------|
| perpetual_id | u32 | 永续合约 ID | 1 |
| order_id | Vec<u8> | 订单标识 | BCS 序列化字节 |
| subaccount | Vec<u8> | 子账户标识 | BCS 序列化字节 |
| reason | u8 | 移除原因 | 0=Cancel, 2=Filled |
| remaining_quantity | u64 | 剩余数量 | 0 (完全成交时) |
| total_filled_quantity | u64 | 累计成交量 | 100000000 |
| timestamp_ms | u64 | 事件时间戳 | 1707000000000 |

---

## 3. 现有事件重命名清单

### 3.1 重命名映射

| 现有名称 | 新名称 | 文件位置 |
|----------|--------|----------|
| FillEvent | FillEventV1 | sui-types/src/dex_events.rs |
| PositionUpdateEvent | PositionUpdateEventV1 | sui-types/src/dex_events.rs |
| BalanceUpdateEvent | BalanceUpdateEventV1 | sui-types/src/dex_events.rs |
| TransferEvent | TransferEventV1 | sui-types/src/dex_events.rs |
| LiquidationEvent | LiquidationEventV1 | sui-types/src/dex_events.rs |
| FundingSettlementEvent | FundingSettlementEventV1 | sui-types/src/dex_events.rs |
| PerpetualCreatedEvent | PerpetualCreatedEventV1 | sui-types/src/dex_events.rs |

### 3.2 影响范围

| 组件 | 需要修改 |
|------|----------|
| sui-types | 事件结构定义重命名 |
| sui-execution | 事件发射代码（emit 调用） |
| dex-indexer | Handler 中的事件类型匹配 |
| dex-realtime | 事件过滤器和解析逻辑 |

---

## 4. 现有事件结构（参考）

### 4.1 FillEventV1

```rust
/// 成交事件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FillEventV1 {
    /// 永续合约 ID
    pub perpetual_id: u32,

    /// 成交 ID（唯一标识）
    pub fill_id: Vec<u8>,

    /// Maker 订单 ID
    pub maker_order_id: Vec<u8>,

    /// Taker 订单 ID
    pub taker_order_id: Vec<u8>,

    /// Maker 子账户
    pub maker_subaccount: Vec<u8>,

    /// Taker 子账户
    pub taker_subaccount: Vec<u8>,

    /// 成交价格
    pub price: u64,

    /// 成交数量
    pub quantity: u64,

    /// Maker 手续费（正数为收取，负数为返还）
    pub maker_fee: i64,

    /// Taker 手续费
    pub taker_fee: i64,

    /// Maker 是否为买方
    pub maker_is_buyer: bool,

    /// 事件时间戳（毫秒）
    pub timestamp_ms: u64,
}
```

### 4.2 PositionUpdateEventV1

```rust
/// 持仓更新事件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionUpdateEventV1 {
    /// 永续合约 ID
    pub perpetual_id: u32,

    /// 子账户标识
    pub subaccount: Vec<u8>,

    /// 持仓方向
    /// 0 = None (无持仓), 1 = Long, 2 = Short
    pub side: u8,

    /// 持仓数量（绝对值）
    pub size: u64,

    /// 平均入场价格
    pub entry_price: u64,

    /// 未实现盈亏
    pub unrealized_pnl: i64,

    /// 已实现盈亏（本次变更产生）
    pub realized_pnl: i64,

    /// 事件时间戳（毫秒）
    pub timestamp_ms: u64,
}
```

### 4.3 LiquidationEventV1

```rust
/// 清算事件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiquidationEventV1 {
    /// 永续合约 ID
    pub perpetual_id: u32,

    /// 被清算的子账户
    pub liquidated_subaccount: Vec<u8>,

    /// 清算方（保险基金或清算者）
    pub liquidator_subaccount: Option<Vec<u8>>,

    /// 清算价格
    pub price: u64,

    /// 清算数量
    pub quantity: u64,

    /// 清算前的持仓
    pub position_size_before: u64,

    /// 清算后的持仓
    pub position_size_after: u64,

    /// 清算类型
    /// 0 = Partial (部分清算), 1 = Full (全部清算)
    pub liquidation_type: u8,

    /// 事件时间戳（毫秒）
    pub timestamp_ms: u64,
}
```

### 4.4 BalanceUpdateEventV1

```rust
/// 余额更新事件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BalanceUpdateEventV1 {
    /// 子账户标识
    pub subaccount: Vec<u8>,

    /// 资产类型（如 USDC）
    pub asset_type: u8,

    /// 变更前余额
    pub balance_before: u64,

    /// 变更后余额
    pub balance_after: u64,

    /// 变更原因
    /// 0 = Deposit, 1 = Withdraw, 2 = Fee, 3 = PnL, 4 = Funding, 5 = Liquidation
    pub reason: u8,

    /// 事件时间戳（毫秒）
    pub timestamp_ms: u64,
}
```

### 4.5 TransferEventV1

```rust
/// 转账事件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferEventV1 {
    /// 发送方子账户
    pub from_subaccount: Vec<u8>,

    /// 接收方子账户
    pub to_subaccount: Vec<u8>,

    /// 资产类型
    pub asset_type: u8,

    /// 转账金额
    pub amount: u64,

    /// 事件时间戳（毫秒）
    pub timestamp_ms: u64,
}
```

### 4.6 FundingSettlementEventV1

```rust
/// 资金费结算事件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FundingSettlementEventV1 {
    /// 永续合约 ID
    pub perpetual_id: u32,

    /// 子账户标识
    pub subaccount: Vec<u8>,

    /// 资金费率（带符号，正数表示多头付给空头）
    pub funding_rate: i64,

    /// 资金费金额（带符号）
    pub funding_payment: i64,

    /// 结算时的持仓量
    pub position_size: u64,

    /// 结算时的标记价格
    pub mark_price: u64,

    /// 事件时间戳（毫秒）
    pub timestamp_ms: u64,
}
```

### 4.7 PerpetualCreatedEventV1

```rust
/// 永续合约创建事件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerpetualCreatedEventV1 {
    /// 永续合约 ID
    pub perpetual_id: u32,

    /// 交易对名称（如 "BTC-USDC"）
    pub symbol: String,

    /// Base 资产精度（小数位数）
    pub base_decimals: u8,

    /// Quote 资产精度（小数位数）
    pub quote_decimals: u8,

    /// 价格精度（小数位数）
    pub price_decimals: u8,

    /// 初始保证金率（基点，如 500 = 5%）
    pub initial_margin_bps: u16,

    /// 维持保证金率（基点）
    pub maintenance_margin_bps: u16,

    /// 事件时间戳（毫秒）
    pub timestamp_ms: u64,
}
```

---

## 5. 事件发射点汇总

### 5.1 发射位置表

| 事件 | 发射函数 | 文件 | 触发条件 |
|------|----------|------|----------|
| **OrderbookSnapshotEvent** | **execute_place_order()** | **sui-execution/src/dex/commands/order.rs** | **交易触发 + 距上次快照 ≥250ms** |
| **OrderbookSnapshotEvent** | **execute_cancel_order()** | **sui-execution/src/dex/commands/order.rs** | **交易触发 + 距上次快照 ≥250ms** |
| **OrderbookSnapshotEvent** | **execute_cancel_all_orders()** | **sui-execution/src/dex/commands/order.rs** | **交易触发 + 距上次快照 ≥250ms** |
| OrderPlacedEventV1 | execute_place_order() | sui-execution/src/dex/commands/order.rs | 订单进入订单簿 |
| OrderRemovedEventV1 | execute_place_order() | sui-execution/src/dex/commands/order.rs | 订单完全成交 |
| OrderRemovedEventV1 | execute_cancel_order() | sui-execution/src/dex/commands/order.rs | 用户取消订单 |
| OrderRemovedEventV1 | execute_liquidate() | sui-execution/src/dex/commands/order.rs | 清算移除订单 |
| FillEventV1 | execute_place_order() | sui-execution/src/dex/commands/order.rs | 订单匹配成交 |
| PositionUpdateEventV1 | execute_place_order() | sui-execution/src/dex/commands/order.rs | 成交后持仓变化 |
| PositionUpdateEventV1 | execute_liquidate() | sui-execution/src/dex/commands/order.rs | 清算后持仓变化 |
| LiquidationEventV1 | execute_liquidate() | sui-execution/src/dex/commands/order.rs | 清算执行 |
| BalanceUpdateEventV1 | execute_deposit() | sui-execution/src/dex/commands/account.rs | 充值 |
| BalanceUpdateEventV1 | execute_withdraw() | sui-execution/src/dex/commands/account.rs | 提现 |
| TransferEventV1 | execute_transfer() | sui-execution/src/dex/commands/account.rs | 账户间转账 |
| FundingSettlementEventV1 | execute_settle_funding() | sui-execution/src/dex/commands/market.rs | 资金费结算 |
| PerpetualCreatedEventV1 | execute_create_perpetual() | sui-execution/src/dex/commands/market.rs | 创建永续合约 |

### 5.2 事件发射示例代码

```rust
// sui-execution/src/dex.rs

use sui_types::dex_events::*;

impl DexEngine {
    /// 执行下单
    pub fn execute_place_order(&mut self, order: Order) -> Result<()> {
        // 1. 验证订单
        self.validate_order(&order)?;

        // 2. 尝试撮合
        let matches = self.match_order(&order)?;

        // 3. 处理成交
        for m in &matches {
            // 发射成交事件
            emit(FillEventV1 {
                perpetual_id: order.perpetual_id,
                fill_id: m.fill_id.clone(),
                maker_order_id: m.maker_order_id.clone(),
                taker_order_id: order.order_id.clone(),
                maker_subaccount: m.maker_subaccount.clone(),
                taker_subaccount: order.subaccount.clone(),
                price: m.price,
                quantity: m.quantity,
                maker_fee: m.maker_fee,
                taker_fee: m.taker_fee,
                maker_is_buyer: m.maker_is_buyer,
                timestamp_ms: current_timestamp_ms(),
            });

            // 更新持仓并发射事件
            self.update_position_and_emit(/* ... */)?;
        }

        // 4. 处理剩余订单
        let remaining_qty = order.quantity - matches.iter().map(|m| m.quantity).sum::<u64>();

        if remaining_qty > 0 && order.order_type == OrderType::Limit {
            // 进入订单簿
            self.orderbook.add_order(&order, remaining_qty)?;

            // 发射订单进入簿事件
            emit(OrderPlacedEventV1 {
                perpetual_id: order.perpetual_id,
                order_id: order.order_id.clone(),
                subaccount: order.subaccount.clone(),
                side: order.side as u8,
                price: order.price,
                quantity: remaining_qty,
                order_type: order.order_type as u8,
                reduce_only: order.reduce_only,
                client_order_id: order.client_order_id,
                timestamp_ms: current_timestamp_ms(),
            });
        } else if remaining_qty == 0 {
            // 完全成交，发射移除事件
            emit(OrderRemovedEventV1 {
                perpetual_id: order.perpetual_id,
                order_id: order.order_id.clone(),
                subaccount: order.subaccount.clone(),
                reason: OrderRemovalReason::Filled as u8,
                remaining_quantity: 0,
                total_filled_quantity: order.quantity,
                timestamp_ms: current_timestamp_ms(),
            });
        }

        Ok(())
    }

    /// 执行取消订单
    pub fn execute_cancel_order(&mut self, order_id: &[u8]) -> Result<()> {
        let order = self.orderbook.get_order(order_id)?;
        let remaining = self.orderbook.remove_order(order_id)?;

        emit(OrderRemovedEventV1 {
            perpetual_id: order.perpetual_id,
            order_id: order_id.to_vec(),
            subaccount: order.subaccount.clone(),
            reason: OrderRemovalReason::Cancel as u8,
            remaining_quantity: remaining,
            total_filled_quantity: order.quantity - remaining,
            timestamp_ms: current_timestamp_ms(),
        });

        Ok(())
    }
}
```

---

## 6. 事件数据流

### 6.1 完整数据流图

```
┌──────────────────────────────────────────────────────────────────┐
│                    sui-execution (撮合引擎)                       │
├──────────────────────────────────────────────────────────────────┤
│                                                                   │
│  execute_place_order()                                           │
│       │                                                           │
│       ├─→ FillEventV1 ─────────────────────────────────────────┐ │
│       ├─→ PositionUpdateEventV1 ───────────────────────────────┤ │
│       ├─→ OrderPlacedEventV1 ──────────────────────────────────┤ │
│       └─→ OrderRemovedEventV1 ─────────────────────────────────┤ │
│                                                                  │ │
│  execute_cancel_order()                                          │ │
│       └─→ OrderRemovedEventV1 ─────────────────────────────────┤ │
│                                                                  │ │
│  execute_liquidate()                                             │ │
│       ├─→ LiquidationEventV1 ──────────────────────────────────┤ │
│       ├─→ PositionUpdateEventV1 ───────────────────────────────┤ │
│       └─→ OrderRemovedEventV1 ─────────────────────────────────┤ │
│                                                                  │ │
└──────────────────────────────────────────────────────────────────┘ │
                                                                     │
                            Sui Transaction                          │
                                 │                                   │
         ┌───────────────────────┴───────────────────────┐          │
         │                                               │          │
         ▼                                               ▼          │
   Checkpoint API                             sui_subscribeEvent    │
         │                                               │          │
         ▼                                               ▼          │
   dex-indexer                                    dex-realtime      │
         │                                               │          │
         ▼                                               ├─→ 订单簿  │
   PostgreSQL                                            ├─→ K 线   │
   (持久化)                                               └─→ Redis  │
                                                                     │
                                                              dex-ws │
                                                           (WebSocket)│
```

### 6.2 事件消费关系

| 事件 | dex-indexer | dex-realtime | 用途 |
|------|:-----------:|:------------:|------|
| **OrderbookSnapshotEvent** | **-** | **→ 直接写入 Redis Hash** | **订单簿全量快照** |
| FillEventV1 | → dex_fills | → Redis Stream → K线聚合 | 成交记录 |
| OrderPlacedEventV1 | → dex_orders | ~~→ 内存订单簿~~ (已简化) | 订单状态 |
| OrderRemovedEventV1 | → dex_orders (状态更新) | ~~→ 内存订单簿~~ (已简化) | 订单移除 |
| PositionUpdateEventV1 | → dex_positions | → Redis Stream | 持仓变化 |
| LiquidationEventV1 | → dex_liquidations | → Redis Stream | 清算通知 |
| BalanceUpdateEventV1 | → dex_balances | - | 余额变化 |
| TransferEventV1 | → dex_transfers | - | 转账记录 |
| FundingSettlementEventV1 | → dex_funding | → Redis Stream | 资金费 |
| PerpetualCreatedEventV1 | → dex_perpetuals | - | 市场配置 |

> **注意**：采用链上快照推送方案后，dex-realtime 不再需要维护内存订单簿。
> OrderPlacedEventV1 和 OrderRemovedEventV1 仍然保留用于 dex-indexer 持久化和用户订单状态跟踪。
