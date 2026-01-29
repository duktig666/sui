# DEX Indexer 技术方案 V3

> 版本: V3
> 日期: 2026-01-29
> 状态: 设计中

---

## 1. 概述与设计决策

### 1.1 项目背景

DEX Indexer 是 Sui DEX 的链下索引服务，负责将 DEX 引擎产生的事件（成交、持仓、余额等）索引到链下数据库，并通过 REST API 和 WebSocket 对外提供查询服务。

**核心特点**：
- DEX 引擎使用 **原生 Rust** 实现（非 Move 合约）
- 事件通过 **gRPC** 从 DEX 引擎传输到 Indexer
- API 设计 **对标 Hyperliquid**（POST /info + POST /exchange）

### 1.2 分阶段规划

| 阶段 | 名称 | 核心内容 | 延迟 |
|------|------|---------|------|
| **Phase 1** | OnChainUpdates | Checkpoint 确认后的事件（Fills、Positions、Balances） | ~700ms+ |
| **Phase 2** | OffChainUpdates | 订单状态实时更新（OrderPlace/Update/Remove）+ WebSocket | <10ms |

**Phase 1 目标**：
- 功能验证：验证完整的索引 → 存储 → 查询链路
- 数据一致性：所有数据基于 Checkpoint 确认，不需要处理回滚
- 简化架构：单一事件流，降低实现复杂度

**Phase 2 目标**：
- 用户体验：订单状态实时更新（<10ms）
- WebSocket 推送：订单簿、成交、K线实时数据
- 双通道架构：OffChain（乐观）+ OnChain（最终确认）

### 1.3 核心设计决策

#### 决策 1：Checkpoint-Only OnChainUpdates

**选择**：所有 OnChainUpdates 事件在 Checkpoint 时发送（~700ms+）

**理由**（参考 `dex-indexer-full-by-dydx-analysis.md`）：
- 实现简单，维护成本低
- 数据一致性最高（单一来源）
- 不需要处理回滚情况
- API 设计简单（单一事件流）

**对比**：
| 方案 | 延迟 | 复杂度 | 数据一致性 |
|------|------|--------|-----------|
| **Checkpoint-Only** ✅ | ~700ms+ | 低 | 最高 |
| 双层设计 | Optimistic ~400ms | 中 | 需状态转换 |

#### 决策 2：不引入 Kafka

**选择**：Phase 1 不引入 Kafka，使用 gRPC 直连 + PostgreSQL Watermark

**理由**（参考 `sui-indexer-alt` 设计）：
- 单一消费者场景，不需要消息广播
- 借鉴 `sui-indexer-alt` 的 Pipeline 机制实现背压控制
- Watermark 断点续传替代 Kafka offset 重放
- 降低运维复杂度

**触发引入 Kafka 的条件**（Phase 2+）：
- 多 Indexer 实例需要事件广播
- 流量峰值 > 10x 需要削峰填谷
- 新增独立服务（分析/风控）需要解耦

#### 决策 3：API 对标 Hyperliquid

**选择**：采用 `POST /info` + `POST /exchange` 模式，而非 RESTful

**理由**：
- 与 Hyperliquid 保持一致，便于客户端迁移
- 使用 `type` 字段区分查询类型，扩展性好
- Exchange API 使用 EIP-712 签名，安全性高

---

## 2. 系统架构

### 2.1 Phase 1 架构图（Checkpoint-Only OnChainUpdates）

```
┌─────────────────────────────────────────────────────────────────────────┐
│                        Sui 验证器 + 原生 Rust DEX 引擎                    │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│  ┌──────────────┐      ┌──────────────────────────────────┐             │
│  │  撮合引擎    │ ───► │  OnChainUpdates (Checkpoint)      │             │
│  │ (Matching)   │      │  - Fills, Positions, Balances     │             │
│  └──────────────┘      │  - Transfers, Liquidations        │             │
│                        │  - FundingRates                    │             │
│                        └──────────────┬───────────────────┘             │
│                                       │                                  │
└───────────────────────────────────────┼──────────────────────────────────┘
                                        │ gRPC Stream
                                        ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                     DEX Indexer (Phase 1)                                │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│  ┌────────────────────┐   借鉴 sui-indexer-alt                          │
│  │ DexEventStreamClient│  - gRPC 连接管理                                │
│  │ (事件流订阅)        │  - 自动重连                                     │
│  └─────────┬──────────┘                                                  │
│            │                                                             │
│            ▼                                                             │
│  ┌────────────────────────────────────────────────────────────────┐     │
│  │                    Pipeline Layer                               │     │
│  │  ┌─────────────┐ ┌─────────────┐ ┌─────────────┐ ┌───────────┐ │     │
│  │  │FillsHandler │ │PositionsHdl│ │CandlesHandler│ │FundingHdl │ │     │
│  │  └──────┬──────┘ └──────┬──────┘ └──────┬──────┘ └─────┬─────┘ │     │
│  │         │               │               │              │        │     │
│  │         ▼               ▼               ▼              ▼        │     │
│  │  ┌─────────────────────────────────────────────────────────┐   │     │
│  │  │  Collector (批量收集) → Committer (写入) → Watermark    │   │     │
│  │  └─────────────────────────────────────────────────────────┘   │     │
│  └────────────────────────────────────────────────────────────────┘     │
│            │                                                             │
│            ▼                                                             │
│  ┌────────────────────┐                                                  │
│  │    PostgreSQL      │  - fills, positions, candles                     │
│  │                    │  - funding_rates, balances                       │
│  │                    │  - dex_watermarks                                │
│  └─────────┬──────────┘                                                  │
│            │                                                             │
│            ▼                                                             │
│  ┌────────────────────┐                                                  │
│  │    REST API        │  - POST /info (查询)                             │
│  │    (Axum)          │  - POST /exchange (交易)                         │
│  └────────────────────┘                                                  │
│                                                                          │
└─────────────────────────────────────────────────────────────────────────┘
```

### 2.2 Phase 2 架构图（+ OffChainUpdates + WebSocket）

```
┌─────────────────────────────────────────────────────────────────────────┐
│                        Sui 验证器 + 原生 Rust DEX 引擎                    │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│  ┌──────────────┐      ┌──────────────────────────────────┐             │
│  │  撮合引擎    │ ───► │  OnChainUpdates (Checkpoint)      │ ──► gRPC   │
│  │ (Matching)   │      └──────────────────────────────────┘             │
│  │              │                                                        │
│  │              │      ┌──────────────────────────────────┐             │
│  │              │ ───► │  OffChainUpdates (实时)           │ ──► gRPC   │
│  └──────────────┘      │  - OrderPlace/Update/Remove       │             │
│                        │  - OrderBook L2 (深度快照)        │             │
│                        └──────────────────────────────────┘             │
│                                                                          │
└───────────────────────────────────────────┬─────────────────────────────┘
                                            │
                      ┌─────────────────────┼─────────────────────┐
                      │                     │                     │
                      ▼                     ▼                     ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                     DEX Indexer (Phase 2)                                │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│  ┌────────────────────────────────────────────────────────────────┐     │
│  │                  OnChain Pipeline (同 Phase 1)                  │     │
│  │  gRPC → Processor → Collector → Committer → PostgreSQL         │     │
│  └────────────────────────────────────────────────────────────────┘     │
│                                                                          │
│  ┌────────────────────────────────────────────────────────────────┐     │
│  │                  OffChain Pipeline (新增)                       │     │
│  │  gRPC → OrderBook Cache → Redis → WebSocket Broadcast          │     │
│  └────────────────────────────────────────────────────────────────┘     │
│                                                                          │
│  ┌────────────────────┐  ┌────────────────────┐                         │
│  │    PostgreSQL      │  │      Redis         │                         │
│  │  (历史数据)        │  │  (实时缓存)        │                         │
│  └─────────┬──────────┘  └─────────┬──────────┘                         │
│            │                       │                                     │
│            └───────────┬───────────┘                                     │
│                        ▼                                                 │
│  ┌────────────────────────────────────────────────────────────────┐     │
│  │                        API Layer                                │     │
│  │  ┌────────────────┐  ┌────────────────────────────────────┐    │     │
│  │  │ REST API       │  │ WebSocket Server                    │    │     │
│  │  │ POST /info     │  │ - allMids, l2Book, trades, candle   │    │     │
│  │  │ POST /exchange │  │ - orderUpdates, userFills           │    │     │
│  │  └────────────────┘  └────────────────────────────────────┘    │     │
│  └────────────────────────────────────────────────────────────────┘     │
│                                                                          │
└─────────────────────────────────────────────────────────────────────────┘
```

### 2.3 数据流分类

| 通道 | 触发时机 | 延迟 | 数据内容 | 存储 | 阶段 |
|------|---------|------|---------|------|------|
| **OnChainUpdates** | Checkpoint | ~700ms+ | Fills, Positions, Balances, Transfers, Liquidations, FundingRates | PostgreSQL | Phase 1 |
| **OffChainUpdates** | 撮合完成 | <10ms | OrderPlace/Update/Remove, OrderBook L2 | Redis + WebSocket | Phase 2 |

---

## 3. gRPC 事件定义（Proto）

### 3.1 OnChainUpdates 事件（Phase 1）

```protobuf
syntax = "proto3";
package dex.indexer.v1;

// ==================== OnChainUpdates 事件 ====================

// 成交事件
message FillEvent {
  uint64 fill_id = 1;                    // 成交 ID（唯一）
  uint64 market_id = 2;                  // 市场 ID
  uint64 maker_order_id = 3;             // Maker 订单 ID
  uint64 taker_order_id = 4;             // Taker 订单 ID
  string maker_address = 5;              // Maker 地址 (0x...)
  string taker_address = 6;              // Taker 地址 (0x...)
  string side = 7;                       // Taker 方向: "B"=买入, "A"=卖出
  string price = 8;                      // 成交价格 (Decimal string)
  string quantity = 9;                   // 成交数量 (Decimal string)
  string maker_fee = 10;                 // Maker 手续费 (USDC)
  string taker_fee = 11;                 // Taker 手续费 (USDC)
  uint64 timestamp_ms = 12;              // 成交时间戳 (毫秒)
  uint64 checkpoint_sequence = 13;       // Checkpoint 序号
  string hash = 14;                      // 交易哈希
}

// 持仓更新事件
message PositionUpdateEvent {
  uint64 position_id = 1;                // 持仓 ID
  string owner = 2;                      // 持仓者地址
  uint64 market_id = 3;                  // 市场 ID
  string size = 4;                       // 持仓数量 (正=多头, 负=空头)
  string entry_price = 5;                // 开仓均价
  string leverage = 6;                   // 杠杆倍数
  string leverage_type = 7;              // "cross" | "isolated"
  string margin = 8;                     // 保证金
  string unrealized_pnl = 9;             // 未实现盈亏
  string liquidation_price = 10;         // 预估清算价 (可为空)
  string return_on_equity = 11;          // 收益率
  uint64 timestamp_ms = 12;              // 更新时间戳
  uint64 checkpoint_sequence = 13;       // Checkpoint 序号
}

// 余额更新事件
message BalanceUpdateEvent {
  string owner = 1;                      // 用户地址
  string asset = 2;                      // 资产名称 (如 "USDC")
  string total = 3;                      // 总余额
  string available = 4;                  // 可用余额
  string locked = 5;                     // 冻结余额 (挂单占用)
  uint64 timestamp_ms = 6;               // 更新时间戳
  uint64 checkpoint_sequence = 7;        // Checkpoint 序号
}

// 资金费率事件
message FundingRateEvent {
  uint64 market_id = 1;                  // 市场 ID
  string funding_rate = 2;               // 资金费率 (8小时费率)
  string mark_price = 3;                 // 标记价格
  string index_price = 4;                // 指数价格
  string premium = 5;                    // 溢价率
  string open_interest = 6;              // 未平仓合约量
  uint64 timestamp_ms = 7;               // 结算时间戳
  uint64 checkpoint_sequence = 8;        // Checkpoint 序号
}

// 清算事件
message LiquidationEvent {
  uint64 liquidation_id = 1;             // 清算 ID
  uint64 position_id = 2;                // 被清算的持仓 ID
  string owner = 3;                      // 被清算者地址
  uint64 market_id = 4;                  // 市场 ID
  string size = 5;                       // 清算数量
  string price = 6;                      // 清算价格
  string liquidator = 7;                 // 清算人地址
  string pnl = 8;                        // 清算盈亏
  uint64 timestamp_ms = 9;               // 清算时间戳
  uint64 checkpoint_sequence = 10;       // Checkpoint 序号
}

// 转账事件
message TransferEvent {
  uint64 transfer_id = 1;                // 转账 ID
  string transfer_type = 2;              // "deposit" | "withdraw" | "internal"
  string from_address = 3;               // 来源地址
  string to_address = 4;                 // 目标地址
  string asset = 5;                      // 资产名称
  string amount = 6;                     // 转账数量
  string fee = 7;                        // 手续费
  uint64 timestamp_ms = 8;               // 转账时间戳
  uint64 checkpoint_sequence = 9;        // Checkpoint 序号
}

// 市场配置更新事件
message MarketUpdateEvent {
  uint64 market_id = 1;                  // 市场 ID
  string symbol = 2;                     // 交易对符号 (如 "BTC")
  string base_asset = 3;                 // 基础资产
  string quote_asset = 4;                // 报价资产
  uint32 price_decimals = 5;             // 价格精度
  uint32 size_decimals = 6;              // 数量精度
  string min_order_size = 7;             // 最小订单数量
  uint32 max_leverage = 8;               // 最大杠杆
  string maker_fee = 9;                  // Maker 费率
  string taker_fee = 10;                 // Taker 费率
  string status = 11;                    // "active" | "suspended"
  bool only_isolated = 12;               // 是否仅支持逐仓
  uint64 timestamp_ms = 13;              // 更新时间戳
  uint64 checkpoint_sequence = 14;       // Checkpoint 序号
}
```

### 3.2 OffChainUpdates 事件（Phase 2）

```protobuf
// ==================== OffChainUpdates 事件 ====================

// 订单放置事件
message OrderPlaceEvent {
  uint64 order_id = 1;                   // 订单 ID
  uint64 market_id = 2;                  // 市场 ID
  string owner = 3;                      // 下单者地址
  string side = 4;                       // "B"=买入, "A"=卖出
  string order_type = 5;                 // "Limit" | "Market" | "StopMarket" | "StopLimit"
  string price = 6;                      // 委托价格
  string quantity = 7;                   // 委托数量
  string tif = 8;                        // "Gtc" | "Ioc" | "Alo"
  bool reduce_only = 9;                  // 是否仅减仓
  string trigger_price = 10;             // 触发价格 (条件单)
  string trigger_type = 11;              // "tp" | "sl" (止盈/止损)
  string client_order_id = 12;           // 客户端订单 ID (可选)
  uint64 timestamp_ms = 13;              // 下单时间戳
}

// 订单更新事件
message OrderUpdateEvent {
  uint64 order_id = 1;                   // 订单 ID
  string filled_quantity = 2;            // 已成交数量
  string remaining_quantity = 3;         // 剩余数量
  string avg_fill_price = 4;             // 平均成交价
  string status = 5;                     // "open" | "partialFilled"
  uint64 timestamp_ms = 6;               // 更新时间戳
}

// 订单移除事件
message OrderRemoveEvent {
  uint64 order_id = 1;                   // 订单 ID
  string reason = 2;                     // "filled" | "canceled" | "expired" | "rejected"
  string closed_pnl = 3;                 // 平仓盈亏 (平仓单)
  uint64 timestamp_ms = 4;               // 移除时间戳
}

// 订单簿 L2 快照
message OrderBookL2Snapshot {
  uint64 market_id = 1;                  // 市场 ID
  string coin = 2;                       // 交易对名称
  repeated Level bids = 3;               // 买盘 (价格降序)
  repeated Level asks = 4;               // 卖盘 (价格升序)
  uint64 timestamp_ms = 5;               // 快照时间戳
}

message Level {
  string price = 1;                      // 价格
  string size = 2;                       // 数量
  uint32 num_orders = 3;                 // 该价位订单数
}
```

