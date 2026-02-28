meishe# Phase 6 Step 1: OrderbookDeltaEvent + Delta 计算 实施计划

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 定义 `OrderbookDeltaEvent` / `OrderbookDelta` 类型，在 `PerpetualState` 中添加 `delta_sequence`，在 `InlineOrderbook` 上实现 `get_level_quantity()` / `build_delta()`，在订单执行函数中计算 delta（暂存为返回值，Step 2 才接入 gRPC），同时移除 `OrderbookSnapshotEvent` 发射。

**Architecture:** 在 `sui-types` 中新增 delta 事件类型和序列号字段，在 `InlineOrderbook` 上新增 delta 计算方法。在 `sui-execution` 的 5 个订单执行函数中，将 snapshot 发射替换为 delta 计算。Delta 事件**不写入 TransactionEvents**（不调用 `to_sui_event()`），仅作为函数返回值供后续 Step 2 的 DexStreamingManager 消费。

**Tech Stack:** Rust, BCS serialization, sui-types, sui-execution

---

## 关键设计决策

1. **Delta 不写入 TransactionEvents** — `OrderbookDeltaEvent` 仅通过 gRPC 推送（Step 2），不走 Checkpoint 路径
2. **Snapshot 停止发射** — 移除所有 5 处 `build_snapshot() → to_sui_event()` 调用
3. **Delta 暂存为返回值** — Step 1 中 delta 通过函数返回值传出，Step 2 再接入 streaming
4. **序列号在引擎内部生成** — `PerpetualState.delta_sequence` 每市场独立递增

## 受影响的文件总览

| 文件 | 操作 | 说明 |
|------|------|------|
| `crates/sui-types/src/dex_events.rs` | 修改 | 新增 `OrderbookDeltaEvent` + `OrderbookDelta` 类型 |
| `crates/sui-types/src/dex/perpetual.rs` | 修改 | `PerpetualState` 加 `delta_sequence`；`InlineOrderbook` 加 `get_level_quantity` / `build_delta` |
| `sui-execution/src/dex/commands/order.rs` | 修改 | 5 个函数中替换 snapshot → delta 计算 |

---

### Task 1: 定义 OrderbookDeltaEvent 和 OrderbookDelta 类型

**Files:**
- Modify: `crates/sui-types/src/dex_events.rs:577-623` (在 OrderbookSnapshotEvent 之后添加)

**Step 1: 写失败测试**

在 `dex_events.rs` 的 `#[cfg(test)] mod tests` 中（`test_all_event_struct_tags_unique` 之前），添加以下测试：

```rust
    #[test]
    fn test_orderbook_delta_event_bcs_roundtrip() {
        let event = OrderbookDeltaEvent {
            perpetual_id: 0,
            sequence: 42,
            updates: vec![
                OrderbookDelta { side: 0, price: 50000, quantity: 100 },
                OrderbookDelta { side: 1, price: 50100, quantity: 0 },
            ],
            timestamp_ms: 1704067200000,
        };

        let bytes = bcs::to_bytes(&event).expect("serialization should succeed");
        let deserialized: OrderbookDeltaEvent =
            bcs::from_bytes(&bytes).expect("deserialization should succeed");
        assert_eq!(event, deserialized);
    }

    #[test]
    fn test_orderbook_delta_event_bcs_size() {
        // 典型场景：2 个价格档位变更 → 预期 ~55 bytes
        let event = OrderbookDeltaEvent {
            perpetual_id: 0,
            sequence: 1,
            updates: vec![
                OrderbookDelta { side: 0, price: 50000, quantity: 100 },
                OrderbookDelta { side: 1, price: 50100, quantity: 200 },
            ],
            timestamp_ms: 1000000,
        };

        let bytes = bcs::to_bytes(&event).expect("serialization should succeed");
        // perpetual_id(4) + sequence(8) + vec_len(1) + 2*OrderbookDelta(2*17) + timestamp_ms(8) = 55
        assert!(bytes.len() <= 100, "Delta event should be compact, got {} bytes", bytes.len());
    }

    #[test]
    fn test_orderbook_delta_event_empty_updates() {
        let event = OrderbookDeltaEvent {
            perpetual_id: 0,
            sequence: 0,
            updates: vec![],
            timestamp_ms: 1704067200000,
        };

        let bytes = bcs::to_bytes(&event).expect("serialization should succeed");
        let deserialized: OrderbookDeltaEvent =
            bcs::from_bytes(&bytes).expect("deserialization should succeed");
        assert_eq!(event, deserialized);
        assert!(event.updates.is_empty());
    }

    #[test]
    fn test_orderbook_delta_struct_tag() {
        let tag = OrderbookDeltaEvent::struct_tag();
        assert_eq!(tag.address, DEX_EVENTS_PACKAGE);
        assert_eq!(tag.module.as_str(), DEX_EVENTS_MODULE);
        assert_eq!(tag.name.as_str(), "OrderbookDeltaEvent");
    }
```

