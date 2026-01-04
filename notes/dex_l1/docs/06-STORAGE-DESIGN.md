# DEX L1 存储层设计 / Storage Layer Design

> **版本**: v1.1
> **状态**: Draft
> **最后更新**: 2025-12-31
> **目标读者**: 技术评审 / 架构师

---

## 1. 概述 / Overview

### 1.1 设计原则 / Design Principles

1. **复用 typed-store**: 不自己封装 RocksDB
2. **分层架构**: 内存缓存 → WAL → 快照 → 持久化
3. **异步持久化**: 不阻塞主路径
4. **快速恢复**: < 5 分钟 RTO

---

## 2. 复用 Sui 存储基础设施 / Reusing Sui Storage

### 2.1 typed-store 复用 / typed-store Reuse

```rust
// ✅ 使用 typed-store
use typed_store::rocks::DBMap;
use typed_store_derive::DBMapUtils;

#[derive(DBMapUtils)]
pub struct DexTables {
    /// 订单存储
    pub orders: DBMap<OrderId, Order>,
    /// 余额存储
    pub balances: DBMap<(AccountId, AssetId), Balance>,
    /// 成交记录
    pub trades: DBMap<TradeId, Trade>,
    /// 市场配置
    pub markets: DBMap<MarketId, MarketConfig>,
    /// 序列号
    pub sequences: DBMap<SeqNumber, TxDigest>,
}

// ❌ 禁止自己封装 RocksDB
use rocksdb::DB; // 禁止！
```

### 2.2 sui-storage 复用 / sui-storage Reuse

```rust
// 复用 ShardedLRU 缓存
use sui_storage::sharded_lru::ShardedLruCache;

pub struct StateCache {
    balances: ShardedLruCache<(AccountId, AssetId), Balance>,
    orders: ShardedLruCache<OrderId, Order>,
}
```

---

## 3. 分层存储架构 / Layered Storage Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Storage Layers                            │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌─────────────────────────────────────────────────────────┐│
│  │ L1: StateCache (DashMap)                                ││
│  │     热数据内存缓存                                       ││
│  │     • 订单簿                                             ││
│  │     • 活跃余额                                           ││
│  │     延迟: < 1μs                                          ││
│  └─────────────────────────────────────────────────────────┘│
│                           │                                  │
│                           ▼                                  │
│  ┌─────────────────────────────────────────────────────────┐│
│  │ L2: WAL (dex-storage)                                   ││
│  │     写前日志，DEX 专用                                   ││
│  │     • 顺序写入                                           ││
│  │     • 批量 fsync                                         ││
│  │     延迟: < 10ms                                         ││
│  └─────────────────────────────────────────────────────────┘│
│                           │                                  │
│                           ▼                                  │
│  ┌─────────────────────────────────────────────────────────┐│
│  │ L3: Snapshot (dex-storage)                              ││
│  │     定期快照                                             ││
│  │     • 每 N 条记录                                        ││
│  │     • 压缩存储 (LZ4)                                     ││
│  └─────────────────────────────────────────────────────────┘│
│                           │                                  │
│                           ▼                                  │
│  ┌─────────────────────────────────────────────────────────┐│
│  │ L4: typed-store (RocksDB)                               ││
│  │     持久化层 (复用 Sui)                                  ││
│  │     • KV 存储                                            ││
│  │     • 自动压缩                                           ││
│  └─────────────────────────────────────────────────────────┘│
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

---

## 4. WAL 设计 / Write-Ahead Log Design

### 4.1 WAL 结构 / WAL Structure

