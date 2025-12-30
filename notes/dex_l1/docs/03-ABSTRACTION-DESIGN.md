# DEX L1 抽象层设计 / Abstraction Layer Design

> **版本**: v1.0
> **状态**: Draft
> **目标读者**: 技术评审 / 架构师

---

## 1. 概述 / Overview

### 1.1 设计目标 / Design Goals

1. **DEX VM 与 Move VM 解耦**: 支持独立演进
2. **优雅复用 Sui 基础设施**: 最大化利用现有组件
3. **最小化侵入式修改**: 便于跟踪 Sui 上游更新
4. **避免重复造轮子**: 复用优先，必要时才自实现

### 1.2 设计原则 / Design Principles

| 原则 | 描述 |
|-----|------|
| **复用优先 (Reuse First)** | 优先使用 Sui 现有组件 |
| **分层解耦 (Layered Decoupling)** | 通过抽象层隔离变化 |
| **接口隔离 (Interface Segregation)** | 定义最小必要接口 |
| **依赖倒置 (Dependency Inversion)** | 依赖抽象而非具体实现 |
| **最小侵入 (Minimal Invasion)** | 扩展而非修改 Sui 代码 |

---

## 2. Sui 可复用组件分析 / Sui Reusable Components

### 2.1 必须复用 (直接依赖) / Must Reuse

| 组件 | 路径 | DEX 使用场景 |
|-----|------|-------------|
| **typed-store** | `crates/typed-store/` | KV 存储抽象 + RocksDB 封装 |
| **mysten-network** | `crates/mysten-network/` | P2P 网络 (anemo) + gRPC |
| **mysten-metrics** | `crates/mysten-metrics/` | Prometheus 指标 + 追踪 |
| **shared-crypto** | `crates/shared-crypto/` | 签名验证 + Intent 框架 |
| **mysten-common** | `crates/mysten-common/` | 通用工具 + 压缩 |

#### typed-store 复用策略
```rust
// ✅ 使用 typed-store 的 DBMap
use typed_store::rocks::DBMap;
use typed_store_derive::DBMapUtils;

#[derive(DBMapUtils)]
pub struct DexTables {
    pub orders: DBMap<OrderId, Order>,
    pub balances: DBMap<(AccountId, AssetId), Balance>,
}

// ❌ 禁止自己封装 RocksDB
use rocksdb::DB; // 禁止！
```

#### mysten-network 复用策略
```rust
// ✅ 使用 anemo 网络层
use mysten_network::config::Config;
use anemo::Network;

// ❌ 禁止自己实现 P2P
use libp2p::*; // 禁止！
```

### 2.2 强烈建议复用 / Strongly Recommended

| 组件 | 路径 | DEX 使用场景 |
|-----|------|-------------|
| **sui-storage** | `crates/sui-storage/` | ShardedLRU 缓存 + 云存储 |
| **sui-types** (部分) | `crates/sui-types/` | 地址 + Effects + Gas |
| **consensus-config** | `consensus/config/` | 共识参数配置 |

### 2.3 Workspace 共享依赖 / Workspace Dependencies

| 依赖 | 用途 |
|-----|------|
| **fastcrypto** | Ed25519/Secp256k1/BLS 签名 |
| **bcs** | 确定性序列化 |
| **dashmap** | 无锁并发 HashMap |
| **parking_lot** | 高性能锁原语 |
| **tokio** | 异步运行时 |
| **tracing** | 结构化日志 |
| **prometheus** | 指标采集 |

---

## 3. 复用依赖图 / Reuse Dependency Graph

