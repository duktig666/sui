# 订单簿推送策略分析

> 创建日期: 2026-02-05
> 状态: **✅ 已实施 - 方案 A（复用 Sui Event）**
> 实施日期: 2026-02-05

## 1. 背景

当前 Phase 2 实时通道设计中，订单簿推送采用"链上发事件 + 链下构建"模式。在实现过程中发现以下问题：

1. **Maker 订单更新问题**：部分成交时缺少专门的更新事件
2. **链下状态一致性风险**：依赖本地状态推算，可能与链上不一致
3. **启动恢复复杂**：需要从 PostgreSQL 加载历史订单重建订单簿

本文档对比分析两种订单簿推送方案，为技术选型提供参考。

---

## 2. 方案概述

| 方案 | 描述 |
|------|------|
| **方案 A** | 链上发事件 → dex-realtime 实时构建订单簿 |
| **方案 B** | 链上内存订单簿 → 250ms 推送完整快照 |

---

## 3. 方案 A：链上事件 + 链下构建

### 3.1 架构图

```
┌─────────────────────────────────────────────────────────────────┐
│                         方案 A 架构                              │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│   Sui 撮合引擎 (链上)                                            │
│       │                                                         │
│       ├─→ OrderPlacedEventV1                                    │
│       ├─→ OrderRemovedEventV1                                   │
│       └─→ FillEventV1                                           │
│              │                                                  │
│              │ sui_subscribeEvent                               │
│              ▼                                                  │
│   dex-realtime (链下)                                           │
│       │                                                         │
│       ├─→ 内存构建订单簿                                         │
│       ├─→ 定期快照到 Redis (~100ms)                              │
│       └─→ 启动从 PostgreSQL 恢复                                 │
│              │                                                  │
│              ▼                                                  │
│   dex-ws → WebSocket 客户端                                     │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

### 3.2 优点

| 优点 | 说明 |
|------|------|
| ✅ 延迟更低 | 事件粒度推送，无需等待快照周期 |
| ✅ 带宽更省 | 只传输变化（增量），不是完整订单簿 |
| ✅ 链上逻辑简单 | 只需发事件，无需维护额外的推送逻辑 |
| ✅ 灵活性高 | 链下可自定义聚合逻辑（如按精度聚合） |

### 3.3 缺点

| 缺点 | 说明 |
|------|------|
| ❌ 链下逻辑复杂 | 需要处理事件顺序、重复、丢失 |
| ❌ 状态一致性风险 | 链下订单簿可能与链上不一致 |
| ❌ 启动恢复复杂 | 需要从 PostgreSQL 加载历史订单重建 |
| ❌ Maker 更新问题 | 部分成交需要额外事件或推算逻辑 |

### 3.4 Maker 更新问题详解

当前事件设计中，Maker 订单部分成交时的处理存在问题：

```
Maker 订单部分成交
    │
    └─→ 只发 FillEventV1
        └─→ dex-realtime 需要自己推算剩余数量
```

**问题**：
- 依赖本地状态维护
- 如果 dex-realtime 重启或丢失状态，无法正确恢复

**解决方案**：每次 Maker 成交都发 OrderPlacedEventV1（携带新的剩余数量）

---

## 4. 方案 B：链上订单簿快照推送

### 4.1 架构图

```
┌─────────────────────────────────────────────────────────────────┐
│                         方案 B 架构                              │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│   Sui 撮合引擎 (链上)                                            │
│       │                                                         │
│       ├─→ 维护内存订单簿                                         │
│       │                                                         │
│       └─→ 每 250ms 发射 OrderbookSnapshotEvent                  │
│              │                                                  │
│              │ 完整订单簿数据                                    │
│              │ { bids: [...], asks: [...] }                    │
│              │                                                  │
│              │ sui_subscribeEvent                               │
│              ▼                                                  │
│   dex-realtime (链下)                                           │
│       │                                                         │
│       └─→ 直接使用，无需构建                                     │
│              │                                                  │
│              ▼                                                  │
│   dex-ws → WebSocket 客户端                                     │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