### 3.3 事件批次封装

```protobuf
// ==================== 事件批次封装 ====================

// OnChainUpdates 事件批次
message OnChainEventBatch {
  uint64 checkpoint_sequence = 1;        // Checkpoint 序号
  uint64 epoch = 2;                      // Epoch 号
  uint64 timestamp_ms = 3;               // 批次时间戳

  repeated FillEvent fills = 10;
  repeated PositionUpdateEvent position_updates = 11;
  repeated BalanceUpdateEvent balance_updates = 12;
  repeated FundingRateEvent funding_rates = 13;
  repeated LiquidationEvent liquidations = 14;
  repeated TransferEvent transfers = 15;
  repeated MarketUpdateEvent market_updates = 16;
}

// OffChainUpdates 事件批次
message OffChainEventBatch {
  uint64 sequence = 1;                   // 序列号
  uint64 timestamp_ms = 2;               // 批次时间戳

  repeated OrderPlaceEvent order_places = 10;
  repeated OrderUpdateEvent order_updates = 11;
  repeated OrderRemoveEvent order_removes = 12;
  repeated OrderBookL2Snapshot orderbook_snapshots = 13;
}
```

### 3.4 gRPC Service 定义

```protobuf
// ==================== gRPC Service ====================

service DexEventService {
  // OnChainUpdates 订阅 (Phase 1)
  rpc SubscribeOnChainEvents(SubscribeOnChainRequest)
      returns (stream OnChainEventBatch);

  // OffChainUpdates 订阅 (Phase 2)
  rpc SubscribeOffChainEvents(SubscribeOffChainRequest)
      returns (stream OffChainEventBatch);

  // 获取最新 Checkpoint 序号
  rpc GetLatestCheckpoint(GetLatestCheckpointRequest)
      returns (GetLatestCheckpointResponse);
}

message SubscribeOnChainRequest {
  uint64 from_checkpoint = 1;            // 起始 Checkpoint (断点续传)
  repeated string event_types = 2;       // 订阅的事件类型 (空=全部)
}

message SubscribeOffChainRequest {
  uint64 from_sequence = 1;              // 起始序列号
  repeated uint64 market_ids = 2;        // 订阅的市场 (空=全部)
}

message GetLatestCheckpointRequest {}

message GetLatestCheckpointResponse {
  uint64 checkpoint_sequence = 1;
  uint64 epoch = 2;
  uint64 timestamp_ms = 3;
}
```

### 3.5 Rust 类型定义

```rust
use serde::{Deserialize, Serialize};
use rust_decimal::Decimal;

// ==================== OnChainUpdates ====================

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FillEvent {
    pub fill_id: u64,
    pub market_id: u64,
    pub maker_order_id: u64,
    pub taker_order_id: u64,
    pub maker_address: String,
    pub taker_address: String,
    pub side: Side,
    pub price: Decimal,
    pub quantity: Decimal,
    pub maker_fee: Decimal,
    pub taker_fee: Decimal,
    pub timestamp_ms: u64,
    pub checkpoint_sequence: u64,
    pub hash: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PositionUpdateEvent {
    pub position_id: u64,
    pub owner: String,
    pub market_id: u64,
    pub size: Decimal,
    pub entry_price: Decimal,
    pub leverage: Decimal,
    pub leverage_type: LeverageType,
    pub margin: Decimal,
    pub unrealized_pnl: Decimal,
    pub liquidation_price: Option<Decimal>,
    pub return_on_equity: Decimal,
    pub timestamp_ms: u64,
    pub checkpoint_sequence: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BalanceUpdateEvent {
    pub owner: String,
    pub asset: String,
    pub total: Decimal,
    pub available: Decimal,
    pub locked: Decimal,
    pub timestamp_ms: u64,
    pub checkpoint_sequence: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FundingRateEvent {
    pub market_id: u64,
    pub funding_rate: Decimal,
    pub mark_price: Decimal,
    pub index_price: Decimal,
    pub premium: Decimal,
    pub open_interest: Decimal,
    pub timestamp_ms: u64,
    pub checkpoint_sequence: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LiquidationEvent {
    pub liquidation_id: u64,
    pub position_id: u64,
    pub owner: String,
    pub market_id: u64,
    pub size: Decimal,
    pub price: Decimal,
    pub liquidator: String,
    pub pnl: Decimal,
    pub timestamp_ms: u64,
    pub checkpoint_sequence: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransferEvent {
    pub transfer_id: u64,
    pub transfer_type: TransferType,
    pub from_address: String,
    pub to_address: String,
    pub asset: String,
    pub amount: Decimal,
    pub fee: Decimal,
    pub timestamp_ms: u64,
    pub checkpoint_sequence: u64,
}

// ==================== 枚举类型 ====================

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Side {
    #[serde(rename = "B")]
    Buy,
    #[serde(rename = "A")]
    Sell,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LeverageType {
    Cross,
    Isolated,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TransferType {
    Deposit,
    Withdraw,
    Internal,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum OrderStatus {
    Open,
    PartialFilled,
    Filled,
    Canceled,
    Expired,
    Rejected,
}

// ==================== 事件批次 ====================

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OnChainEventBatch {
    pub checkpoint_sequence: u64,
    pub epoch: u64,
    pub timestamp_ms: u64,
    pub fills: Vec<FillEvent>,
    pub position_updates: Vec<PositionUpdateEvent>,
    pub balance_updates: Vec<BalanceUpdateEvent>,
    pub funding_rates: Vec<FundingRateEvent>,
    pub liquidations: Vec<LiquidationEvent>,
    pub transfers: Vec<TransferEvent>,
}
```

---

## 4. REST API 设计（对标 Hyperliquid）

### 4.1 端点概览

**设计原则**：采用 Hyperliquid 的 POST 模式，通过 `type` 字段区分请求类型，而非 RESTful 风格。

| 端点 | 用途 | 签名要求 | 阶段 |
|------|------|---------|------|
| `POST /info` | 查询数据（市场、订单簿、持仓等） | 无需 | Phase 1 |
| `POST /exchange` | 交易操作（下单、撤单等） | EIP-712 | Phase 1 |

### 4.2 Info API 完整列表（POST /info）

#### 4.2.1 市场数据

| type | 参数 | 说明 | 对标 Hyperliquid |
|------|------|------|-----------------|
| `meta` | 无 | 永续合约元数据（杠杆、精度等） | ✓ |
| `metaAndAssetCtxs` | 无 | 永续元数据 + 实时市场数据 | ✓ |
| `spotMeta` | 无 | 现货代币元数据 | ✓ |
| `spotMetaAndAssetCtxs` | 无 | 现货元数据 + 实时数据 | ✓ |
| `allMids` | 无 | 所有交易对中间价 | ✓ |

#### 4.2.2 订单簿与行情

| type | 参数 | 说明 | 对标 Hyperliquid |
|------|------|------|-----------------|
| `l2Book` | `coin` | 订单簿深度（买卖盘） | ✓ |
| `candleSnapshot` | `coin`, `interval`, `startTime`, `endTime` | K线数据 | ✓ |
| `recentTrades` | `coin` | 最近成交记录 | ✓ |

#### 4.2.3 用户账户

| type | 参数 | 说明 | 对标 Hyperliquid |
|------|------|------|-----------------|
| `clearinghouseState` | `user` | 永续账户状态（保证金、持仓） | ✓ |
| `spotClearinghouseState` | `user` | 现货余额 | ✓ |
| `userVaultEquities` | `user` | Vault 权益 | ✓ |

#### 4.2.4 订单查询

| type | 参数 | 说明 | 对标 Hyperliquid |
|------|------|------|-----------------|
| `openOrders` | `user` | 当前挂单（简化） | ✓ |
| `frontendOpenOrders` | `user` | 当前挂单（完整信息） | ✓ |
| `orderStatus` | `user`, `oid` | 单个订单状态 | ✓ |
| `historicalOrders` | `user` | 历史订单 | ✓ |

#### 4.2.5 成交与资金费

| type | 参数 | 说明 | 对标 Hyperliquid |
|------|------|------|-----------------|
| `userFills` | `user` | 成交记录（默认最近100） | ✓ |
| `userFillsByTime` | `user`, `startTime`, `endTime` | 按时间范围成交 | ✓ |
| `userFunding` | `user`, `startTime`, `endTime` | 资金费记录 | ✓ |
| `fundingHistory` | `coin`, `startTime`, `endTime` | 市场资金费率历史 | ✓ |
| `predictedFundings` | 无 | 预测资金费率 | ✓ |

#### 4.2.6 Builder / 手续费

| type | 参数 | 说明 | 对标 Hyperliquid |
|------|------|------|-----------------|
| `maxBuilderFee` | `user`, `builder` | Builder 授权费率查询 | ✓ |
| `userFees` | `user` | 用户费率等级 | ✓ |

### 4.3 Exchange API 完整列表（POST /exchange）

**签名要求**：所有 Exchange 操作需要 EIP-712 签名。

#### 4.3.1 订单操作

| action.type | 参数 | 说明 | 签名方法 |
|-------------|------|------|---------|
| `order` | `orders[]`, `grouping` | 下单（支持批量） | signL1Action |
| `cancel` | `cancels[]` | 撤单（支持批量） | signL1Action |
| `cancelByCloid` | `cancels[]` | 按客户端ID撤单 | signL1Action |
| `modify` | `oid`, `order` | 修改订单 | signL1Action |
| `batchModify` | `modifies[]` | 批量修改订单 | signL1Action |

#### 4.3.2 账户操作

| action.type | 参数 | 说明 | 签名方法 |
|-------------|------|------|---------|
| `updateLeverage` | `asset`, `isCross`, `leverage` | 更新杠杆 | signL1Action |
| `updateIsolatedMargin` | `asset`, `isBuy`, `ntli` | 更新逐仓保证金 | signL1Action |

#### 4.3.3 资金操作

| action.type | 参数 | 说明 | 签名方法 |
|-------------|------|------|---------|
| `usdSend` | `destination`, `amount` | USDC 转账 | signUserSignedAction |
| `withdraw3` | `destination`, `amount` | 提现到 L1 | signUserSignedAction |
| `vaultDeposit` | `vaultAddress`, `amount` | 存入 Vault | signL1Action |
| `vaultWithdraw` | `vaultAddress`, `amount` | 取出 Vault | signL1Action |

#### 4.3.4 授权操作

| action.type | 参数 | 说明 | 签名方法 |
|-------------|------|------|---------|
| `approveBuilderFee` | `builder`, `maxFeeRate` | 授权 Builder 费率 | signUserSignedAction |

### 4.4 请求/响应格式示例

#### 4.4.1 Info API 示例

**请求: meta**
```json
{
  "type": "meta"
}
```

**响应: meta**
```json
{
  "universe": [
    {
      "name": "BTC",
      "szDecimals": 5,
      "maxLeverage": 50,
      "onlyIsolated": false
    },
    {
      "name": "ETH",
      "szDecimals": 4,
      "maxLeverage": 50,
      "onlyIsolated": false
    }
  ]
}
```

**请求: l2Book**
```json
{
  "type": "l2Book",
  "coin": "BTC"
}
```

**响应: l2Book**
```json
{
  "coin": "BTC",
  "time": 1706500000000,
  "levels": [
    [
      {"px": "42000.0", "sz": "1.5", "n": 3},
      {"px": "41999.5", "sz": "2.0", "n": 2}
    ],
    [
      {"px": "42000.5", "sz": "1.2", "n": 2},
      {"px": "42001.0", "sz": "3.0", "n": 5}
    ]
  ]
}
```

**请求: clearinghouseState**
```json
{
  "type": "clearinghouseState",
  "user": "0x1234567890abcdef1234567890abcdef12345678"
}
```

**响应: clearinghouseState**
```json
{
  "marginSummary": {
    "accountValue": "10000.0",
    "totalNtlPos": "5000.0",
    "totalRawUsd": "5000.0",
    "totalMarginUsed": "1000.0"
  },
  "crossMarginSummary": {
    "accountValue": "8000.0",
    "totalNtlPos": "4000.0",
    "totalRawUsd": "4000.0",
    "totalMarginUsed": "800.0"
  },
  "withdrawable": "4000.0",
  "assetPositions": [
    {
      "position": {
        "coin": "BTC",
        "szi": "0.5",
        "leverage": {
          "type": "cross",
          "value": 10
        },
        "entryPx": "42000.0",
        "positionValue": "21000.0",
        "unrealizedPnl": "500.0",
        "returnOnEquity": "0.05",
        "liquidationPx": "38000.0"
      },
      "type": "oneWay"
    }
  ],
  "time": 1706500000000
}
```

**请求: candleSnapshot**
```json
{
  "type": "candleSnapshot",
  "req": {
    "coin": "BTC",
    "interval": "1h",
    "startTime": 1706400000000,
    "endTime": 1706500000000
  }
}
```

**响应: candleSnapshot**
```json
[
  {
    "t": 1706400000000,
    "T": 1706403600000,
    "s": "BTC",
    "i": "1h",
    "o": "42000.0",
    "c": "42500.0",
    "h": "42800.0",
    "l": "41800.0",
    "v": "150.5",
    "n": 1234
  }
]
```

**请求: userFills**
```json
{
  "type": "userFills",
  "user": "0x1234567890abcdef1234567890abcdef12345678"
}
```

**响应: userFills**
```json
[
  {
    "coin": "BTC",
    "px": "42000.0",
    "sz": "0.1",
    "side": "B",
    "time": 1706500000000,
    "startPosition": "0.4",
    "dir": "Open Long",
    "closedPnl": "0.0",
    "hash": "0xabc123...",
    "oid": 12345,
    "crossed": true,
    "fee": "2.1",
    "tid": 67890,
    "feeToken": "USDC"
  }
]
```

#### 4.4.2 Exchange API 示例

**请求: order (下单)**
```json
{
  "action": {
    "type": "order",
    "orders": [
      {
        "a": 0,
        "b": true,
        "p": "42000.0",
        "s": "0.1",
        "r": false,
        "t": {
          "limit": {
            "tif": "Gtc"
          }
        },
        "c": "client-order-001"
      }
    ],
    "grouping": "na"
  },
  "nonce": 1706500000000,
  "signature": {
    "r": "0x...",
    "s": "0x...",
    "v": 27
  },
  "vaultAddress": null
}
```

**响应: order**
```json
{
  "status": "ok",
  "response": {
    "type": "order",
    "data": {
      "statuses": [
        {
          "resting": {
            "oid": 12345
          }
        }
      ]
    }
  }
}
```

**请求: cancel (撤单)**
```json
{
  "action": {
    "type": "cancel",
    "cancels": [
      {
        "a": 0,
        "o": 12345
      }
    ]
  },
  "nonce": 1706500000001,
  "signature": {
    "r": "0x...",
    "s": "0x...",
    "v": 27
  }
}
```

### 4.5 错误响应格式

```json
{
  "status": "err",
  "response": "Error message describing the issue"
}
```

**常见错误码**：

| 错误类型 | 响应示例 |
|---------|---------|
| 参数错误 | `{"status": "err", "response": "Invalid coin: UNKNOWN"}` |
| 用户不存在 | `{"status": "err", "response": "User not found"}` |
| 余额不足 | `{"status": "err", "response": "Insufficient margin"}` |
| 订单不存在 | `{"status": "err", "response": "Order not found"}` |
| 签名错误 | `{"status": "err", "response": "Invalid signature"}` |

### 4.6 Rust 类型定义

