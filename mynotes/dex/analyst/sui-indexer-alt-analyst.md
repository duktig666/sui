# sui-indexer-alt 深度分析报告

> 分析日期: 2026-01-27
> 分析目标: 深入理解 Sui 索引器架构，为 DEX 开发提供参考

## 1. 项目概述

### 1.1 定位
`sui-indexer-alt` 是 Sui 区块链的新一代索引器系统，用于从 checkpoint 数据中提取、转换和存储区块链数据，为上层 RPC 服务（GraphQL、JSON-RPC）提供数据支撑。

### 1.2 与旧版索引器的区别
- **新架构**: 采用模块化的 pipeline 架构，支持并发和顺序两种处理模式
- **存储灵活**: 主要使用 PostgreSQL，部分组件支持 RocksDB（consistent store）
- **性能优化**: 支持乱序处理和批量提交，提高吞吐量

### 1.3 部署架构

`sui-indexer-alt` 是**完全链下的独立服务**，不参与共识，不存储完整区块链状态，仅从 Checkpoint 数据中提取和索引信息。

#### 1.3.1 部署架构图

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                              Sui Network                                         │
│  ┌───────────────┐    ┌───────────────┐    ┌───────────────┐                    │
│  │  Validator 1  │    │  Validator 2  │    │  Validator N  │   ← 共识层         │
│  └───────┬───────┘    └───────┬───────┘    └───────┬───────┘                    │
│          └──────────────────┬─┴──────────────────┬─┘                            │
│                             ▼                    ▼                               │
│                    ┌─────────────────┐    ┌─────────────────┐                   │
│                    │   Full Node 1   │    │   Full Node N   │  ← 全节点同步     │
│                    └────────┬────────┘    └────────┬────────┘                   │
│                             │                      │                             │
│                             ▼                      ▼                             │
│                    ┌─────────────────────────────────────┐                      │
│                    │        Checkpoint Store             │ ← S3/GCS/本地        │
│                    │  (https://checkpoints.mainnet.sui.io) │                    │
│                    └──────────────────┬──────────────────┘                      │
└───────────────────────────────────────┼─────────────────────────────────────────┘
                                        │
════════════════════════════════════════╪═══════════════════════════════════════════
                                        │  链下（Off-chain）
                                        ▼
                    ┌──────────────────────────────────────┐
                    │         sui-indexer-alt              │  ← 索引器服务
                    │  ┌────────────────────────────────┐  │
                    │  │     Ingestion Service          │  │
                    │  └────────────────────────────────┘  │
                    │  ┌────────────────────────────────┐  │
                    │  │     Pipeline Processing        │  │
                    │  └────────────────────────────────┘  │
                    └──────────────────┬───────────────────┘
                                       │
                    ┌──────────────────┼───────────────────┐
                    ▼                  ▼                   ▼
          ┌─────────────────┐  ┌─────────────────┐  ┌──────────────┐
          │   PostgreSQL    │  │    RocksDB      │  │  Prometheus  │
          │  (主存储)       │  │ (Consistent     │  │  (指标)      │
          │                 │  │   Store)        │  │              │
          └────────┬────────┘  └────────┬────────┘  └──────────────┘
                   │                    │
                   ▼                    ▼
          ┌─────────────────┐  ┌─────────────────┐
          │  GraphQL RPC    │  │   gRPC API      │  ← 对外服务
          │  JSON-RPC       │  │ (Consistent)    │
          └─────────────────┘  └─────────────────┘
```

#### 1.3.2 数据源选项

> 源码: [`crates/sui-indexer-alt/src/args.rs`](../../crates/sui-indexer-alt/src/args.rs)

索引器支持四种 Checkpoint 数据来源，只需配置其中一种：

| 数据源 | 命令行参数 | 使用场景 | 延迟 |
|--------|------------|----------|------|
| 远程存储 | `--remote-store-url` | 生产环境，从 S3/GCS 读取 | 秒级 |
| 本地文件 | `--local-ingestion-path` | 开发测试，本地 checkpoint 文件 | 无延迟 |
| RPC 接口 | `--rpc-api-url` | 从全节点 RPC 获取 | 秒级 |
| gRPC 流式 | `--streaming-url` | 实时同步，最低延迟 | 亚秒级 |

#### 1.3.3 典型部署方式

**开发/测试环境（单机）**:
```
本地 Checkpoint 文件 → Indexer → PostgreSQL → RPC 服务
```

**生产环境（分离部署）**:
```
Sui 全节点 → Checkpoint Store (S3) → Indexer 集群 → PostgreSQL 主从 → RPC 集群
                                         ↓
                                   Prometheus/Grafana
```

**低延迟场景**:
```
Sui 全节点 ──gRPC Streaming──→ Indexer → PostgreSQL → RPC 服务
```

## 2. 项目结构

### 2.1 核心 Crate 依赖关系

```
sui-indexer-alt                    # 主索引器二进制
    ├── sui-indexer-alt-framework  # 核心框架（ingestion + pipeline）
    │   └── sui-indexer-alt-framework-store-traits  # 存储抽象接口
    ├── sui-indexer-alt-schema     # 数据库 schema 和迁移
    └── sui-indexer-alt-metrics    # Prometheus 指标

sui-indexer-alt-graphql            # GraphQL RPC 服务
    └── sui-indexer-alt-reader     # 数据库读取层

sui-indexer-alt-jsonrpc            # JSON-RPC 服务
    └── sui-indexer-alt-reader

sui-indexer-alt-consistent-store   # 一致性存储（RocksDB）
    └── sui-indexer-alt-consistent-api  # gRPC API 定义

sui-indexer-alt-e2e-tests          # 端到端测试
sui-indexer-alt-restorer           # 数据恢复工具
sui-indexer-alt-object-store       # 对象存储抽象
```

### 2.2 关键文件路径

| 模块 | 路径 |
|------|------|
| 框架核心 | [`crates/sui-indexer-alt-framework/src/lib.rs`](../../crates/sui-indexer-alt-framework/src/lib.rs) |
| Pipeline 模块 | [`crates/sui-indexer-alt-framework/src/pipeline/mod.rs`](../../crates/sui-indexer-alt-framework/src/pipeline/mod.rs) |
| 并发 Pipeline | [`crates/sui-indexer-alt-framework/src/pipeline/concurrent/mod.rs`](../../crates/sui-indexer-alt-framework/src/pipeline/concurrent/mod.rs) |
| Ingestion 服务 | [`crates/sui-indexer-alt-framework/src/ingestion/mod.rs`](../../crates/sui-indexer-alt-framework/src/ingestion/mod.rs) |
| 主索引器 | [`crates/sui-indexer-alt/src/lib.rs`](../../crates/sui-indexer-alt/src/lib.rs) |
| Handler 实现 | [`crates/sui-indexer-alt/src/handlers/mod.rs`](../../crates/sui-indexer-alt/src/handlers/mod.rs) |
| 配置系统 | [`crates/sui-indexer-alt/src/config.rs`](../../crates/sui-indexer-alt/src/config.rs) |
| Schema 定义 | [`crates/sui-indexer-alt-schema/src/schema.rs`](../../crates/sui-indexer-alt-schema/src/schema.rs) |
| Consistent Store | [`crates/sui-indexer-alt-consistent-store/src/lib.rs`](../../crates/sui-indexer-alt-consistent-store/src/lib.rs) |

## 3. 核心架构

### 3.1 整体数据流

```
                                    ┌──────────────────────────────────────┐
                                    │          Checkpoint Sources           │
                                    │  (Remote Store / Local / RPC / gRPC)  │
                                    └──────────────────┬───────────────────┘
                                                       │
                                                       ▼
┌──────────────────────────────────────────────────────────────────────────────┐
│                            Ingestion Service                                  │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────┐    ┌──────────────┐  │
│  │  Streaming  │    │   Remote    │    │    Local    │    │     RPC      │  │
│  │   Client    │    │   Client    │    │   Client    │    │    Client    │  │
│  └──────┬──────┘    └──────┬──────┘    └──────┬──────┘    └──────┬───────┘  │
│         └──────────────────┴─────────────────┬┴──────────────────┘          │
│                                              │                               │
│                                    ┌─────────▼─────────┐                    │
│                                    │    Broadcaster    │                    │
│                                    │  (Fan-out to N)   │                    │
│                                    └─────────┬─────────┘                    │
└──────────────────────────────────────────────┼──────────────────────────────┘
                                               │
              ┌────────────────────────────────┼────────────────────────────────┐
              │                                │                                │
              ▼                                ▼                                ▼
┌──────────────────────┐        ┌──────────────────────┐        ┌──────────────────────┐
│   Concurrent Pipeline │        │   Concurrent Pipeline │        │  Sequential Pipeline │
│   (kv_transactions)   │        │    (kv_objects)       │        │   (sum_displays)     │
├──────────────────────┤        ├──────────────────────┤        ├──────────────────────┤
│ Processor ─► Collector│        │ Processor ─► Collector│        │ Processor ─► Committer│
│     ─► Committer      │        │     ─► Committer      │        │                      │
│     ─► Watermark      │        │     ─► Watermark      │        │                      │
│     ─► Pruner         │        │     ─► Pruner         │        │                      │
└──────────┬───────────┘        └──────────┬───────────┘        └──────────┬───────────┘
           │                               │                               │
           └───────────────────────────────┴───────────────────────────────┘
                                           │
                                           ▼
                               ┌───────────────────────┐
                               │      PostgreSQL       │
                               │   (or RocksDB for     │
                               │   consistent store)   │
                               └───────────────────────┘
```

### 3.2 Ingestion Service（数据摄取服务）

> 源码: [`crates/sui-indexer-alt-framework/src/ingestion/mod.rs:94-203`](../../crates/sui-indexer-alt-framework/src/ingestion/mod.rs)

#### 3.2.1 Checkpoint 数据源

支持多种 checkpoint 获取方式：

| 数据源 | 实现文件 | 使用场景 |
|--------|----------|----------|
| Remote Store | [`remote_client.rs`](../../crates/sui-indexer-alt-framework/src/ingestion/remote_client.rs) | 从 S3/GCS 等远程存储读取 |
| Local Path | [`local_client.rs`](../../crates/sui-indexer-alt-framework/src/ingestion/local_client.rs) | 从本地文件系统读取 |
| RPC API | [`rpc_client.rs`](../../crates/sui-indexer-alt-framework/src/ingestion/rpc_client.rs) | 从全节点 RPC 获取 |
| gRPC Streaming | [`streaming_client.rs`](../../crates/sui-indexer-alt-framework/src/ingestion/streaming_client.rs) | 流式获取最新 checkpoint |

#### 3.2.2 配置参数

> 源码: [`crates/sui-indexer-alt-framework/src/ingestion/mod.rs:46-68`](../../crates/sui-indexer-alt-framework/src/ingestion/mod.rs)

```rust
pub struct IngestionConfig {
    pub checkpoint_buffer_size: usize,      // 默认 5000
    pub ingest_concurrency: usize,          // 默认 200
    pub retry_interval_ms: u64,             // 默认 200ms
    pub streaming_backoff_initial_batch_size: usize,
    pub streaming_backoff_max_batch_size: usize,
    pub streaming_connection_timeout_ms: u64,
    pub streaming_statement_timeout_ms: u64,
}
```

#### 3.2.3 背压机制

- `Broadcaster` 使用 channel 向下游 pipeline 分发 checkpoint
- 如果任一订阅者处理速度慢，会对整个 ingestion 产生背压
- 通过 `commit_hi_rx` 接收下游反馈，避免过度超前

### 3.3 Pipeline 系统

> 源码: [`crates/sui-indexer-alt-framework/src/pipeline/mod.rs`](../../crates/sui-indexer-alt-framework/src/pipeline/mod.rs)

#### 3.3.1 两种 Pipeline 模式

| 特性 | Concurrent Pipeline | Sequential Pipeline |
|------|---------------------|---------------------|
| 处理顺序 | 乱序处理 | 严格顺序 |
| 吞吐量 | 高 | 较低 |
| 数据写入 | 仅追加（append-only） | 可原地更新 |
| 使用场景 | KV 存储、事件索引 | 汇总表、状态维护 |
| Watermark | 追踪已提交的最高点 | 严格递增 |

#### 3.3.2 Concurrent Pipeline 组件

> 源码: [`crates/sui-indexer-alt-framework/src/pipeline/concurrent/mod.rs:206-288`](../../crates/sui-indexer-alt-framework/src/pipeline/concurrent/mod.rs)

```
┌─────────────────────────────────────────────────────────────────────────┐
│                        Concurrent Pipeline                               │
│                                                                          │
│  ┌──────────┐    ┌──────────┐    ┌──────────┐    ┌──────────────┐       │
│  │Processor │ ─► │Collector │ ─► │Committer │ ─► │CommitWatermark│      │
│  │(FANOUT=N)│    │          │    │(W个并发) │    │              │       │
│  └──────────┘    └──────────┘    └──────────┘    └──────────────┘       │
│                                                                          │
│                  ┌──────────────┐    ┌──────────┐                        │
│                  │ReaderWatermark│ ─► │  Pruner  │                       │
│                  └──────────────┘    └──────────┘                        │
└─────────────────────────────────────────────────────────────────────────┘
```

各组件职责：
- **Processor**: 从 Checkpoint 提取数据，转换为 `Value` 类型
- **Collector**: 收集处理结果，组装成批次
- **Committer**: 将批次写入数据库
- **CommitWatermark**: 维护已提交数据的高水位
- **ReaderWatermark**: 维护可安全读取的低水位
- **Pruner**: 根据保留策略删除旧数据

#### 3.3.3 Handler 接口

> 源码: [`crates/sui-indexer-alt-framework/src/pipeline/concurrent/mod.rs:58-105`](../../crates/sui-indexer-alt-framework/src/pipeline/concurrent/mod.rs)

```rust
pub trait Handler: Processor {
    type Store: Store;
    type Batch: Default + Send + Sync + 'static;

    const MIN_EAGER_ROWS: usize = 50;       // 积攒够多少行才提交
    const MAX_PENDING_ROWS: usize = 5000;   // 最大待处理行数（背压阈值）
    const MAX_WATERMARK_UPDATES: usize = 10_000;

    fn batch(&self, batch: &mut Self::Batch, values: &mut IntoIter<Self::Value>) -> BatchStatus;

    async fn commit<'a>(&self, batch: &Self::Batch, conn: &mut Connection<'a>) -> Result<usize>;

    async fn prune<'a>(&self, from: u64, to_exclusive: u64, conn: &mut Connection<'a>) -> Result<usize>;
}
```

### 3.4 Processor 接口

> 源码: [`crates/sui-indexer-alt-framework/src/pipeline/processor.rs`](../../crates/sui-indexer-alt-framework/src/pipeline/processor.rs)

```rust
pub trait Processor: Send + Sync {
    const NAME: &'static str;           // Pipeline 名称
    const FANOUT: usize = 1;            // 并行处理数
    type Value: FieldCount + Send + Sync + 'static;