```
┌─────────────────────────────────────────────────────────────────┐
│                     DEX L1 Application                          │
├─────────────────────────────────────────────────────────────────┤
│  dex-engine │ dex-sequencer │ dex-integration                  │
├─────────────┴───────────────┴───────────────────────────────────┤
│                    DEX Custom Layer                             │
│  dex-types  │  dex-storage (WAL + Snapshot)                    │
├─────────────────────────────────────────────────────────────────┤
│              Sui Reusable Infrastructure (直接依赖)             │
│  ┌───────────────┐ ┌───────────────┐ ┌───────────────────────┐ │
│  │ typed-store   │ │mysten-network │ │ shared-crypto         │ │
│  │ (RocksDB KV)  │ │(anemo + gRPC) │ │ (Intent + Signature)  │ │
│  └───────────────┘ └───────────────┘ └───────────────────────┘ │
│  ┌───────────────┐ ┌───────────────┐ ┌───────────────────────┐ │
│  │mysten-metrics │ │ mysten-common │ │ sui-storage (可选)    │ │
│  │ (Prometheus)  │ │ (Utilities)   │ │ (Cloud + Cache)       │ │
│  └───────────────┘ └───────────────┘ └───────────────────────┘ │
├─────────────────────────────────────────────────────────────────┤
│              Workspace Shared Dependencies                      │
│  fastcrypto │ bcs │ dashmap │ tokio │ tracing │ prometheus     │
└─────────────────────────────────────────────────────────────────┘
```

---

## 4. 抽象层架构 / Abstraction Layer Architecture

### 4.1 四层抽象 / Four-Layer Abstraction

```
┌─────────────────────────────────────────────────────────┐
│                    Application Layer                     │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────┐  │
│  │  DEX App    │  │  Move App   │  │  Future Apps    │  │
│  └──────┬──────┘  └──────┬──────┘  └────────┬────────┘  │
├─────────┴────────────────┴──────────────────┴───────────┤
│               Execution Abstraction Layer                │
│  ┌─────────────────────────────────────────────────┐    │
│  │           ExecutionEngine Trait                 │    │
│  └─────────────────────────────────────────────────┘    │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────┐  │
│  │  DexEngine  │  │  MoveVM     │  │  HybridEngine   │  │
│  │  Adapter    │  │  Adapter    │  │  (DEX + Move)   │  │
│  └─────────────┘  └─────────────┘  └─────────────────┘  │
├─────────────────────────────────────────────────────────┤
│               Consensus Abstraction Layer                │
│  ┌─────────────────────────────────────────────────┐    │
│  │           ConsensusProvider Trait               │    │
│  └─────────────────────────────────────────────────┘    │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────┐  │
│  │  Sequencer  │  │  Mysticeti  │  │  Hybrid         │  │
│  │  Provider   │  │  Provider   │  │  (Fast + BFT)   │  │
│  └─────────────┘  └─────────────┘  └─────────────────┘  │
├─────────────────────────────────────────────────────────┤
│                 Storage Abstraction Layer                │
│  ┌─────────────────────────────────────────────────┐    │
│  │           StateStore Trait                      │    │
│  └─────────────────────────────────────────────────┘    │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────┐  │
│  │  DexStore   │  │  SuiStore   │  │  Unified Store  │  │
│  └─────────────┘  └─────────────┘  └─────────────────┘  │
├─────────────────────────────────────────────────────────┤
│              Network Abstraction Layer                   │
│  ┌─────────────────────────────────────────────────┐    │
│  │           NetworkService Trait                  │    │
│  └─────────────────────────────────────────────────┘    │
│           (复用 Sui anemo P2P 网络层)                    │
└─────────────────────────────────────────────────────────┘
```

### 4.2 核心 Trait 设计 / Core Trait Design

#### ExecutionEngine Trait
```rust
/// 执行引擎抽象 / Execution Engine Abstraction
pub trait ExecutionEngine: Send + Sync {
    /// 执行交易 / Execute transaction
    fn execute(&self, tx: &Transaction) -> Result<Effects, ExecutionError>;

    /// 验证交易 / Validate transaction
    fn validate(&self, tx: &Transaction) -> Result<(), ValidationError>;

    /// 获取执行效果 / Get execution effects
    fn get_effects(&self, tx_digest: &TxDigest) -> Result<Option<Effects>>;
}

/// DEX 引擎适配器 / DEX Engine Adapter
pub struct DexEngineAdapter {
    engine: Arc<MatchingEngine>,
    sequencer: Arc<Sequencer>,
}

impl ExecutionEngine for DexEngineAdapter {
    fn execute(&self, tx: &Transaction) -> Result<Effects> {
        // 1. 序列号分配
        // 2. 撮合执行
        // 3. 状态更新
        // 4. 生成 Effects
    }
}

/// Move VM 适配器 / Move VM Adapter
pub struct MoveVMAdapter {
    vm: Arc<MoveVM>,
}

impl ExecutionEngine for MoveVMAdapter {
    fn execute(&self, tx: &Transaction) -> Result<Effects> {
        // 委托给标准 Sui 执行流程
    }
}
```