同时在 `test_all_event_struct_tags_unique` 中添加 `OrderbookDeltaEvent::struct_tag()`。

**Step 2: 运行测试确认失败**

Run: `cargo nextest run -p sui-types --lib -- test_orderbook_delta`
Expected: FAIL — `OrderbookDeltaEvent` 和 `OrderbookDelta` 未定义

**Step 3: 实现类型定义**

在 `dex_events.rs` 的 `OrderbookSnapshotEvent` 实现块之后（line 623 后）添加：

```rust
// ============================================================================
// OrderbookDeltaEvent - Incremental L2 orderbook update (gRPC-only, not in TransactionEvents)
// ============================================================================

/// 单个价格档位的变更描述
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OrderbookDelta {
    /// 方向: 0 = Bid, 1 = Ask
    pub side: u8,
    /// 价格（subticks）
    pub price: u64,
    /// 该价格档位的新总数量（0 = 档位已清空）
    pub quantity: u64,
}

/// 增量 L2 订单簿更新事件
///
/// 仅描述本次操作导致的价格档位变更。
/// 不纳入 TransactionEvents，仅通过 gRPC 流式推送。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OrderbookDeltaEvent {
    /// 永续合约市场 ID
    pub perpetual_id: u32,
    /// 序列号（每个市场独立递增）
    pub sequence: u64,
    /// 变更的价格档位列表
    pub updates: Vec<OrderbookDelta>,
    /// 时间戳（毫秒）
    pub timestamp_ms: u64,
}

impl OrderbookDeltaEvent {
    pub fn struct_tag() -> StructTag {
        StructTag {
            address: DEX_EVENTS_PACKAGE,
            module: Identifier::new(DEX_EVENTS_MODULE).expect("valid module name"),
            name: Identifier::new("OrderbookDeltaEvent").expect("valid struct name"),
            type_params: vec![],
        }
    }
}
```

**Step 4: 运行测试确认通过**

Run: `cargo nextest run -p sui-types --lib -- test_orderbook_delta`
Expected: 4 tests PASS

**Step 5: 提交**

```bash
git add crates/sui-types/src/dex_events.rs
git commit -m "feat(dex): add OrderbookDeltaEvent and OrderbookDelta types

Incremental L2 orderbook update event for gRPC streaming (Phase 6 Step 1).
Not added to TransactionEvents - flows via gRPC only."
```

---

### Task 2: PerpetualState 添加 delta_sequence 字段

**Files:**
- Modify: `crates/sui-types/src/dex/perpetual.rs:683-764`

**Step 1: 写失败测试**

在 `perpetual.rs` 的 `#[cfg(test)] mod tests` 末尾添加：

```rust
    #[test]
    fn test_delta_sequence_increment() {
        let id = UID::new(ObjectID::from_single_byte(0xAA));
        let params = PerpetualParams::default_for_test(0);
        let version = SequenceNumber::new();
        let mut state = PerpetualState::new(id, 0, params, version);

        assert_eq!(state.delta_sequence, 0);
        assert_eq!(state.next_delta_sequence(), 0);
        assert_eq!(state.delta_sequence, 1);
        assert_eq!(state.next_delta_sequence(), 1);
        assert_eq!(state.delta_sequence, 2);
    }
```

**Step 2: 运行测试确认失败**

Run: `cargo nextest run -p sui-types --lib -- test_delta_sequence`
Expected: FAIL — `delta_sequence` 字段不存在

**Step 3: 实现**

3a. 在 `PerpetualState` 结构体中添加字段（line 701 后，`initial_shared_version` 之后）：

```rust
    /// Delta 事件序列号（每个市场独立递增，仅 gRPC 流式推送使用）
    pub delta_sequence: u64,
```

3b. 在 `PerpetualState::new()` 中初始化（line 721 `initial_shared_version: version,` 之后）：

```rust
            delta_sequence: 0,
```

3c. 在 `impl PerpetualState` 中添加方法（`is_active` 方法之后）：