    async fn process(&self, checkpoint: &Arc<Checkpoint>) -> Result<Vec<Self::Value>>;
}
```

### 3.5 数据写入机制

> 源码: [`crates/sui-indexer-alt-framework/src/pipeline/concurrent/committer.rs`](../../crates/sui-indexer-alt-framework/src/pipeline/concurrent/committer.rs)

#### 3.5.1 写入流程

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                       Concurrent Pipeline 完整写入流程                           │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                  │
│  Ingestion ──► Processor ──► Collector ──► Committer ──► CommitWatermark        │
│      │            │              │             │               │                │
│      │            │              │             │               └──► DB (watermark)
│      │            │              │             └──► DB (batch data)             │
│      │            │              │                                               │
│      │            │              ├─ pending: BTreeMap<checkpoint, data>         │
│      │            │              ├─ poll.tick() 每 500ms 检查                    │
│      │            │              └─ pending_rows >= 50 立即触发                  │
│      │            │                                                              │
│      │            └─ FANOUT 并发 (默认 1)                                        │
│      │                                                                           │
│      └─ Broadcaster 广播到所有 Pipeline                                         │
│                                                                                  │
│  背压传播链:                                                                     │
│  ┌─────────────────────────────────────────────────────────────────────────┐    │
│  │ pending_rows >= 5000 → Collector 停止接收 → Processor 阻塞 → Ingestion 阻塞 │    │
│  └─────────────────────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────────────────────┘
```