#### ConsensusProvider Trait
```rust
/// 共识提供者抽象 / Consensus Provider Abstraction
pub trait ConsensusProvider: Send + Sync {
    /// 提交交易 / Submit transaction
    fn submit(&self, tx: Transaction) -> Result<SeqNumber>;

    /// 订阅共识输出 / Subscribe to consensus output
    fn subscribe(&self) -> Receiver<ConsensusOutput>;

    /// 获取已确认交易 / Get committed transactions
    fn get_committed(&self, seq: SeqNumber) -> Result<Option<Transaction>>;
}

/// Sequencer 提供者 (< 50ms)
pub struct SequencerProvider {
    sequencer: Arc<Sequencer>,
}

/// Mysticeti 提供者 (~600ms)
pub struct MysticetiProvider {
    consensus: Arc<ConsensusEngine>,
}

/// 混合共识 / Hybrid Consensus
pub struct HybridConsensus {
    sequencer: SequencerProvider,  // DEX 交易
    mysticeti: MysticetiProvider,   // Move 交易
    classifier: TransactionClassifier,
}
```

#### StateStore Trait
```rust
/// 状态存储抽象 / State Store Abstraction
pub trait StateStore: Send + Sync {
    /// 读取状态 / Read state
    fn read(&self, key: &[u8]) -> Result<Option<Vec<u8>>>;

    /// 写入状态 / Write state
    fn write(&self, key: &[u8], value: &[u8]) -> Result<()>;

    /// 批量写入 / Batch write
    fn write_batch(&self, batch: WriteBatch) -> Result<()>;

    /// 创建快照 / Create snapshot
    fn snapshot(&self) -> Result<Snapshot>;

    /// 从快照恢复 / Recover from snapshot
    fn recover(&self, snapshot: &Snapshot) -> Result<()>;
}
```

#### TransactionClassifier Trait
```rust
/// 交易分类器 / Transaction Classifier
pub trait TransactionClassifier: Send + Sync {
    /// 分类交易类型 / Classify transaction type
    fn classify(&self, tx: &Transaction) -> TransactionType;
}

pub enum TransactionType {
    /// 纯 DEX 交易 → Sequencer + DexEngine
    DexOnly,
    /// 纯 Move 交易 → Mysticeti + MoveVM
    MoveOnly,
    /// 混合交易 (存取款) → 特殊处理
    Hybrid,
}
```

---

## 5. Sui 扩展点分析 / Sui Extension Points

### 5.1 Authority 层扩展

```
sui-core/src/authority.rs
┌─────────────────────────────────────────────────────────────┐
│                      Authority                               │
│  ┌─────────────────────────────────────────────────────────┐│
│  │  handle_transaction()                                    ││
│  │       │                                                  ││
│  │       ▼                                                  ││
│  │  ┌─────────────────┐                                    ││
│  │  │ Transaction     │ ◄── 扩展点: 注入 TransactionRouter ││
│  │  │ Router          │                                    ││
│  │  └────────┬────────┘                                    ││
│  │           │                                              ││
│  │     ┌─────┴─────┐                                       ││
│  │     ▼           ▼                                       ││
│  │  ┌──────┐   ┌──────┐                                    ││
│  │  │ DEX  │   │ Sui  │                                    ││
│  │  │ Path │   │ Path │                                    ││
│  │  └──────┘   └──────┘                                    ││
│  └─────────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────────┘
```

**修改策略**: 依赖注入，不修改 authority.rs 源码

### 5.2 Execution 层扩展

```
sui-execution/src/execution_engine.rs
┌─────────────────────────────────────────────────────────────┐
│                    Execution Engine                          │
│  ┌─────────────────────────────────────────────────────────┐│
│  │  execute_transaction()                                   ││
│  │       │                                                  ││
│  │       ▼                                                  ││
│  │  ┌─────────────────┐                                    ││
│  │  │ Precompile      │ ◄── 扩展点: 注册 DEX Precompile    ││
│  │  │ Check           │                                    ││
│  │  └────────┬────────┘                                    ││
│  │           │                                              ││
│  │     ┌─────┴─────┐                                       ││
│  │     ▼           ▼                                       ││
│  │  ┌──────┐   ┌──────┐                                    ││
│  │  │ DEX  │   │ Move │                                    ││
│  │  │Native│   │ VM   │                                    ││
│  │  └──────┘   └──────┘                                    ││
│  └─────────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────────┘
```