```rust
    /// 获取并递增 delta 序列号
    pub fn next_delta_sequence(&mut self) -> u64 {
        let seq = self.delta_sequence;
        self.delta_sequence += 1;
        seq
    }
```

**Step 4: 运行测试确认通过**

Run: `cargo nextest run -p sui-types --lib -- test_delta_sequence`
Expected: PASS

**Step 5: 检查 BCS 兼容性**

由于 `PerpetualState` 使用 BCS 序列化存储在链上对象中，新增字段会改变序列化格式。这对开发环境可接受（需要 `make reset` 重置数据），但需确认：

Run: `cargo nextest run -p sui-types --lib -- perpetual`
Expected: 所有现有 perpetual 测试通过（测试中直接构造对象，不依赖旧 BCS 数据）

**Step 6: 提交**

```bash
git add crates/sui-types/src/dex/perpetual.rs
git commit -m "feat(dex): add delta_sequence to PerpetualState

Per-market monotonic sequence number for orderbook delta events.
Starts at 0, increments on each delta emission."
```

---

### Task 3: InlineOrderbook 添加 get_level_quantity 和 build_delta 方法

**Files:**
- Modify: `crates/sui-types/src/dex/perpetual.rs:306-450` (InlineOrderbook impl block)
- Modify: `crates/sui-types/src/dex_events.rs` (import OrderbookDeltaEvent)

**Step 1: 写失败测试**

在 `perpetual.rs` 的 `#[cfg(test)] mod tests` 末尾添加：

```rust
    #[test]
    fn test_get_level_quantity_empty() {
        let ob = InlineOrderbook::new();
        assert_eq!(ob.get_level_quantity(Side::Buy, 50000), 0);
        assert_eq!(ob.get_level_quantity(Side::Sell, 50000), 0);
    }

    #[test]
    fn test_get_level_quantity_with_orders() {
        let mut ob = InlineOrderbook::new();
        ob.add_order(make_limit_order(1, 0, 0, Side::Buy, 100, 50000));
        ob.add_order(make_limit_order(2, 1, 0, Side::Buy, 200, 50000));
        ob.add_order(make_limit_order(3, 0, 0, Side::Sell, 150, 50100));

        assert_eq!(ob.get_level_quantity(Side::Buy, 50000), 300);
        assert_eq!(ob.get_level_quantity(Side::Sell, 50100), 150);
        assert_eq!(ob.get_level_quantity(Side::Buy, 49999), 0);
    }

    #[test]
    fn test_build_delta_no_change() {
        let ob = InlineOrderbook::new();
        let before: Vec<(Side, u64, u64)> = vec![(Side::Buy, 50000, 0)];
        let delta = ob.build_delta(0, 0, &before, 1000);

        assert_eq!(delta.perpetual_id, 0);
        assert_eq!(delta.sequence, 0);
        assert!(delta.updates.is_empty());
        assert_eq!(delta.timestamp_ms, 1000);
    }

    #[test]
    fn test_build_delta_level_added() {
        let mut ob = InlineOrderbook::new();
        ob.add_order(make_limit_order(1, 0, 0, Side::Buy, 100, 50000));

        // 该档位之前不存在（qty=0），现在有 100
        let before = vec![(Side::Buy, 50000, 0u64)];
        let delta = ob.build_delta(0, 1, &before, 1000);

        assert_eq!(delta.updates.len(), 1);
        assert_eq!(delta.updates[0].side, 0); // Bid
        assert_eq!(delta.updates[0].price, 50000);
        assert_eq!(delta.updates[0].quantity, 100);
    }

    #[test]
    fn test_build_delta_level_removed() {
        let ob = InlineOrderbook::new();

        // 该档位之前有 100，现在没了
        let before = vec![(Side::Sell, 50100, 100u64)];
        let delta = ob.build_delta(0, 2, &before, 1000);

        assert_eq!(delta.updates.len(), 1);
        assert_eq!(delta.updates[0].side, 1); // Ask
        assert_eq!(delta.updates[0].price, 50100);
        assert_eq!(delta.updates[0].quantity, 0); // removed
    }

    #[test]
    fn test_build_delta_multiple_levels() {
        let mut ob = InlineOrderbook::new();
        ob.add_order(make_limit_order(1, 0, 0, Side::Buy, 50, 50000)); // was 100, now 50
        ob.add_order(make_limit_order(2, 0, 0, Side::Sell, 200, 50100)); // new

        let before = vec![
            (Side::Buy, 50000, 100u64),  // 变化: 100 → 50
            (Side::Sell, 50100, 0u64),   // 变化: 0 → 200
            (Side::Sell, 50200, 300u64), // 无变化: 300 → 300（不存在了，会变成 0）
        ];
        let delta = ob.build_delta(0, 3, &before, 1000);

        // 3 个都发生了变化
        assert_eq!(delta.updates.len(), 3);
    }
```

