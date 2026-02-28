# Phase 6-02: 增量事件类型设计

> 创建日期: 2026-02-25
> 更新日期: 2026-02-27
> 状态: ⚠️ 需更新 — Delta 不纳入 TransactionEvents，Snapshot 停止发射
> 关联文档: [00-overview](./00-overview.md) | [01-streaming-source](./01-streaming-source.md)

> **2026-02-27 架构决策变更通知**
>
> 根据 [08-architecture-qa.md](./08-architecture-qa.md) 确认的决策：
> - **Q5=A2**: OrderbookDeltaEvent **不纳入 TransactionEvents**，仅通过 gRPC 推送
> - **Q3=C+**: OrderbookSnapshotEvent **停止发射**（Checkpoint 不再处理订单簿）
>
> **本文档需更新的部分**：
> - §2 OrderbookDeltaEvent → 类型定义保留，但不调用 `emit_event()`
> - §3 事件发射策略 → 移除"双重发射"（delta + snapshot 同时在 TransactionEvents 中）
> - §4 Snapshot 降频 → 改为完全停止发射
> - Delta 数据流转路径：引擎计算 → DexStreamingManager → gRPC → dex-streamer
>
> 当前文档的 OrderbookDeltaEvent 类型定义（§2.1）保持有效。

## 1. 现状分析

### 1.1 当前事件模型

当前每次订单操作（下单、撤单、批量撤单）均发射 `OrderbookSnapshotEvent` 全量快照：

```rust
// dex-sui/crates/sui-types/src/dex_events.rs

pub struct OrderbookSnapshotEvent {
    pub perpetual_id: u32,
    pub bids: Vec<PriceLevel>,   // 全部买单价格档位（按价格降序）
    pub asks: Vec<PriceLevel>,   // 全部卖单价格档位（按价格升序）
    pub timestamp_ms: u64,
}

pub struct PriceLevel {
    pub price: u64,              // 价格（subticks）
    pub quantity: u64,           // 该价位总数量
}
```

快照由 `InlineOrderbook::build_snapshot()` 构建（`dex-sui/crates/sui-types/src/dex/perpetual.rs:422`），遍历 `BTreeMap<u64, PriceLevel>` 中所有价格档位，过滤 `total_quantums > 0` 的档位后序列化为 `Vec<EventPriceLevel>`。

事件发射点（`dex-sui/sui-execution/src/dex/commands/order.rs`）：

| 函数 | 发射条件 | 说明 |
|------|----------|------|
| `execute_place_order` | 有成交或有 resting order | 跳过空订单簿 |
| `execute_place_order_with_eip712` | 有成交或有 resting order | 无条件发射 |
| `execute_cancel_order` | 撤单成功 | 始终发射（含空簿） |
| `execute_cancel_all_orders` | 至少一笔撤单成功 | 一次发射汇总 |
| `execute_cancel_order_with_eip712` | 撤单成功 | 始终发射 |

### 1.2 问题

| 问题 | 量化 |
|------|------|
| 每次事件大小 | 100 档深度 ~3.2KB（200 个 `PriceLevel` x 16B + BCS 开销） |
| 每秒带宽 (10 markets, 100 tx/s) | ~3.2 MB/s |
| 冗余数据 | 每次操作通常只改变 1-3 个价格档位，其余 197+ 个档位重复传输 |
| Indexer 处理 | Checkpoint 内同市场多个快照仅保留最后一个，前面的全量序列化/反序列化浪费 |

---

## 2. OrderbookDeltaEvent 设计

### 2.1 结构定义