**修改策略**: Precompile 注册机制，保持 Move VM 不变

### 5.3 Storage 层扩展

```
typed-store + dex-storage
┌─────────────────────────────────────────────────────────────┐
│                    Storage Layer                             │
│  ┌───────────────────────┐  ┌───────────────────────────┐  │
│  │    Sui Tables         │  │    DEX Tables             │  │
│  │  ┌─────────────────┐  │  │  ┌─────────────────────┐  │  │
│  │  │ objects         │  │  │  │ orders              │  │  │
│  │  │ transactions    │  │  │  │ balances            │  │  │
│  │  │ effects         │  │  │  │ trades              │  │  │
│  │  └─────────────────┘  │  │  └─────────────────────┘  │  │
│  └───────────────────────┘  └───────────────────────────┘  │
│                    │                      │                 │
│                    └──────────┬───────────┘                 │
│                               ▼                             │
│                    ┌─────────────────────┐                  │
│                    │     RocksDB         │                  │
│                    │  (Shared Instance)  │                  │
│                    └─────────────────────┘                  │
└─────────────────────────────────────────────────────────────┘
```

**修改策略**: 添加 DEX 专用 Column Family，扩展而非替换

---

## 6. 模块边界设计 / Module Boundary Design

### 6.1 代码组织 / Code Organization

```
crates/
├── sui-core-ext/           # Sui Core 扩展 (不修改 sui-core)
│   ├── src/
│   │   ├── lib.rs
│   │   ├── router.rs       # TransactionRouter
│   │   ├── dispatcher.rs   # ExecutionDispatcher
│   │   └── bridge.rs       # Sui 桥接
│   └── Cargo.toml
│
├── dex-runtime/            # DEX 运行时
│   ├── src/
│   │   ├── lib.rs
│   │   ├── engine.rs       # ExecutionEngine impl
│   │   ├── consensus.rs    # ConsensusProvider impl
│   │   └── store.rs        # StateStore impl
│   └── Cargo.toml
│
├── abstraction/            # 抽象 Trait 定义
│   ├── src/
│   │   ├── lib.rs
│   │   ├── execution.rs    # ExecutionEngine trait
│   │   ├── consensus.rs    # ConsensusProvider trait
│   │   ├── storage.rs      # StateStore trait
│   │   └── classifier.rs   # TransactionClassifier trait
│   └── Cargo.toml
│
├── dex-types/              # (已有) DEX 类型定义
├── dex-engine/             # (已有) 撮合引擎
├── dex-sequencer/          # (已有) 排序器
├── dex-storage/            # (已有) 存储层
└── dex-integration/        # (已有) Sui 集成
```

### 6.2 模块职责 / Module Responsibilities

| 模块 | 职责 | 依赖 |
|-----|------|------|
| **abstraction** | 定义核心 Trait | 无外部依赖 |
| **sui-core-ext** | Sui Core 扩展层 | sui-core, abstraction |
| **dex-runtime** | DEX 运行时 | dex-*, abstraction |
| **dex-types** | 类型定义 | sui-types |
| **dex-engine** | 撮合引擎 | dex-types, dashmap |
| **dex-sequencer** | 排序器 | dex-types, mysten-network |
| **dex-storage** | 存储层 | dex-types, typed-store |
| **dex-integration** | Sui 集成 | dex-*, sui-core-ext |

---

## 7. 依赖管理策略 / Dependency Management

### 7.1 上游跟踪策略 / Upstream Sync Strategy