### 4.2 优点

| 优点 | 说明 |
|------|------|
| ✅ 链下逻辑极简 | 直接使用快照，无需构建维护 |
| ✅ 状态绝对一致 | 链上是权威数据源 |
| ✅ 启动恢复简单 | 等一个快照即可，无需历史数据 |
| ✅ 无 Maker 更新问题 | 快照包含最新状态 |
| ✅ 类似 Hyperliquid | Hyperliquid 就是全量推送模式 |

### 4.3 缺点

| 缺点 | 说明 |
|------|------|
| ❌ 延迟增加 | 最坏情况 250ms 延迟（平均 125ms） |
| ❌ 带宽消耗大 | 每次推送完整订单簿 |
| ❌ 链上逻辑增加 | 需要定时器触发推送 |
| ❌ 事件大小限制 | 深度订单簿可能超过事件大小限制 |

---

## 5. 关键维度对比

| 维度 | 方案 A（事件+链下构建） | 方案 B（快照推送） |
|------|------------------------|-------------------|
| **延迟** | ⭐⭐⭐ 更低（事件粒度） | ⭐⭐ 增加 ~125ms（平均） |
| **带宽** | ⭐⭐⭐ 增量传输 | ⭐ 全量传输 |
| **一致性** | ⭐⭐ 可能不一致 | ⭐⭐⭐ 绝对一致 |
| **链下复杂度** | ⭐ 复杂（构建+恢复） | ⭐⭐⭐ 简单 |
| **链上复杂度** | ⭐⭐⭐ 简单 | ⭐⭐ 需要定时推送 |
| **启动恢复** | ⭐ 复杂（需历史数据） | ⭐⭐⭐ 简单（等快照） |
| **可扩展性** | ⭐⭐⭐ 多市场独立 | ⭐⭐ 多市场带宽翻倍 |

---

## 6. 数据量估算

### 6.1 假设条件

- 单个市场订单簿深度：100 档 × 2 (买卖)
- 每档数据大小：~50 bytes (price + size + count)

### 6.2 方案 A（事件）

```
单次事件大小：~200 bytes
假设每秒 100 次事件
带宽：100 × 200 = 20 KB/s
```

### 6.3 方案 B（快照）

```
单次快照大小：200 档 × 50 bytes = 10 KB
每秒 4 次（250ms）
带宽：4 × 10 KB = 40 KB/s
```

### 6.4 多市场扩展

**10 个市场时**：
- 方案 A：~200 KB/s
- 方案 B：~400 KB/s

---

## 7. 混合方案 C（推荐）

结合两者优点的混合方案：