```rust
// 新增至 dex-sui/crates/sui-types/src/dex_events.rs

/// 增量 L2 订单簿更新事件
///
/// 仅描述本次操作导致的价格档位变更，取代每次发射全量快照。
/// quantity=0 表示该价格档位已被移除。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OrderbookDeltaEvent {
    /// 永续合约市场 ID
    pub perpetual_id: u32,
    /// 序列号（每个市场独立递增，用于排序与缺口检测）
    pub sequence: u64,
    /// 变更的价格档位列表
    pub updates: Vec<OrderbookDelta>,
    /// 时间戳（毫秒）
    pub timestamp_ms: u64,
}

/// 单个价格档位的变更描述
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OrderbookDelta {
    /// 方向: 0 = Bid (买), 1 = Ask (卖)
    pub side: u8,
    /// 价格（subticks）
    pub price: u64,
    /// 该价格档位的新总数量（0 = 该档位已移除）
    pub quantity: u64,
}
```

### 2.2 BCS 序列化大小估算

```
OrderbookDeltaEvent:
  perpetual_id: 4 bytes (u32)
  sequence:     8 bytes (u64)
  updates:      ULEB128 长度 + N * OrderbookDelta
  timestamp_ms: 8 bytes (u64)

OrderbookDelta:
  side:     1 byte  (u8)
  price:    8 bytes (u64)
  quantity: 8 bytes (u64)
  ────────────────────
  合计:     17 bytes

典型场景 (2 个价格档位变更):
  4 + 8 + 1 + 2*17 + 8 = 55 bytes

最坏场景 (单次操作影响 10 个价格档位):
  4 + 8 + 1 + 10*17 + 8 = 191 bytes
```

### 2.3 与全量快照对比

| 指标 | OrderbookSnapshotEvent (100 档) | OrderbookDeltaEvent (avg 2 档) |
|------|------|------|
| 单次大小 | ~3,200 bytes | ~55 bytes |
| 10 markets, 100 tx/s | ~3.2 MB/s | ~55 KB/s |
| 缩减倍数 | - | **~58x** |
| Indexer 反序列化开销 | 解析 200 个 PriceLevel | 解析 2 个 OrderbookDelta |

### 2.4 struct_tag 与 Event 转换

遵循现有事件模式（`DEX_EVENTS_PACKAGE` + `DEX_EVENTS_MODULE` + BCS）：

```rust
impl OrderbookDeltaEvent {
    pub fn struct_tag() -> StructTag {
        StructTag {
            address: DEX_EVENTS_PACKAGE,
            module: Identifier::new(DEX_EVENTS_MODULE).expect("valid module name"),
            name: Identifier::new("OrderbookDeltaEvent").expect("valid struct name"),
            type_params: vec![],
        }
    }

    pub fn to_sui_event(&self, sender: SuiAddress) -> Event {
        Event::new(
            &DEX_EVENTS_PACKAGE,
            ident_str!(DEX_EVENTS_MODULE),
            sender,
            Self::struct_tag(),
            bcs::to_bytes(self).expect("OrderbookDeltaEvent BCS serialization should not fail"),
        )
    }
}

impl OrderbookDelta {
    pub const SIDE_BID: u8 = 0;
    pub const SIDE_ASK: u8 = 1;
}
```

---

## 3. Delta 计算逻辑

### 3.1 核心思路

在订单操作前后对比受影响价格档位的数量变化，生成 delta 列表：

```
操作前: price=50000, quantity=300
操作后: price=50000, quantity=200
────────────────────────────────────
Delta:  side=Bid, price=50000, quantity=200  (数量减少)

操作前: price=50100, quantity=0 (不存在)
操作后: price=50100, quantity=150
────────────────────────────────────
Delta:  side=Ask, price=50100, quantity=150  (新增档位)

操作前: price=49900, quantity=100
操作后: price=49900, quantity=0 (被清空)
────────────────────────────────────
Delta:  side=Bid, price=49900, quantity=0    (档位移除)
```

### 3.2 实现方案: Snapshot-before-after Diff

在 `InlineOrderbook` 上新增方法，记录操作前后的受影响价位状态：