**各阶段职责**:

| 阶段 | 职责 | 关键配置 |
|------|------|----------|
| **Processor** | 从 Checkpoint 提取数据，转换为 `Value` | `FANOUT` |
| **Collector** | 按 checkpoint 排序，组装批次 | `MIN_EAGER_ROWS`, `MAX_PENDING_ROWS` |
| **Committer** | 并发写入数据库，失败重试 | `write_concurrency` |
| **CommitWatermark** | 更新 watermark，通知 pruner | `watermark_interval_ms` |

#### 3.5.2 批量写入策略

> 源码: [`crates/sui-indexer-alt-framework/src/pipeline/concurrent/mod.rs:63-69`](../../crates/sui-indexer-alt-framework/src/pipeline/concurrent/mod.rs)

```rust
pub trait Handler: Processor {
    const MIN_EAGER_ROWS: usize = 50;      // 最少积攒多少行才提交
    const MAX_PENDING_ROWS: usize = 5000;  // 最大待处理行数（触发背压）
    // ...
}
```

- **MIN_EAGER_ROWS**: 当收集够这么多行时，立即提交（不等待更多数据）
- **MAX_PENDING_ROWS**: 达到此阈值时，暂停上游处理（背压）

#### 3.5.2.1 Collector 详细机制

> 源码: [`crates/sui-indexer-alt-framework/src/pipeline/concurrent/collector.rs:58-72`](../../crates/sui-indexer-alt-framework/src/pipeline/concurrent/collector.rs)