```
┌─────────────────────────────────────────────────────────────┐
│                      WAL File Format                         │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌─────────────────────────────────────────────────────────┐│
│  │ Header (64 bytes)                                       ││
│  │ ┌──────────┬──────────┬──────────┬──────────┐          ││
│  │ │ Magic    │ Version  │ SeqStart │ Checksum │          ││
│  │ │ (8)      │ (4)      │ (8)      │ (8)      │          ││
│  │ └──────────┴──────────┴──────────┴──────────┘          ││
│  └─────────────────────────────────────────────────────────┘│
│                                                              │
│  ┌─────────────────────────────────────────────────────────┐│
│  │ Entry 1                                                 ││
│  │ ┌──────────┬──────────┬──────────┬──────────┐          ││
│  │ │ SeqNo    │ Length   │ Data     │ CRC32    │          ││
│  │ │ (8)      │ (4)      │ (var)    │ (4)      │          ││
│  │ └──────────┴──────────┴──────────┴──────────┘          ││
│  └─────────────────────────────────────────────────────────┘│
│                                                              │
│  ┌─────────────────────────────────────────────────────────┐│
│  │ Entry 2                                                 ││
│  │ ...                                                     ││
│  └─────────────────────────────────────────────────────────┘│
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

### 4.2 WAL 实现 / WAL Implementation

```rust
pub struct WriteAheadLog {
    /// 当前文件
    current_file: File,
    /// 当前序列号
    current_seq: AtomicU64,
    /// 写入缓冲区
    buffer: Vec<u8>,
    /// 配置
    config: WalConfig,
}

pub struct WalConfig {
    /// WAL 目录
    pub dir: PathBuf,
    /// 单文件最大大小
    pub max_file_size: u64,      // 默认: 64MB
    /// 同步间隔
    pub sync_interval: Duration,  // 默认: 10ms
    /// 批量大小
    pub batch_size: usize,        // 默认: 100
}

impl WriteAheadLog {
    /// 追加记录 (批量写入)
    pub fn append(&mut self, entry: WalEntry) -> Result<SeqNumber> {
        let seq = self.current_seq.fetch_add(1, Ordering::SeqCst);

        // 序列化到缓冲区
        self.buffer.extend_from_slice(&entry.serialize());

        // 达到批量大小时刷盘
        if self.buffer.len() >= self.config.batch_size {
            self.flush()?;
        }

        Ok(SeqNumber(seq))
    }

    /// 刷盘 (Group Commit)
    pub fn flush(&mut self) -> Result<()> {
        self.current_file.write_all(&self.buffer)?;
        self.current_file.sync_data()?;
        self.buffer.clear();
        Ok(())
    }
}
```

### 4.3 WAL 优化 / WAL Optimization

| 优化 | 技术 | 效果 |
|-----|------|------|
| Group Commit | 批量写入 | 减少 fsync 次数 |
| 预分配文件 | fallocate | 减少文件扩展开销 |
| 顺序写入 | append-only | 最大化磁盘吞吐 |
| 异步刷盘 | tokio::fs | 不阻塞主路径 |

---

## 5. 快照机制 / Snapshot Mechanism

### 5.1 快照结构 / Snapshot Structure

```rust
pub struct Snapshot {
    /// 快照序列号
    pub seq: SeqNumber,
    /// 时间戳
    pub timestamp: u64,
    /// 订单簿状态
    pub orderbooks: HashMap<MarketId, OrderBookSnapshot>,
    /// 余额状态
    pub balances: HashMap<(AccountId, AssetId), Balance>,
    /// 校验和
    pub checksum: [u8; 32],
}

pub struct OrderBookSnapshot {
    pub market_id: MarketId,
    pub bids: Vec<(Price, Vec<Order>)>,
    pub asks: Vec<(Price, Vec<Order>)>,
}
```

### 5.2 快照策略 / Snapshot Strategy

```
┌─────────────────────────────────────────────────────────────┐
│                    Snapshot Strategy                         │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  触发条件 (任一满足):                                        │
│  1. WAL 条目数 >= 10,000                                    │
│  2. WAL 文件大小 >= 64MB                                    │
│  3. 时间间隔 >= 5 分钟                                      │
│                                                              │
│  快照流程:                                                   │
│  ┌────────────┐     ┌────────────┐     ┌────────────┐       │
│  │ 1. 暂停    │────►│ 2. 序列化  │────►│ 3. 压缩    │       │
│  │    写入    │     │    状态    │     │    (LZ4)   │       │
│  └────────────┘     └────────────┘     └────────────┘       │
│                                              │               │
│                                              ▼               │
│  ┌────────────┐     ┌────────────┐     ┌────────────┐       │
│  │ 6. 恢复    │◄────│ 5. 删除旧  │◄────│ 4. 写入    │       │
│  │    写入    │     │    WAL     │     │    磁盘    │       │
│  └────────────┘     └────────────┘     └────────────┘       │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