```rust
// dex-sui/crates/sui-types/src/dex/perpetual.rs

impl InlineOrderbook {
    /// 获取指定价格档位的当前数量（用于 delta 计算）
    pub fn get_level_quantity(&self, side: Side, price: u64) -> u64 {
        match side {
            Side::Buy => self.bids.get(&price)
                .map(|l| l.total_quantums)
                .unwrap_or(0),
            Side::Sell => self.asks.get(&price)
                .map(|l| l.total_quantums)
                .unwrap_or(0),
        }
    }

    /// 构建增量 delta 事件
    ///
    /// `affected_levels`: 本次操作影响的 (side, price) 列表
    /// `before_quantities`: 操作前各价位的数量（调用方在操作前记录）
    pub fn build_delta(
        &self,
        perpetual_id: u32,
        sequence: u64,
        affected_levels: &[(Side, u64)],
        before_quantities: &[(Side, u64, u64)], // (side, price, qty_before)
        timestamp_ms: u64,
    ) -> OrderbookDeltaEvent {
        let mut updates = Vec::new();

        for &(side, price, qty_before) in before_quantities {
            let qty_after = self.get_level_quantity(side, price);
            if qty_after != qty_before {
                updates.push(OrderbookDelta {
                    side: match side {
                        Side::Buy => OrderbookDelta::SIDE_BID,
                        Side::Sell => OrderbookDelta::SIDE_ASK,
                    },
                    price,
                    quantity: qty_after,
                });
            }
        }

        OrderbookDeltaEvent {
            perpetual_id,
            sequence,
            updates,
            timestamp_ms,
        }
    }
}
```

### 3.3 各操作场景的 Delta 收集

#### 3.3.1 PlaceOrder (下单)

下单操作影响的价格档位包括：

1. **Taker 吃单方**: 被吃掉的 maker 价格档位（对手方）
2. **Resting 挂单**: 如果订单有剩余未成交部分，新增/更新自身方向的一个价格档位

```rust
// order.rs execute_place_order 中的改造伪码

// ── 操作前 ──
// 记录可能受影响的对手方价位（matching 范围内的所有价位）
let before_levels: Vec<(Side, u64, u64)> = collect_matching_levels_before(
    &state.orderbook, taker_side, taker_price
);
// 记录 taker 自身价位（如果会 resting）
let taker_level_before = state.orderbook.get_level_quantity(
    taker_side, order_price
);

// ── 执行撮合 ──
let match_result = state.orderbook.match_order(...);

// ── 操作后：计算 delta ──
let mut all_before = before_levels;
if match_result.remaining > 0 && is_resting {
    all_before.push((taker_side, order_price, taker_level_before));
}
let delta = state.orderbook.build_delta(
    perpetual_id, next_sequence, &[], &all_before, timestamp_ms
);
if !delta.updates.is_empty() {
    events.push(delta.to_sui_event(ctx.transaction_signer));
}
```

#### 3.3.2 CancelOrder (撤单)

撤单只影响一个价格档位：

```rust
// ── 操作前 ──
let order = state.orderbook.get_order(&order_id).unwrap();
let (side, price) = (order.side, order.price);
let qty_before = state.orderbook.get_level_quantity(side, price);

// ── 执行撤单 ──
state.orderbook.cancel_order(&order_id);

// ── 操作后 ──
let qty_after = state.orderbook.get_level_quantity(side, price);
let delta = OrderbookDeltaEvent {
    perpetual_id,
    sequence: next_sequence,
    updates: vec![OrderbookDelta {
        side: side.to_u8(),
        price,
        quantity: qty_after,
    }],
    timestamp_ms,
};
events.push(delta.to_sui_event(ctx.transaction_signer));
```

#### 3.3.3 CancelAllOrders (批量撤单)

批量撤单影响多个价格档位，但可合并为一个 delta 事件：

```rust
// ── 操作前 ──
// 记录该 subaccount 所有订单涉及的价位
let orders = state.orderbook.get_orders_for_subaccount(&subaccount_id);
let mut before_map: HashMap<(Side, u64), u64> = HashMap::new();
for order_id in &orders {
    if let Some(order) = state.orderbook.get_order(order_id) {
        before_map.entry((order.side, order.price))
            .or_insert_with(|| state.orderbook.get_level_quantity(order.side, order.price));
    }
}

// ── 执行批量撤单 ──
for order_id in &orders {
    state.orderbook.cancel_order(order_id);
}

// ── 操作后：一次性 diff ──
let mut updates = Vec::new();
for ((side, price), qty_before) in &before_map {
    let qty_after = state.orderbook.get_level_quantity(*side, *price);
    if qty_after != *qty_before {
        updates.push(OrderbookDelta {
            side: side.to_u8(),
            price: *price,
            quantity: qty_after,
        });
    }
}
// 单个 delta 事件覆盖所有变更
```