```
┌─────────────────────────────────────────────────────────────┐
│                  Upstream Sync Workflow                      │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  Sui Upstream ──► Fork Repo ──► DEX Branch                  │
│       │               │              │                       │
│       │               │              │                       │
│       └───────────────┴──────────────┘                       │
│                       │                                      │
│              ┌────────┴────────┐                            │
│              ▼                 ▼                            │
│       ┌──────────┐      ┌──────────┐                        │
│       │ Sui Core │      │ DEX Ext  │                        │
│       │ (不修改) │      │ (扩展)   │                        │
│       └──────────┘      └──────────┘                        │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

**关键原则**：
1. **最小化 Fork 修改**: 只添加扩展点钩子
2. **使用 Patch 文件**: 跟踪所有修改
3. **定期 Rebase**: 每周同步上游更新

### 7.2 修改点清单 / Modification Checklist

| 文件 | 修改类型 | 说明 |
|-----|---------|------|
| `sui-node/src/main.rs` | 配置注入 | 加载 DEX 配置 |
| `sui-core/src/authority.rs` | Hook 注入 | TransactionRouter |
| `sui-execution/src/lib.rs` | Precompile 注册 | DEX Precompile |
| `Cargo.toml` (根) | 依赖添加 | 添加 dex-* crates |

### 7.3 版本兼容 / Version Compatibility

```rust
// Trait 版本化
pub trait ExecutionEngine {
    const VERSION: u32 = 1;

    fn execute(&self, tx: &Transaction) -> Result<Effects>;

    // v2 新增方法使用 default 实现
    fn execute_batch(&self, txs: &[Transaction]) -> Result<Vec<Effects>> {
        txs.iter().map(|tx| self.execute(tx)).collect()
    }
}
```

---

## 8. 复用策略总结 / Reuse Strategy Summary

### 8.1 存储层复用策略

| 需求 | 使用 | 禁止 |
|-----|------|------|
| RocksDB 封装 | `typed-store` | 自己封装 |
| KV 抽象 | `Map<K,V>` trait | 自定义 trait |
| 表定义 | `DBMapUtils` 宏 | 手动实现 |
| 缓存 | `sui-storage::ShardedLRU` | 自己实现 |
| **DEX 专用** | WAL + Snapshot | - |

### 8.2 网络层复用策略

| 需求 | 使用 | 禁止 |
|-----|------|------|
| P2P 网络 | `anemo` | libp2p |
| RPC 框架 | `tonic/gRPC` | 自定义协议 |
| 编解码 | `BCS + Protobuf` | 自定义格式 |
| **DEX 专用** | Sequencer 广播协议 | - |

### 8.3 密码学复用策略

| 需求 | 使用 | 禁止 |
|-----|------|------|
| 签名验证 | `fastcrypto` | 自己实现 |
| Intent 框架 | `shared-crypto` | 自定义 |
| **DEX 专用** | DEX Intent Scope | - |

---

## 9. 配置与特性开关 / Configuration & Feature Flags

### 9.1 运行时配置

```toml
# dex-config.toml
[dex]
enabled = true
mode = "hybrid"  # "dex_only" | "move_only" | "hybrid"

[dex.sequencer]
batch_size = 1000
batch_timeout_ms = 5

[dex.engine]
markets = ["BTC-USDT", "ETH-USDT"]

[dex.storage]
wal_path = "/data/dex/wal"
snapshot_interval = 10000
```

### 9.2 Feature Flags

```toml
# Cargo.toml
[features]
default = ["dex"]
dex = ["dex-engine", "dex-sequencer", "dex-storage"]
dex-metrics = ["mysten-metrics"]
dex-profiling = ["pprof"]
```

---

## 10. 升级与迁移 / Upgrade & Migration

### 10.1 热升级支持

```
┌─────────────────────────────────────────────────────────────┐
│                    Hot Upgrade Flow                          │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  1. 暂停新订单接收                                           │
│  2. 等待 pending 订单处理完成                                │
│  3. 创建状态快照                                             │
│  4. 加载新版本代码                                           │
│  5. 从快照恢复状态                                           │
│  6. 恢复订单接收                                             │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

### 10.2 状态迁移方案

```rust
// 版本化状态结构
#[derive(Serialize, Deserialize)]
pub struct DexState {
    pub version: u32,
    pub data: DexStateData,
}

// 迁移函数
pub fn migrate(old: DexState) -> Result<DexState> {
    match old.version {
        1 => migrate_v1_to_v2(old),
        2 => Ok(old), // 当前版本
        _ => Err(Error::UnsupportedVersion),
    }
}
```

---

*文档版本: v1.0 | 最后更新: 2025-01-01*