**批次触发条件**（按优先级）:

```rust
// collector.rs:211-213
if pending_rows >= H::MIN_EAGER_ROWS {
    poll.reset_immediately()  // 立即触发批次收集
}
```

| 触发条件 | 代码位置 | 说明 |
|----------|----------|------|
| `pending_rows >= MIN_EAGER_ROWS` | :211-213 | 数据量足够，立即触发 |
| `poll.tick()` 定时触发 | :102 | 每 `collect_interval_ms` 定时检查 |
| `BatchStatus::Ready` | :136-139 | Handler 返回批次已满 |
| `pending_rows >= MAX_PENDING_ROWS` | :184 | 暂停接收，形成背压 |

**数据结构与排序**:

```rust
// 使用 BTreeMap 按 checkpoint 序号排序存储
let mut pending: BTreeMap<u64, PendingCheckpoint<H>> = BTreeMap::new();
```

**过期数据跳过**:

> 源码: [`collector.rs:186-193`](../../crates/sui-indexer-alt-framework/src/pipeline/concurrent/collector.rs)

```rust
// 跳过已过期的 checkpoint（低于 reader_lo）
let reader_lo = main_reader_lo.wait().await.load(Ordering::Relaxed);
if indexed.checkpoint() < reader_lo {
    indexed.values.clear();  // 清空数据，但保留 watermark 用于推进
    metrics.total_collector_skipped_checkpoints.inc();
}
```

#### 3.5.3 并发写入控制

> 源码: [`crates/sui-indexer-alt-framework/src/pipeline/concurrent/committer.rs:30-35`](../../crates/sui-indexer-alt-framework/src/pipeline/concurrent/committer.rs)

```rust
pub struct CommitterConfig {
    pub write_concurrency: usize,     // 默认 5，并发写入数
    pub collect_interval_ms: u64,     // 默认 500ms，收集间隔
    pub watermark_interval_ms: u64,   // 默认 500ms，水位更新间隔
}
```

Committer 使用信号量控制并发写入数量：
```rust
// committer.rs:89-91
let write_concurrency = config.write_concurrency.try_into().unwrap_or(usize::MAX);
let write_limiter = Arc::new(Semaphore::new(write_concurrency));
```

#### 3.5.4 重试机制

> 源码: [`crates/sui-indexer-alt-framework/src/pipeline/concurrent/committer.rs:58-63`](../../crates/sui-indexer-alt-framework/src/pipeline/concurrent/committer.rs)

采用**指数退避重试**策略：

```rust
const INITIAL_RETRY_INTERVAL: Duration = Duration::from_millis(100);
const MAX_RETRY_INTERVAL: Duration = Duration::from_secs(1);

// 使用 backoff crate 实现
backoff::future::retry(
    backoff::ExponentialBackoff {
        initial_interval: INITIAL_RETRY_INTERVAL,
        max_interval: MAX_RETRY_INTERVAL,
        max_elapsed_time: None,  // 永不放弃
        ..Default::default()
    },
    || async {
        handler.commit(&batch, conn).await
    }
)
```

**重试特点**:
- 初始间隔: 100ms
- 最大间隔: 1s
- 无最大重试次数限制（永不放弃）
- 适用于瞬时数据库故障

#### 3.5.5 幂等写入保证

> 源码: [`crates/sui-indexer-alt/src/handlers/kv_transactions.rs:76-80`](../../crates/sui-indexer-alt/src/handlers/kv_transactions.rs)

使用 Diesel ORM 的 `on_conflict_do_nothing` 保证幂等性：

```rust
async fn commit<'a>(&self, batch: &Self::Batch, conn: &mut Connection<'a>) -> Result<usize> {
    Ok(diesel::insert_into(kv_transactions::table)
        .values(batch)
        .on_conflict_do_nothing()   // 冲突时忽略，保证幂等
        .execute(conn)
        .await?)
}
```

**幂等性意义**:
- 索引器重启后可安全重新处理同一 checkpoint
- 多个索引器实例可并行写入同一数据库
- 无需担心重复数据

#### 3.5.6 Diesel ORM 与 diesel-async

> 源码: [`crates/sui-indexer-alt-framework/src/postgres/mod.rs`](../../crates/sui-indexer-alt-framework/src/postgres/mod.rs)

使用 `diesel-async` 实现异步数据库操作：

```rust
// 连接池配置
pub type Db = Pool<AsyncPgConnection>;

// 连接获取
pub async fn connection<'a>(&'a self) -> Result<Object<'a, AsyncPgConnection>> {
    self.db.get().await.map_err(|e| anyhow!("Failed to get connection: {}", e))
}

// 自动迁移
pub async fn run_migrations(&self) -> Result<()> {
    conn.run_pending_migrations(MIGRATIONS)
        .await
        .map_err(|e| anyhow!("Migration failed: {}", e))?;
    Ok(())
}
```