### 3.4 辅助函数: 收集 matching 范围内的价位

```rust
impl InlineOrderbook {
    /// 收集 taker 价格范围内所有可能被吃的对手方价位
    ///
    /// 用于在 matching 前记录 baseline 状态
    fn collect_affected_levels(
        &self,
        taker_side: Side,
        taker_price: u64,
    ) -> Vec<(Side, u64, u64)> {
        match taker_side {
            Side::Buy => {
                // 买单吃 asks: 从最低价到 taker_price 以下的所有 ask 档位
                self.asks.range(..=taker_price)
                    .map(|(&price, level)| (Side::Sell, price, level.total_quantums))
                    .collect()
            }
            Side::Sell => {
                // 卖单吃 bids: 从 taker_price 以上到最高价的所有 bid 档位
                self.bids.range(taker_price..)
                    .map(|(&price, level)| (Side::Buy, price, level.total_quantums))
                    .collect()
            }
        }
    }
}
```

---

## 4. 序列号 (Sequence) 机制

### 4.1 设计目标

序列号用于：
1. **排序**: 消费端按 sequence 排列 delta 事件，保证应用顺序正确
2. **缺口检测**: 发现 sequence 不连续时，知道有事件丢失
3. **恢复**: 检测到缺口后，从最近的全量快照重建

### 4.2 存储位置

在 `PerpetualState` 中新增字段：

```rust
// dex-sui/crates/sui-types/src/dex/perpetual.rs

pub struct PerpetualState {
    pub id: UID,
    pub perpetual_id: u32,
    pub params: PerpetualParams,
    pub open_interest: OpenInterestData,
    pub orderbook: InlineOrderbook,
    pub funding_index: i128,
    pub last_funding_update: u64,
    pub version: SequenceNumber,
    pub initial_shared_version: SequenceNumber,
    /// Delta 事件序列号（每个市场独立递增）
    pub delta_sequence: u64,                      // 新增字段
}
```

**注意**: `PerpetualState` 是链上持久化对象。新增字段会改变 BCS 序列化格式，需要：
- 在字段末尾添加（BCS 兼容追加字段）
- 初始值为 0
- 现有数据反序列化后使用 `#[serde(default)]` 处理

### 4.3 序列号递增逻辑

```rust
impl PerpetualState {
    /// 获取并递增 delta 序列号
    pub fn next_delta_sequence(&mut self) -> u64 {
        let seq = self.delta_sequence;
        self.delta_sequence = seq + 1;
        seq
    }
}
```

在每个事件发射点调用：

```rust
// order.rs
let sequence = state.next_delta_sequence();
let delta = state.orderbook.build_delta(perpetual_id, sequence, ...);
```

### 4.4 序列号与全量快照的关系

全量快照事件也应携带当前序列号（用于消费端确定快照对应的时间点）：

```rust
pub struct OrderbookSnapshotEvent {
    pub perpetual_id: u32,
    pub bids: Vec<PriceLevel>,
    pub asks: Vec<PriceLevel>,
    pub timestamp_ms: u64,
    pub sequence: u64,   // 新增：快照对应的 delta 序列号
}
```

消费端逻辑：
1. 收到 Snapshot(sequence=N) → 重建完整订单簿，设置 last_seq=N
2. 收到 Delta(sequence=N+1) → 应用增量更新，last_seq=N+1
3. 收到 Delta(sequence=N+3) → 检测到缺口(N+2 丢失)，请求全量快照恢复

---

## 5. 双事件策略

### 5.1 策略概述