**Step 2: 运行测试确认失败**

Run: `cargo nextest run -p sui-types --lib -- test_get_level_quantity test_build_delta`
Expected: FAIL — 方法不存在

**Step 3: 实现 get_level_quantity 和 build_delta**

在 `perpetual.rs` 顶部添加 import：

```rust
use crate::dex_events::{OrderbookDelta, OrderbookDeltaEvent};
```

在 `InlineOrderbook` 的 `impl` 块中，`build_snapshot` 方法之后添加：

```rust
    /// 获取指定价格档位的当前总数量
    pub fn get_level_quantity(&self, side: Side, price: u64) -> u64 {
        match side {
            Side::Buy => self.bids.get(&price)
                .map(|l| l.total_quantums).unwrap_or(0),
            Side::Sell => self.asks.get(&price)
                .map(|l| l.total_quantums).unwrap_or(0),
        }
    }

    /// 构建增量 delta 事件：对比操作前后的价格档位数量
    pub fn build_delta(
        &self,
        perpetual_id: u32,
        sequence: u64,
        before_quantities: &[(Side, u64, u64)],
        timestamp_ms: u64,
    ) -> OrderbookDeltaEvent {
        let mut updates = Vec::new();
        for &(side, price, qty_before) in before_quantities {
            let qty_after = self.get_level_quantity(side, price);
            if qty_after != qty_before {
                updates.push(OrderbookDelta {
                    side: match side {
                        Side::Buy => 0,
                        Side::Sell => 1,
                    },
                    price,
                    quantity: qty_after,
                });
            }
        }
        OrderbookDeltaEvent { perpetual_id, sequence, updates, timestamp_ms }
    }
```

**Step 4: 运行测试确认通过**

Run: `cargo nextest run -p sui-types --lib -- test_get_level_quantity test_build_delta`
Expected: 6 tests PASS

**Step 5: 提交**

```bash
git add crates/sui-types/src/dex/perpetual.rs
git commit -m "feat(dex): add get_level_quantity and build_delta to InlineOrderbook

Delta calculation compares before/after quantities at affected price levels.
Returns OrderbookDeltaEvent with only changed levels."
```

---

### Task 4: execute_place_order — 替换 snapshot 为 delta 计算

**Files:**
- Modify: `sui-execution/src/dex/commands/order.rs:38-549` (execute_place_order 函数)

**核心思路：**
1. 在 `match_order()` 之前，收集所有可能受影响的价格档位的 before 数量
2. 在 snapshot 发射位置，改为 `build_delta()` 计算 delta
3. Delta 通过返回值传出（修改 `CommandResult`），暂不接入 streaming

**需要收集的受影响价格档位：**
- **对手方**: 每个 fill 的 price（maker 侧被吃的档位）
- **挂单方**: 如果 taker 有 remaining（resting order），记录 taker 的 price

**Step 1: 在 match_order 前收集 before 数量**

在 `order.rs` 中，`match_order()` 调用之前（约 line 249），添加收集逻辑：

```rust
    // --- Delta 计算：收集操作前的价格档位数量 ---
    // 对手方可能受影响的价格范围
    let opposite_side = match params.side {
        Side::Buy => Side::Sell,
        Side::Sell => Side::Buy,
    };
    let before_quantities: Vec<(Side, u64, u64)> = {
        let levels = match opposite_side {
            Side::Buy => &state.orderbook.bids,
            Side::Sell => &state.orderbook.asks,
        };
        let mut qty_list: Vec<(Side, u64, u64)> = levels.iter()
            .map(|(&price, level)| (opposite_side, price, level.total_quantums))
            .collect();
        // taker 的挂单价位（如果最终 resting）
        qty_list.push((params.side, params.subticks, state.orderbook.get_level_quantity(params.side, params.subticks)));
        qty_list
    };
```

**Step 2: 替换 snapshot 发射为 delta 计算**

将 line 494-503（snapshot 发射块）替换为：