**写入特点**:
- 完全异步，不阻塞线程
- 使用连接池（deadpool）管理数据库连接
- 自动应用 schema 迁移

#### 3.5.7 并发 vs 顺序 Pipeline 写入对比

> 源码: [`crates/sui-indexer-alt-framework/src/pipeline/sequential/committer.rs:22-37`](../../crates/sui-indexer-alt-framework/src/pipeline/sequential/committer.rs)

| 特性 | 并发 Pipeline | 顺序 Pipeline |
|------|--------------|---------------|
| **写入顺序** | 乱序写入，按批次提交 | 严格按 checkpoint 顺序 |
| **事务边界** | 每批独立事务 | 写入 + watermark 同一事务 |
| **幂等保证** | `on_conflict_do_nothing` | 按顺序避免重复写入 |
| **checkpoint_lag** | 无 | 可配置滞后（等待确认） |
| **适用场景** | append-only KV 存储 | 汇总表、状态维护 |

**顺序 Pipeline 事务写入**:

```rust
// sequential/committer.rs:225-230
store.transaction(|conn| {
    async {
        // Watermark 和数据在同一事务中更新，保证原子性
        conn.set_committer_watermark(H::NAME, watermark).await?;
        handler.commit(&batch, conn).await
    }.scope_boxed()
}).await;
```

**Watermark 更新差异**:
- **并发 Pipeline**: Committer 写入成功后，通过 channel 异步发送 watermark 到 CommitWatermark task
- **顺序 Pipeline**: Watermark 与数据写入在同一数据库事务中，保证原子性

#### 3.5.8 性能调优建议

| 参数 | 默认值 | 调优方向 | 建议 |
|------|--------|----------|------|
| `MIN_EAGER_ROWS` | 50 | 延迟 ↔ 吞吐 | 降低可减少延迟，升高可提升批量效率 |
| `MAX_PENDING_ROWS` | 5000 | 内存 ↔ 缓冲 | 根据内存容量调整，过大可能 OOM |
| `write_concurrency` | 5 | 并发 ↔ 连接 | 应小于 DB 连接池大小 |
| `collect_interval_ms` | 500 | 延迟 ↔ CPU | 降低可减少批次延迟 |
| `watermark_interval_ms` | 500 | 一致性 ↔ IO | 降低可更频繁更新进度 |

**吞吐优化场景**（追赶历史数据）:
```toml
[committer]
write_concurrency = 10
collect_interval_ms = 100

[pipeline.kv_transactions]
# 覆盖默认配置
```

**低延迟场景**（实时同步）:
```toml
[committer]
collect_interval_ms = 50
watermark_interval_ms = 100
```

## 4. 数据 Schema

### 4.1 数据库表设计

> 源码: [`crates/sui-indexer-alt-schema/src/schema.rs`](../../crates/sui-indexer-alt-schema/src/schema.rs)

#### 4.1.1 KV 存储表（键值对，支持历史版本）

| 表名 | 主键 | 用途 |
|------|------|------|
| `kv_transactions` | `tx_digest` | 交易数据（含 effects、events） |
| `kv_objects` | `(object_id, object_version)` | 对象数据（含历史版本） |
| `kv_checkpoints` | `sequence_number` | Checkpoint 元数据 |
| `kv_packages` | `(package_id, package_version)` | Move 包 |
| `kv_epoch_starts` | `epoch` | Epoch 开始信息 |
| `kv_epoch_ends` | `epoch` | Epoch 结束信息 |
| `kv_feature_flags` | `(protocol_version, flag_name)` | 功能开关 |
| `kv_protocol_configs` | `(protocol_version, config_name)` | 协议配置 |

#### 4.1.2 索引表（按特定维度查询）

| 表名 | 主键 | 用途 |
|------|------|------|
| `tx_digests` | `tx_sequence_number` | 序号 → 交易摘要映射 |
| `tx_affected_addresses` | `(affected, tx_sequence_number)` | 地址相关交易 |
| `tx_affected_objects` | `(affected, tx_sequence_number)` | 对象相关交易 |
| `tx_calls` | `(package, module, function, tx_sequence_number)` | 函数调用索引 |
| `tx_kinds` | `(tx_kind, tx_sequence_number)` | 交易类型索引 |
| `tx_balance_changes` | `tx_sequence_number` | 余额变动 |
| `ev_emit_mod` | `(package, module, tx_sequence_number)` | 模块事件索引 |
| `ev_struct_inst` | `(package, module, name, instantiation, tx_sequence_number)` | 事件类型索引 |

#### 4.1.3 对象状态表

| 表名 | 主键 | 用途 |
|------|------|------|
| `obj_versions` | `(object_id, object_version)` | 对象版本历史 |
| `obj_info` | `(object_id, cp_sequence_number)` | 对象信息（按 checkpoint） |
| `coin_balance_buckets` | `(object_id, cp_sequence_number)` | 代币余额桶 |

#### 4.1.4 辅助表

| 表名 | 主键 | 用途 |
|------|------|------|
| `cp_sequence_numbers` | `cp_sequence_number` | Checkpoint → 交易序号映射 |
| `sum_displays` | `object_type` | Display 配置汇总 |
| `watermarks` | `pipeline` | Pipeline 水位记录 |

### 4.2 Watermark 设计

> 源码: [`crates/sui-indexer-alt-schema/src/schema.rs:231-241`](../../crates/sui-indexer-alt-schema/src/schema.rs)

```rust
diesel::table! {
    watermarks (pipeline) {
        pipeline -> Text,                    // Pipeline 名称
        epoch_hi_inclusive -> Int8,          // 已处理的最高 epoch
        checkpoint_hi_inclusive -> Int8,     // 已提交的最高 checkpoint
        tx_hi -> Int8,                       // 已处理的最高交易序号
        timestamp_ms_hi_inclusive -> Int8,   // 最高时间戳
        reader_lo -> Int8,                   // 可安全读取的最低点（用于 pruning）
        pruner_timestamp -> Timestamp,       // 上次 prune 时间
        pruner_hi -> Int8,                   // 已 prune 到的位置
    }
}
```