Phase 6 采用 **Delta 为主 + Snapshot 为辅** 的双事件策略：

```
每次订单操作
    │
    ├─→ OrderbookDeltaEvent (始终发射, ~55 bytes)
    │       → 低延迟通道 (DexStreamingManager)
    │       → 高频增量推送
    │
    └─→ OrderbookSnapshotEvent (降频发射)
            → Checkpoint 通道 (dex-indexer)
            → 恢复/对账/新订阅
```

### 5.2 Snapshot 降频策略

从"每次操作发射"改为条件性发射：

```rust
/// Snapshot 发射策略
struct SnapshotPolicy {
    /// 最小间隔（同一市场两次快照之间的最短时间）
    min_interval_ms: u64,  // 默认: 5000 (5秒)
    /// 最大 delta 计数（累计 N 个 delta 后强制发射一次快照）
    max_delta_count: u64,  // 默认: 100
}
```

在 `PerpetualState` 中追踪：

```rust
pub struct PerpetualState {
    // ... 现有字段 ...
    pub delta_sequence: u64,
    /// 上次发射全量快照时的 sequence
    pub last_snapshot_sequence: u64,       // 新增
    /// 上次发射全量快照时的时间戳
    pub last_snapshot_timestamp_ms: u64,   // 新增
}
```

发射判断逻辑：

```rust
fn should_emit_snapshot(state: &PerpetualState, current_timestamp_ms: u64) -> bool {
    let delta_count = state.delta_sequence - state.last_snapshot_sequence;
    let time_elapsed = current_timestamp_ms - state.last_snapshot_timestamp_ms;

    // 任一条件满足即发射快照
    delta_count >= SNAPSHOT_MAX_DELTA_COUNT
        || time_elapsed >= SNAPSHOT_MIN_INTERVAL_MS
}
```

### 5.3 迁移路径

**Phase 6a (初始)**:
- 新增 `OrderbookDeltaEvent`，每次操作都发射
- `OrderbookSnapshotEvent` 保持现有逻辑不变（每次操作都发射）
- 两者并行，验证 delta 正确性

**Phase 6b (优化)**:
- Delta 验证通过后，Snapshot 降频
- dex-indexer 的 `orderbook_snapshots.rs` handler 同步适配
- dex-streamer 以 delta 为主路径

**Phase 6c (稳定)**:
- 移除不必要的高频 Snapshot 发射
- Snapshot 仅在定时/定量条件下发射

---

## 6. 事件版本策略

### 6.1 命名规范

当前项目中事件命名有两种模式：

| 模式 | 示例 | 适用场景 |
|------|------|----------|
| 无后缀 | `FillEvent`, `OrderbookSnapshotEvent` | 初始版本，未来不太可能破坏性变更 |
| V1 后缀 | `OrderPlacedEventV1`, `OrderRemovedEventV1` | 预期会有 V2 版本 |

`OrderbookDeltaEvent` 作为全新事件类型，初始版本不加后缀。理由：
- 结构简洁（perpetual_id + sequence + updates + timestamp），未来扩展可追加字段
- BCS 支持尾部追加字段（向前兼容），多数演进不需要新版本
- 如果未来确需 breaking change，再引入 `OrderbookDeltaEventV2`

### 6.2 向前兼容保证

BCS 编码的向前兼容规则：
- 在 struct 末尾追加新字段 + `#[serde(default)]` → 旧版消费端可忽略新字段
- 不能删除字段或改变字段顺序
- 不能改变已有字段类型

示例演进路径：

```rust
// V1（当前）
pub struct OrderbookDeltaEvent {
    pub perpetual_id: u32,
    pub sequence: u64,
    pub updates: Vec<OrderbookDelta>,
    pub timestamp_ms: u64,
}

// 未来扩展（追加字段，BCS 兼容）
pub struct OrderbookDeltaEvent {
    pub perpetual_id: u32,
    pub sequence: u64,
    pub updates: Vec<OrderbookDelta>,
    pub timestamp_ms: u64,
    #[serde(default)]
    pub checkpoint_sequence: u64,    // 新增：所在 checkpoint 编号
    #[serde(default)]
    pub is_snapshot_attached: bool,  // 新增：本次是否附带了全量快照
}
```