### 5.3 增量快照 / Incremental Snapshot

```rust
pub struct IncrementalSnapshot {
    /// 基础快照引用
    pub base_snapshot_id: SnapshotId,
    /// 增量变更
    pub changes: Vec<StateChange>,
}

pub enum StateChange {
    OrderAdded(Order),
    OrderRemoved(OrderId),
    BalanceUpdated(AccountId, AssetId, Balance),
    TradeExecuted(Trade),
}
```

---

## 6. 恢复流程 / Recovery Flow

### 6.1 启动恢复 / Startup Recovery

```
┌─────────────────────────────────────────────────────────────┐
│                    Recovery Flow                             │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌────────────┐                                             │
│  │ 1. 加载    │ → 找到最新的有效快照                        │
│  │    快照    │                                             │
│  └─────┬──────┘                                             │
│        │                                                    │
│        ▼                                                    │
│  ┌────────────┐                                             │
│  │ 2. 验证    │ → 校验 checksum                             │
│  │    校验和  │                                             │
│  └─────┬──────┘                                             │
│        │                                                    │
│        ▼                                                    │
│  ┌────────────┐                                             │
│  │ 3. 恢复    │ → 将快照状态加载到内存                      │
│  │    状态    │                                             │
│  └─────┬──────┘                                             │
│        │                                                    │
│        ▼                                                    │
│  ┌────────────┐                                             │
│  │ 4. 重放    │ → 应用快照后的 WAL 记录                     │
│  │    WAL     │                                             │
│  └─────┬──────┘                                             │
│        │                                                    │
│        ▼                                                    │
│  ┌────────────┐                                             │
│  │ 5. 验证    │ → 确认状态一致性                            │
│  │    一致性  │                                             │
│  └─────┬──────┘                                             │
│        │                                                    │
│        ▼                                                    │
│  ┌────────────┐                                             │
│  │ 6. 服务    │ → 开始接受请求                              │
│  │    就绪    │                                             │
│  └────────────┘                                             │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

### 6.2 一致性验证 / Consistency Verification

```rust
pub fn verify_consistency(state: &DexState) -> Result<()> {
    // 1. 验证余额守恒
    let total_balances: HashMap<AssetId, u64> = state
        .balances
        .iter()
        .fold(HashMap::new(), |mut acc, (_, balance)| {
            *acc.entry(balance.asset).or_default() += balance.total();
            acc
        });

    // 2. 验证订单簿余额匹配
    for (market_id, orderbook) in &state.orderbooks {
        let locked_base: u64 = orderbook
            .asks
            .values()
            .flat_map(|level| &level.orders)
            .map(|o| o.remaining)
            .sum();

        let locked_quote: u64 = orderbook
            .bids
            .values()
            .flat_map(|level| &level.orders)
            .map(|o| o.remaining * o.price)
            .sum();

        // 验证锁定金额与订单簿一致
    }

    // 3. 验证序列号连续
    verify_sequence_continuity(&state.sequences)?;

    Ok(())
}
```

---

## 7. 持久化策略权衡 / Persistence Trade-offs

> **重要**：持久化策略与确认语义紧密相关，详见 `02-ARCHITECTURE-OVERVIEW.md` **ADR-006: 确认语义与持久化等级**。

### 7.1 策略对比 / Strategy Comparison

| 策略 | 延迟 | 持久性 | RPO | 适用场景 | 确认级别 |
|-----|------|-------|-----|---------|---------|
| 同步 WAL (fsync) | 高 (~20ms) | 最强 | = 0 | Hard Confirmation | 硬确认 |
| 异步 WAL (batch) | 低 (~5ms) | 强 | 不保证 | Soft Confirmation | 软确认 |
| 纯内存 | 最低 | 弱 | 不保证 | 测试环境 | - |

**与 ADR-006 的映射**：
- **Soft Confirmation (< 50ms)**：使用异步 WAL，写入已启动但不保证 fsync 完成
- **Hard Confirmation (< 100ms)**：使用同步 WAL，2f+1 节点 fsync 完成后才返回 ack

### 7.2 配置建议 / Configuration Recommendations

```toml
[dex.storage]
# WAL 配置
wal_dir = "/data/dex/wal"
wal_max_file_size = 67108864  # 64MB
wal_sync_interval_ms = 10     # 10ms
wal_batch_size = 100