## 5. 已实现的 Pipeline（Handler）

### 5.1 完整 Pipeline 列表

> 源码: [`crates/sui-indexer-alt/src/handlers/`](../../crates/sui-indexer-alt/src/handlers/)

| Handler | 类型 | 输入 | 输出表 | 源码 |
|---------|------|------|--------|------|
| `CpSequenceNumbers` | Concurrent | Checkpoint summary | `cp_sequence_numbers` | [`cp_sequence_numbers.rs`](../../crates/sui-indexer-alt/src/handlers/cp_sequence_numbers.rs) |
| `KvTransactions` | Concurrent | Transaction data | `kv_transactions` | [`kv_transactions.rs`](../../crates/sui-indexer-alt/src/handlers/kv_transactions.rs) |
| `KvObjects` | Concurrent | Output objects | `kv_objects` | [`kv_objects.rs`](../../crates/sui-indexer-alt/src/handlers/kv_objects.rs) |
| `KvCheckpoints` | Concurrent | Checkpoint | `kv_checkpoints` | [`kv_checkpoints.rs`](../../crates/sui-indexer-alt/src/handlers/kv_checkpoints.rs) |
| `KvPackages` | Concurrent | Package objects | `kv_packages` | [`kv_packages.rs`](../../crates/sui-indexer-alt/src/handlers/kv_packages.rs) |
| `KvEpochStarts` | Concurrent | Epoch change events | `kv_epoch_starts` | [`kv_epoch_starts.rs`](../../crates/sui-indexer-alt/src/handlers/kv_epoch_starts.rs) |
| `KvEpochEnds` | Concurrent | Epoch end events | `kv_epoch_ends` | [`kv_epoch_ends.rs`](../../crates/sui-indexer-alt/src/handlers/kv_epoch_ends.rs) |
| `KvFeatureFlags` | Concurrent | Genesis + protocol | `kv_feature_flags` | [`kv_feature_flags.rs`](../../crates/sui-indexer-alt/src/handlers/kv_feature_flags.rs) |
| `KvProtocolConfigs` | Concurrent | Genesis + protocol | `kv_protocol_configs` | [`kv_protocol_configs.rs`](../../crates/sui-indexer-alt/src/handlers/kv_protocol_configs.rs) |
| `TxDigests` | Concurrent | Transactions | `tx_digests` | [`tx_digests.rs`](../../crates/sui-indexer-alt/src/handlers/tx_digests.rs) |
| `TxAffectedAddresses` | Concurrent | Transactions | `tx_affected_addresses` | [`tx_affected_addresses.rs`](../../crates/sui-indexer-alt/src/handlers/tx_affected_addresses.rs) |
| `TxAffectedObjects` | Concurrent | Effects | `tx_affected_objects` | [`tx_affected_objects.rs`](../../crates/sui-indexer-alt/src/handlers/tx_affected_objects.rs) |
| `TxBalanceChanges` | Concurrent | Effects | `tx_balance_changes` | [`tx_balance_changes.rs`](../../crates/sui-indexer-alt/src/handlers/tx_balance_changes.rs) |
| `TxCalls` | Concurrent | Transactions | `tx_calls` | [`tx_calls.rs`](../../crates/sui-indexer-alt/src/handlers/tx_calls.rs) |
| `TxKinds` | Concurrent | Transactions | `tx_kinds` | [`tx_kinds.rs`](../../crates/sui-indexer-alt/src/handlers/tx_kinds.rs) |
| `EvEmitMod` | Concurrent | Events | `ev_emit_mod` | [`ev_emit_mod.rs`](../../crates/sui-indexer-alt/src/handlers/ev_emit_mod.rs) |
| `EvStructInst` | Concurrent | Events | `ev_struct_inst` | [`ev_struct_inst.rs`](../../crates/sui-indexer-alt/src/handlers/ev_struct_inst.rs) |
| `ObjVersions` | Concurrent | Effects | `obj_versions` | [`obj_versions.rs`](../../crates/sui-indexer-alt/src/handlers/obj_versions.rs) |
| `ObjInfo` | Concurrent | Objects | `obj_info` | [`obj_info.rs`](../../crates/sui-indexer-alt/src/handlers/obj_info.rs) |
| `CoinBalanceBuckets` | Concurrent | Coin objects | `coin_balance_buckets` | [`coin_balance_buckets.rs`](../../crates/sui-indexer-alt/src/handlers/coin_balance_buckets.rs) |
| `SumDisplays` | Sequential | Display objects | `sum_displays` | [`sum_displays.rs`](../../crates/sui-indexer-alt/src/handlers/sum_displays.rs) |

### 5.2 Handler 实现示例

> 源码: [`crates/sui-indexer-alt/src/handlers/kv_transactions.rs:19-91`](../../crates/sui-indexer-alt/src/handlers/kv_transactions.rs)