```rust
use serde::{Deserialize, Serialize};
use rust_decimal::Decimal;

// ==================== Info API Request ====================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum InfoRequest {
    // 市场数据
    Meta,
    MetaAndAssetCtxs,
    SpotMeta,
    SpotMetaAndAssetCtxs,
    AllMids,

    // 订单簿与行情
    L2Book { coin: String },
    CandleSnapshot { req: CandleRequest },
    RecentTrades { coin: String },

    // 用户账户
    ClearinghouseState { user: String },
    SpotClearinghouseState { user: String },
    UserVaultEquities { user: String },

    // 订单查询
    OpenOrders { user: String },
    FrontendOpenOrders { user: String },
    OrderStatus { user: String, oid: u64 },
    HistoricalOrders { user: String },

    // 成交与资金费
    UserFills { user: String },
    UserFillsByTime { user: String, start_time: u64, end_time: Option<u64> },
    UserFunding { user: String, start_time: u64, end_time: Option<u64> },
    FundingHistory { coin: String, start_time: u64, end_time: Option<u64> },
    PredictedFundings,

    // Builder / 手续费
    MaxBuilderFee { user: String, builder: String },
    UserFees { user: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CandleRequest {
    pub coin: String,
    pub interval: String,
    pub start_time: u64,
    pub end_time: Option<u64>,
}

// ==================== Exchange API Request ====================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExchangeRequest {
    pub action: ExchangeAction,
    pub nonce: u64,
    pub signature: Signature,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vault_address: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ExchangeAction {
    // 订单操作
    Order {
        orders: Vec<OrderRequest>,
        grouping: OrderGrouping,
    },
    Cancel {
        cancels: Vec<CancelRequest>,
    },
    CancelByCloid {
        cancels: Vec<CancelByCloidRequest>,
    },
    Modify {
        oid: u64,
        order: OrderRequest,
    },
    BatchModify {
        modifies: Vec<ModifyRequest>,
    },

    // 账户操作
    UpdateLeverage {
        asset: u32,
        is_cross: bool,
        leverage: u32,
    },
    UpdateIsolatedMargin {
        asset: u32,
        is_buy: bool,
        ntli: i64,
    },

    // 资金操作
    UsdSend {
        destination: String,
        amount: String,
    },
    Withdraw3 {
        destination: String,
        amount: String,
    },
    VaultDeposit {
        vault_address: String,
        amount: String,
    },
    VaultWithdraw {
        vault_address: String,
        amount: String,
    },

    // 授权操作
    ApproveBuilderFee {
        builder: String,
        max_fee_rate: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderRequest {
    pub a: u32,           // asset index
    pub b: bool,          // is_buy
    pub p: String,        // price
    pub s: String,        // size
    pub r: bool,          // reduce_only
    pub t: OrderType,     // order type
    #[serde(skip_serializing_if = "Option::is_none")]
    pub c: Option<String>, // client_order_id
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OrderType {
    Limit { tif: TimeInForce },
    Trigger {
        trigger_px: String,
        is_market: bool,
        tpsl: TpSlType,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum TimeInForce {
    Gtc,  // Good Till Cancel
    Ioc,  // Immediate Or Cancel
    Alo,  // Add Liquidity Only (Post Only)
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TpSlType {
    Tp,  // Take Profit
    Sl,  // Stop Loss
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OrderGrouping {
    Na,          // No grouping
    NormalTpsl,  // Normal with TP/SL
    PositionTpsl, // Position TP/SL
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelRequest {
    pub a: u32,  // asset index
    pub o: u64,  // order_id
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelByCloidRequest {
    pub asset: u32,
    pub cloid: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModifyRequest {
    pub oid: u64,
    pub order: OrderRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signature {
    pub r: String,
    pub s: String,
    pub v: u8,
}

// ==================== API Response ====================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ApiResponse<T> {
    Ok { status: String, response: T },
    Err { status: String, response: String },
}

impl<T> ApiResponse<T> {
    pub fn ok(data: T) -> Self {
        ApiResponse::Ok {
            status: "ok".to_string(),
            response: data,
        }
    }

    pub fn err(msg: impl Into<String>) -> Self {
        ApiResponse::Err {
            status: "err".to_string(),
            response: msg.into(),
        }
    }
}
```

---

## 5. WebSocket API 设计（Phase 2）

### 5.1 连接端点

| 环境 | WebSocket URL |
|------|--------------|
| 主网 | `wss://api.dex.example.com/ws` |
| 测试网 | `wss://api-testnet.dex.example.com/ws` |

### 5.2 订阅类型

#### 5.2.1 公开数据订阅

| 订阅类型 | 说明 | 参数 | 推送频率 |
|---------|------|------|---------|
| `allMids` | 所有交易对中间价 | 无 | 实时 |
| `l2Book` | 订单簿深度 | `coin` | 实时（有变化时） |
| `trades` | 最新成交 | `coin` | 实时 |
| `candle` | K线更新 | `coin`, `interval` | 每秒/每根K线 |

#### 5.2.2 用户数据订阅（需认证）

| 订阅类型 | 说明 | 参数 | 推送频率 |
|---------|------|------|---------|
| `orderUpdates` | 订单状态变化 | `user` | 实时 |
| `userFills` | 用户成交推送 | `user` | 实时 |
| `userFunding` | 用户资金费结算 | `user` | 每8小时 |
| `webData2` | 用户综合数据 | `user` | 实时 |

### 5.3 消息格式

#### 5.3.1 订阅请求

```json
{
  "method": "subscribe",
  "subscription": {
    "type": "l2Book",
    "coin": "BTC"
  }
}
```

```json
{
  "method": "subscribe",
  "subscription": {
    "type": "orderUpdates",
    "user": "0x1234..."
  }
}
```

#### 5.3.2 取消订阅

```json
{
  "method": "unsubscribe",
  "subscription": {
    "type": "l2Book",
    "coin": "BTC"
  }
}
```

#### 5.3.3 推送消息格式

**allMids 推送**
```json
{
  "channel": "allMids",
  "data": {
    "mids": {
      "BTC": "42000.5",
      "ETH": "2500.0",
      "SOL": "95.5"
    },
    "time": 1706500000000
  }
}
```

**l2Book 推送**
```json
{
  "channel": "l2Book",
  "data": {
    "coin": "BTC",
    "time": 1706500000000,
    "levels": [
      [
        {"px": "42000.0", "sz": "1.5", "n": 3},
        {"px": "41999.5", "sz": "2.0", "n": 2}
      ],
      [
        {"px": "42000.5", "sz": "1.2", "n": 2},
        {"px": "42001.0", "sz": "3.0", "n": 5}
      ]
    ]
  }
}
```

**trades 推送**
```json
{
  "channel": "trades",
  "data": [
    {
      "coin": "BTC",
      "side": "B",
      "px": "42000.0",
      "sz": "0.1",
      "time": 1706500000000,
      "hash": "0xabc123...",
      "tid": 12345
    }
  ]
}
```

**candle 推送**
```json
{
  "channel": "candle",
  "data": {
    "t": 1706500000000,
    "T": 1706503600000,
    "s": "BTC",
    "i": "1h",
    "o": "42000.0",
    "c": "42500.0",
    "h": "42800.0",
    "l": "41800.0",
    "v": "150.5",
    "n": 1234
  }
}
```

**orderUpdates 推送**
```json
{
  "channel": "orderUpdates",
  "data": [
    {
      "order": {
        "coin": "BTC",
        "side": "B",
        "limitPx": "42000.0",
        "sz": "0.1",
        "oid": 12345,
        "timestamp": 1706500000000,
        "origSz": "0.1",
        "cloid": "client-order-001"
      },
      "status": "open",
      "statusTimestamp": 1706500000000
    }
  ]
}
```

**userFills 推送**
```json
{
  "channel": "userFills",
  "data": {
    "user": "0x1234...",
    "fills": [
      {
        "coin": "BTC",
        "px": "42000.0",
        "sz": "0.1",
        "side": "B",
        "time": 1706500000000,
        "startPosition": "0.4",
        "dir": "Open Long",
        "closedPnl": "0.0",
        "hash": "0xabc123...",
        "oid": 12345,
        "crossed": true,
        "fee": "2.1",
        "tid": 67890
      }
    ]
  }
}
```

### 5.4 心跳保活机制

**心跳请求（客户端发送）**
```json
{
  "method": "ping"
}
```

**心跳响应（服务端返回）**
```json
{
  "channel": "pong"
}
```

**超时规则**：
- 服务端 60 秒无活动发送 ping
- 客户端需在 10 秒内响应 pong
- 建议客户端每 30 秒主动发送 ping

### 5.5 重连策略

```rust
/// WebSocket 重连配置
pub struct ReconnectConfig {
    /// 初始重连延迟 (毫秒)
    pub initial_delay_ms: u64,      // 默认: 1000
    /// 最大重连延迟 (毫秒)
    pub max_delay_ms: u64,          // 默认: 30000
    /// 延迟增长倍数
    pub backoff_multiplier: f64,    // 默认: 2.0
    /// 最大重连次数 (0=无限)
    pub max_retries: u32,           // 默认: 0
    /// 添加随机抖动
    pub jitter: bool,               // 默认: true
}
```

**重连流程**：
1. 连接断开 → 等待 `initial_delay_ms`
2. 第 N 次重试 → 等待 `min(initial_delay_ms * backoff_multiplier^N, max_delay_ms)`
3. 重连成功 → 重新订阅所有频道
4. 重连失败达 `max_retries` → 通知上层应用

### 5.6 Rust WebSocket 类型定义

```rust
use serde::{Deserialize, Serialize};

// ==================== WebSocket 消息 ====================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "camelCase")]
pub enum WsClientMessage {
    Subscribe { subscription: WsSubscription },
    Unsubscribe { subscription: WsSubscription },
    Ping,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum WsSubscription {
    AllMids,
    L2Book { coin: String },
    Trades { coin: String },
    Candle { coin: String, interval: String },
    OrderUpdates { user: String },
    UserFills { user: String },
    UserFunding { user: String },
    WebData2 { user: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "channel", rename_all = "camelCase")]
pub enum WsServerMessage {
    // 系统消息
    Pong,
    Error { data: String },
    SubscriptionResponse { data: SubscriptionResult },

    // 公开数据
    AllMids { data: AllMidsData },
    L2Book { data: L2BookData },
    Trades { data: Vec<TradeData> },
    Candle { data: CandleData },

    // 用户数据
    OrderUpdates { data: Vec<OrderUpdateData> },
    UserFills { data: UserFillsData },
    UserFunding { data: UserFundingData },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionResult {
    pub method: String,
    pub subscription: WsSubscription,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AllMidsData {
    pub mids: std::collections::HashMap<String, String>,
    pub time: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct L2BookData {
    pub coin: String,
    pub time: u64,
    pub levels: (Vec<Level>, Vec<Level>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Level {
    pub px: String,
    pub sz: String,
    pub n: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeData {
    pub coin: String,
    pub side: String,
    pub px: String,
    pub sz: String,
    pub time: u64,
    pub hash: String,
    pub tid: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CandleData {
    pub t: u64,           // open time
    #[serde(rename = "T")]
    pub close_time: u64,  // close time
    pub s: String,        // symbol
    pub i: String,        // interval
    pub o: String,        // open
    pub c: String,        // close
    pub h: String,        // high
    pub l: String,        // low
    pub v: String,        // volume
    pub n: u64,           // trades count
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderUpdateData {
    pub order: OrderInfo,
    pub status: String,
    pub status_timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderInfo {
    pub coin: String,
    pub side: String,
    pub limit_px: String,
    pub sz: String,
    pub oid: u64,
    pub timestamp: u64,
    pub orig_sz: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloid: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserFillsData {
    pub user: String,
    pub fills: Vec<FillInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FillInfo {
    pub coin: String,
    pub px: String,
    pub sz: String,
    pub side: String,
    pub time: u64,
    pub start_position: String,
    pub dir: String,
    pub closed_pnl: String,
    pub hash: String,
    pub oid: u64,
    pub crossed: bool,
    pub fee: String,
    pub tid: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserFundingData {
    pub user: String,
    pub funding_payments: Vec<FundingPayment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FundingPayment {
    pub coin: String,
    pub usdc: String,
    pub szi: String,
    pub funding_rate: String,
    pub time: u64,
}
```

---

## 6. 数据模型

### 6.1 核心实体定义