### 6.3 消费端识别

消费端（indexer/streamer）通过 `event.type_.name` 字符串识别事件类型，与现有事件完全一致：

```rust
// dex-indexer 或 dex-streamer 中
match event.type_.name.as_str() {
    "OrderbookDeltaEvent" => {
        let delta: OrderbookDeltaEvent = bcs::from_bytes(&event.contents)?;
        // 处理增量更新
    }
    "OrderbookSnapshotEvent" => {
        let snapshot: OrderbookSnapshotEvent = bcs::from_bytes(&event.contents)?;
        // 处理全量快照
    }
    _ => {}
}
```

---

## 7. 与流式框架的集成

### 7.1 DexStreamingManager 事件枚举

`DexStreamingManager` 负责从 AuthorityState 拦截事件并广播（详见 `01-streaming-source.md`）。新增 Delta 变体：

```rust
// 新 crate: dex-streamer 或 sui-core 内部模块

/// DEX 流式事件（从执行层拦截的低延迟事件）
#[derive(Clone, Debug)]
pub enum DexStreamEvent {
    /// 成交事件（taker-maker 撮合）
    Fill(FillEvent),
    /// 订单簿快照（全量，降频）
    OrderbookSnapshot(OrderbookSnapshotEvent),
    /// 订单状态变更
    OrderUpdate(OrderUpdateEvent),
    /// 仓位变更
    PositionUpdate(PositionUpdateEvent),
    /// 余额变更
    BalanceUpdate(BalanceUpdateEvent),
    /// 订单上簿
    OrderPlaced(OrderPlacedEventV1),
    /// 订单移除
    OrderRemoved(OrderRemovedEventV1),
    /// 订单簿增量更新（Phase 6 Step 2 启用）
    OrderbookDelta(OrderbookDeltaEvent),
}
```

### 7.2 事件过滤与路由

DexStreamingManager 从 `TransactionOutputs` 中提取所有 DEX 事件，按类型路由到不同的 broadcast channel：

```rust
impl DexStreamingManager {
    /// 处理一笔交易的事件，提取 DEX 事件并广播
    /// API 签名与 01-streaming-source.md 一致
    pub fn process_dex_events(
        &self,
        tx_digest: &TransactionDigest,
        timestamp_ms: u64,
        events: &TransactionEvents,
    ) {
        // 按 package_id 过滤 DEX 事件，BCS 反序列化后通过 broadcast 发送
        // 详细实现见 01-streaming-source.md 第 3.3 节
        for event in events.data.iter() {
            if event.package_id != self.dex_package_id {
                continue;
            }

            match event.type_.name.as_str() {
                "OrderbookDeltaEvent" => {
                    if let Ok(delta) = bcs::from_bytes::<OrderbookDeltaEvent>(&event.contents) {
                        let _ = self.tx.send(DexStreamEvent::OrderbookDelta(delta));
                    }
                }
                "OrderbookSnapshotEvent" => {
                    if let Ok(snapshot) = bcs::from_bytes::<OrderbookSnapshotEvent>(&event.contents) {
                        let _ = self.tx.send(DexStreamEvent::OrderbookSnapshot(snapshot));
                    }
                }
                "FillEvent" => {
                    if let Ok(fill) = bcs::from_bytes::<FillEvent>(&event.contents) {
                        let _ = self.tx.send(DexStreamEvent::Fill(fill));
                    }
                }
                "OrderPlacedEventV1" => {
                    if let Ok(placed) = bcs::from_bytes::<OrderPlacedEventV1>(&event.contents) {
                        let _ = self.tx.send(DexStreamEvent::OrderPlaced(placed));
                    }
                }
                "OrderRemovedEventV1" => {
                    if let Ok(removed) = bcs::from_bytes::<OrderRemovedEventV1>(&event.contents) {
                        let _ = self.tx.send(DexStreamEvent::OrderRemoved(removed));
                    }
                }
                _ => {} // 其他事件（OrderUpdate、Position、Balance 等）暂不走低延迟通道
            }
        }
    }
}
```

