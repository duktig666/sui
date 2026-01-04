# 10-PERFORMANCE-DESIGN.md
# 性能优化设计 / Performance Optimization Design

> **文档状态 / Status**: 详细设计 / Detailed Design
> **版本**: v1.1
> **最后更新 / Last Updated**: 2025-12-31
> **关联文档 / Related**: 04-SEQUENCER-DESIGN.md, 05-MATCHING-ENGINE-DESIGN.md, 06-STORAGE-DESIGN.md

---

## 目录 / Table of Contents

1. [性能目标 / Performance Goals](#1-性能目标--performance-goals)
2. [延迟分解 / Latency Breakdown](#2-延迟分解--latency-breakdown)
3. [撮合引擎优化 / Matching Engine](#3-撮合引擎优化--matching-engine)
4. [Sequencer 优化 / Sequencer](#4-sequencer-优化--sequencer)
5. [存储层优化 / Storage Layer](#5-存储层优化--storage-layer)
6. [内存管理 / Memory Management](#6-内存管理--memory-management)
7. [并发优化 / Concurrency](#7-并发优化--concurrency)
8. [网络优化 / Network](#8-网络优化--network)
9. [监控调优 / Monitoring](#9-监控调优--monitoring)
10. [硬件选型 / Hardware](#10-硬件选型--hardware)
11. [测试策略 / Testing Strategy](#11-测试策略--testing-strategy)

---

## 1. 性能目标 / Performance Goals

### 1.1 核心指标 / Key Metrics

| 指标 | 目标值 | 挑战级别 | 说明 |
|-----|-------|---------|------|
| **端到端延迟 P99** | < 50ms | 高 | 从订单提交到确认完成 |
| **撮合延迟 P99** | < 10μs | 中 | 单次撮合操作 |
| **峰值吞吐量** | ≥ 200,000 TPS | 极高 | 持续处理能力 |
| **软确认延迟** | < 50ms | 中 | Sequencer 确认 |
| **硬确认延迟** | < 100ms | 中 | 2f+1 验证者确认 |

### 1.2 性能预算分配 / Latency Budget

```
┌─────────────────────────────────────────────────────────┐
│                50ms 端到端延迟预算                       │
├─────────────────────────────────────────────────────────┤
│                                                         │
│  网络接收解析     ████                    5ms  (10%)    │
│  Sequencer 排序   ████                    5ms  (10%)    │
│  撮合执行         ████████               10ms  (20%)    │
│  状态更新         ████████               10ms  (20%)    │
│  持久化           ████████               10ms  (20%)    │
│  广播确认         ████████               10ms  (20%)    │
│                                                         │
│  总计             ████████████████████████ 50ms (100%)  │
│                                                         │
└─────────────────────────────────────────────────────────┘
```

### 1.3 吞吐量分析 / Throughput Analysis

**20 万 TPS 达成策略**:

```
目标: 200,000 TPS = 5μs/tx (单核上限)

实际策略: 批量 + 并行
├── 单核撮合: ~100,000 TPS (10μs/tx)
├── 批量处理: 1000 tx/batch
│   └── 有效延迟分摊: 10ms / 1000 = 10μs/tx
├── 多市场并行: 4+ 市场独立线程
│   └── 理论: 4 × 100K = 400K TPS
└── 异步持久化: 不阻塞主路径
    └── WAL 批量写入 + Group Commit
```

---

## 2. 延迟分解 / Latency Breakdown

### 2.1 关键路径分析 / Critical Path

```
┌─────────────────────────────────────────────────────────┐
│                   关键路径时序                           │
├─────────────────────────────────────────────────────────┤
│                                                         │
│  T0: 订单到达网络接口                                    │
│      │                                                  │
│      ▼ (1-2ms) 网络接收 + 反序列化                       │
│                                                         │
│  T1: 进入 Sequencer 队列                                │
│      │                                                  │
│      ▼ (1-3ms) 序列号分配 + 签名验证                     │
│                                                         │
│  T2: 订单路由到撮合引擎                                  │
│      │                                                  │
│      ▼ (5-10μs) 订单簿查找 + 撮合                       │
│                                                         │
│  T3: 成交结算                                           │
│      │                                                  │
│      ▼ (1-5μs) 余额更新 + 手续费计算                     │
│                                                         │
│  T4: 状态持久化                                         │
│      │                                                  │
│      ▼ (1-5ms) WAL 写入 (可并行)                        │
│                                                         │
│  T5: 结果广播                                           │
│      │                                                  │
│      ▼ (5-10ms) 网络广播 + 确认收集                      │
│                                                         │
│  T6: 最终确认 (< 50ms)                                  │
│                                                         │
└─────────────────────────────────────────────────────────┘
```

### 2.2 延迟热点识别 / Latency Hotspots

| 热点 | 预期延迟 | 优化策略 |
|-----|---------|---------|
| 网络 I/O | 5-10ms | 零拷贝、连接池 |
| 序列化 | 1-2ms | 预分配 buffer、避免动态分配 |
| 锁竞争 | 0-5ms | 无锁设计、分片 |
| 磁盘 I/O | 1-10ms | 异步、批量、mmap |
| 内存分配 | 0.1-1ms | 对象池、Arena |

---

## 3. 撮合引擎优化 / Matching Engine

### 3.1 数据结构优化 / Data Structure

**订单簿设计权衡**:

| 结构 | 插入 | 删除 | 查找最优 | 内存 | 选择 |
|-----|------|-----|---------|------|------|
| BTreeMap | O(log n) | O(log n) | O(1) | 中 | ✅ Phase 1 |
| Skip List | O(log n) | O(log n) | O(1) | 高 | 备选 |
| 定制 B+Tree | O(log n) | O(log n) | O(1) | 低 | Phase 2 |

**BTreeMap 优化**:

```rust
// 使用 Decimal 作为 key 的优化
// 预计算 hash，避免重复计算
pub struct OptimizedOrderBook {
    // 价格层级 -> 订单队列
    // BTreeMap 保证价格有序
    bids: BTreeMap<Price, OrderQueue>,
    asks: BTreeMap<Price, OrderQueue>,

    // 最优价格缓存
    best_bid: Option<Price>,
    best_ask: Option<Price>,

    // 订单索引 (O(1) 查找)
    order_index: HashMap<OrderId, OrderRef>,
}
```

### 3.2 Cache-Friendly 布局 / Cache Optimization

```rust
// 热数据紧凑布局，最大化 cache 命中
#[repr(C)]
pub struct OrderCompact {
    // 64 bytes = 1 cache line
    pub id: u64,           // 8
    pub price: u64,        // 8 (定点数)
    pub quantity: u64,     // 8
    pub filled: u64,       // 8
    pub account_id: u64,   // 8
    pub market_id: u32,    // 4
    pub side: u8,          // 1
    pub order_type: u8,    // 1
    pub tif: u8,           // 1
    pub status: u8,        // 1
    pub _padding: [u8; 16], // 16 (对齐)
}

static_assert!(std::mem::size_of::<OrderCompact>() == 64);
```

### 3.3 撮合算法优化 / Matching Algorithm

```rust
impl MatchingEngine {
    /// 批量撮合优化
    /// 单次调用处理多个订单，减少函数调用开销
    #[inline(always)]
    pub fn match_batch(&mut self, orders: &[Order]) -> Vec<Trade> {
        let mut trades = Vec::with_capacity(orders.len() * 2);

        for order in orders {
            // 内联撮合逻辑，避免函数调用
            let matched = self.match_single_inline(order);
            trades.extend(matched);
        }

        trades
    }

    /// 内联单订单撮合
    #[inline(always)]
    fn match_single_inline(&mut self, order: &Order) -> SmallVec<[Trade; 4]> {
        // SmallVec 避免小数量时的堆分配
        let mut trades = SmallVec::new();

        let opposite_book = match order.side {
            Side::Buy => &mut self.asks,
            Side::Sell => &mut self.bids,
        };

        let mut remaining = order.quantity;

        // 避免迭代器分配
        while remaining > Decimal::ZERO {
            let best = match order.side {
                Side::Buy => opposite_book.first_entry(),
                Side::Sell => opposite_book.last_entry(),
            };

            let Some(mut entry) = best else { break };

            // 价格检查 (分支预测友好)
            if !self.price_matches(order, entry.key()) {
                break;
            }

            // 撮合执行
            let (trade, consumed) = self.execute_match(order, entry.get_mut(), remaining);
            remaining -= consumed;
            trades.push(trade);

            if entry.get().is_empty() {
                entry.remove();
            }
        }

        trades
    }
}
```

### 3.4 SIMD 优化 / SIMD Acceleration

```rust
#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

/// SIMD 价格比较 (4 个价格并行比较)
#[inline]
#[target_feature(enable = "avx2")]
unsafe fn compare_prices_simd(
    prices: &[u64; 4],
    target: u64,
) -> u32 {
    let prices_vec = _mm256_loadu_si256(prices.as_ptr() as *const __m256i);
    let target_vec = _mm256_set1_epi64x(target as i64);

    // 并行比较 4 个价格
    let cmp = _mm256_cmpgt_epi64(prices_vec, target_vec);

    // 提取比较结果
    _mm256_movemask_pd(_mm256_castsi256_pd(cmp)) as u32
}

/// 批量数量计算 (向量化)
#[inline]
#[target_feature(enable = "avx2")]
unsafe fn sum_quantities_simd(quantities: &[u64]) -> u64 {
    let mut sum = _mm256_setzero_si256();

    for chunk in quantities.chunks_exact(4) {
        let vals = _mm256_loadu_si256(chunk.as_ptr() as *const __m256i);
        sum = _mm256_add_epi64(sum, vals);
    }

    // 水平求和
    let low = _mm256_extracti128_si256(sum, 0);
    let high = _mm256_extracti128_si256(sum, 1);
    let sum128 = _mm_add_epi64(low, high);

    _mm_extract_epi64(sum128, 0) as u64 + _mm_extract_epi64(sum128, 1) as u64
}
```

---

## 4. Sequencer 优化 / Sequencer

### 4.1 序列号分配 / Sequence Number Allocation

```rust
use std::sync::atomic::{AtomicU64, Ordering};

pub struct SequenceAllocator {
    // 原子计数器，无锁分配
    counter: AtomicU64,

    // 批量预分配 (减少原子操作)
    batch_size: u64,
}

impl SequenceAllocator {
    /// 批量分配序列号
    /// 一次性获取多个序列号，减少原子操作次数
    #[inline]
    pub fn allocate_batch(&self, count: u64) -> SequenceRange {
        let start = self.counter.fetch_add(count, Ordering::Relaxed);
        SequenceRange { start, count }
    }

    /// 单个分配 (热路径)
    #[inline(always)]
    pub fn allocate_one(&self) -> u64 {
        self.counter.fetch_add(1, Ordering::Relaxed)
    }
}
```

### 4.2 批次聚合 / Batch Aggregation

```rust
pub struct BatchAggregator {
    /// 批次缓冲区 (预分配)
    buffer: Vec<Transaction>,

    /// 批次大小阈值
    batch_size: usize,  // 如 1000

    /// 最大等待时间
    max_wait: Duration,  // 如 1ms

    /// 上次刷新时间
    last_flush: Instant,
}

impl BatchAggregator {
    /// 添加交易到批次
    #[inline]
    pub fn add(&mut self, tx: Transaction) -> Option<Batch> {
        self.buffer.push(tx);

        // 达到批次大小或超时
        if self.buffer.len() >= self.batch_size
            || self.last_flush.elapsed() >= self.max_wait
        {
            Some(self.flush())
        } else {
            None
        }
    }

    #[inline]
    fn flush(&mut self) -> Batch {
        self.last_flush = Instant::now();
        let txs = std::mem::replace(&mut self.buffer, Vec::with_capacity(self.batch_size));
        Batch::new(txs)
    }
}
```

### 4.3 网络优化 / Network Optimization

```rust
// TCP 优化配置
pub struct NetworkConfig {
    /// 禁用 Nagle 算法，减少延迟
    pub tcp_nodelay: bool,  // true

    /// 快速确认
    pub tcp_quickack: bool,  // true

    /// 发送缓冲区大小
    pub send_buffer_size: usize,  // 256KB

    /// 接收缓冲区大小
    pub recv_buffer_size: usize,  // 256KB

    /// 连接池大小
    pub connection_pool_size: usize,  // 100
}

// 零拷贝发送
pub async fn send_zero_copy(socket: &TcpStream, data: &[u8]) -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        use nix::sys::socket::MsgFlags;
        // 使用 sendfile / splice 零拷贝
        socket.send_with_flags(data, MsgFlags::MSG_ZEROCOPY)?;
    }
    Ok(())
}
```

---

## 5. 存储层优化 / Storage Layer

### 5.1 WAL 优化 / WAL Optimization

```rust
pub struct OptimizedWAL {
    /// 内存映射文件
    mmap: MmapMut,

    /// 写入位置
    write_pos: AtomicU64,

    /// Group Commit 缓冲
    commit_buffer: Mutex<Vec<LogEntry>>,

    /// 刷盘策略
    sync_policy: SyncPolicy,
}

/// 同步策略（见 02-ARCHITECTURE-OVERVIEW.md ADR-006）
pub enum SyncPolicy {
    /// 每次写入同步 (用于 Hard Confirmation，RPO=0)
    EveryWrite,

    /// 批量同步 (用于 Soft Confirmation，低延迟优先)
    Batched { interval: Duration, max_size: usize },

    /// 周期同步 (仅测试环境)
    Periodic { interval: Duration },
}

impl OptimizedWAL {
    /// Group Commit: 批量写入
    pub async fn commit_batch(&self, entries: Vec<LogEntry>) -> Result<()> {
        // 1. 序列化到连续内存
        let data = self.serialize_batch(&entries)?;

        // 2. 单次 mmap 写入
        let pos = self.write_pos.fetch_add(data.len() as u64, Ordering::SeqCst);
        self.mmap[pos as usize..pos as usize + data.len()].copy_from_slice(&data);

        // 3. 根据策略决定是否立即 fsync
        if matches!(self.sync_policy, SyncPolicy::EveryWrite) {
            self.mmap.flush()?;
        }

        Ok(())
    }
}
```

### 5.2 快照优化 / Snapshot Optimization

```rust
pub struct SnapshotManager {
    /// 压缩算法
    compression: Compression,

    /// 并行序列化线程数
    parallelism: usize,
}

pub enum Compression {
    None,
    LZ4 { level: i32 },      // 快速压缩
    Zstd { level: i32 },     // 高压缩比
}

impl SnapshotManager {
    /// 增量快照
    /// 只序列化变更部分，大幅减少 I/O
    pub async fn create_incremental(
        &self,
        base: &Snapshot,
        changes: &StateChanges,
    ) -> Result<Snapshot> {
        // 1. 计算差异
        let delta = self.compute_delta(base, changes)?;

        // 2. 并行压缩
        let compressed = self.compress_parallel(&delta).await?;

        // 3. 写入存储
        let path = self.write_snapshot(&compressed).await?;

        Ok(Snapshot { path, base: Some(base.id), delta_size: compressed.len() })
    }

    /// 并行序列化
    async fn compress_parallel(&self, data: &[u8]) -> Result<Vec<u8>> {
        let chunk_size = data.len() / self.parallelism;

        let handles: Vec<_> = data
            .chunks(chunk_size)
            .map(|chunk| {
                let chunk = chunk.to_vec();
                let compression = self.compression.clone();
                tokio::spawn(async move {
                    compress_chunk(&chunk, &compression)
                })
            })
            .collect();

        let results = futures::future::join_all(handles).await;
        // ... 合并结果
        Ok(vec![])
    }
}
```

### 5.3 状态缓存优化 / State Cache

```rust
use dashmap::DashMap;

pub struct StateCache {
    /// 分片缓存 (DashMap 自动分片)
    balances: DashMap<(AccountId, AssetId), Balance>,
    orders: DashMap<OrderId, Order>,

    /// LRU 淘汰 (热点数据优先)
    lru: Mutex<LruCache<OrderId, ()>>,

    /// 预热列表
    warmup_list: Vec<OrderId>,
}

impl StateCache {
    /// 批量预热
    pub async fn warmup(&self, keys: &[OrderId]) {
        // 并行加载热点数据
        let futures: Vec<_> = keys
            .iter()
            .map(|key| self.load_from_storage(*key))
            .collect();

        let results = futures::future::join_all(futures).await;

        for (key, result) in keys.iter().zip(results) {
            if let Ok(order) = result {
                self.orders.insert(*key, order);
            }
        }
    }

    /// 无锁读取
    #[inline]
    pub fn get_balance(&self, account: AccountId, asset: AssetId) -> Option<Balance> {
        self.balances.get(&(account, asset)).map(|r| r.clone())
    }
}
```

---

## 6. 内存管理 / Memory Management

### 6.1 对象池 / Object Pool

```rust
use crossbeam_queue::ArrayQueue;

pub struct ObjectPool<T> {
    pool: ArrayQueue<T>,
    factory: fn() -> T,
}

impl<T> ObjectPool<T> {
    pub fn new(capacity: usize, factory: fn() -> T) -> Self {
        let pool = ArrayQueue::new(capacity);
        // 预分配对象
        for _ in 0..capacity {
            let _ = pool.push(factory());
        }
        Self { pool, factory }
    }

    /// 获取对象 (优先从池中获取)
    #[inline]
    pub fn acquire(&self) -> PooledObject<T> {
        let obj = self.pool.pop().unwrap_or_else(|| (self.factory)());
        PooledObject { obj: Some(obj), pool: self }
    }
}

pub struct PooledObject<'a, T> {
    obj: Option<T>,
    pool: &'a ObjectPool<T>,
}

impl<T> Drop for PooledObject<'_, T> {
    fn drop(&mut self) {
        if let Some(obj) = self.obj.take() {
            // 归还到池中 (忽略失败)
            let _ = self.pool.pool.push(obj);
        }
    }
}
```

### 6.2 Arena 分配器 / Arena Allocator

```rust
use bumpalo::Bump;

pub struct ArenaAllocator {
    /// 每线程独立 Arena
    arenas: ThreadLocal<RefCell<Bump>>,

    /// Arena 大小
    arena_size: usize,  // 如 4MB
}

impl ArenaAllocator {
    /// 在 Arena 中分配
    #[inline]
    pub fn alloc<T>(&self, value: T) -> &T {
        self.arenas
            .get_or(|| RefCell::new(Bump::with_capacity(self.arena_size)))
            .borrow()
            .alloc(value)
    }

    /// 批量分配 (避免重复边界检查)
    #[inline]
    pub fn alloc_slice<T: Copy>(&self, values: &[T]) -> &[T] {
        self.arenas
            .get_or(|| RefCell::new(Bump::with_capacity(self.arena_size)))
            .borrow()
            .alloc_slice_copy(values)
    }

    /// 重置 Arena (批处理结束后)
    pub fn reset(&self) {
        if let Some(arena) = self.arenas.get() {
            arena.borrow_mut().reset();
        }
    }
}
```

### 6.3 预分配策略 / Pre-allocation

```rust
pub struct PreallocatedBuffers {
    /// 交易缓冲区 (避免运行时分配)
    tx_buffer: Vec<Transaction>,

    /// 成交缓冲区
    trade_buffer: Vec<Trade>,

    /// 序列化缓冲区
    serialize_buffer: Vec<u8>,
}

impl PreallocatedBuffers {
    pub fn new() -> Self {
        Self {
            // 预分配 10000 交易容量
            tx_buffer: Vec::with_capacity(10_000),

            // 预分配 20000 成交容量
            trade_buffer: Vec::with_capacity(20_000),

            // 预分配 10MB 序列化缓冲
            serialize_buffer: Vec::with_capacity(10 * 1024 * 1024),
        }
    }

    /// 重用缓冲区
    #[inline]
    pub fn clear(&mut self) {
        self.tx_buffer.clear();
        self.trade_buffer.clear();
        self.serialize_buffer.clear();
    }
}
```

---

## 7. 并发优化 / Concurrency

### 7.1 多市场并行 / Multi-Market Parallelism

```rust
pub struct ParallelExecutor {
    /// 每个市场独立线程
    market_threads: HashMap<MarketId, JoinHandle<()>>,

    /// 市场任务通道
    channels: HashMap<MarketId, Sender<Task>>,
}

impl ParallelExecutor {
    /// 初始化: 每个市场一个专用线程
    pub fn new(markets: &[MarketId]) -> Self {
        let mut market_threads = HashMap::new();
        let mut channels = HashMap::new();

        for market_id in markets {
            let (tx, rx) = crossbeam_channel::unbounded();

            let handle = std::thread::Builder::new()
                .name(format!("market-{}", market_id))
                .spawn(move || {
                    // 绑定 CPU 核心
                    core_affinity::set_for_current(CoreId { id: market_id.0 as usize });

                    // 市场专用撮合循环
                    for task in rx {
                        process_market_task(task);
                    }
                })
                .expect("spawn market thread");

            market_threads.insert(*market_id, handle);
            channels.insert(*market_id, tx);
        }

        Self { market_threads, channels }
    }

    /// 分发任务到对应市场
    #[inline]
    pub fn dispatch(&self, market_id: MarketId, task: Task) {
        if let Some(tx) = self.channels.get(&market_id) {
            let _ = tx.send(task);
        }
    }
}
```

### 7.2 CPU 核心亲和性 / CPU Affinity

```rust
use core_affinity::CoreId;

pub struct CpuAffinityConfig {
    /// Sequencer 专用核心
    pub sequencer_cores: Vec<usize>,  // [0, 1]

    /// 撮合引擎核心
    pub matching_cores: Vec<usize>,   // [2, 3, 4, 5]

    /// 存储层核心
    pub storage_cores: Vec<usize>,    // [6, 7]

    /// 网络 I/O 核心
    pub network_cores: Vec<usize>,    // [8, 9]
}

impl CpuAffinityConfig {
    pub fn apply(&self) {
        // 示例: 为 Sequencer 设置亲和性
        for &core_id in &self.sequencer_cores {
            core_affinity::set_for_current(CoreId { id: core_id });
        }
    }
}
```

### 7.3 无锁队列 / Lock-Free Queue

```rust
use crossbeam_queue::SegQueue;

pub struct LockFreeOrderQueue {
    /// 无锁并发队列
    queue: SegQueue<Order>,

    /// 容量监控
    size: AtomicUsize,
}

impl LockFreeOrderQueue {
    #[inline]
    pub fn push(&self, order: Order) {
        self.queue.push(order);
        self.size.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn pop(&self) -> Option<Order> {
        self.queue.pop().map(|order| {
            self.size.fetch_sub(1, Ordering::Relaxed);
            order
        })
    }

    /// 批量弹出 (减少原子操作)
    pub fn pop_batch(&self, max: usize) -> Vec<Order> {
        let mut batch = Vec::with_capacity(max);
        for _ in 0..max {
            match self.queue.pop() {
                Some(order) => batch.push(order),
                None => break,
            }
        }
        self.size.fetch_sub(batch.len(), Ordering::Relaxed);
        batch
    }
}
```

### 7.4 读写分离 / Read-Write Separation

```rust
pub struct ReadWriteSeparation<T> {
    /// 读缓存 (多个读者)
    read_cache: Arc<RwLock<T>>,

    /// 写缓冲 (单写者)
    write_buffer: Mutex<Vec<Update<T>>>,

    /// 同步间隔
    sync_interval: Duration,
}

impl<T: Clone> ReadWriteSeparation<T> {
    /// 读操作 (无阻塞)
    #[inline]
    pub fn read(&self) -> RwLockReadGuard<T> {
        self.read_cache.read().unwrap()
    }

    /// 写操作 (缓冲)
    pub fn write(&self, update: Update<T>) {
        self.write_buffer.lock().unwrap().push(update);
    }

    /// 定期同步
    pub fn sync(&self) {
        let updates: Vec<_> = {
            let mut buffer = self.write_buffer.lock().unwrap();
            std::mem::take(&mut *buffer)
        };

        if !updates.is_empty() {
            let mut cache = self.read_cache.write().unwrap();
            for update in updates {
                update.apply(&mut cache);
            }
        }
    }
}
```

---

## 8. 网络优化 / Network

### 8.1 连接池 / Connection Pool

```rust
pub struct ConnectionPool {
    /// 空闲连接
    idle: ArrayQueue<TcpStream>,

    /// 最大连接数
    max_size: usize,

    /// 当前连接数
    current: AtomicUsize,
}

impl ConnectionPool {
    /// 获取连接
    pub async fn acquire(&self) -> Result<PooledConnection> {
        // 1. 尝试从池中获取
        if let Some(conn) = self.idle.pop() {
            return Ok(PooledConnection { conn, pool: self });
        }

        // 2. 创建新连接 (如果未达上限)
        let current = self.current.fetch_add(1, Ordering::SeqCst);
        if current < self.max_size {
            let conn = TcpStream::connect(self.addr).await?;
            self.configure_socket(&conn)?;
            return Ok(PooledConnection { conn, pool: self });
        }

        // 3. 等待可用连接
        self.current.fetch_sub(1, Ordering::SeqCst);
        // ... 等待逻辑
        Err(Error::PoolExhausted)
    }

    fn configure_socket(&self, socket: &TcpStream) -> Result<()> {
        socket.set_nodelay(true)?;
        socket.set_recv_buffer_size(256 * 1024)?;
        socket.set_send_buffer_size(256 * 1024)?;
        Ok(())
    }
}
```

### 8.2 消息批量合并 / Message Batching

```rust
pub struct MessageBatcher {
    /// 批量缓冲
    buffer: Vec<Message>,

    /// 最大批量大小
    max_batch_size: usize,

    /// 最大等待时间
    max_latency: Duration,
}

impl MessageBatcher {
    /// 添加消息
    pub fn add(&mut self, msg: Message) -> Option<Vec<Message>> {
        self.buffer.push(msg);

        if self.buffer.len() >= self.max_batch_size {
            Some(self.flush())
        } else {
            None
        }
    }

    /// 强制刷新
    pub fn flush(&mut self) -> Vec<Message> {
        std::mem::replace(&mut self.buffer, Vec::with_capacity(self.max_batch_size))
    }
}
```

### 8.3 协议优化 / Protocol Optimization

```rust
/// 紧凑二进制协议
#[derive(Serialize, Deserialize)]
pub struct CompactOrder {
    // 使用 varint 编码
    #[serde(with = "varint")]
    pub id: u64,

    #[serde(with = "varint")]
    pub price: u64,

    #[serde(with = "varint")]
    pub quantity: u64,

    // 使用位字段
    pub flags: u8,  // side(1) | type(2) | tif(2) | reserved(3)
}

impl CompactOrder {
    /// 解析 flags
    #[inline]
    pub fn side(&self) -> Side {
        if self.flags & 0x01 == 0 { Side::Buy } else { Side::Sell }
    }

    #[inline]
    pub fn order_type(&self) -> OrderType {
        match (self.flags >> 1) & 0x03 {
            0 => OrderType::Limit,
            1 => OrderType::Market,
            _ => OrderType::Limit,
        }
    }
}
```

---

## 9. 监控调优 / Monitoring

### 9.1 性能指标采集 / Metrics Collection

```rust
use prometheus::{Counter, Histogram, IntGauge};

pub struct PerformanceMetrics {
    /// 延迟直方图
    pub order_latency: Histogram,
    pub matching_latency: Histogram,
    pub storage_latency: Histogram,

    /// 吞吐量计数
    pub orders_processed: Counter,
    pub trades_executed: Counter,

    /// 实时状态
    pub pending_orders: IntGauge,
    pub active_connections: IntGauge,
    pub memory_usage: IntGauge,
}

impl PerformanceMetrics {
    /// 记录订单延迟
    pub fn record_order_latency(&self, start: Instant) {
        let duration = start.elapsed().as_secs_f64();
        self.order_latency.observe(duration);
    }

    /// 记录撮合延迟
    pub fn record_matching_latency(&self, start: Instant) {
        let duration = start.elapsed().as_micros() as f64;
        self.matching_latency.observe(duration);
    }
}
```

### 9.2 火焰图分析 / Flamegraph

```bash
# 生成火焰图
cargo flamegraph --bin dex-engine -- --benchmark

# 或使用 perf
perf record -g --call-graph dwarf ./target/release/dex-engine --benchmark
perf script | inferno-collapse-perf | inferno-flamegraph > flamegraph.svg
```

### 9.3 延迟追踪 / Latency Tracing

```rust
use tracing::{instrument, span, Level};

#[instrument(skip(order))]
pub async fn process_order(order: Order) -> Result<Receipt> {
    let _sequencer = span!(Level::TRACE, "sequencer").entered();
    let sequence = sequencer.assign(order).await?;
    drop(_sequencer);

    let _matching = span!(Level::TRACE, "matching").entered();
    let trades = engine.match_order(&order)?;
    drop(_matching);

    let _storage = span!(Level::TRACE, "storage").entered();
    storage.persist(&trades).await?;
    drop(_storage);

    Ok(Receipt { sequence, trades })
}
```

### 9.4 动态参数调优 / Dynamic Tuning

```rust
pub struct TuningParameters {
    /// 批次大小 (动态调整)
    pub batch_size: AtomicUsize,

    /// 刷盘间隔
    pub flush_interval_ms: AtomicU64,

    /// 连接池大小
    pub pool_size: AtomicUsize,
}

impl TuningParameters {
    /// 根据负载自动调整
    pub fn auto_tune(&self, metrics: &PerformanceMetrics) {
        let p99_latency = metrics.order_latency.get_sample_sum() / metrics.order_latency.get_sample_count();

        // 延迟过高时减小批次
        if p99_latency > 0.045 {  // 45ms
            let current = self.batch_size.load(Ordering::Relaxed);
            self.batch_size.store(current * 3 / 4, Ordering::Relaxed);
        }

        // 延迟很低时增大批次
        if p99_latency < 0.020 {  // 20ms
            let current = self.batch_size.load(Ordering::Relaxed);
            self.batch_size.store(current * 5 / 4, Ordering::Relaxed);
        }
    }
}
```

---

## 10. 硬件选型 / Hardware

### 10.1 推荐配置 / Recommended Configuration

| 组件 | 最低配置 | 推荐配置 | 说明 |
|-----|---------|---------|------|
| **CPU** | 8 核 3.0GHz | 16 核 3.5GHz+ | 高主频优先 |
| **内存** | 32 GB | 128 GB | DDR4-3200+ |
| **存储** | 500GB NVMe | 2TB NVMe | 读写 > 3GB/s |
| **网络** | 10 GbE | 25/100 GbE | 低延迟网卡 |

### 10.2 CPU 选型 / CPU Selection

**优先高主频**:
- 撮合是单线程热路径，受主频限制
- 多核用于多市场并行

**推荐型号**:
- AMD EPYC 9654 (96 核, 2.4GHz 基频, 3.55GHz 睿频)
- Intel Xeon w9-3595X (60 核, 2.0GHz 基频, 4.8GHz 睿频)
- 或高频消费级: AMD Ryzen 9 7950X (16 核, 5.7GHz 睿频)

### 10.3 内存配置 / Memory Configuration

```
容量需求分析:
├── 订单簿: ~1GB / 市场 × 10 市场 = 10GB
├── 状态缓存: ~5GB
├── WAL 缓冲: ~2GB
├── 连接缓冲: ~2GB
├── 其他: ~3GB
└── 安全冗余: 2x

推荐: 64GB ~ 128GB DDR4/DDR5

带宽需求:
├── 峰值写入: 200K TPS × 200B = 40MB/s
├── 峰值读取: 500K QPS × 500B = 250MB/s
└── 推荐: DDR4-3200 或 DDR5-4800
```

### 10.4 存储选型 / Storage Selection

| 场景 | 类型 | 推荐 |
|-----|------|-----|
| WAL | NVMe SSD | 三星 980 PRO / Intel P5800X |
| 快照 | NVMe SSD | 写入耐久性高的企业级 |
| 归档 | HDD 或对象存储 | 成本优化 |

**NVMe 性能要求**:
- 顺序写入: ≥ 5GB/s
- 随机写 IOPS: ≥ 500K
- 写入耐久性: ≥ 1 DWPD (每日全盘写入)

### 10.5 网络配置 / Network Configuration

```bash
# 系统调优 (Linux)

# 增大缓冲区
sysctl -w net.core.rmem_max=16777216
sysctl -w net.core.wmem_max=16777216

# TCP 优化
sysctl -w net.ipv4.tcp_low_latency=1
sysctl -w net.ipv4.tcp_fastopen=3

# 禁用 IRQBALANCE，手动绑定
systemctl stop irqbalance
# 将网卡中断绑定到特定 CPU
```

---

## 11. 测试策略 / Testing Strategy

### 11.1 单元基准测试 / Unit Benchmarks

```rust
use criterion::{criterion_group, criterion_main, Criterion};

fn matching_benchmark(c: &mut Criterion) {
    let mut engine = MatchingEngine::new();

    c.bench_function("match_single_order", |b| {
        let order = create_test_order();
        b.iter(|| engine.match_order(&order))
    });

    c.bench_function("match_batch_1000", |b| {
        let orders: Vec<_> = (0..1000).map(|_| create_test_order()).collect();
        b.iter(|| engine.match_batch(&orders))
    });
}

criterion_group!(benches, matching_benchmark);
criterion_main!(benches);
```

### 11.2 集成性能测试 / Integration Tests

```rust
#[tokio::test]
async fn test_end_to_end_latency() {
    let dex = setup_dex().await;

    let mut latencies = Vec::new();

    for _ in 0..10_000 {
        let start = Instant::now();
        let result = dex.submit_order(create_order()).await;
        latencies.push(start.elapsed());
        assert!(result.is_ok());
    }

    latencies.sort();
    let p99 = latencies[9900];

    assert!(p99 < Duration::from_millis(50), "P99 latency: {:?}", p99);
}

#[tokio::test]
async fn test_throughput() {
    let dex = setup_dex().await;

    let start = Instant::now();
    let count = 100_000;

    let handles: Vec<_> = (0..count)
        .map(|_| {
            let dex = dex.clone();
            tokio::spawn(async move {
                dex.submit_order(create_order()).await
            })
        })
        .collect();

    for handle in handles {
        handle.await.unwrap().unwrap();
    }

    let elapsed = start.elapsed();
    let tps = count as f64 / elapsed.as_secs_f64();

    assert!(tps >= 100_000.0, "TPS: {:.0}", tps);
}
```

### 11.3 压力测试 / Stress Testing

```bash
# 使用 wrk 或自定义工具
wrk -t16 -c1000 -d60s http://localhost:8080/api/v1/order

# 或使用专用压测工具
dex-benchmark \
    --threads 32 \
    --connections 1000 \
    --duration 300s \
    --target-tps 200000 \
    --report stress_test_report.json
```

### 11.4 长期稳定性测试 / Soak Testing

```rust
/// 72 小时稳定性测试
#[tokio::test]
#[ignore]  // 手动运行
async fn test_72h_stability() {
    let dex = setup_dex().await;
    let duration = Duration::from_hours(72);
    let start = Instant::now();

    let mut total_orders = 0u64;
    let mut total_errors = 0u64;

    while start.elapsed() < duration {
        // 模拟真实负载模式
        let load = simulate_market_load();

        for order in load {
            match dex.submit_order(order).await {
                Ok(_) => total_orders += 1,
                Err(_) => total_errors += 1,
            }
        }

        // 定期检查健康状态
        if total_orders % 100_000 == 0 {
            let health = dex.health_check().await;
            assert!(health.is_healthy());
        }
    }

    let error_rate = total_errors as f64 / total_orders as f64;
    assert!(error_rate < 0.001, "Error rate: {:.4}%", error_rate * 100.0);
}
```

---

## 12. 附录 / Appendix

### 12.1 性能检查清单 / Performance Checklist

| 检查项 | 状态 | 说明 |
|-------|------|-----|
| ☐ 禁用 debug assert | | `--release` 编译 |
| ☐ 启用 LTO | | `lto = "fat"` |
| ☐ CPU 原生指令集 | | `target-cpu=native` |
| ☐ 预分配内存 | | 避免运行时分配 |
| ☐ 对象池 | | 复用频繁创建的对象 |
| ☐ 批量操作 | | 减少系统调用 |
| ☐ 异步 I/O | | 不阻塞主路径 |
| ☐ 无锁数据结构 | | 减少竞争 |
| ☐ CPU 亲和性 | | 减少上下文切换 |
| ☐ NUMA 感知 | | 本地内存访问 |

### 12.2 Cargo 优化配置 / Cargo Configuration

```toml
# Cargo.toml

[profile.release]
opt-level = 3
lto = "fat"
codegen-units = 1
panic = "abort"
strip = true

[profile.release.build-override]
opt-level = 3

# 依赖优化
[profile.release.package."*"]
opt-level = 3
```

```toml
# .cargo/config.toml

[build]
rustflags = ["-C", "target-cpu=native"]

[target.x86_64-unknown-linux-gnu]
rustflags = [
    "-C", "target-cpu=native",
    "-C", "link-arg=-fuse-ld=lld",
]
```

---

## 变更历史 / Change History

| 版本 | 日期 | 变更内容 | 状态 |
|-----|------|---------|------|
| v1.0 | 2025-12-31 | 初始版本 | ✅ 有效 |
| v1.1 | 2025-12-31 | SyncPolicy 引用 ADR-006 确认语义 | ✅ 有效 |

### 待对齐事项 / Alignment Notes

| 章节 | 状态 | 说明 |
|-----|------|------|
| 1. 性能目标 | ✅ 有效 | 与 01-REQUIREMENTS 指标口径表一致 |
| 5.1 WAL 优化 | ✅ 有效 | 与 ADR-006 确认语义对齐 |
| 2. 延迟分解 | ✅ 有效 | 50ms 预算分配与架构图一致 |
| 10. 硬件建议 | ⚠️ 待验证 | 需性能测试确认硬件配置 |

---

> **系列文档完成 / Series Complete**
>
> 本文档是 DEX L1 设计文档系列的最后一篇。完整系列:
> 1. [01-REQUIREMENTS.md](./01-REQUIREMENTS.md) - 需求规格
> 2. [02-ARCHITECTURE-OVERVIEW.md](./02-ARCHITECTURE-OVERVIEW.md) - 架构设计
> 3. [03-ABSTRACTION-DESIGN.md](./03-ABSTRACTION-DESIGN.md) - 抽象层设计
> 4. [04-SEQUENCER-DESIGN.md](./04-SEQUENCER-DESIGN.md) - Sequencer 设计
> 5. [05-MATCHING-ENGINE-DESIGN.md](./05-MATCHING-ENGINE-DESIGN.md) - 撮合引擎设计
> 6. [06-STORAGE-DESIGN.md](./06-STORAGE-DESIGN.md) - 存储层设计
> 7. [07-MOVE-INTEGRATION-DESIGN.md](./07-MOVE-INTEGRATION-DESIGN.md) - Move 集成设计
> 8. [08-SPOT-OVERVIEW.md](./08-SPOT-OVERVIEW.md) - 现货交易概要
> 9. [09-PERPETUAL-OVERVIEW.md](./09-PERPETUAL-OVERVIEW.md) - 永续合约概要
> 10. [10-PERFORMANCE-DESIGN.md](./10-PERFORMANCE-DESIGN.md) - 性能优化设计 (本文档)