```
┌─────────────────────────────────────────────────────────────────┐
│                       混合方案 C                                 │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│   链上：                                                         │
│   ├─→ 实时发射 OrderPlacedEventV1, OrderRemovedEventV1          │
│   └─→ 每 5s 发射 OrderbookSnapshotEvent（校验用）                │
│                                                                 │
│   链下 (dex-realtime)：                                         │
│   ├─→ 实时处理事件更新订单簿（低延迟）                            │
│   └─→ 收到快照时校验/修正订单簿（保证一致性）                     │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

### 7.1 混合方案优点

| 优点 | 说明 |
|------|------|
| ✅ 低延迟 | 实时事件更新 |
| ✅ 最终一致 | 定期快照校验 |
| ✅ 启动简单 | 等一个快照即可开始 |
| ✅ 自愈能力 | 状态漂移可自动修正 |

---

## 8. 实现细节

### 8.1 OrderbookSnapshotEventV1 定义

```rust
/// 订单簿快照事件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderbookSnapshotEventV1 {
    pub perpetual_id: u32,
    pub timestamp_ms: u64,
    /// 买盘：按价格降序
    pub bids: Vec<PriceLevel>,
    /// 卖盘：按价格升序
    pub asks: Vec<PriceLevel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceLevel {
    pub price: u64,
    pub size: u64,
    pub order_count: u32,
}
```

### 8.2 链上定时推送逻辑

```rust
// 撮合引擎内部
impl MatchingEngine {
    /// 每个区块结束时检查是否需要推送
    pub fn on_block_end(&mut self) {
        let now = current_timestamp_ms();

        for (perpetual_id, orderbook) in &self.orderbooks {
            if now - orderbook.last_snapshot_time >= 250 {
                emit(OrderbookSnapshotEventV1 {
                    perpetual_id: *perpetual_id,
                    timestamp_ms: now,
                    bids: orderbook.get_bids_snapshot(100),  // 前 100 档
                    asks: orderbook.get_asks_snapshot(100),
                });
                orderbook.last_snapshot_time = now;
            }
        }
    }
}
```

### 8.3 链下处理逻辑（方案 B）

```rust
// dex-realtime - 极其简单
fn on_orderbook_snapshot(&mut self, event: &OrderbookSnapshotEventV1) {
    // 直接替换，无需构建
    self.redis.hset(
        format!("dex:orderbook:{}", event.perpetual_id),
        "bids", serde_json::to_string(&event.bids)?,
        "asks", serde_json::to_string(&event.asks)?,
        "updated_at", event.timestamp_ms,
    )?;
}
```

---

## 9. 建议

### 9.1 MVP 阶段：选择方案 B（快照推送）

| 理由 | 说明 |
|------|------|
| 开发速度 | 链下逻辑极简，快速上线 |
| 正确性优先 | 绝对一致性，避免状态漂移 bug |
| 类似 Hyperliquid | 验证可行性 |
| 延迟可接受 | 250ms 对大多数用户足够 |

### 9.2 生产优化：升级到混合方案 C

当需要更低延迟时：
1. 保留快照作为校验机制
2. 增加实时事件处理路径
3. 定期对账修正

---

## 10. 总结

| 阶段 | 推荐方案 | 理由 |
|------|----------|------|
| **MVP** | 方案 B（快照推送） | 简单、一致、快速上线 |
| **生产优化** | 方案 C（混合） | 低延迟 + 最终一致 |

---

## 11. 实施记录（2026-02-05）

### 11.1 最终选择

**选择方案 A（复用 Sui Event）+ 方案 B 的全量快照模式**

即：通过标准 Sui Event 推送链上订单簿全量快照，而非自定义 Streamer。

### 11.2 实施文件

| 文件 | 操作 | 内容 |
|------|------|------|
| `sui-types/src/dex_events.rs` | 新增 | OrderbookSnapshotEvent, PriceLevelSnapshot, OrderbookStats |
| `sui-types/src/dex/perpetual.rs` | 修改 | 添加 last_snapshot_timestamp_ms, snapshot_sequence_number 字段 |
| `sui-execution/src/dex/helpers.rs` | 新增 | generate_orderbook_snapshot() 函数 |
| `sui-execution/src/dex/commands/order.rs` | 修改 | 在 execute_place_order/cancel_order/cancel_all_orders 中添加快照发射 |

### 11.3 配置参数

| 参数 | 值 | 定义位置 |
|------|-----|----------|
| ORDERBOOK_SNAPSHOT_INTERVAL_MS | 250 | sui-types/src/dex/perpetual.rs |
| ORDERBOOK_SNAPSHOT_MAX_DEPTH | 100 | sui-types/src/dex/perpetual.rs |

### 11.4 关键设计决策

1. **触发方式**：交易触发（非定时器）
   - 每次订单操作时检查是否需要发射快照
   - 避免了链上定时器的复杂性

2. **序列号管理**：
   - `sequence_number`：快照序列号，单调递增
   - `checkpoint_sequence`：Checkpoint 序列号（目前为 TODO）

3. **时间戳来源**：
   - 目前使用 0 作为占位符
   - TODO：从执行上下文获取实际时间戳

---

## 12. 相关文档

- 事件定义：`05-event-definitions.md`
- 延迟分析：`08-realtime-latency-analysis.md`
- 设计决策：`04-design-decisions.md`