```rust
    // 构建增量 delta（不写入 TransactionEvents，仅供 gRPC 流式推送）
    let delta_event = state.orderbook.build_delta(
        perpetual_id,
        state.next_delta_sequence(),
        &before_quantities,
        ctx.epoch_timestamp_ms,
    );
```

注意：`delta_event` 暂不使用（Step 2 才接入 DexStreamingManager）。为避免 unused 警告，使用 `let _delta_event = ...` 或通过返回值传出。

由于技术文档要求 Delta 不进入 TransactionEvents，且 Step 2 才建立 gRPC 通道，Step 1 先用 `let _ = delta_event;` 占位。

**Step 3: 确认测试通过**

Run: `cargo check -p sui-execution`
Expected: 编译通过，无错误

Run: `SUI_SKIP_SIMTESTS=1 cargo nextest run -p sui-types --lib`
Expected: 全部通过

**Step 4: 提交**

```bash
git add sui-execution/src/dex/commands/order.rs
git commit -m "feat(dex): replace snapshot with delta calculation in execute_place_order

Collect before-quantities for affected price levels before matching,
then build OrderbookDeltaEvent after matching. Snapshot emission removed.
Delta is computed but not yet connected to gRPC (Step 2)."
```

---

### Task 5: execute_place_order_with_eip712 — 替换 snapshot 为 delta 计算

**Files:**
- Modify: `sui-execution/src/dex/commands/order.rs:580-1179` (execute_place_order_with_eip712 函数)

**Step 1: 添加 before 数量收集**

与 Task 4 相同模式，在 `match_order()` 调用之前添加 `before_quantities` 收集。

**Step 2: 替换 snapshot 发射**

将 line 1118-1125（snapshot 发射块）替换为 delta 计算。

**Step 3: 确认编译通过**

Run: `cargo check -p sui-execution`
Expected: 编译通过

**Step 4: 提交**

```bash
git add sui-execution/src/dex/commands/order.rs
git commit -m "feat(dex): replace snapshot with delta in execute_place_order_with_eip712"
```

---

### Task 6: execute_cancel_order — 替换 snapshot 为 delta 计算

**Files:**
- Modify: `sui-execution/src/dex/commands/order.rs:1194-1415` (execute_cancel_order 函数)

**核心思路：** 撤单只影响一个价格档位（被撤订单的 side + price）。

**Step 1: 在 remove_order 前收集 before 数量**

在 `state.orderbook.remove_order()` 调用之前（约 line 1332），添加：

```rust
        let qty_before = state.orderbook.get_level_quantity(side, price);
        let before_quantities = vec![(side, price, qty_before)];
```

**Step 2: 替换 snapshot 发射**

将 line 1347-1349（snapshot 发射块）替换为：

```rust
        let _ = state.orderbook.build_delta(
            perpetual_id,
            state.next_delta_sequence(),
            &before_quantities,
            ctx.epoch_timestamp_ms,
        );
```

**Step 3: 确认编译通过**

Run: `cargo check -p sui-execution`

**Step 4: 提交**

```bash
git add sui-execution/src/dex/commands/order.rs
git commit -m "feat(dex): replace snapshot with delta in execute_cancel_order"
```

---

### Task 7: execute_cancel_all_orders — 替换 snapshot 为 delta 计算

**Files:**
- Modify: `sui-execution/src/dex/commands/order.rs:1416-1540` (execute_cancel_all_orders 函数)

**核心思路：** 批量撤单可能影响多个档位。在循环 remove 前收集所有受影响档位。

**Step 1: 在 cancel 循环前收集 before 数量**

在 `for order_id in order_ids` 循环之前（约 line 1462），先收集所有被撤订单的档位：

```rust
    // 收集所有待撤订单的价格档位 before 数量
    let mut before_quantities: Vec<(Side, u64, u64)> = Vec::new();
    let mut seen_levels = std::collections::HashSet::new();
    for oid in &order_ids {
        if let Some(order) = state.orderbook.get_order(oid) {
            let key = (order.side, order.subticks);
            if seen_levels.insert(key) {
                before_quantities.push((
                    order.side,
                    order.subticks,
                    state.orderbook.get_level_quantity(order.side, order.subticks),
                ));
            }
        }
    }
```

需要在文件顶部添加 `use std::collections::HashSet;`（但已经有 `use std::collections::BTreeMap;`，加一个 `HashSet` 即可）。

注意：`Side` 需要实现 `Hash` + `Eq`。检查 Side 的 derive：

```rust
// 如果 Side 没有 derive Hash，改用 (u8, u64) 作为 key：
let key = (order.side as u8, order.subticks);
let mut seen_levels = std::collections::HashSet::<(u8, u64)>::new();
```