#### 6.1.1 Market（市场/交易对）

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PerpAsset {
    pub name: String,           // "BTC"
    pub sz_decimals: u32,       // 数量精度 (5 = 0.00001)
    pub max_leverage: u32,      // 最大杠杆 (50)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub only_isolated: Option<bool>,  // 是否仅支持逐仓
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotAsset {
    pub name: String,           // "USDC"
    pub sz_decimals: u32,       // 数量精度
    pub wei_decimals: u32,      // Wei 精度
    pub index: u32,             // 资产索引
    pub token_id: String,       // 代币 ID
    pub is_canonical: bool,     // 是否为规范代币
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetCtx {
    pub funding: String,        // 当前资金费率
    pub open_interest: String,  // 未平仓合约
    pub prev_day_px: String,    // 24h前价格
    pub day_ntl_vlm: String,    // 24h名义成交量
    pub premium: Option<String>, // 溢价率
    pub oracle_px: String,      // 预言机价格
    pub mark_px: String,        // 标记价格
    pub mid_px: Option<String>, // 中间价
    pub impact_pxs: Option<(String, String)>, // (买入/卖出冲击价)
}
```

#### 6.1.2 Order（订单）

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenOrder {
    pub coin: String,
    pub limit_px: String,
    pub oid: u64,
    pub side: String,           // "B" | "A"
    pub sz: String,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrontendOrder {
    pub coin: String,
    pub is_position_tpsl: bool,
    pub is_trigger: bool,
    pub limit_px: String,
    pub oid: u64,
    pub order_type: String,
    pub orig_sz: String,
    pub reduce_only: bool,
    pub side: String,
    pub sz: String,
    pub timestamp: u64,
    pub trigger_condition: String,
    pub trigger_px: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloid: Option<String>,
    pub children: Option<Vec<FrontendOrder>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoricalOrder {
    pub order: FrontendOrder,
    pub status: String,
    pub status_timestamp: u64,
}
```

#### 6.1.3 Fill（成交）

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserFill {
    pub coin: String,
    pub px: String,
    pub sz: String,
    pub side: String,
    pub time: u64,
    pub start_position: String,
    pub dir: String,            // "Open Long" | "Close Long" | "Open Short" | "Close Short"
    pub closed_pnl: String,
    pub hash: String,
    pub oid: u64,
    pub crossed: bool,          // true=吃单, false=挂单
    pub fee: String,
    pub tid: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fee_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub liquidation_mark_px: Option<String>,
}
```

#### 6.1.4 Position（持仓）

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetPosition {
    pub position: PositionInfo,
    #[serde(rename = "type")]
    pub position_type: String,  // "oneWay"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PositionInfo {
    pub coin: String,
    pub szi: String,            // 持仓数量 (正=多, 负=空)
    pub leverage: LeverageInfo,
    pub entry_px: Option<String>,
    pub position_value: String,
    pub unrealized_pnl: String,
    pub return_on_equity: String,
    pub liquidation_px: Option<String>,
    pub margin_used: String,
    pub max_trade_szs: (String, String),  // (可买, 可卖)
    pub cum_funding: CumFunding,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LeverageInfo {
    #[serde(rename = "type")]
    pub leverage_type: String,  // "cross" | "isolated"
    pub value: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_usd: Option<String>,  // 逐仓保证金
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CumFunding {
    pub all_time: String,
    pub since_open: String,
    pub since_change: String,
}
```

#### 6.1.5 Margin（保证金）

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarginSummary {
    pub account_value: String,
    pub total_ntl_pos: String,
    pub total_raw_usd: String,
    pub total_margin_used: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClearinghouseState {
    pub margin_summary: MarginSummary,
    pub cross_margin_summary: MarginSummary,
    pub withdrawable: String,
    pub asset_positions: Vec<AssetPosition>,
    pub time: u64,
}
```

#### 6.1.6 Candle（K线）

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candle {
    pub t: u64,           // 开盘时间
    #[serde(rename = "T")]
    pub close_time: u64,  // 收盘时间
    pub s: String,        // 交易对
    pub i: String,        // 时间周期 (1m, 5m, 15m, 1h, 4h, 1d)
    pub o: String,        // 开盘价
    pub c: String,        // 收盘价
    pub h: String,        // 最高价
    pub l: String,        // 最低价
    pub v: String,        // 成交量
    pub n: u64,           // 成交笔数
}
```

#### 6.1.7 Funding（资金费）

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FundingHistory {
    pub coin: String,
    pub funding_rate: String,
    pub premium: String,
    pub time: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserFundingRecord {
    pub time: u64,
    pub coin: String,
    pub usdc: String,
    pub szi: String,
    pub funding_rate: String,
}
```

### 6.2 响应结构示例

#### 6.2.1 meta 响应

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetaResponse {
    pub universe: Vec<PerpAsset>,
}
```

#### 6.2.2 metaAndAssetCtxs 响应

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetaAndAssetCtxsResponse(pub MetaResponse, pub Vec<AssetCtx>);
```

#### 6.2.3 l2Book 响应

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct L2BookResponse {
    pub coin: String,
    pub time: u64,
    pub levels: (Vec<L2Level>, Vec<L2Level>),  // (bids, asks)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct L2Level {
    pub px: String,
    pub sz: String,
    pub n: u32,
}
```

#### 6.2.4 clearinghouseState 响应

```rust
// 已在 6.1.5 中定义 ClearinghouseState
```

#### 6.2.5 userFills 响应

```rust
// Vec<UserFill>，已在 6.1.3 中定义
```

#### 6.2.6 fundingHistory 响应

```rust
// Vec<FundingHistory>，已在 6.1.7 中定义
```

---

## 7. 存储设计

### 7.1 存储架构

```
┌─────────────────────────────────────────────────────────────┐
│                      存储层架构                              │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  Phase 1: PostgreSQL Only                                   │
│  ┌─────────────────────────────────────────────────────┐   │
│  │  PostgreSQL                                          │   │
│  │  ├── markets (交易对配置)                           │   │
│  │  ├── orders (活跃订单)                              │   │
│  │  ├── fills (成交记录，按天分区)                    │   │
│  │  ├── perpetual_positions (永续持仓)                 │   │
│  │  ├── balances (账户余额)                            │   │
│  │  ├── candles (K线数据，按月分区)                   │   │
│  │  ├── funding_rates (资金费率，按月分区)            │   │
│  │  ├── user_funding_records (用户资金费记录)          │   │
│  │  ├── transfers (充提记录)                           │   │
│  │  ├── liquidations (清算记录)                        │   │
│  │  └── dex_watermarks (断点续传标记)                  │   │
│  └─────────────────────────────────────────────────────┘   │
│                                                              │
│  Phase 2: + Redis (实时缓存)                                │
│  ┌─────────────────────────────────────────────────────┐   │
│  │  Redis                                               │   │
│  │  ├── orderbook:{market_id} (订单簿快照)             │   │
│  │  ├── mid_prices (所有中间价)                        │   │
│  │  ├── user:{address}:orders (用户活跃订单)           │   │
│  │  └── recent_trades:{market_id} (最近成交)           │   │
│  └─────────────────────────────────────────────────────┘   │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

### 7.2 表结构 DDL

#### 7.2.1 markets 表（交易对配置）

```sql
-- 交易对配置表
CREATE TABLE markets (
    id BIGSERIAL PRIMARY KEY,
    market_id BIGINT NOT NULL UNIQUE,
    symbol VARCHAR(32) NOT NULL,              -- "BTC", "ETH"
    name VARCHAR(64) NOT NULL,                -- "BTC-PERP"
    market_type VARCHAR(16) NOT NULL,         -- "perpetual" | "spot"
    base_asset VARCHAR(32) NOT NULL,          -- "BTC"
    quote_asset VARCHAR(32) NOT NULL,         -- "USDC"
    price_decimals SMALLINT NOT NULL,         -- 价格精度
    size_decimals SMALLINT NOT NULL,          -- 数量精度
    min_order_size DECIMAL(36, 18) NOT NULL,  -- 最小订单数量
    max_leverage SMALLINT NOT NULL DEFAULT 1, -- 最大杠杆
    maker_fee DECIMAL(18, 8) NOT NULL,        -- Maker 费率
    taker_fee DECIMAL(18, 8) NOT NULL,        -- Taker 费率
    only_isolated BOOLEAN NOT NULL DEFAULT false,
    status VARCHAR(16) NOT NULL DEFAULT 'active',  -- "active" | "suspended"
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW(),
    checkpoint_sequence BIGINT NOT NULL
);

CREATE UNIQUE INDEX idx_markets_symbol ON markets (symbol);
CREATE INDEX idx_markets_type ON markets (market_type);
```

#### 7.2.2 orders 表（活跃订单）

```sql
-- 活跃订单表
CREATE TABLE orders (
    id BIGSERIAL PRIMARY KEY,
    order_id BIGINT NOT NULL UNIQUE,
    market_id BIGINT NOT NULL REFERENCES markets(market_id),
    owner VARCHAR(66) NOT NULL,               -- 用户地址 (0x...)
    side VARCHAR(4) NOT NULL,                 -- "B" | "A"
    order_type VARCHAR(16) NOT NULL,          -- "Limit" | "Market" | "StopMarket" | "StopLimit"
    price DECIMAL(36, 18) NOT NULL,           -- 委托价格
    quantity DECIMAL(36, 18) NOT NULL,        -- 委托数量
    filled_quantity DECIMAL(36, 18) NOT NULL DEFAULT 0,
    remaining_quantity DECIMAL(36, 18) NOT NULL,
    status VARCHAR(16) NOT NULL DEFAULT 'open',  -- "open" | "partialFilled"
    tif VARCHAR(8) NOT NULL DEFAULT 'Gtc',    -- "Gtc" | "Ioc" | "Alo"
    reduce_only BOOLEAN NOT NULL DEFAULT false,
    trigger_price DECIMAL(36, 18),            -- 触发价格 (条件单)
    trigger_type VARCHAR(8),                  -- "tp" | "sl"
    client_order_id VARCHAR(66),              -- 客户端订单 ID
    created_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL DEFAULT NOW(),
    checkpoint_sequence BIGINT NOT NULL
);

-- 按用户+状态查询
CREATE INDEX idx_orders_owner_status ON orders (owner, status);
-- 按市场+状态查询
CREATE INDEX idx_orders_market_status ON orders (market_id, status);
-- 按客户端订单 ID 查询
CREATE INDEX idx_orders_cloid ON orders (owner, client_order_id) WHERE client_order_id IS NOT NULL;
```

#### 7.2.3 fills 表（成交记录，按天分区）

```sql
-- 成交记录表（按天分区，高写入量）
CREATE TABLE fills (
    id BIGSERIAL,
    fill_id BIGINT NOT NULL,
    market_id BIGINT NOT NULL,
    maker_order_id BIGINT NOT NULL,
    taker_order_id BIGINT NOT NULL,
    maker_address VARCHAR(66) NOT NULL,
    taker_address VARCHAR(66) NOT NULL,
    side VARCHAR(4) NOT NULL,                 -- Taker 方向: "B" | "A"
    price DECIMAL(36, 18) NOT NULL,
    quantity DECIMAL(36, 18) NOT NULL,
    maker_fee DECIMAL(36, 18) NOT NULL,
    taker_fee DECIMAL(36, 18) NOT NULL,
    closed_pnl_maker DECIMAL(36, 18),
    closed_pnl_taker DECIMAL(36, 18),
    tx_hash VARCHAR(128) NOT NULL,
    created_at TIMESTAMP NOT NULL,
    checkpoint_sequence BIGINT NOT NULL,
    PRIMARY KEY (id, created_at)
) PARTITION BY RANGE (created_at);

-- 创建分区（示例：按天）
CREATE TABLE fills_2026_01_29 PARTITION OF fills
    FOR VALUES FROM ('2026-01-29') TO ('2026-01-30');

CREATE TABLE fills_2026_01_30 PARTITION OF fills
    FOR VALUES FROM ('2026-01-30') TO ('2026-01-31');

-- 自动创建分区的函数
CREATE OR REPLACE FUNCTION create_fills_partition()
RETURNS TRIGGER AS $$
DECLARE
    partition_date DATE;
    partition_name TEXT;
    start_date DATE;
    end_date DATE;
BEGIN
    partition_date := DATE(NEW.created_at);
    partition_name := 'fills_' || TO_CHAR(partition_date, 'YYYY_MM_DD');
    start_date := partition_date;
    end_date := partition_date + INTERVAL '1 day';

    IF NOT EXISTS (
        SELECT 1 FROM pg_class WHERE relname = partition_name
    ) THEN
        EXECUTE format(
            'CREATE TABLE %I PARTITION OF fills FOR VALUES FROM (%L) TO (%L)',
            partition_name, start_date, end_date
        );
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- 索引（每个分区自动继承）
CREATE INDEX idx_fills_market_time ON fills (market_id, created_at DESC);
CREATE INDEX idx_fills_maker ON fills (maker_address, created_at DESC);
CREATE INDEX idx_fills_taker ON fills (taker_address, created_at DESC);
CREATE INDEX idx_fills_checkpoint ON fills (checkpoint_sequence);
CREATE UNIQUE INDEX idx_fills_fill_id ON fills (fill_id, created_at);
```

#### 7.2.4 perpetual_positions 表（永续持仓）

```sql
-- 永续持仓表（当前状态，UPSERT 更新）
CREATE TABLE perpetual_positions (
    id BIGSERIAL PRIMARY KEY,
    position_id BIGINT NOT NULL UNIQUE,
    owner VARCHAR(66) NOT NULL,
    market_id BIGINT NOT NULL REFERENCES markets(market_id),
    size DECIMAL(36, 18) NOT NULL,            -- 正=多头, 负=空头
    entry_price DECIMAL(36, 18) NOT NULL,
    leverage DECIMAL(10, 2) NOT NULL,
    leverage_type VARCHAR(16) NOT NULL,       -- "cross" | "isolated"
    margin DECIMAL(36, 18) NOT NULL,
    unrealized_pnl DECIMAL(36, 18) NOT NULL DEFAULT 0,
    liquidation_price DECIMAL(36, 18),
    return_on_equity DECIMAL(18, 8) NOT NULL DEFAULT 0,
    cum_funding_all_time DECIMAL(36, 18) NOT NULL DEFAULT 0,
    cum_funding_since_open DECIMAL(36, 18) NOT NULL DEFAULT 0,
    created_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL DEFAULT NOW(),
    checkpoint_sequence BIGINT NOT NULL,
    UNIQUE (owner, market_id)
);

-- 按用户查询所有持仓
CREATE INDEX idx_positions_owner ON perpetual_positions (owner);
-- 按市场查询所有持仓
CREATE INDEX idx_positions_market ON perpetual_positions (market_id);
```

#### 7.2.5 balances 表（账户余额）

```sql
-- 账户余额表（当前状态，UPSERT 更新）
CREATE TABLE balances (
    id BIGSERIAL PRIMARY KEY,
    owner VARCHAR(66) NOT NULL,
    asset VARCHAR(32) NOT NULL,               -- "USDC", "BTC", etc.
    total DECIMAL(36, 18) NOT NULL,
    available DECIMAL(36, 18) NOT NULL,
    locked DECIMAL(36, 18) NOT NULL DEFAULT 0,
    updated_at TIMESTAMP NOT NULL DEFAULT NOW(),
    checkpoint_sequence BIGINT NOT NULL,
    UNIQUE (owner, asset)
);

CREATE INDEX idx_balances_owner ON balances (owner);
```

#### 7.2.6 candles 表（K线数据，按月分区）

```sql
-- K线数据表（按月分区，时序数据）
CREATE TABLE candles (
    id BIGSERIAL,
    market_id BIGINT NOT NULL,
    interval VARCHAR(8) NOT NULL,             -- "1m", "5m", "15m", "1h", "4h", "1d"
    open_time TIMESTAMP NOT NULL,
    close_time TIMESTAMP NOT NULL,
    open DECIMAL(36, 18) NOT NULL,
    high DECIMAL(36, 18) NOT NULL,
    low DECIMAL(36, 18) NOT NULL,
    close DECIMAL(36, 18) NOT NULL,
    volume DECIMAL(36, 18) NOT NULL,
    trades_count BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    checkpoint_sequence BIGINT NOT NULL,
    PRIMARY KEY (id, open_time),
    UNIQUE (market_id, interval, open_time)
) PARTITION BY RANGE (open_time);

-- 按月分区
CREATE TABLE candles_2026_01 PARTITION OF candles
    FOR VALUES FROM ('2026-01-01') TO ('2026-02-01');

CREATE INDEX idx_candles_market_interval ON candles (market_id, interval, open_time DESC);
```

#### 7.2.7 funding_rates 表（资金费率，按月分区）

```sql
-- 市场资金费率历史（按月分区）
CREATE TABLE funding_rates (
    id BIGSERIAL,
    market_id BIGINT NOT NULL,
    funding_rate DECIMAL(18, 10) NOT NULL,
    premium DECIMAL(18, 10) NOT NULL,
    mark_price DECIMAL(36, 18) NOT NULL,
    index_price DECIMAL(36, 18) NOT NULL,
    open_interest DECIMAL(36, 18) NOT NULL,
    settlement_time TIMESTAMP NOT NULL,
    checkpoint_sequence BIGINT NOT NULL,
    PRIMARY KEY (id, settlement_time)
) PARTITION BY RANGE (settlement_time);

-- 按月分区
CREATE TABLE funding_rates_2026_01 PARTITION OF funding_rates
    FOR VALUES FROM ('2026-01-01') TO ('2026-02-01');

CREATE INDEX idx_funding_market_time ON funding_rates (market_id, settlement_time DESC);
CREATE UNIQUE INDEX idx_funding_unique ON funding_rates (market_id, settlement_time);
```

#### 7.2.8 user_funding_records 表（用户资金费记录）

```sql
-- 用户资金费支付/收取记录
CREATE TABLE user_funding_records (
    id BIGSERIAL,
    owner VARCHAR(66) NOT NULL,
    market_id BIGINT NOT NULL,
    position_size DECIMAL(36, 18) NOT NULL,
    funding_rate DECIMAL(18, 10) NOT NULL,
    payment DECIMAL(36, 18) NOT NULL,         -- 正=支付, 负=收取
    settlement_time TIMESTAMP NOT NULL,
    checkpoint_sequence BIGINT NOT NULL,
    PRIMARY KEY (id, settlement_time)
) PARTITION BY RANGE (settlement_time);

CREATE TABLE user_funding_records_2026_01 PARTITION OF user_funding_records
    FOR VALUES FROM ('2026-01-01') TO ('2026-02-01');

CREATE INDEX idx_user_funding_owner ON user_funding_records (owner, settlement_time DESC);
```

#### 7.2.9 transfers 表（充提记录）

```sql
-- 充值/提现/内部转账记录
CREATE TABLE transfers (
    id BIGSERIAL PRIMARY KEY,
    transfer_id BIGINT NOT NULL UNIQUE,
    transfer_type VARCHAR(16) NOT NULL,       -- "deposit" | "withdraw" | "internal"
    from_address VARCHAR(66) NOT NULL,
    to_address VARCHAR(66) NOT NULL,
    asset VARCHAR(32) NOT NULL,
    amount DECIMAL(36, 18) NOT NULL,
    fee DECIMAL(36, 18) NOT NULL DEFAULT 0,
    status VARCHAR(16) NOT NULL,              -- "pending" | "confirmed" | "failed"
    tx_hash VARCHAR(128),
    created_at TIMESTAMP NOT NULL,
    confirmed_at TIMESTAMP,
    checkpoint_sequence BIGINT NOT NULL
);

CREATE INDEX idx_transfers_from ON transfers (from_address, created_at DESC);
CREATE INDEX idx_transfers_to ON transfers (to_address, created_at DESC);
```

#### 7.2.10 liquidations 表（清算记录）

```sql
-- 清算记录
CREATE TABLE liquidations (
    id BIGSERIAL PRIMARY KEY,
    liquidation_id BIGINT NOT NULL UNIQUE,
    position_id BIGINT NOT NULL,
    owner VARCHAR(66) NOT NULL,
    liquidator VARCHAR(66) NOT NULL,
    market_id BIGINT NOT NULL,
    size DECIMAL(36, 18) NOT NULL,
    price DECIMAL(36, 18) NOT NULL,
    pnl DECIMAL(36, 18) NOT NULL,
    created_at TIMESTAMP NOT NULL,
    checkpoint_sequence BIGINT NOT NULL
);

CREATE INDEX idx_liquidations_owner ON liquidations (owner, created_at DESC);
CREATE INDEX idx_liquidations_market ON liquidations (market_id, created_at DESC);
```

#### 7.2.11 dex_watermarks 表（断点续传标记）

```sql
-- 断点续传标记（借鉴 sui-indexer-alt）
CREATE TABLE dex_watermarks (
    pipeline VARCHAR(64) PRIMARY KEY,
    epoch_hi BIGINT NOT NULL,
    checkpoint_hi BIGINT NOT NULL,
    tx_hi BIGINT NOT NULL DEFAULT 0,
    timestamp_ms BIGINT NOT NULL,
    reader_lo BIGINT NOT NULL DEFAULT 0,
    pruner_hi BIGINT NOT NULL DEFAULT 0,
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
);

-- 初始化各 Pipeline 的 Watermark
INSERT INTO dex_watermarks (pipeline, epoch_hi, checkpoint_hi, timestamp_ms)
VALUES
    ('fills', 0, 0, 0),
    ('positions', 0, 0, 0),
    ('balances', 0, 0, 0),
    ('candles', 0, 0, 0),
    ('funding', 0, 0, 0),
    ('transfers', 0, 0, 0),
    ('liquidations', 0, 0, 0);
```

### 7.3 索引设计原则

| 查询模式 | 索引策略 | 示例 |
|---------|---------|------|
| 用户查询自己的数据 | `(owner, timestamp DESC)` | `idx_fills_taker` |
| 按市场查询 | `(market_id, timestamp DESC)` | `idx_fills_market_time` |
| 唯一性约束 | `UNIQUE INDEX` | `idx_fills_fill_id` |
| 断点续传 | `(checkpoint_sequence)` | `idx_fills_checkpoint` |
| 条件索引 | `WHERE condition IS NOT NULL` | `idx_orders_cloid` |

### 7.4 分区策略

| 表 | 分区键 | 分区粒度 | 说明 |
|----|-------|---------|------|
| fills | `created_at` | 按天 | 高写入量，保留 90 天 |
| candles | `open_time` | 按月 | 中等写入，保留 2 年 |
| funding_rates | `settlement_time` | 按月 | 低写入，保留 2 年 |
| user_funding_records | `settlement_time` | 按月 | 低写入，保留 2 年 |

### 7.5 数据保留策略

```sql
-- 自动清理超过保留期限的分区
CREATE OR REPLACE FUNCTION drop_old_partitions(
    table_name TEXT,
    retention_days INT
) RETURNS VOID AS $$
DECLARE
    partition_name TEXT;
    cutoff_date DATE := CURRENT_DATE - retention_days;
BEGIN
    FOR partition_name IN
        SELECT tablename FROM pg_tables
        WHERE tablename LIKE table_name || '_%'
          AND tablename < table_name || '_' || TO_CHAR(cutoff_date, 'YYYY_MM_DD')
    LOOP
        EXECUTE 'DROP TABLE IF EXISTS ' || partition_name;
        RAISE NOTICE 'Dropped partition: %', partition_name;
    END LOOP;
END;
$$ LANGUAGE plpgsql;

-- 定期任务：每天清理过期分区
-- SELECT drop_old_partitions('fills', 90);
-- SELECT drop_old_partitions('candles', 730);
```

### 7.6 Redis 缓存设计（Phase 2）

```
# 订单簿快照
Key: orderbook:{market_id}
Type: Hash
Fields:
  - bids: JSON array of levels
  - asks: JSON array of levels
  - time: timestamp_ms
TTL: None (持久)

# 所有中间价
Key: mid_prices
Type: Hash
Fields:
  - BTC: "42000.5"
  - ETH: "2500.0"
TTL: None (持久)

# 用户活跃订单
Key: user:{address}:orders
Type: SortedSet
Score: order_id
Member: JSON order object
TTL: None (持久)

# 最近成交
Key: recent_trades:{market_id}
Type: List
Element: JSON trade object
Length: 保留最近 1000 条
TTL: None (自动裁剪)
```

---

## 8. Pipeline 实现（借鉴 sui-indexer-alt）

### 8.1 Pipeline 架构概览

```
┌─────────────────────────────────────────────────────────────────────────┐
│                          Pipeline 架构                                   │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│  gRPC Stream                                                            │
│       │                                                                 │
│       ▼                                                                 │
│  ┌──────────────────────────────────────────────────────────────┐      │
│  │                    Processor Layer                            │      │
│  │  ┌────────────┐ ┌────────────┐ ┌────────────┐ ┌────────────┐│      │
│  │  │FillsHandler│ │PositionHdl│ │CandleHandler│ │FundingHdl  ││      │
│  │  └─────┬──────┘ └─────┬──────┘ └─────┬──────┘ └─────┬──────┘│      │
│  │        │              │              │              │        │      │
│  └────────┼──────────────┼──────────────┼──────────────┼────────┘      │
│           │              │              │              │                │
│           ▼              ▼              ▼              ▼                │
│  ┌──────────────────────────────────────────────────────────────┐      │
│  │                    Collector Layer                            │      │
│  │        批量收集处理结果，按 checkpoint 边界分组               │      │
│  └───────────────────────────┬──────────────────────────────────┘      │
│                              │                                          │
│                              ▼                                          │
│  ┌──────────────────────────────────────────────────────────────┐      │
│  │                    Committer Layer                            │      │
│  │        批量写入数据库，保证幂等性                             │      │
│  └───────────────────────────┬──────────────────────────────────┘      │
│                              │                                          │
│                              ▼                                          │
│  ┌──────────────────────────────────────────────────────────────┐      │
│  │                    Watermark Layer                            │      │
│  │        更新断点续传标记，记录处理进度                         │      │
│  └──────────────────────────────────────────────────────────────┘      │
│                                                                          │
└─────────────────────────────────────────────────────────────────────────┘
```

### 8.2 Handler Trait 定义

```rust
use async_trait::async_trait;
use anyhow::Result;
use sqlx::PgPool;

/// 事件处理器 Trait（借鉴 sui-indexer-alt 的 Handler）
#[async_trait]
pub trait Handler: Send + Sync + 'static {
    /// Handler 名称（用于日志和 Watermark）
    fn name(&self) -> &'static str;

    /// 处理事件批次，返回待写入的数据
    async fn process(&self, batch: &OnChainEventBatch) -> Result<ProcessedBatch>;

    /// 批量写入数据库（幂等）
    async fn commit(&self, pool: &PgPool, data: ProcessedBatch) -> Result<()>;

    /// 是否并发安全（可多实例并行处理）
    fn is_concurrent_safe(&self) -> bool {
        false // 默认顺序处理
    }
}

/// 处理后的数据批次
#[derive(Debug)]
pub struct ProcessedBatch {
    pub checkpoint_sequence: u64,
    pub timestamp_ms: u64,
    pub rows: Vec<Box<dyn DatabaseRow>>,
}

/// 可写入数据库的行
pub trait DatabaseRow: Send + Sync {
    fn table_name(&self) -> &'static str;
    fn to_insert_query(&self) -> String;
}
```

### 8.3 FillsHandler 完整实现

```rust
use crate::events::{FillEvent, OnChainEventBatch};
use crate::pipeline::{Handler, ProcessedBatch, DatabaseRow};
use async_trait::async_trait;
use anyhow::Result;
use sqlx::{PgPool, Row};

pub struct FillsHandler;

impl FillsHandler {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Handler for FillsHandler {
    fn name(&self) -> &'static str {
        "fills"
    }

    async fn process(&self, batch: &OnChainEventBatch) -> Result<ProcessedBatch> {
        let rows: Vec<Box<dyn DatabaseRow>> = batch
            .fills
            .iter()
            .map(|fill| Box::new(FillRow::from(fill.clone())) as Box<dyn DatabaseRow>)
            .collect();

        Ok(ProcessedBatch {
            checkpoint_sequence: batch.checkpoint_sequence,
            timestamp_ms: batch.timestamp_ms,
            rows,
        })
    }

    async fn commit(&self, pool: &PgPool, data: ProcessedBatch) -> Result<()> {
        if data.rows.is_empty() {
            return Ok(());
        }

        // 使用 COPY 批量插入提升性能
        let mut tx = pool.begin().await?;

        for row in &data.rows {
            let fill = row.as_any().downcast_ref::<FillRow>().unwrap();

            sqlx::query(
                r#"
                INSERT INTO fills (
                    fill_id, market_id, maker_order_id, taker_order_id,
                    maker_address, taker_address, side, price, quantity,
                    maker_fee, taker_fee, tx_hash, created_at, checkpoint_sequence
                ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
                ON CONFLICT (fill_id, created_at) DO NOTHING
                "#,
            )
            .bind(fill.fill_id as i64)
            .bind(fill.market_id as i64)
            .bind(fill.maker_order_id as i64)
            .bind(fill.taker_order_id as i64)
            .bind(&fill.maker_address)
            .bind(&fill.taker_address)
            .bind(&fill.side)
            .bind(&fill.price)
            .bind(&fill.quantity)
            .bind(&fill.maker_fee)
            .bind(&fill.taker_fee)
            .bind(&fill.tx_hash)
            .bind(fill.created_at)
            .bind(fill.checkpoint_sequence as i64)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    fn is_concurrent_safe(&self) -> bool {
        true // fills 可以并行写入
    }
}

#[derive(Debug, Clone)]
struct FillRow {
    fill_id: u64,
    market_id: u64,
    maker_order_id: u64,
    taker_order_id: u64,
    maker_address: String,
    taker_address: String,
    side: String,
    price: rust_decimal::Decimal,
    quantity: rust_decimal::Decimal,
    maker_fee: rust_decimal::Decimal,
    taker_fee: rust_decimal::Decimal,
    tx_hash: String,
    created_at: chrono::NaiveDateTime,
    checkpoint_sequence: u64,
}

impl From<FillEvent> for FillRow {
    fn from(e: FillEvent) -> Self {
        Self {
            fill_id: e.fill_id,
            market_id: e.market_id,
            maker_order_id: e.maker_order_id,
            taker_order_id: e.taker_order_id,
            maker_address: e.maker_address,
            taker_address: e.taker_address,
            side: match e.side {
                Side::Buy => "B".to_string(),
                Side::Sell => "A".to_string(),
            },
            price: e.price,
            quantity: e.quantity,
            maker_fee: e.maker_fee,
            taker_fee: e.taker_fee,
            tx_hash: e.hash,
            created_at: chrono::NaiveDateTime::from_timestamp_millis(e.timestamp_ms as i64)
                .unwrap_or_default(),
            checkpoint_sequence: e.checkpoint_sequence,
        }
    }
}

impl DatabaseRow for FillRow {
    fn table_name(&self) -> &'static str {
        "fills"
    }

    fn to_insert_query(&self) -> String {
        "INSERT INTO fills ...".to_string()
    }
}

impl FillRow {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
```

### 8.4 PositionsHandler 实现

```rust
pub struct PositionsHandler;

#[async_trait]
impl Handler for PositionsHandler {
    fn name(&self) -> &'static str {
        "positions"
    }

    async fn process(&self, batch: &OnChainEventBatch) -> Result<ProcessedBatch> {
        let rows: Vec<Box<dyn DatabaseRow>> = batch
            .position_updates
            .iter()
            .map(|pos| Box::new(PositionRow::from(pos.clone())) as Box<dyn DatabaseRow>)
            .collect();

        Ok(ProcessedBatch {
            checkpoint_sequence: batch.checkpoint_sequence,
            timestamp_ms: batch.timestamp_ms,
            rows,
        })
    }

    async fn commit(&self, pool: &PgPool, data: ProcessedBatch) -> Result<()> {
        if data.rows.is_empty() {
            return Ok(());
        }

        let mut tx = pool.begin().await?;

        for row in &data.rows {
            let pos = row.as_any().downcast_ref::<PositionRow>().unwrap();

            // UPSERT: 持仓状态为最新状态
            sqlx::query(
                r#"
                INSERT INTO perpetual_positions (
                    position_id, owner, market_id, size, entry_price,
                    leverage, leverage_type, margin, unrealized_pnl,
                    liquidation_price, return_on_equity, created_at,
                    updated_at, checkpoint_sequence
                ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, NOW(), $13)
                ON CONFLICT (owner, market_id)
                DO UPDATE SET
                    size = EXCLUDED.size,
                    entry_price = EXCLUDED.entry_price,
                    leverage = EXCLUDED.leverage,
                    margin = EXCLUDED.margin,
                    unrealized_pnl = EXCLUDED.unrealized_pnl,
                    liquidation_price = EXCLUDED.liquidation_price,
                    return_on_equity = EXCLUDED.return_on_equity,
                    updated_at = NOW(),
                    checkpoint_sequence = EXCLUDED.checkpoint_sequence
                WHERE perpetual_positions.checkpoint_sequence < EXCLUDED.checkpoint_sequence
                "#,
            )
            .bind(pos.position_id as i64)
            .bind(&pos.owner)
            .bind(pos.market_id as i64)
            .bind(&pos.size)
            .bind(&pos.entry_price)
            .bind(&pos.leverage)
            .bind(&pos.leverage_type)
            .bind(&pos.margin)
            .bind(&pos.unrealized_pnl)
            .bind(&pos.liquidation_price)
            .bind(&pos.return_on_equity)
            .bind(pos.created_at)
            .bind(pos.checkpoint_sequence as i64)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }
}
```

### 8.5 CandlesHandler（K线聚合逻辑）

```rust
use std::collections::HashMap;

pub struct CandlesHandler {
    /// 内存中的 K 线缓存（按 market_id + interval）
    candle_cache: tokio::sync::RwLock<HashMap<(u64, String), CandleAggregator>>,
}

impl CandlesHandler {
    pub fn new() -> Self {
        Self {
            candle_cache: tokio::sync::RwLock::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl Handler for CandlesHandler {
    fn name(&self) -> &'static str {
        "candles"
    }

    async fn process(&self, batch: &OnChainEventBatch) -> Result<ProcessedBatch> {
        let mut cache = self.candle_cache.write().await;
        let mut completed_candles = Vec::new();

        // 按成交更新 K 线
        for fill in &batch.fills {
            let intervals = ["1m", "5m", "15m", "1h", "4h", "1d"];

            for interval in intervals {
                let key = (fill.market_id, interval.to_string());
                let aggregator = cache
                    .entry(key.clone())
                    .or_insert_with(|| CandleAggregator::new(fill.market_id, interval));

                if let Some(candle) = aggregator.add_trade(
                    fill.price,
                    fill.quantity,
                    fill.timestamp_ms,
                    batch.checkpoint_sequence,
                ) {
                    completed_candles.push(candle);
                }
            }
        }

        let rows: Vec<Box<dyn DatabaseRow>> = completed_candles
            .into_iter()
            .map(|c| Box::new(c) as Box<dyn DatabaseRow>)
            .collect();

        Ok(ProcessedBatch {
            checkpoint_sequence: batch.checkpoint_sequence,
            timestamp_ms: batch.timestamp_ms,
            rows,
        })
    }

    async fn commit(&self, pool: &PgPool, data: ProcessedBatch) -> Result<()> {
        if data.rows.is_empty() {
            return Ok(());
        }

        let mut tx = pool.begin().await?;

        for row in &data.rows {
            let candle = row.as_any().downcast_ref::<CandleRow>().unwrap();

            // UPSERT: 更新或插入 K 线
            sqlx::query(
                r#"
                INSERT INTO candles (
                    market_id, interval, open_time, close_time,
                    open, high, low, close, volume, trades_count,
                    checkpoint_sequence
                ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
                ON CONFLICT (market_id, interval, open_time)
                DO UPDATE SET
                    high = GREATEST(candles.high, EXCLUDED.high),
                    low = LEAST(candles.low, EXCLUDED.low),
                    close = EXCLUDED.close,
                    volume = candles.volume + EXCLUDED.volume,
                    trades_count = candles.trades_count + EXCLUDED.trades_count,
                    checkpoint_sequence = EXCLUDED.checkpoint_sequence
                "#,
            )
            .bind(candle.market_id as i64)
            .bind(&candle.interval)
            .bind(candle.open_time)
            .bind(candle.close_time)
            .bind(&candle.open)
            .bind(&candle.high)
            .bind(&candle.low)
            .bind(&candle.close)
            .bind(&candle.volume)
            .bind(candle.trades_count as i64)
            .bind(candle.checkpoint_sequence as i64)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }
}

/// K 线聚合器
struct CandleAggregator {
    market_id: u64,
    interval: String,
    interval_ms: u64,
    current_candle: Option<CandleRow>,
}

impl CandleAggregator {
    fn new(market_id: u64, interval: &str) -> Self {
        let interval_ms = match interval {
            "1m" => 60_000,
            "5m" => 300_000,
            "15m" => 900_000,
            "1h" => 3_600_000,
            "4h" => 14_400_000,
            "1d" => 86_400_000,
            _ => 60_000,
        };

        Self {
            market_id,
            interval: interval.to_string(),
            interval_ms,
            current_candle: None,
        }
    }

    /// 添加成交，返回已完成的 K 线（如果有）
    fn add_trade(
        &mut self,
        price: rust_decimal::Decimal,
        quantity: rust_decimal::Decimal,
        timestamp_ms: u64,
        checkpoint_sequence: u64,
    ) -> Option<CandleRow> {
        let candle_start = (timestamp_ms / self.interval_ms) * self.interval_ms;
        let candle_end = candle_start + self.interval_ms;

        // 检查是否需要关闭当前 K 线
        let completed = if let Some(ref current) = self.current_candle {
            if timestamp_ms >= current.close_time.timestamp_millis() as u64 {
                Some(current.clone())
            } else {
                None
            }
        } else {
            None
        };

        // 更新或创建新 K 线
        if let Some(ref mut current) = self.current_candle {
            if timestamp_ms < current.close_time.timestamp_millis() as u64 {
                // 更新当前 K 线
                current.high = current.high.max(price);
                current.low = current.low.min(price);
                current.close = price;
                current.volume += quantity;
                current.trades_count += 1;
                current.checkpoint_sequence = checkpoint_sequence;
            } else {
                // 新 K 线
                self.current_candle = Some(CandleRow {
                    market_id: self.market_id,
                    interval: self.interval.clone(),
                    open_time: chrono::NaiveDateTime::from_timestamp_millis(candle_start as i64)
                        .unwrap(),
                    close_time: chrono::NaiveDateTime::from_timestamp_millis(candle_end as i64)
                        .unwrap(),
                    open: price,
                    high: price,
                    low: price,
                    close: price,
                    volume: quantity,
                    trades_count: 1,
                    checkpoint_sequence,
                });
            }
        } else {
            // 首根 K 线
            self.current_candle = Some(CandleRow {
                market_id: self.market_id,
                interval: self.interval.clone(),
                open_time: chrono::NaiveDateTime::from_timestamp_millis(candle_start as i64)
                    .unwrap(),
                close_time: chrono::NaiveDateTime::from_timestamp_millis(candle_end as i64)
                    .unwrap(),
                open: price,
                high: price,
                low: price,
                close: price,
                volume: quantity,
                trades_count: 1,
                checkpoint_sequence,
            });
        }

        completed
    }
}
```

### 8.6 批量写入与幂等保证

```rust
/// Pipeline 协调器
pub struct PipelineCoordinator {
    handlers: Vec<Box<dyn Handler>>,
    pool: PgPool,
}

impl PipelineCoordinator {
    pub fn new(pool: PgPool) -> Self {
        Self {
            handlers: vec![
                Box::new(FillsHandler::new()),
                Box::new(PositionsHandler),
                Box::new(BalancesHandler),
                Box::new(CandlesHandler::new()),
                Box::new(FundingHandler),
                Box::new(TransfersHandler),
                Box::new(LiquidationsHandler),
            ],
            pool,
        }
    }

    /// 处理一个事件批次
    pub async fn process_batch(&self, batch: OnChainEventBatch) -> Result<()> {
        let checkpoint = batch.checkpoint_sequence;

        // 1. 检查是否已处理（幂等）
        if self.is_checkpoint_processed(checkpoint).await? {
            tracing::debug!("Checkpoint {} already processed, skipping", checkpoint);
            return Ok(());
        }

        // 2. 并行处理各 Handler
        let mut tasks = Vec::new();
        for handler in &self.handlers {
            let batch_clone = batch.clone();
            let pool = self.pool.clone();
            let handler_name = handler.name();

            tasks.push(async move {
                let processed = handler.process(&batch_clone).await?;
                handler.commit(&pool, processed).await?;
                Ok::<_, anyhow::Error>(handler_name)
            });
        }

        // 3. 等待所有 Handler 完成
        let results = futures::future::join_all(tasks).await;
        for result in results {
            result?;
        }

        // 4. 更新全局 Watermark
        self.update_watermark(checkpoint, batch.timestamp_ms).await?;

        Ok(())
    }

    async fn is_checkpoint_processed(&self, checkpoint: u64) -> Result<bool> {
        let row: Option<(i64,)> = sqlx::query_as(
            "SELECT checkpoint_hi FROM dex_watermarks WHERE pipeline = 'global' AND checkpoint_hi >= $1"
        )
            .bind(checkpoint as i64)
            .fetch_optional(&self.pool)
            .await?;

        Ok(row.is_some())
    }

    async fn update_watermark(&self, checkpoint: u64, timestamp_ms: u64) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO dex_watermarks (pipeline, epoch_hi, checkpoint_hi, timestamp_ms)
            VALUES ('global', 0, $1, $2)
            ON CONFLICT (pipeline)
            DO UPDATE SET
                checkpoint_hi = GREATEST(dex_watermarks.checkpoint_hi, EXCLUDED.checkpoint_hi),
                timestamp_ms = EXCLUDED.timestamp_ms,
                updated_at = NOW()
            "#,
        )
        .bind(checkpoint as i64)
        .bind(timestamp_ms as i64)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}
```

---

## 9. gRPC 客户端实现

### 9.1 DexEventStreamingClient Trait

```rust
use async_trait::async_trait;
use anyhow::Result;
use tokio::sync::mpsc;

/// gRPC 事件流客户端 Trait
#[async_trait]
pub trait DexEventStreamingClient: Send + Sync {
    /// 订阅 OnChainUpdates 事件流
    async fn subscribe_onchain_events(
        &self,
        from_checkpoint: u64,
    ) -> Result<mpsc::Receiver<OnChainEventBatch>>;

    /// 获取最新 Checkpoint
    async fn get_latest_checkpoint(&self) -> Result<u64>;

    /// 健康检查
    async fn health_check(&self) -> Result<bool>;
}
```

### 9.2 gRPC 客户端完整实现

```rust
use tonic::transport::{Channel, Endpoint};
use tonic::Request;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;

/// gRPC 客户端配置
#[derive(Debug, Clone)]
pub struct GrpcClientConfig {
    /// gRPC 服务端点
    pub endpoint: String,
    /// 连接超时
    pub connect_timeout: Duration,
    /// 请求超时
    pub request_timeout: Duration,
    /// 初始重连延迟
    pub initial_reconnect_delay: Duration,
    /// 最大重连延迟
    pub max_reconnect_delay: Duration,
    /// 重连延迟倍增因子
    pub reconnect_backoff: f64,
    /// 接收缓冲区大小
    pub buffer_size: usize,
}

impl Default for GrpcClientConfig {
    fn default() -> Self {
        Self {
            endpoint: "http://127.0.0.1:50051".to_string(),
            connect_timeout: Duration::from_secs(10),
            request_timeout: Duration::from_secs(30),
            initial_reconnect_delay: Duration::from_secs(1),
            max_reconnect_delay: Duration::from_secs(60),
            reconnect_backoff: 2.0,
            buffer_size: 1000,
        }
    }
}

/// gRPC 事件流客户端
pub struct GrpcEventStreamClient {
    config: GrpcClientConfig,
    channel: tokio::sync::RwLock<Option<Channel>>,
}

impl GrpcEventStreamClient {
    pub fn new(config: GrpcClientConfig) -> Self {
        Self {
            config,
            channel: tokio::sync::RwLock::new(None),
        }
    }

    /// 获取或创建连接
    async fn get_channel(&self) -> Result<Channel> {
        // 尝试复用现有连接
        {
            let guard = self.channel.read().await;
            if let Some(ref channel) = *guard {
                return Ok(channel.clone());
            }
        }

        // 创建新连接
        let endpoint = Endpoint::from_shared(self.config.endpoint.clone())?
            .connect_timeout(self.config.connect_timeout)
            .timeout(self.config.request_timeout)
            .tcp_keepalive(Some(Duration::from_secs(30)));

        let channel = endpoint.connect().await?;

        // 保存连接
        {
            let mut guard = self.channel.write().await;
            *guard = Some(channel.clone());
        }

        Ok(channel)
    }

    /// 重置连接（用于重连）
    async fn reset_channel(&self) {
        let mut guard = self.channel.write().await;
        *guard = None;
    }
}

#[async_trait]
impl DexEventStreamingClient for GrpcEventStreamClient {
    async fn subscribe_onchain_events(
        &self,
        from_checkpoint: u64,
    ) -> Result<mpsc::Receiver<OnChainEventBatch>> {
        let (tx, rx) = mpsc::channel(self.config.buffer_size);
        let config = self.config.clone();

        // 启动后台任务处理事件流
        let client = self.clone();
        tokio::spawn(async move {
            let mut current_checkpoint = from_checkpoint;
            let mut reconnect_delay = config.initial_reconnect_delay;

            loop {
                match client.stream_events_inner(current_checkpoint, tx.clone()).await {
                    Ok(last_checkpoint) => {
                        current_checkpoint = last_checkpoint + 1;
                        reconnect_delay = config.initial_reconnect_delay;
                    }
                    Err(e) => {
                        tracing::error!("Stream error: {}, reconnecting in {:?}", e, reconnect_delay);
                        client.reset_channel().await;
                        tokio::time::sleep(reconnect_delay).await;
                        reconnect_delay = Duration::from_secs_f64(
                            (reconnect_delay.as_secs_f64() * config.reconnect_backoff)
                                .min(config.max_reconnect_delay.as_secs_f64()),
                        );
                    }
                }
            }
        });

        Ok(rx)
    }

    async fn get_latest_checkpoint(&self) -> Result<u64> {
        let channel = self.get_channel().await?;
        let mut client = dex_event_service_client::DexEventServiceClient::new(channel);

        let response = client
            .get_latest_checkpoint(Request::new(GetLatestCheckpointRequest {}))
            .await?;

        Ok(response.into_inner().checkpoint_sequence)
    }

    async fn health_check(&self) -> Result<bool> {
        match self.get_channel().await {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }
}

impl GrpcEventStreamClient {
    async fn stream_events_inner(
        &self,
        from_checkpoint: u64,
        tx: mpsc::Sender<OnChainEventBatch>,
    ) -> Result<u64> {
        let channel = self.get_channel().await?;
        let mut client = dex_event_service_client::DexEventServiceClient::new(channel);

        let request = SubscribeOnChainRequest {
            from_checkpoint,
            event_types: vec![], // 订阅所有事件
        };

        let mut stream = client
            .subscribe_on_chain_events(Request::new(request))
            .await?
            .into_inner();

        let mut last_checkpoint = from_checkpoint;

        while let Some(result) = stream.next().await {
            match result {
                Ok(batch) => {
                    last_checkpoint = batch.checkpoint_sequence;
                    if tx.send(batch).await.is_err() {
                        // 接收方已关闭
                        break;
                    }
                }
                Err(e) => {
                    return Err(anyhow::anyhow!("Stream error: {}", e));
                }
            }
        }

        Ok(last_checkpoint)
    }
}

impl Clone for GrpcEventStreamClient {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            channel: tokio::sync::RwLock::new(None),
        }
    }
}
```

### 9.3 背压控制

```rust
/// 带背压控制的事件处理器
pub struct BackpressureController {
    /// 处理中的批次数量上限
    max_in_flight: usize,
    /// 当前处理中的批次数量
    in_flight: std::sync::atomic::AtomicUsize,
    /// 等待信号量
    semaphore: tokio::sync::Semaphore,
}

impl BackpressureController {
    pub fn new(max_in_flight: usize) -> Self {
        Self {
            max_in_flight,
            in_flight: std::sync::atomic::AtomicUsize::new(0),
            semaphore: tokio::sync::Semaphore::new(max_in_flight),
        }
    }

    /// 获取处理许可（阻塞直到有可用槽位）
    pub async fn acquire(&self) -> BackpressurePermit {
        let permit = self.semaphore.acquire().await.unwrap();
        self.in_flight
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        BackpressurePermit {
            controller: self,
            _permit: permit,
        }
    }

    /// 当前处理中的数量
    pub fn in_flight(&self) -> usize {
        self.in_flight.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// 是否已满
    pub fn is_full(&self) -> bool {
        self.in_flight() >= self.max_in_flight
    }
}

pub struct BackpressurePermit<'a> {
    controller: &'a BackpressureController,
    _permit: tokio::sync::SemaphorePermit<'a>,
}

impl<'a> Drop for BackpressurePermit<'a> {
    fn drop(&mut self) {
        self.controller
            .in_flight
            .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// 带背压控制的主循环
pub async fn run_with_backpressure(
    client: impl DexEventStreamingClient,
    coordinator: PipelineCoordinator,
    from_checkpoint: u64,
    max_in_flight: usize,
) -> Result<()> {
    let backpressure = BackpressureController::new(max_in_flight);
    let mut rx = client.subscribe_onchain_events(from_checkpoint).await?;

    while let Some(batch) = rx.recv().await {
        // 等待可用槽位
        let permit = backpressure.acquire().await;
        let checkpoint = batch.checkpoint_sequence;

        // 异步处理批次
        let coordinator = coordinator.clone();
        tokio::spawn(async move {
            if let Err(e) = coordinator.process_batch(batch).await {
                tracing::error!("Failed to process checkpoint {}: {}", checkpoint, e);
            }
            drop(permit); // 释放槽位
        });
    }

    Ok(())
}
```

---

## 10. 配置系统

### 10.1 配置文件结构（TOML）

```toml
# dex-indexer.toml

[indexer]
# 服务名称
name = "dex-indexer"
# 环境
environment = "mainnet"  # "mainnet" | "testnet" | "devnet"

[grpc]
# DEX 引擎 gRPC 端点
endpoint = "http://127.0.0.1:50051"
# 连接超时 (秒)
connect_timeout_secs = 10
# 请求超时 (秒)
request_timeout_secs = 30
# 初始重连延迟 (秒)
initial_reconnect_delay_secs = 1
# 最大重连延迟 (秒)
max_reconnect_delay_secs = 60
# 重连延迟倍增因子
reconnect_backoff = 2.0
# 接收缓冲区大小
buffer_size = 1000

[database]
# PostgreSQL 连接字符串
url = "postgres://user:password@localhost:5432/dex_indexer"
# 最大连接数
max_connections = 20
# 最小连接数
min_connections = 5
# 连接超时 (秒)
connect_timeout_secs = 30
# 空闲超时 (秒)
idle_timeout_secs = 600

[redis]
# Redis 连接字符串 (Phase 2)
url = "redis://localhost:6379"
# 连接池大小
pool_size = 10

[api]
# REST API 监听地址
rest_bind = "0.0.0.0:8080"
# WebSocket 监听地址 (Phase 2)
ws_bind = "0.0.0.0:8081"
# 请求并发限制
max_concurrent_requests = 1000
# 请求超时 (秒)
request_timeout_secs = 30

[pipeline]
# 并发处理批次数量
max_in_flight = 10
# 每个 Handler 的批量写入大小
batch_size = 1000
# 写入间隔 (毫秒)
flush_interval_ms = 100

[pipeline.fills]
enabled = true
concurrent = true

[pipeline.positions]
enabled = true
concurrent = false

[pipeline.candles]
enabled = true
concurrent = false
# K线缓存超时 (秒)
cache_timeout_secs = 300

[pipeline.funding]
enabled = true
concurrent = true

[pipeline.transfers]
enabled = true
concurrent = true

[pipeline.liquidations]
enabled = true
concurrent = true

[monitoring]
# Prometheus metrics 端点
metrics_bind = "0.0.0.0:9090"
# 日志级别
log_level = "info"  # "debug" | "info" | "warn" | "error"
# 日志格式
log_format = "json"  # "json" | "pretty"

[retention]
# fills 保留天数
fills_retention_days = 90
# candles 保留天数
candles_retention_days = 730
# funding 保留天数
funding_retention_days = 730
```

### 10.2 环境变量覆盖

所有配置项支持环境变量覆盖，格式：`DEX_INDEXER_{SECTION}_{KEY}`

```bash
# 示例
export DEX_INDEXER_DATABASE_URL="postgres://..."
export DEX_INDEXER_GRPC_ENDPOINT="http://..."
export DEX_INDEXER_MONITORING_LOG_LEVEL="debug"
```

### 10.3 配置加载代码

```rust
use config::{Config, ConfigError, Environment, File};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct IndexerConfig {
    pub indexer: IndexerMeta,
    pub grpc: GrpcConfig,
    pub database: DatabaseConfig,
    pub redis: Option<RedisConfig>,
    pub api: ApiConfig,
    pub pipeline: PipelineConfig,
    pub monitoring: MonitoringConfig,
    pub retention: RetentionConfig,
}

impl IndexerConfig {
    pub fn load() -> Result<Self, ConfigError> {
        let config_path = std::env::var("CONFIG_PATH")
            .unwrap_or_else(|_| "dex-indexer.toml".to_string());

        Config::builder()
            // 1. 加载默认配置
            .add_source(File::with_name("config/default").required(false))
            // 2. 加载环境特定配置
            .add_source(File::with_name(&config_path).required(false))
            // 3. 环境变量覆盖
            .add_source(
                Environment::with_prefix("DEX_INDEXER")
                    .separator("_")
                    .try_parsing(true),
            )
            .build()?
            .try_deserialize()
    }
}
```

---

## 11. 部署架构

### 11.1 单节点部署（Phase 1）

```
┌─────────────────────────────────────────────────────────────┐
│                     单节点部署架构                           │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌─────────────────────────────────────────────────────┐   │
│  │                   Host Machine                       │   │
│  │                                                      │   │
│  │  ┌──────────────┐  ┌──────────────┐                 │   │
│  │  │ DEX Indexer  │  │  PostgreSQL  │                 │   │
│  │  │   Service    │◄─┤    (主库)    │                 │   │
│  │  │              │  │              │                 │   │
│  │  │ - gRPC Client│  │ - 16GB RAM   │                 │   │
│  │  │ - Pipeline   │  │ - SSD 500GB  │                 │   │
│  │  │ - REST API   │  │              │                 │   │
│  │  └──────┬───────┘  └──────────────┘                 │   │
│  │         │                                            │   │
│  │         │ gRPC                                       │   │
│  │         ▼                                            │   │
│  │  ┌──────────────┐                                   │   │
│  │  │  DEX Engine  │  (同机器或远程)                   │   │
│  │  │  (gRPC Server)                                   │   │
│  │  └──────────────┘                                   │   │
│  │                                                      │   │
│  └─────────────────────────────────────────────────────┘   │
│                                                              │
│  最低配置: 4 CPU, 8GB RAM, 100GB SSD                       │
│  推荐配置: 8 CPU, 32GB RAM, 500GB SSD                      │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

### 11.2 高可用部署（Phase 2）

```
┌─────────────────────────────────────────────────────────────┐
│                     高可用部署架构                           │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│                    ┌──────────────────┐                     │
│                    │   Load Balancer  │                     │
│                    │   (Nginx/HAProxy)│                     │
│                    └────────┬─────────┘                     │
│                             │                               │
│           ┌─────────────────┼─────────────────┐            │
│           │                 │                 │            │
│           ▼                 ▼                 ▼            │
│  ┌─────────────┐   ┌─────────────┐   ┌─────────────┐      │
│  │ Indexer #1  │   │ Indexer #2  │   │ Indexer #3  │      │
│  │ (Primary)   │   │ (Standby)   │   │ (API Only)  │      │
│  │ - Pipeline  │   │ - Hot Standby│  │ - REST API  │      │
│  │ - REST API  │   │ - REST API  │   │ - WebSocket │      │
│  └──────┬──────┘   └──────┬──────┘   └──────┬──────┘      │
│         │                 │                 │              │
│         └─────────────────┼─────────────────┘              │
│                           │                                │
│                           ▼                                │
│  ┌─────────────────────────────────────────────────────┐  │
│  │                  PostgreSQL Cluster                  │  │
│  │  ┌─────────┐   ┌─────────┐   ┌─────────┐           │  │
│  │  │ Primary │◄──┤Replica 1│   │Replica 2│           │  │
│  │  │ (Write) │   │ (Read)  │   │ (Read)  │           │  │
│  │  └─────────┘   └─────────┘   └─────────┘           │  │
│  └─────────────────────────────────────────────────────┘  │
│                           │                                │
│                           ▼                                │
│  ┌─────────────────────────────────────────────────────┐  │
│  │                   Redis Cluster                      │  │
│  │  ┌─────────┐   ┌─────────┐   ┌─────────┐           │  │
│  │  │ Master  │   │ Replica │   │ Replica │           │  │
│  │  └─────────┘   └─────────┘   └─────────┘           │  │
│  └─────────────────────────────────────────────────────┘  │
│                                                            │
└─────────────────────────────────────────────────────────────┘
```

### 11.3 Docker Compose 示例

```yaml
# docker-compose.yml
version: '3.8'

services:
  dex-indexer:
    image: dex-indexer:latest
    container_name: dex-indexer
    restart: unless-stopped
    ports:
      - "8080:8080"   # REST API
      - "8081:8081"   # WebSocket (Phase 2)
      - "9090:9090"   # Metrics
    environment:
      - DEX_INDEXER_DATABASE_URL=postgres://dex:password@postgres:5432/dex_indexer
      - DEX_INDEXER_GRPC_ENDPOINT=http://dex-engine:50051
      - DEX_INDEXER_REDIS_URL=redis://redis:6379
      - RUST_LOG=info
    depends_on:
      postgres:
        condition: service_healthy
      redis:
        condition: service_healthy
    volumes:
      - ./config:/app/config:ro
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:8080/health"]
      interval: 10s
      timeout: 5s
      retries: 3

  postgres:
    image: postgres:15-alpine
    container_name: dex-postgres
    restart: unless-stopped
    ports:
      - "5432:5432"
    environment:
      - POSTGRES_USER=dex
      - POSTGRES_PASSWORD=password
      - POSTGRES_DB=dex_indexer
    volumes:
      - postgres_data:/var/lib/postgresql/data
      - ./init.sql:/docker-entrypoint-initdb.d/init.sql:ro
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U dex -d dex_indexer"]
      interval: 5s
      timeout: 5s
      retries: 5

  redis:
    image: redis:7-alpine
    container_name: dex-redis
    restart: unless-stopped
    ports:
      - "6379:6379"
    volumes:
      - redis_data:/data
    healthcheck:
      test: ["CMD", "redis-cli", "ping"]
      interval: 5s
      timeout: 3s
      retries: 5

  prometheus:
    image: prom/prometheus:latest
    container_name: dex-prometheus
    restart: unless-stopped
    ports:
      - "9091:9090"
    volumes:
      - ./prometheus.yml:/etc/prometheus/prometheus.yml:ro
      - prometheus_data:/prometheus

  grafana:
    image: grafana/grafana:latest
    container_name: dex-grafana
    restart: unless-stopped
    ports:
      - "3000:3000"
    environment:
      - GF_SECURITY_ADMIN_PASSWORD=admin
    volumes:
      - grafana_data:/var/lib/grafana
      - ./grafana/dashboards:/etc/grafana/provisioning/dashboards:ro

volumes:
  postgres_data:
  redis_data:
  prometheus_data:
  grafana_data:
```

---

## 12. 监控与运维

### 12.1 Prometheus 指标

```rust
use prometheus::{
    Counter, Gauge, Histogram, IntCounter, IntGauge, Registry,
    histogram_opts, opts,
};
use lazy_static::lazy_static;

lazy_static! {
    pub static ref REGISTRY: Registry = Registry::new();

    // ==================== 事件处理指标 ====================

    /// 处理的事件批次总数
    pub static ref BATCHES_PROCESSED: IntCounter = IntCounter::new(
        "dex_indexer_batches_processed_total",
        "Total number of event batches processed"
    ).unwrap();

    /// 处理的事件总数（按类型）
    pub static ref EVENTS_PROCESSED: IntCounter = IntCounter::with_opts(
        opts!("dex_indexer_events_processed_total", "Total events processed")
            .variable_label("event_type")
    ).unwrap();

    /// 处理延迟（从事件发生到写入完成）
    pub static ref PROCESSING_LATENCY: Histogram = Histogram::with_opts(
        histogram_opts!(
            "dex_indexer_processing_latency_seconds",
            "Event processing latency in seconds",
            vec![0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0]
        )
    ).unwrap();

    /// 当前处理的 Checkpoint
    pub static ref CURRENT_CHECKPOINT: IntGauge = IntGauge::new(
        "dex_indexer_current_checkpoint",
        "Current checkpoint being processed"
    ).unwrap();

    /// Checkpoint 落后数量
    pub static ref CHECKPOINT_LAG: IntGauge = IntGauge::new(
        "dex_indexer_checkpoint_lag",
        "Number of checkpoints behind latest"
    ).unwrap();

    // ==================== 数据库指标 ====================

    /// 数据库写入总数（按表）
    pub static ref DB_WRITES: IntCounter = IntCounter::with_opts(
        opts!("dex_indexer_db_writes_total", "Total database writes")
            .variable_label("table")
    ).unwrap();

    /// 数据库写入延迟
    pub static ref DB_WRITE_LATENCY: Histogram = Histogram::with_opts(
        histogram_opts!(
            "dex_indexer_db_write_latency_seconds",
            "Database write latency in seconds",
            vec![0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0]
        )
    ).unwrap();

    /// 数据库连接池状态
    pub static ref DB_POOL_SIZE: IntGauge = IntGauge::new(
        "dex_indexer_db_pool_size",
        "Current database connection pool size"
    ).unwrap();

    /// 数据库错误总数
    pub static ref DB_ERRORS: IntCounter = IntCounter::with_opts(
        opts!("dex_indexer_db_errors_total", "Total database errors")
            .variable_label("error_type")
    ).unwrap();

    // ==================== gRPC 指标 ====================

    /// gRPC 连接状态
    pub static ref GRPC_CONNECTED: IntGauge = IntGauge::new(
        "dex_indexer_grpc_connected",
        "gRPC connection status (1=connected, 0=disconnected)"
    ).unwrap();

    /// gRPC 重连次数
    pub static ref GRPC_RECONNECTS: IntCounter = IntCounter::new(
        "dex_indexer_grpc_reconnects_total",
        "Total gRPC reconnection attempts"
    ).unwrap();

    /// gRPC 接收消息数
    pub static ref GRPC_MESSAGES_RECEIVED: IntCounter = IntCounter::new(
        "dex_indexer_grpc_messages_received_total",
        "Total gRPC messages received"
    ).unwrap();

    // ==================== API 指标 ====================

    /// API 请求总数（按端点）
    pub static ref API_REQUESTS: IntCounter = IntCounter::with_opts(
        opts!("dex_indexer_api_requests_total", "Total API requests")
            .variable_label("endpoint")
            .variable_label("status")
    ).unwrap();

    /// API 请求延迟
    pub static ref API_LATENCY: Histogram = Histogram::with_opts(
        histogram_opts!(
            "dex_indexer_api_latency_seconds",
            "API request latency in seconds",
            vec![0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0]
        )
    ).unwrap();

    /// 当前活跃 WebSocket 连接数
    pub static ref WS_CONNECTIONS: IntGauge = IntGauge::new(
        "dex_indexer_ws_connections",
        "Current active WebSocket connections"
    ).unwrap();
}

pub fn register_metrics() {
    REGISTRY.register(Box::new(BATCHES_PROCESSED.clone())).unwrap();
    REGISTRY.register(Box::new(EVENTS_PROCESSED.clone())).unwrap();
    REGISTRY.register(Box::new(PROCESSING_LATENCY.clone())).unwrap();
    REGISTRY.register(Box::new(CURRENT_CHECKPOINT.clone())).unwrap();
    REGISTRY.register(Box::new(CHECKPOINT_LAG.clone())).unwrap();
    REGISTRY.register(Box::new(DB_WRITES.clone())).unwrap();
    REGISTRY.register(Box::new(DB_WRITE_LATENCY.clone())).unwrap();
    REGISTRY.register(Box::new(DB_POOL_SIZE.clone())).unwrap();
    REGISTRY.register(Box::new(DB_ERRORS.clone())).unwrap();
    REGISTRY.register(Box::new(GRPC_CONNECTED.clone())).unwrap();
    REGISTRY.register(Box::new(GRPC_RECONNECTS.clone())).unwrap();
    REGISTRY.register(Box::new(GRPC_MESSAGES_RECEIVED.clone())).unwrap();
    REGISTRY.register(Box::new(API_REQUESTS.clone())).unwrap();
    REGISTRY.register(Box::new(API_LATENCY.clone())).unwrap();
    REGISTRY.register(Box::new(WS_CONNECTIONS.clone())).unwrap();
}
```

### 12.2 Grafana Dashboard 建议

| Panel 名称 | 指标 | 类型 | 告警阈值 |
|-----------|------|------|---------|
| 处理延迟 | `dex_indexer_processing_latency_seconds` | Histogram | P99 > 5s |
| Checkpoint 落后 | `dex_indexer_checkpoint_lag` | Gauge | > 100 |
| 事件吞吐量 | `rate(dex_indexer_events_processed_total[1m])` | Counter Rate | < 10/s |
| 数据库写入延迟 | `dex_indexer_db_write_latency_seconds` | Histogram | P99 > 1s |
| gRPC 连接状态 | `dex_indexer_grpc_connected` | Gauge | = 0 |
| API 错误率 | `rate(dex_indexer_api_requests_total{status="error"}[5m])` | Counter Rate | > 1% |
| WebSocket 连接数 | `dex_indexer_ws_connections` | Gauge | - |

### 12.3 告警规则

```yaml
# prometheus-alerts.yml
groups:
  - name: dex-indexer
    rules:
      - alert: HighProcessingLatency
        expr: histogram_quantile(0.99, rate(dex_indexer_processing_latency_seconds_bucket[5m])) > 5
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "High event processing latency"
          description: "P99 latency is {{ $value }}s"

      - alert: CheckpointLagHigh
        expr: dex_indexer_checkpoint_lag > 100
        for: 5m
        labels:
          severity: critical
        annotations:
          summary: "Checkpoint lag is too high"
          description: "Lag is {{ $value }} checkpoints"

      - alert: GrpcDisconnected
        expr: dex_indexer_grpc_connected == 0
        for: 1m
        labels:
          severity: critical
        annotations:
          summary: "gRPC connection lost"

      - alert: DatabaseErrors
        expr: rate(dex_indexer_db_errors_total[5m]) > 0.1
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "Database errors detected"
          description: "Error rate is {{ $value }}/s"

      - alert: ApiErrorRateHigh
        expr: |
          rate(dex_indexer_api_requests_total{status="error"}[5m])
          / rate(dex_indexer_api_requests_total[5m]) > 0.01
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "API error rate > 1%"
```

### 12.4 日志规范

```rust
use tracing::{info, warn, error, debug, instrument};

// 结构化日志示例
#[instrument(skip(batch), fields(checkpoint = batch.checkpoint_sequence))]
pub async fn process_batch(batch: OnChainEventBatch) -> Result<()> {
    let start = std::time::Instant::now();

    info!(
        checkpoint = batch.checkpoint_sequence,
        fills_count = batch.fills.len(),
        positions_count = batch.position_updates.len(),
        "Processing event batch"
    );

    // 处理逻辑...

    let elapsed = start.elapsed();
    info!(
        checkpoint = batch.checkpoint_sequence,
        elapsed_ms = elapsed.as_millis(),
        "Batch processed successfully"
    );

    Ok(())
}

// 错误日志示例
fn log_error(checkpoint: u64, err: &anyhow::Error) {
    error!(
        checkpoint = checkpoint,
        error = %err,
        error_chain = ?err.chain().collect::<Vec<_>>(),
        "Failed to process batch"
    );
}
```

---

## 13. 错误处理与恢复

### 13.1 Watermark 断点续传

```rust
/// Watermark 管理器
pub struct WatermarkManager {
    pool: PgPool,
}

impl WatermarkManager {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// 获取 Pipeline 的当前 Watermark
    pub async fn get_watermark(&self, pipeline: &str) -> Result<Option<Watermark>> {
        let row: Option<Watermark> = sqlx::query_as(
            r#"
            SELECT pipeline, epoch_hi, checkpoint_hi, tx_hi, timestamp_ms, reader_lo, pruner_hi
            FROM dex_watermarks
            WHERE pipeline = $1
            "#,
        )
        .bind(pipeline)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row)
    }

    /// 更新 Watermark（带条件检查，防止回退）
    pub async fn update_watermark(&self, watermark: &Watermark) -> Result<bool> {
        let result = sqlx::query(
            r#"
            INSERT INTO dex_watermarks (pipeline, epoch_hi, checkpoint_hi, tx_hi, timestamp_ms)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (pipeline)
            DO UPDATE SET
                epoch_hi = EXCLUDED.epoch_hi,
                checkpoint_hi = EXCLUDED.checkpoint_hi,
                tx_hi = EXCLUDED.tx_hi,
                timestamp_ms = EXCLUDED.timestamp_ms,
                updated_at = NOW()
            WHERE dex_watermarks.checkpoint_hi < EXCLUDED.checkpoint_hi
            "#,
        )
        .bind(&watermark.pipeline)
        .bind(watermark.epoch_hi as i64)
        .bind(watermark.checkpoint_hi as i64)
        .bind(watermark.tx_hi as i64)
        .bind(watermark.timestamp_ms as i64)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    /// 获取所有 Pipeline 中最小的 Checkpoint（用于清理）
    pub async fn get_min_checkpoint(&self) -> Result<u64> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COALESCE(MIN(checkpoint_hi), 0) FROM dex_watermarks"
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(row.0 as u64)
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Watermark {
    pub pipeline: String,
    pub epoch_hi: i64,
    pub checkpoint_hi: i64,
    pub tx_hi: i64,
    pub timestamp_ms: i64,
    pub reader_lo: i64,
    pub pruner_hi: i64,
}
```

### 13.2 幂等写入保证

```rust
/// 幂等写入策略
pub enum IdempotencyStrategy {
    /// ON CONFLICT DO NOTHING - 跳过重复
    SkipDuplicate,
    /// ON CONFLICT DO UPDATE - 更新为新值（需要比较 checkpoint）
    UpdateIfNewer,
    /// 使用 checkpoint_sequence 作为版本号
    VersionCheck,
}

/// 通用幂等写入包装
pub async fn idempotent_insert<T: InsertableRow>(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    row: &T,
    strategy: IdempotencyStrategy,
) -> Result<bool> {
    let sql = match strategy {
        IdempotencyStrategy::SkipDuplicate => {
            format!(
                "INSERT INTO {} ({}) VALUES ({}) ON CONFLICT ({}) DO NOTHING",
                row.table_name(),
                row.column_names().join(", "),
                row.placeholders().join(", "),
                row.unique_key().join(", ")
            )
        }
        IdempotencyStrategy::UpdateIfNewer => {
            format!(
                r#"
                INSERT INTO {} ({}) VALUES ({})
                ON CONFLICT ({})
                DO UPDATE SET {}
                WHERE {}.checkpoint_sequence < EXCLUDED.checkpoint_sequence
                "#,
                row.table_name(),
                row.column_names().join(", "),
                row.placeholders().join(", "),
                row.unique_key().join(", "),
                row.update_clause(),
                row.table_name()
            )
        }
        IdempotencyStrategy::VersionCheck => {
            // 先检查版本，再决定是否插入
            format!(
                r#"
                INSERT INTO {} ({})
                SELECT {} WHERE NOT EXISTS (
                    SELECT 1 FROM {} WHERE {} AND checkpoint_sequence >= $N
                )
                "#,
                row.table_name(),
                row.column_names().join(", "),
                row.placeholders().join(", "),
                row.table_name(),
                row.unique_key_conditions()
            )
        }
    };

    let result = sqlx::query(&sql)
        .execute(&mut **tx)
        .await?;

    Ok(result.rows_affected() > 0)
}
```

### 13.3 异常场景处理

| 场景 | 检测方式 | 处理策略 |
|------|---------|---------|
| gRPC 连接断开 | 心跳超时 | 指数退避重连 |
| 数据库连接失败 | 连接池错误 | 重试 3 次后告警 |
| 重复事件 | ON CONFLICT | 跳过或更新 |
| 乱序事件 | checkpoint_sequence 比较 | 仅处理更新的事件 |
| 写入超时 | 事务超时 | 回滚重试 |
| 内存不足 | OOM | 背压控制 + 限流 |
| 磁盘满 | 写入失败 | 告警 + 暂停处理 |

### 13.4 数据一致性验证

```rust
/// 定期一致性检查
pub async fn verify_consistency(pool: &PgPool) -> Result<ConsistencyReport> {
    let mut report = ConsistencyReport::default();

    // 1. 检查 Watermark 连续性
    let watermarks = get_all_watermarks(pool).await?;
    let min_checkpoint = watermarks.iter().map(|w| w.checkpoint_hi).min().unwrap_or(0);
    let max_checkpoint = watermarks.iter().map(|w| w.checkpoint_hi).max().unwrap_or(0);

    if max_checkpoint - min_checkpoint > 100 {
        report.warnings.push(format!(
            "Watermark gap detected: min={}, max={}",
            min_checkpoint, max_checkpoint
        ));
    }

    // 2. 检查 fills 表数据完整性
    let gaps = sqlx::query_as::<_, (i64, i64)>(
        r#"
        SELECT checkpoint_sequence, LEAD(checkpoint_sequence) OVER (ORDER BY checkpoint_sequence) as next_cp
        FROM (SELECT DISTINCT checkpoint_sequence FROM fills ORDER BY checkpoint_sequence LIMIT 10000) t
        WHERE LEAD(checkpoint_sequence) OVER (ORDER BY checkpoint_sequence) - checkpoint_sequence > 1
        "#
    )
    .fetch_all(pool)
    .await?;

    if !gaps.is_empty() {
        report.errors.push(format!("Found {} checkpoint gaps in fills", gaps.len()));
    }

    // 3. 检查持仓与成交的一致性（抽样）
    let sample_users = get_sample_users(pool, 100).await?;
    for user in sample_users {
        let position_size = get_user_position_size(pool, &user).await?;
        let computed_size = compute_position_from_fills(pool, &user).await?;

        if (position_size - computed_size).abs() > Decimal::new(1, 8) {
            report.errors.push(format!(
                "Position mismatch for {}: stored={}, computed={}",
                user, position_size, computed_size
            ));
        }
    }

    Ok(report)
}

#[derive(Default)]
pub struct ConsistencyReport {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub checked_at: chrono::DateTime<chrono::Utc>,
}
```

---

## 14. 测试策略

### 14.1 单元测试覆盖

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_candle_aggregator() {
        let mut agg = CandleAggregator::new(1, "1m");

        // 第一笔交易
        let candle = agg.add_trade(
            Decimal::new(42000, 0),
            Decimal::new(1, 0),
            1706500000000,
            1,
        );
        assert!(candle.is_none()); // 未完成

        // 同一分钟内的第二笔
        let candle = agg.add_trade(
            Decimal::new(42100, 0),
            Decimal::new(2, 0),
            1706500030000,
            1,
        );
        assert!(candle.is_none());

        // 下一分钟的交易，触发上一根 K 线完成
        let candle = agg.add_trade(
            Decimal::new(42200, 0),
            Decimal::new(1, 0),
            1706500060000,
            2,
        );
        assert!(candle.is_some());

        let completed = candle.unwrap();
        assert_eq!(completed.open, Decimal::new(42000, 0));
        assert_eq!(completed.high, Decimal::new(42100, 0));
        assert_eq!(completed.close, Decimal::new(42100, 0));
        assert_eq!(completed.volume, Decimal::new(3, 0));
    }

    #[test]
    fn test_fill_event_serialization() {
        let fill = FillEvent {
            fill_id: 1,
            market_id: 0,
            maker_order_id: 100,
            taker_order_id: 101,
            maker_address: "0x123".to_string(),
            taker_address: "0x456".to_string(),
            side: Side::Buy,
            price: Decimal::new(42000, 0),
            quantity: Decimal::new(1, 1),
            maker_fee: Decimal::new(21, 2),
            taker_fee: Decimal::new(42, 2),
            timestamp_ms: 1706500000000,
            checkpoint_sequence: 1,
            hash: "0xabc".to_string(),
        };

        let json = serde_json::to_string(&fill).unwrap();
        let parsed: FillEvent = serde_json::from_str(&json).unwrap();

        assert_eq!(fill.fill_id, parsed.fill_id);
        assert_eq!(fill.price, parsed.price);
    }

    #[test]
    fn test_info_request_parsing() {
        let json = r#"{"type": "l2Book", "coin": "BTC"}"#;
        let request: InfoRequest = serde_json::from_str(json).unwrap();

        match request {
            InfoRequest::L2Book { coin } => assert_eq!(coin, "BTC"),
            _ => panic!("Expected L2Book request"),
        }
    }
}
```

### 14.2 集成测试方案

```rust
#[cfg(test)]
mod integration_tests {
    use sqlx::PgPool;
    use testcontainers::{clients::Cli, images::postgres::Postgres};

    async fn setup_test_db() -> PgPool {
        let docker = Cli::default();
        let postgres = docker.run(Postgres::default());
        let port = postgres.get_host_port(5432);

        let url = format!("postgres://postgres:postgres@localhost:{}/postgres", port);
        let pool = PgPool::connect(&url).await.unwrap();

        // 执行 migration
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        pool
    }

    #[tokio::test]
    async fn test_fills_handler_commit() {
        let pool = setup_test_db().await;
        let handler = FillsHandler::new();

        let batch = OnChainEventBatch {
            checkpoint_sequence: 1,
            epoch: 0,
            timestamp_ms: 1706500000000,
            fills: vec![/* test data */],
            ..Default::default()
        };

        let processed = handler.process(&batch).await.unwrap();
        handler.commit(&pool, processed).await.unwrap();

        // 验证数据已写入
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM fills")
            .fetch_one(&pool)
            .await
            .unwrap();

        assert_eq!(count.0, batch.fills.len() as i64);
    }

    #[tokio::test]
    async fn test_idempotent_write() {
        let pool = setup_test_db().await;
        let handler = FillsHandler::new();

        let batch = create_test_batch(1);

        // 第一次写入
        let processed = handler.process(&batch).await.unwrap();
        handler.commit(&pool, processed).await.unwrap();

        // 重复写入（应该被跳过）
        let processed = handler.process(&batch).await.unwrap();
        handler.commit(&pool, processed).await.unwrap();

        // 验证只有一条记录
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM fills")
            .fetch_one(&pool)
            .await
            .unwrap();

        assert_eq!(count.0, batch.fills.len() as i64);
    }
}
```

### 14.3 压力测试指标

| 测试场景 | 目标指标 | 测试方法 |
|---------|---------|---------|
| 峰值吞吐 | 10,000 events/s | wrk + mock gRPC server |
| 持续写入 | 1,000 events/s (24h) | 长时间稳定性测试 |
| API 延迟 | P99 < 100ms | ab / hey 压测 |
| WebSocket 广播 | 10,000 连接 | k6 WebSocket 测试 |
| 重连恢复 | < 5s | 杀进程后观察恢复时间 |
| 数据库故障恢复 | < 30s | 模拟 PostgreSQL 故障 |

### 14.4 回归测试

```bash
#!/bin/bash
# regression-test.sh

set -e

echo "=== DEX Indexer Regression Test ==="

# 1. 单元测试
echo "Running unit tests..."
cargo test --lib

# 2. 集成测试
echo "Running integration tests..."
cargo test --test '*' -- --test-threads=1

# 3. API 合规测试（对标 Hyperliquid）
echo "Running API compliance tests..."
./scripts/test-api-compliance.sh

# 4. 性能基准测试
echo "Running benchmark..."
cargo bench

# 5. 数据一致性检查
echo "Checking data consistency..."
cargo run --bin verify-consistency

echo "=== All tests passed ==="
```

---

## 附录

### A. 与 V2 版本差异对照

| 维度 | V2 | V3 |
|------|----|----|
| 分阶段规划 | 隐含 | 明确 Phase 1/2 分离 |
| gRPC Proto | 基础定义 | 完整可编译 Proto |
| REST API | 已对标 Hyperliquid | 完善请求/响应示例 |
| WebSocket | 未详细 | 完整设计 (Phase 2) |
| 数据模型 | 基础 | 完整对标 Hyperliquid |
| DDL | 基础表定义 | 完整分区 + 索引 |
| Pipeline | 概念设计 | 完整 Handler 实现 |
| 测试策略 | 无 | 完整覆盖 |
| 错误处理 | 简略 | 详细机制 |
| 监控指标 | 基础 | Prometheus + Grafana |

### B. Hyperliquid API 对照表

| Hyperliquid type | 本方案 type | 状态 |
|------------------|-------------|------|
| `meta` | `meta` | ✓ Phase 1 |
| `metaAndAssetCtxs` | `metaAndAssetCtxs` | ✓ Phase 1 |
| `spotMeta` | `spotMeta` | ✓ Phase 1 |
| `spotMetaAndAssetCtxs` | `spotMetaAndAssetCtxs` | ✓ Phase 1 |
| `allMids` | `allMids` | ✓ Phase 1 |
| `l2Book` | `l2Book` | ✓ Phase 2 (Redis) |
| `candleSnapshot` | `candleSnapshot` | ✓ Phase 1 |
| `recentTrades` | `recentTrades` | ✓ Phase 1 |
| `clearinghouseState` | `clearinghouseState` | ✓ Phase 1 |
| `spotClearinghouseState` | `spotClearinghouseState` | ✓ Phase 1 |
| `openOrders` | `openOrders` | ✓ Phase 1 |
| `frontendOpenOrders` | `frontendOpenOrders` | ✓ Phase 1 |
| `orderStatus` | `orderStatus` | ✓ Phase 1 |
| `historicalOrders` | `historicalOrders` | ✓ Phase 1 |
| `userFills` | `userFills` | ✓ Phase 1 |
| `userFillsByTime` | `userFillsByTime` | ✓ Phase 1 |
| `userFunding` | `userFunding` | ✓ Phase 1 |
| `fundingHistory` | `fundingHistory` | ✓ Phase 1 |
| `predictedFundings` | `predictedFundings` | ✓ Phase 1 |
| `maxBuilderFee` | `maxBuilderFee` | ✓ Phase 1 |
| `userFees` | `userFees` | ✓ Phase 1 |

### C. 参考文档

| 文档 | 路径 | 说明 |
|------|------|------|
| dYdX Indexer 分析 | `sui/mynotes/dex/analyst/dydx-indexer-analyst.md` | dYdX 双通道机制分析 |
| Sui DEX 方案分析 | `sui/mynotes/dex/analyst/dex-indexer-full-by-dydx-analysis.md` | Checkpoint-Only 决策依据 |
| sui-indexer-alt 分析 | `sui/mynotes/dex/analyst/sui-indexer-alt-analyst.md` | Pipeline 模式参考 |
| Hyperliquid API | `dex-ui/notes/hyperliquid/http/` | API 格式参考 |
| V2 技术方案 | `sui/mynotes/dex/tech/dex-indexer-tech-v2.md` | 前一版本方案 |

---

> **版本历史**
>
> | 版本 | 日期 | 变更 |
> |------|------|------|
> | V3.0 | 2026-01-29 | 初始版本，明确 Phase 分离，完善所有章节 |