# 快照配置
snapshot_dir = "/data/dex/snapshots"
snapshot_interval = 10000      # 每 10000 条 WAL
snapshot_retention = 3         # 保留最近 3 个

# 缓存配置
cache_size_mb = 1024           # 1GB 缓存
cache_shards = 64              # 64 分片
```

---

## 8. 关键数据结构 / Key Data Structures

```rust
/// DEX 存储管理器
pub struct DexStorage {
    /// 内存缓存
    cache: StateCache,
    /// 写前日志
    wal: WriteAheadLog,
    /// 快照管理
    snapshots: SnapshotManager,
    /// 持久化表
    tables: DexTables,
}

/// 状态缓存
pub struct StateCache {
    orderbooks: DashMap<MarketId, OrderBook>,
    balances: DashMap<(AccountId, AssetId), Balance>,
    orders: DashMap<OrderId, Order>,
}

/// 快照管理器
pub struct SnapshotManager {
    dir: PathBuf,
    current: AtomicU64,
    retention: usize,
}
```

---

## 9. 性能指标 / Performance Metrics

| 指标 | 目标 | 说明 |
|-----|------|------|
| 缓存读取 | < 1μs | DashMap 访问 |
| WAL 写入 | < 1ms | 批量写入 |
| WAL 刷盘 | < 10ms | Group Commit |
| 快照创建 | < 1s | 增量 + 压缩 |
| 恢复时间 | < 5min | RTO 目标 |

```rust
lazy_static! {
    pub static ref CACHE_HIT_RATE: Gauge = register_gauge!(
        "dex_cache_hit_rate",
        "Cache hit rate"
    ).unwrap();

    pub static ref WAL_WRITE_LATENCY: Histogram = register_histogram!(
        "dex_wal_write_latency_seconds",
        "WAL write latency"
    ).unwrap();

    pub static ref SNAPSHOT_SIZE: Gauge = register_gauge!(
        "dex_snapshot_size_bytes",
        "Latest snapshot size"
    ).unwrap();
}
```

---

## 变更历史 / Change History

| 版本 | 日期 | 变更内容 | 状态 |
|-----|------|---------|------|
| v1.0 | 2025-12-31 | 初始版本 | ✅ 有效 |
| v1.1 | 2025-12-31 | 7.1 持久化策略引用 ADR-006 | ✅ 有效 |

### 待对齐事项 / Alignment Notes

| 章节 | 状态 | 说明 |
|-----|------|------|
| 7.1 持久化策略 | ✅ 有效 | 与 ADR-006 确认语义对齐 |
| 5. WAL 设计 | ✅ 有效 | Group commit 支持 Hard confirmation |
| 8. 恢复流程 | ⚠️ 待验证 | RTO <5min 需实际测试 |

---

*文档版本: v1.1 | 最后更新: 2025-12-31*