### 7.3 dex-streamer OrderbookBuilder

dex-streamer 订阅 `DexStreamEvent::OrderbookDelta` 维护内存中的 L2 订单簿，并写入 Redis：

```rust
/// 链下 L2 订单簿构建器（每个市场一个实例）
pub struct OrderbookBuilder {
    perpetual_id: u32,
    /// Bid 价格 -> 数量
    bids: BTreeMap<u64, u64>,
    /// Ask 价格 -> 数量
    asks: BTreeMap<u64, u64>,
    /// 已应用的最新序列号
    last_sequence: u64,
    /// 是否已初始化（收到过快照）
    initialized: bool,
}

impl OrderbookBuilder {
    /// 应用增量更新
    pub fn apply_delta(&mut self, delta: &OrderbookDeltaEvent) -> Result<(), OrderbookError> {
        // 序列号校验
        if self.initialized && delta.sequence != self.last_sequence + 1 {
            return Err(OrderbookError::SequenceGap {
                expected: self.last_sequence + 1,
                received: delta.sequence,
            });
        }

        for update in &delta.updates {
            let book = match update.side {
                OrderbookDelta::SIDE_BID => &mut self.bids,
                OrderbookDelta::SIDE_ASK => &mut self.asks,
                _ => continue,
            };

            if update.quantity == 0 {
                book.remove(&update.price);
            } else {
                book.insert(update.price, update.quantity);
            }
        }

        self.last_sequence = delta.sequence;
        Ok(())
    }

    /// 从全量快照重建
    pub fn apply_snapshot(&mut self, snapshot: &OrderbookSnapshotEvent, sequence: u64) {
        self.bids.clear();
        self.asks.clear();

        for level in &snapshot.bids {
            if level.quantity > 0 {
                self.bids.insert(level.price, level.quantity);
            }
        }
        for level in &snapshot.asks {
            if level.quantity > 0 {
                self.asks.insert(level.price, level.quantity);
            }
        }

        self.last_sequence = sequence;
        self.initialized = true;
    }
}
```

---

## 8. 事件注册与唯一性

### 8.1 新增事件清单

| 新事件类型 | struct_tag name | 大小 |
|-----------|-----------------|------|
| `OrderbookDeltaEvent` | `"OrderbookDeltaEvent"` | ~55 bytes (avg) |
| `OrderbookDelta` | (内嵌于 Vec，无独立 struct_tag) | 17 bytes each |

### 8.2 事件唯一性验证

现有 `test_all_event_struct_tags_unique` 测试需新增 `OrderbookDeltaEvent`：

```rust
// dex-sui/crates/sui-types/src/dex_events.rs 测试

#[test]
fn test_all_event_struct_tags_unique() {
    let tags = vec![
        FillEvent::struct_tag(),
        PositionUpdateEvent::struct_tag(),
        BalanceUpdateEvent::struct_tag(),
        TransferEvent::struct_tag(),
        FundingSettlementEvent::struct_tag(),
        LiquidationEvent::struct_tag(),
        GlobalAccountsCreatedEvent::struct_tag(),
        PerpetualCreatedEvent::struct_tag(),
        OrderPlacedEventV1::struct_tag(),
        OrderRemovedEventV1::struct_tag(),
        OrderbookSnapshotEvent::struct_tag(),
        OrderbookDeltaEvent::struct_tag(),   // 新增
    ];

    // ... 现有验证逻辑不变
}
```

### 8.3 序列化测试

