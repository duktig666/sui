# Phase 6 Step 3: dex-stream-indexer 设计文档

> 创建日期: 2026-02-27
> 状态: 设计确认

## 目标

实现 dex-stream-indexer 独立服务，从 sui-node 的 gRPC streaming 接收 DEX 事件，维护内存 L2 订单簿，并以 <10ms 延迟写入 Redis 供 dex-api 推送 WebSocket。

## 架构概览

```
sui-node (DexStreamingManager gRPC :50052)
         │
         ↓ Subscribe(market_ids)
    dex-stream-indexer (独立二进制)
         │
         ├─ 接收 DexStreamBatch (delta + fill + order events)
         ├─ 内存 L2Book 维护 (BTreeMap<price, qty>)
         ├─ Gap 检测 → GetSnapshot 恢复
         │
         ↓ Pipeline flush (每 5ms)
    Redis
         ├─ HSET dex:l2book:{id} b:{price} {qty}
         ├─ HSET dex:bbo:{id} best_bid {price} ...
         └─ XADD dex:stream:l2:update * data {...}
                    │
                    ↓ XREAD
              dex-api (StreamConsumer)
                    │
                    ↓ WebSocket push
               前端客户端
```

## 关键设计决策

| # | 决策 | 选择 | 理由 |
|---|------|------|------|
| 1 | 数据来源 | gRPC（从 sui-node） | 满足 <50ms 延迟目标，Redis Streams 路径 1-3s 太慢 |
| 2 | WS 通知机制 | Redis Stream 通知 | 复用 dex-api 的 StreamConsumer 模式，XADD → XREAD |
| 3 | L2 存储格式 | HSET field=`b:{price}`/`a:{price}` | O(1) 单档更新，HDEL 清除空档 |

## 组件设计

### 1. Crate 结构

```
crates/dex-stream-indexer/
├── Cargo.toml
└── src/
    ├── main.rs               # CLI 入口，clap 参数
    ├── lib.rs                # 库入口
    ├── config.rs             # StreamerConfig
    ├── grpc_client.rs        # gRPC 客户端（Subscribe + GetSnapshot）
    ├── orderbook_builder.rs  # 内存 L2Book + delta 应用
    ├── bbo_tracker.rs        # BBO 变更检测
    └── redis_writer.rs       # Pipeline 批量写入
```

### 2. StreamerConfig

```rust
pub struct StreamerConfig {
    pub grpc_addr: String,              // gRPC server 地址（默认 http://127.0.0.1:50052）
    pub redis_url: String,              // Redis URL
    pub flush_interval_ms: u64,         // Redis flush 间隔（默认 5ms）
    pub reconcile_interval_secs: u64,   // 对账间隔（默认 30s）
    pub l2_stream_max_len: usize,       // Stream 最大长度（默认 10000）
    pub market_ids: Vec<u32>,           // 监控的市场列表
}
```

### 3. L2Book（内存订单簿）

```rust
pub struct L2Book {
    pub perpetual_id: u32,
    pub bids: BTreeMap<u64, u64>,   // price → quantity
    pub asks: BTreeMap<u64, u64>,
    pub sequence: u64,              // 最新已应用的 sequence
    pub updated_at: u64,            // 最近更新时间
    pub dirty: bool,                // 是否有未 flush 的变更
}
```

**Delta 应用逻辑：**
- 检查 `delta.sequence == book.sequence + 1`
- 若 gap：标记 stale → 调用 GetSnapshot 恢复
- 应用每个 OrderbookDelta：
  - quantity > 0 → insert/update BTreeMap entry
  - quantity == 0 → remove entry
- 标记 dirty = true

### 4. BBO Tracker

```rust
pub struct BboSnapshot {
    pub best_bid: Option<(u64, u64)>,   // (price, qty)
    pub best_ask: Option<(u64, u64)>,
}
```

比较 flush 前后的 BBO，仅变化时写入 `dex:bbo:{id}`。

### 5. Redis 存储方案

| Key | 类型 | 字段 | 说明 |
|-----|------|------|------|
| `dex:l2book:{id}` | HSET | `b:{price}` → qty, `a:{price}` → qty | L2 档位数据 |
| `dex:l2book:{id}:meta` | HSET | `sequence`, `timestamp` | 元数据 |
| `dex:bbo:{id}` | HSET | `best_bid`, `best_bid_qty`, `best_ask`, `best_ask_qty` | 最优价格 |
| `dex:stream:l2:update` | Stream | `data` → JSON | 通知 dex-api |

**写入模式**（pipeline 批量）：
```redis
HSET dex:l2book:0 b:50000 1500
HDEL dex:l2book:0 a:50100
HSET dex:l2book:0:meta sequence 42 timestamp 1709000000000
HSET dex:bbo:0 best_bid 50000 best_bid_qty 1500 best_ask 50200 best_ask_qty 800
XADD dex:stream:l2:update MAXLEN ~ 10000 * perpetual_id 0 type l2_update
```

### 6. 主运行循环

```rust
loop {
    tokio::select! {
        batch = grpc_stream.message() => {
            for event in batch.events {
                match event {
                    OrderbookDelta(d) => builder.apply_delta(d),
                    Fill(_) | OrderUpdate(_) | ... => { /* 暂不处理 */ }
                }
            }
        }
        _ = flush_interval.tick() => {
            builder.flush_dirty_books(&redis_writer).await;
        }
    }
}
```

### 7. 故障恢复

| 场景 | 处理 |
|------|------|
| Sequence gap | GetSnapshot → 重置 L2Book → flush Redis |
| gRPC 断连 | 指数退避重连 → 重连后所有市场 GetSnapshot |
| dex-stream-indexer 重启 | 启动时所有市场 GetSnapshot 初始化 |
| Redis 断连 | 重连后全量 flush 所有 L2Book |

### 8. dex-api 集成

dex-api 的 StreamConsumer 需新增监听 `dex:stream:l2:update`：
- XREAD 收到通知后，从 `dex:l2book:{id}` HGETALL 读取完整 L2 快照
- 构造 WS 消息推送给 `l2Book:{perpetual_id}` 订阅者
- 优先使用 dex-stream-indexer 数据（低延迟），降级到 dex-indexer 的 checkpoint 快照

## 受影响文件总览

| 操作 | 文件/目录 | 说明 |
|------|----------|------|
| 新建 | `crates/dex-stream-indexer/` | 新 crate（独立二进制） |
| 修改 | 根 `Cargo.toml` | workspace members 新增 |
| 修改 | `crates/dex-api/src/ws/consumer.rs` | 新增 l2:update stream 监听 |
| 修改 | `docker/dex-dev/docker-compose.yml` | 新增 dex-stream-indexer 服务 |

## 延迟预估

| 阶段 | 延迟 |
|------|------|
| DEX 引擎 → gRPC 推送 | ~1-5ms |
| gRPC → dex-stream-indexer 接收 | ~1ms |
| 内存 L2Book 更新 | <0.01ms |
| Buffer → Redis pipeline | ~2-5ms |
| Redis → dex-api WS 推送 | ~5-10ms |
| **总计（引擎 → 前端）** | **<20-50ms** |

## 验证标准

| 指标 | 标准 |
|------|------|
| Delta 应用正确 | 增量正确应用到 L2Book |
| Gap 检测 | 序列号不连续时触发 GetSnapshot |
| Redis 数据正确 | `dex:l2book:{id}` 准确反映内存状态 |
| BBO 更新 | 仅 BBO 变化时写入 |
| 端到端链路 | gRPC → 内存 → Redis → dex-api WS 连通 |
| 引擎到前端延迟 | <50ms |
