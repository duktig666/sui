# Phase 6 Step 4: dex-api 集成设计文档

> 创建日期: 2026-02-27
> 状态: 设计确认

## 目标

将 dex-stream-indexer 的低延迟 L2 订单簿数据集成到 dex-api 的 REST 和 WebSocket 接口，实现 <50ms 端到端延迟，同时保持对 dex-indexer checkpoint 数据的自动降级。

## 架构概览

```
dex-stream-indexer
    │ XADD dex:stream:l2:update
    │ HSET dex:l2book:{id}, dex:bbo:{id}
    ↓
Redis ──── StreamConsumer (dex-api)
    │           │
    │           ├─ dex:stream:l2:update → HGETALL dex:l2book:{id}
    │           │   → 构造全量快照 → broadcast orderbook:{id} + bbo:{id}
    │           │
    │           └─ dex:stream:orderbook (checkpoint)
    │               → 检查 l2_active_markets → 若活跃则抑制
    │
    └──── REST handlers
              ├─ query_l2_book: dex:l2book:{id} → fallback dex:orderbook:{id}
              └─ query_all_mids: dex:bbo:{id} → fallback dex:orderbook:{id}
```

## 关键设计决策

| # | 决策 | 选择 | 理由 |
|---|------|------|------|
| 1 | 双源冲突处理 | 抑制 Checkpoint | l2:update 活跃时（30s 内有更新），跳过同 perpetual_id 的 checkpoint 广播，避免时间倒退 |
| 2 | WS 频道 | 复用 orderbook:{id} | 客户端无需修改订阅，无缝切换到低延迟数据源 |
| 3 | REST fallback | 超时降级（10s） | dex:l2book:{id}:meta.timestamp 超过 10s 视为过期，降级到 checkpoint |

## 组件设计

### 1. StreamConsumer 增强

**新增监听 Stream：**
- `dex:stream:l2:update`（第 9 个 stream）

**抑制机制：**
```rust
// StreamConsumer 新增字段
l2_active_markets: HashMap<u32, Instant>

// 处理 dex:stream:l2:update
fn broadcast_l2_book(&self, data: &Value, conn: &mut MultiplexedConnection) {
    let perpetual_id = data["perpetual_id"];
    // 1. 更新活跃标记
    self.l2_active_markets.insert(perpetual_id, Instant::now());
    // 2. HGETALL dex:l2book:{id}
    // 3. 解析 b:{price}/a:{price} 格式
    // 4. 构造全量快照，广播到 orderbook:{id} 和 bbo:{id}
}

// 处理 dex:stream:orderbook（修改现有逻辑）
fn broadcast_orderbook(&self, data: &Value) {
    let perpetual_id = data["perpetualId"];
    // 检查是否被 l2:update 抑制
    if let Some(last_update) = self.l2_active_markets.get(&perpetual_id) {
        if last_update.elapsed() < Duration::from_secs(30) {
            return; // 抑制，避免时间倒退
        }
    }
    // 原有逻辑不变
}
```

**L2Book 全量快照构造：**
- HGETALL `dex:l2book:{id}` 返回 `{b:50000: "1500", a:50100: "800", ...}`
- 解析 bid/ask 级别，按价格排序（bid 降序，ask 升序）
- 读取 `dex:bbo:{id}` 获取 BBO 数据
- 广播到 `orderbook:{id}` 和 `bbo:{id}` 频道

### 2. REST handlers.rs 改造

**query_l2_book() 双源 fallback：**
```
读取 dex:l2book:{id} (HGETALL)
  → 若有数据:
    读取 dex:l2book:{id}:meta (HMGET sequence, timestamp)
      → 若 timestamp 在 10s 内: 返回 dex-stream-indexer 数据
      → 若 timestamp 过期: fallback 到 dex:orderbook:{id}
  → 若为空: fallback 到 dex:orderbook:{id}
```

**allMids 增强：**
- 优先从 `dex:bbo:{id}` 计算 mid_price = (best_bid + best_ask) / 2
- Fallback 到 `dex:orderbook:{id}` 的 mid_price 字段

### 3. 数据格式映射

**dex-stream-indexer Redis → WS 全量快照：**

| dex:l2book:{id} 字段 | WS 消息字段 |
|----------------------|------------|
| `b:{price}` → qty | `levels[0]` bids (价格降序) |
| `a:{price}` → qty | `levels[1]` asks (价格升序) |

**dex:bbo:{id} → bbo:{id} 频道：**

| Redis 字段 | WS 消息字段 |
|-----------|------------|
| best_bid | bestBid |
| best_bid_qty | bestBidSize |
| best_ask | bestAsk |
| best_ask_qty | bestAskSize |
| (计算) | midPrice |

## 受影响文件

| 操作 | 文件 | 说明 |
|------|------|------|
| 修改 | `crates/dex-api/src/ws/consumer.rs` | 新增 l2:update 监听 + broadcast_l2_book + checkpoint 抑制 |
| 修改 | `crates/dex-api/src/handlers.rs` | query_l2_book 双源 fallback + allMids 增强 |

## 不变的部分

- WS 消息格式（全量快照 levels 格式不变）
- WS 订阅协议（subscribeChannel "orderbook:0" 不变）
- ChannelType / ServerMessage 类型定义
- 其他 REST endpoints（orderStatus、openOrders 等）

## 验证标准

| 指标 | 标准 |
|------|------|
| REST l2Book | 优先从 dex:l2book 返回，fallback 到 checkpoint |
| WS orderbook 推送 | l2:update 通知触发全量快照推送 |
| WS BBO 推送 | l2:update 同时推送 BBO |
| Checkpoint 抑制 | l2:update 活跃时 checkpoint 不推送同 perpetual_id |
| 自动降级 | dex-stream-indexer 离线 >30s 后 checkpoint 恢复工作 |
| 向后兼容 | 现有 WS 客户端无需修改 |