```rust
#[test]
fn test_orderbook_delta_event_serialization() {
    let event = OrderbookDeltaEvent {
        perpetual_id: 0,
        sequence: 42,
        updates: vec![
            OrderbookDelta {
                side: OrderbookDelta::SIDE_BID,
                price: 50000,
                quantity: 200,
            },
            OrderbookDelta {
                side: OrderbookDelta::SIDE_ASK,
                price: 50100,
                quantity: 0,  // 档位移除
            },
        ],
        timestamp_ms: 1704067200000,
    };

    let bytes = bcs::to_bytes(&event).expect("serialization should succeed");
    let deserialized: OrderbookDeltaEvent =
        bcs::from_bytes(&bytes).expect("deserialization should succeed");
    assert_eq!(event, deserialized);

    // 验证大小合理
    assert!(bytes.len() < 100, "Delta event should be compact, got {} bytes", bytes.len());
}

#[test]
fn test_orderbook_delta_event_to_sui_event() {
    let event = OrderbookDeltaEvent {
        perpetual_id: 0,
        sequence: 1,
        updates: vec![OrderbookDelta {
            side: OrderbookDelta::SIDE_BID,
            price: 50000,
            quantity: 100,
        }],
        timestamp_ms: 1704067200000,
    };

    let sender = SuiAddress::ZERO;
    let sui_event = event.to_sui_event(sender);

    assert_eq!(sui_event.package_id, ObjectID::from(DEX_EVENTS_PACKAGE));
    assert_eq!(sui_event.type_.name.as_str(), "OrderbookDeltaEvent");

    let deserialized: OrderbookDeltaEvent =
        bcs::from_bytes(&sui_event.contents).expect("should deserialize");
    assert_eq!(event, deserialized);
}

#[test]
fn test_empty_delta_event() {
    // 无变更时不应发射，但结构上需要支持空 updates
    let event = OrderbookDeltaEvent {
        perpetual_id: 0,
        sequence: 0,
        updates: vec![],
        timestamp_ms: 0,
    };

    let bytes = bcs::to_bytes(&event).expect("serialization should succeed");
    // 空 delta: 4 + 8 + 1(vec len) + 8 = 21 bytes
    assert!(bytes.len() < 25);
}
```

---

## 9. 变更文件清单

| 文件 | 变更 | 说明 |
|------|------|------|
| `dex-sui/crates/sui-types/src/dex_events.rs` | 新增 | `OrderbookDeltaEvent` + `OrderbookDelta` 结构、impl、测试 |
| `dex-sui/crates/sui-types/src/dex/perpetual.rs` | 修改 | `PerpetualState` 新增 `delta_sequence` 字段；`InlineOrderbook` 新增 `get_level_quantity` / `build_delta` / `collect_affected_levels` 方法 |
| `dex-sui/sui-execution/src/dex/commands/order.rs` | 修改 | 各 `execute_*` 函数中新增 Delta 计算与发射逻辑 |
| `dex-sui/crates/dex-indexer/src/handlers/orderbook_snapshots.rs` | 修改 | 新增 `OrderbookDeltaEvent` 处理（或新建 `orderbook_deltas.rs`） |
| `dex-sui/crates/dex-indexer/src/handlers/mod.rs` | 修改 | 注册新 handler |

---

## 10. 开放问题

| # | 问题 | 选项 | 建议 | 状态 |
|---|------|------|------|------|
| 1 | Delta 事件是否需要过 Checkpoint 持久化到 PG？ | A) 纯 Redis 通道，不入 PG<br>B) 入 PG 用于审计追溯 | A) 初期纯 Redis，与 Snapshot 现状一致 | 待确认 |
| 2 | `delta_sequence` 在 `PerpetualState` 中的 BCS 兼容性 | A) 追加字段 + `#[serde(default)]`<br>B) 放入单独的状态对象 | A) 追加字段，简单直接 | 待确认 |
| 3 | Phase 6a 期间两种事件并行对 Checkpoint 大小的影响 | 约增加 55 bytes/操作 (~1.7% overhead) | 可接受，Phase 6b 后 Snapshot 降频可补偿 | 已评估 |
| 4 | `collect_affected_levels` 在大量价格档位时的性能 | BTreeMap range 查询 O(log n + k) | 实际匹配范围通常 < 10 个档位，无性能问题 | 已评估 |