**Step 2: 替换 snapshot 发射**

将 line 1482-1486（snapshot 发射块）替换为：

```rust
    if any_cancelled {
        let _ = state.orderbook.build_delta(
            perpetual_id,
            state.next_delta_sequence(),
            &before_quantities,
            ctx.epoch_timestamp_ms,
        );
    }
```

**Step 3: 确认编译通过**

Run: `cargo check -p sui-execution`

**Step 4: 提交**

```bash
git add sui-execution/src/dex/commands/order.rs
git commit -m "feat(dex): replace snapshot with delta in execute_cancel_all_orders"
```

---

### Task 8: execute_cancel_order_with_eip712 — 替换 snapshot 为 delta 计算

**Files:**
- Modify: `sui-execution/src/dex/commands/order.rs:1541-1744` (execute_cancel_order_with_eip712 函数)

**Step 1: 在 remove_order 前收集 before 数量**

与 Task 6 相同模式。在 `state.orderbook.remove_order()` 之前（约 line 1652）：

```rust
    let qty_before = state.orderbook.get_level_quantity(side, price);
    let before_quantities = vec![(side, price, qty_before)];
```

**Step 2: 替换 snapshot 发射**

将 line 1683-1689（snapshot 发射块）替换为：

```rust
    {
        let _ = state.orderbook.build_delta(
            perpetual_id,
            state.next_delta_sequence(),
            &before_quantities,
            ctx.epoch_timestamp_ms,
        );
    }
```

**Step 3: 确认编译通过**

Run: `cargo check -p sui-execution`

**Step 4: 提交**

```bash
git add sui-execution/src/dex/commands/order.rs
git commit -m "feat(dex): replace snapshot with delta in execute_cancel_order_with_eip712"
```

---

### Task 9: 全量测试 + clippy 检查

**Step 1: 运行 sui-types 全部测试**

Run: `SUI_SKIP_SIMTESTS=1 cargo nextest run -p sui-types --lib`
Expected: 全部通过

**Step 2: 运行 sui-execution 编译**

Run: `cargo check -p sui-execution`
Expected: 编译通过

**Step 3: 运行 clippy**

Run: `cargo xclippy`
Expected: 无新警告

如果有 `unused import` 的 `OrderbookSnapshotEvent` 相关问题（build_snapshot 仍被 InlineOrderbook 导出但不再在 order.rs 中使用），按需清理。

注意：`build_snapshot()` 方法本身**不删除**——它仍然被 dex-indexer 的 orderbook_snapshots handler 使用（通过 BCS 反序列化），且在 Step 2 中 `GetSnapshot` gRPC 需要它。

**Step 4: 运行相关 e2e 测试（可选，耗时较长）**

如果需要验证端到端：
Run: `SUI_SKIP_SIMTESTS=1 cargo nextest run -p dex-indexer --lib`
Expected: indexer 测试通过（indexer 仍处理 OrderbookSnapshotEvent，但执行层不再发射，所以不会收到新的 snapshot event）

**Step 5: 提交（如有修复）**

```bash
git add -A
git commit -m "fix(dex): address clippy warnings from Phase 6 Step 1 changes"
```

---

## 注意事项

### BCS 兼容性
- `PerpetualState` 新增 `delta_sequence` 字段改变了 BCS 布局
- 开发环境需要 `make reset` 重置数据（Docker 全栈）
- 生产环境需要数据迁移或版本化（当前阶段无需考虑）

### dex-indexer 影响
- `orderbook_snapshots.rs` handler 仍然存在，但执行层不再发射 `OrderbookSnapshotEvent`
- indexer 不会报错（只是不再收到该事件），但 Redis 中的 `dex:orderbook:{id}` 数据会**停止更新**
- 这是预期行为：Step 3 的 dex-stream-indexer 会接管订单簿数据写入 `dex:l2book:{id}`
- **过渡期**：如果需要在 Step 3 完成前保持 WS l2Book 功能，可以暂时保留 snapshot 发射（添加 feature flag）

### Delta 暂存策略
- Step 1 中 `let _ = delta_event;` 丢弃 delta 数据
- Step 2 会通过 `DexStreamingManager` 收集这些 delta 并推入 gRPC stream
- 具体接入方式：order.rs 的执行函数需要返回 `Vec<OrderbookDeltaEvent>`，或通过 `DexExecutionContext` 传入 streaming manager 引用