```rust
impl Processor for KvTransactions {
    const NAME: &'static str = "kv_transactions";
    type Value = StoredTransaction;

    async fn process(&self, checkpoint: &Arc<Checkpoint>) -> Result<Vec<Self::Value>> {
        let mut values = Vec::with_capacity(transactions.len());
        for tx in transactions.iter() {
            values.push(StoredTransaction {
                tx_digest: tx.transaction.digest().inner().into(),
                cp_sequence_number: summary.sequence_number as i64,
                timestamp_ms: summary.timestamp_ms as i64,
                raw_transaction: bcs::to_bytes(transaction)?,
                raw_effects: bcs::to_bytes(effects)?,
                events: bcs::to_bytes(&events)?,
                user_signatures: bcs::to_bytes(signatures)?,
            });
        }
        Ok(values)
    }
}

impl Handler for KvTransactions {
    const MIN_EAGER_ROWS: usize = 100;
    const MAX_PENDING_ROWS: usize = 10000;

    async fn commit<'a>(values: &[Self::Value], conn: &mut Connection<'a>) -> Result<usize> {
        Ok(diesel::insert_into(kv_transactions::table)
            .values(values)
            .on_conflict_do_nothing()
            .execute(conn)
            .await?)
    }

    async fn prune<'a>(&self, from: u64, to_exclusive: u64, conn: &mut Connection<'a>) -> Result<usize> {
        let filter = kv_transactions::table.filter(
            kv_transactions::cp_sequence_number.between(from as i64, to_exclusive as i64 - 1)
        );
        Ok(diesel::delete(filter).execute(conn).await?)
    }
}
```

## 6. Consistent Store（一致性存储）

### 6.1 设计目标

> 源码: [`crates/sui-indexer-alt-consistent-store/src/lib.rs`](../../crates/sui-indexer-alt-consistent-store/src/lib.rs)

提供**某个 checkpoint 时刻的一致性视图**，用于查询：
- 按 owner 查询对象
- 按 type 查询对象
- 查询地址余额

### 6.2 与主索引器的区别

| 特性 | 主索引器 (PostgreSQL) | Consistent Store (RocksDB) |
|------|----------------------|---------------------------|
| 存储引擎 | PostgreSQL | RocksDB |
| 一致性 | 最终一致 | 强一致（快照） |
| Pipeline | 支持并发 | 仅顺序 |
| 数据模型 | 多表关联 | KV with 前缀扫描 |
| 查询能力 | SQL | 前缀匹配、范围查询 |

### 6.3 支持的 Handler

> 源码: [`crates/sui-indexer-alt-consistent-store/src/lib.rs:132-134`](../../crates/sui-indexer-alt-consistent-store/src/lib.rs)

```rust
add_sequential!(Balances, balances);             // 余额查询
add_sequential!(ObjectByOwner, object_by_owner); // 按 owner 查对象
add_sequential!(ObjectByType, object_by_type);   // 按 type 查对象
```

### 6.4 RPC 接口

通过 gRPC 提供服务，定义在 [`crates/sui-indexer-alt-consistent-api/`](../../crates/sui-indexer-alt-consistent-api/)：
- `ListOwnedObjects`: 列出某地址拥有的对象
- `ListObjectsByType`: 列出某类型的对象
- `GetBalance`: 获取地址的代币余额
- `GetAvailableRange`: 获取可查询的 checkpoint 范围

## 7. RPC 服务层

### 7.1 GraphQL 服务

> 源码: [`crates/sui-indexer-alt-graphql/`](../../crates/sui-indexer-alt-graphql/)
> 文档: [`crates/sui-indexer-alt-graphql/README.md`](../../crates/sui-indexer-alt-graphql/README.md)

- 基于 `async-graphql` 实现
- 从 PostgreSQL 读取数据
- 支持可选的 Bigtable 和 Consistent Store 数据源
- 支持可选的 Fullnode 用于交易执行和模拟

### 7.2 JSON-RPC 服务

> 源码: [`crates/sui-indexer-alt-jsonrpc/`](../../crates/sui-indexer-alt-jsonrpc/)

- 基于 `jsonrpsee` 实现
- 从 PostgreSQL 读取数据
- 实现 Sui JSON-RPC API 规范

## 8. 配置系统

### 8.1 配置文件结构

> 源码: [`crates/sui-indexer-alt/src/config.rs`](../../crates/sui-indexer-alt/src/config.rs)

```toml
# 生成示例配置: cargo run --bin sui-indexer-alt -- generate-config

[ingestion]
checkpoint_buffer_size = 5000
ingest_concurrency = 200
retry_interval_ms = 200

[committer]
write_concurrency = 5
collect_interval_ms = 500
watermark_interval_ms = 500

[pruner]
interval_ms = 300000
delay_ms = 120000
retention = 4000000
max_chunk_size = 2000

[pipeline.kv_transactions]
# 可覆盖 committer 和 pruner 配置

[pipeline.kv_objects]
# ...
```

### 8.2 配置分层

> 源码: [`crates/sui-indexer-alt/src/config.rs:206-222`](../../crates/sui-indexer-alt/src/config.rs)

```rust
// 配置优先级: per-pipeline > shared > default
impl ConcurrentLayer {
    pub fn finish(self, base: ConcurrentConfig) -> Result<ConcurrentConfig> {
        Ok(ConcurrentConfig {
            committer: self.committer.unwrap_or(base.committer),
            pruner: match (self.pruner, base.pruner) {
                (None, _) | (_, None) => None,
                (Some(p), Some(b)) => Some(p.finish(b)?),
            },
        })
    }
}
```

## 9. 运行与部署

### 9.1 运行索引器

> 文档: [`crates/sui-indexer-alt/README.md`](../../crates/sui-indexer-alt/README.md)

```bash
# 1. 创建数据库
diesel setup \
    --database-url="postgres://postgres:postgrespw@localhost:5432/sui_indexer_alt" \
    --migration-dir ../sui-indexer-alt-schema/migrations

# 2. 生成配置文件
cargo run --bin sui-indexer-alt -- generate-config > indexer_alt_config.toml

# 3. 运行索引器
cargo run --bin sui-indexer-alt -- indexer \
    --database-url postgres://postgres:postgrespw@localhost:5432/sui_indexer_alt \
    --remote-store-url https://checkpoints.mainnet.sui.io \
    --config indexer_alt_config.toml
```

### 9.2 运行 GraphQL 服务

```bash
cargo run --bin sui-indexer-alt-graphql -- rpc \
    --indexer-config indexer_alt_config.toml

# 服务端点:
# - http://localhost:7000/graphql (GraphQL)
# - http://localhost:7000/health (健康检查)
# - http://localhost:9184/metrics (Prometheus)
```

### 9.3 数据源选项

| 参数 | 说明 |
|------|------|
| `--remote-store-url` | 远程 checkpoint 存储 URL |
| `--local-ingestion-path` | 本地 checkpoint 文件路径 |
| `--rpc-api-url` | 全节点 RPC URL |
| `--streaming-url` | gRPC 流式 URL |

## 10. 关键设计决策

### 10.1 为何采用 Pipeline 架构

1. **模块化**: 每个数据类型独立处理，便于维护和扩展
2. **可配置**: 每个 pipeline 可独立配置并发度、批量大小等
3. **容错性**: 单个 pipeline 失败不影响其他 pipeline
4. **灵活性**: 可按需启用/禁用特定 pipeline

### 10.2 并发 vs 顺序 Pipeline

- **并发 Pipeline**: 适用于 append-only 数据，追求高吞吐
- **顺序 Pipeline**: 适用于需要原地更新的汇总表，保证一致性

### 10.3 Watermark 机制

- **CommitWatermark**: 标记已成功写入数据库的最高 checkpoint
- **ReaderWatermark**: 标记可安全用于读取的最低 checkpoint
- **PrunerWatermark**: 标记已清理数据的最高 checkpoint

这套机制确保：
1. 读取操作不会看到部分写入的数据
2. 数据可以按策略自动清理
3. 索引器重启后可正确恢复

## 11. 性能考量

### 11.1 批量写入
- Collector 收集多个 checkpoint 的数据后批量提交
- `MIN_EAGER_ROWS` 和 `MAX_PENDING_ROWS` 控制批量大小

### 11.2 并发控制
- `ingest_concurrency`: 控制 checkpoint 下载并发
- `write_concurrency`: 控制数据库写入并发
- `FANOUT`: 控制单个 pipeline 的处理并发

### 11.3 背压机制
当下游处理慢时，通过 channel 背压自动减慢上游：
1. Committer 写入慢 → Collector channel 满 → Processor 阻塞
2. Processor 阻塞 → Ingestion broadcaster 阻塞

## 12. 与 DEX 开发的关联

### 12.1 可复用的设计模式

1. **Pipeline 架构**: 可用于 DEX 的订单簿状态索引
2. **Watermark 机制**: 可用于跟踪已处理的交易
3. **并发处理模型**: 可用于高吞吐的市场数据处理

### 12.2 可能需要的扩展

1. **实时事件推送**: 当前设计是拉取模式，DEX 需要推送模式
2. **内存状态**: 当前写入数据库，DEX 可能需要内存订单簿
3. **更低延迟**: 当前关注吞吐量，DEX 需要关注延迟

### 12.3 学习要点

1. **Checkpoint 处理流程**: 理解 Sui 的数据结构和处理方式
2. **Object 模型**: 理解对象的创建、修改、删除
3. **Effects 解析**: 理解交易执行结果的结构

## 13. 总结

`sui-indexer-alt` 是一个成熟的区块链索引器实现，其核心特点：

1. **模块化 Pipeline 架构**: 支持灵活的数据处理和扩展
2. **双模式处理**: 并发提升吞吐，顺序保证一致
3. **完善的配置系统**: 支持细粒度调优
4. **多数据源支持**: 适应不同部署场景
5. **完整的水位管理**: 保证数据一致性和可恢复性

对于 DEX 开发，该索引器提供了：
- 交易和事件的完整索引能力
- 对象状态的历史追踪
- 余额查询的一致性保证
- 成熟的并发处理框架参考

---

## 参考文件索引

| 类别 | 文件路径 |
|------|----------|
| 框架入口 | [`crates/sui-indexer-alt-framework/src/lib.rs`](../../crates/sui-indexer-alt-framework/src/lib.rs) |
| Pipeline 核心 | [`crates/sui-indexer-alt-framework/src/pipeline/mod.rs`](../../crates/sui-indexer-alt-framework/src/pipeline/mod.rs) |
| 并发 Pipeline | [`crates/sui-indexer-alt-framework/src/pipeline/concurrent/mod.rs`](../../crates/sui-indexer-alt-framework/src/pipeline/concurrent/mod.rs) |
| 顺序 Pipeline | [`crates/sui-indexer-alt-framework/src/pipeline/sequential/mod.rs`](../../crates/sui-indexer-alt-framework/src/pipeline/sequential/mod.rs) |
| Ingestion | [`crates/sui-indexer-alt-framework/src/ingestion/mod.rs`](../../crates/sui-indexer-alt-framework/src/ingestion/mod.rs) |
| 主索引器 | [`crates/sui-indexer-alt/src/lib.rs`](../../crates/sui-indexer-alt/src/lib.rs) |
| Handler 目录 | [`crates/sui-indexer-alt/src/handlers/`](../../crates/sui-indexer-alt/src/handlers/) |
| 配置 | [`crates/sui-indexer-alt/src/config.rs`](../../crates/sui-indexer-alt/src/config.rs) |
| Schema | [`crates/sui-indexer-alt-schema/src/schema.rs`](../../crates/sui-indexer-alt-schema/src/schema.rs) |
| Consistent Store | [`crates/sui-indexer-alt-consistent-store/src/lib.rs`](../../crates/sui-indexer-alt-consistent-store/src/lib.rs) |
| GraphQL | [`crates/sui-indexer-alt-graphql/README.md`](../../crates/sui-indexer-alt-graphql/README.md) |
| 主 README | [`crates/sui-indexer-alt/README.md`](../../crates/sui-indexer-alt/README.md) |
