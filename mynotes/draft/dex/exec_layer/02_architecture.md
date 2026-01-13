# DEX 执行层架构设计文档

> **版本**: v1.0  
> **日期**: 2026-01-08  
> **状态**: 架构设计文档  
> **目标**: 基于 PRD 的单节点 DEX 执行层架构设计  
> **参考**: `prd/DEX完整业务需求.md`、`exec_layer/01_research.md`、`arch/sui_dex_arch.md`（二期参考）

---

## 目录

1. [执行摘要](#1-执行摘要)
2. [整体架构](#2-整体架构)
3. [模块划分](#3-模块划分)
4. [技术栈选型](#4-技术栈选型)
5. [Object 模型集成设计](#5-object-模型集成设计) ⭐ **重点章节**
6. [存储层设计（Object 模型集成）](#6-存储层设计object-模型集成) ⭐ **重点章节**
7. [数据流设计](#7-数据流设计)
8. [接口设计](#8-接口设计)
9. [可观测性设计](#9-可观测性设计)

---

## 1. 执行摘要

本文档定义了**单节点 DEX 执行层**的架构设计，聚焦 Phase 1（单节点验证）的实现，同时考虑 Phase 2（共识集成）的兼容性。

**核心设计原则**：
1. **Object 模型优先**：优先使用 Sui Object 模型存储账户、资产等状态
2. **一期聚焦单节点**：不涉及共识层，专注于执行层性能
3. **二期兼容设计**：所有设计考虑 Phase 1 到 Phase 2 的平滑演进
4. **复用 Sui 组件**：尽可能复用 Sui 基础设施（typed-store、事件系统等）
5. **性能目标**：延迟 < 50ms、吞吐量 200K TPS、撮合 < 10μs

**关键决策**：
- ✅ **使用 Object 模型**：账户、资产使用 Owned Object，数据结构完全兼容
- ✅ **内存订单簿**：Phase 1 优先使用内存订单簿，性能优先
- ✅ **版本控制**：使用 Lamport 时间戳算法，Phase 1 和 Phase 2 算法相同
- ⚠️ **FastPath 简化**：Phase 1 单节点直接执行，Phase 2 启用完整 FastPath

---

## 2. 整体架构

### 2.1 五层架构图

```
┌─────────────────────────────────────────────────────────────────┐
│  Layer 5: API Layer (API 层)                                     │
│  ├─ JSON-RPC Server (订单提交、查询)                             │
│  ├─ WebSocket Server (实时行情推送)                              │
│  └─ Rate Limiter (限流)                                         │
└─────────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────────┐
│  Layer 4: Sequencer Layer (排序器层)                             │
│  ├─ Transaction Sequencer (全局序列号分配)                       │
│  ├─ Batch Aggregation (5ms 或 1000 tx)                          │
│  └─ Sequence Assignment ([Epoch:16][Counter:48])                │
└─────────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────────┐
│  Layer 3: Execution Layer (执行层)                               │
│  ├─ Native Matching Engine (原生撮合引擎,< 10μs)                 │
│  │   ├─ Orderbook (内存订单簿)                                   │
│  │   ├─ Risk Engine (风控引擎)                                   │
│  │   └─ Perpetual Engine (永续引擎)                              │
│  └─ Object State Manager (Object 状态管理器)                     │
│      ├─ Account Object Manager                                  │
│      ├─ Asset Object Manager                                    │
│      └─ Version Manager (Lamport 时间戳)                         │
└─────────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────────┐
│  Layer 2: Storage Layer (存储层)                                 │
│  ├─ Memory Cache (热数据)                                       │
│  │   ├─ Active Orderbooks (实时订单簿)                           │
│  │   └─ Balance Cache (账户余额缓存)                             │
│  ├─ WAL (Write-Ahead Log, < 10ms)                               │
│  │   ├─ Group Commit (批量 fsync)                                │
│  │   └─ Sequence-based Replay (基于序列号回放)                   │
│  └─ typed-store (RocksDB, Object 持久化)                         │
│      ├─ ObjectStore (Object 存储)                                │
│      ├─ AccountStore (账户存储)                                  │
│      └─ AssetStore (资产存储)                                    │
└─────────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────────┐
│  Layer 1: Event & Monitoring Layer (事件与监控层)                 │
│  ├─ Event System (事件发布)                                      │
│  │   ├─ Order Events (订单事件)                                  │
│  │   ├─ Trade Events (成交事件)                                  │
│  │   └─ Account Events (账户事件)                                │
│  └─ Metrics (监控指标)                                           │
│      ├─ Prometheus (mysten-metrics)                              │
│      └─ Custom Metrics (自定义指标)                              │
└─────────────────────────────────────────────────────────────────┘
```

### 2.2 架构设计原则

| 原则 | 说明 | 体现 |
|-----|------|------|
| **性能优先** | 满足 < 50ms 延迟和 200K TPS | 原生 Rust 引擎 + 无锁并发 |
| **Object 模型优先** | 优先使用 Object 模型存储状态 | 账户、资产使用 Owned Object |
| **兼容性设计** | Phase 1 到 Phase 2 平滑演进 | 数据结构、算法完全兼容 |
| **复用成熟组件** | 80% 复用 Sui 基础设施 | typed-store、事件系统、网络层 |
| **可观测性** | 完整监控与调试能力 | mysten-metrics + 自定义指标 |

---

## 3. 模块划分

基于 `prd/DEX完整业务需求.md` 的 12 个模块，按执行层职责划分：

### 3.1 核心执行模块

| 模块 | 职责 | Phase 1 实现 | Phase 2 兼容性 |
|-----|------|------------|--------------|
| **账户模块** | 账户、资产持仓、仓位管理 | Owned Object + 内存缓存 | ✅ 数据结构兼容 |
| **资产模块** | 资产定义、单位换算 | Immutable Object | ✅ 完全兼容 |
| **撮合结算模块** | 订单簿、撮合、结算 | 内存订单簿 + 原生引擎 | ⚠️ 可选持久化到 Object |
| **风险控制模块** | 保证金计算、风控检查 | 内存状态 + 原生引擎 | ✅ 算法兼容 |
| **合约模块** | 永续合约参数、仓位管理 | Owned Object + 内存状态 | ✅ 数据结构兼容 |
| **资金费率模块** | 资金费率计算、结算 | 内存状态 + 原生引擎 | ✅ 算法兼容 |
| **清算模块** | 清算触发、执行 | 内存状态 + 原生引擎 | ✅ 算法兼容 |

### 3.2 支撑模块

| 模块 | 职责 | Phase 1 实现 | Phase 2 兼容性 |
|-----|------|------------|--------------|
| **上币与市场模块** | 市场配置、上币管理 | Immutable Object | ✅ 完全兼容 |
| **价格预言机模块** | 价格数据、标记价格 | 内存状态 + 外部 API | ⚠️ 可选 Object 存储 |
| **手续费层与收入分成** | 手续费计算、分成 | 内存状态 + 原生引擎 | ✅ 算法兼容 |
| **交易奖励模块** | 奖励计算、发放 | Owned Object + 内存状态 | ✅ 数据结构兼容 |
| **协议 Vault 机制** | Vault 余额、收益分配 | Owned Object | ✅ 数据结构兼容 |

**详细业务需求**：参考 `prd/DEX完整业务需求.md`。

---

## 4. 技术栈选型

### 4.1 核心依赖（复用 Sui）

| 组件 | 选型 | 来源 | 用途 |
|-----|------|------|------|
| **Object 数据结构** | `sui-types/src/object.rs` | Sui | ⭐⭐⭐⭐⭐ **完全复用** |
| **存储层** | typed-store (RocksDB) | Sui | Object 持久化 |
| **序列化** | bcs | Sui | Object 序列化 |
| **事件系统** | `sui-types/src/event.rs` | Sui | 事件发布 |
| **签名验证** | shared-crypto | Sui | 交易签名验证 |
| **监控指标** | mysten-metrics | Sui | Prometheus 集成 |
| **网络层** | Tonic + anemo | Sui | Phase 2 启用 |

### 4.2 自研组件

| 组件 | 技术选型 | 性能目标 |
|-----|---------|---------|
| **撮合引擎** | Rust + BTreeMap + SIMD | < 10μs 单次撮合 |
| **风控引擎** | Rust（保证金计算） | < 1ms 验证 |
| **清算引擎** | Rust（价格监控） | < 5ms 触发 |
| **永续引擎** | Rust（资金费率） | < 10ms 结算 |
| **Sequencer** | Rust（序列号分配） | < 1ms 排序 |
| **版本管理器** | Rust（Lamport 时间戳） | < 1μs 分配 |

### 4.3 第三方依赖

| 组件 | 选型 | 用途 |
|-----|------|------|
| **并发数据结构** | dashmap | 无锁 HashMap |
| **异步运行时** | tokio | 异步 I/O |
| **压缩** | lz4 | 快照压缩 |

---

## 5. Object 模型集成设计 ⭐

> **本章节重点设计 Object 模型在各个模块中的应用，以及 Phase 1 和 Phase 2 的兼容性设计。**

### 5.1 数据结构设计（复用 Sui Object 结构）

#### 5.1.1 Object 数据结构复用

**原则**：直接复用 Sui 的 Object 数据结构定义，不修改任何字段或编码格式。

**位置**：`crates/sui-types/src/object.rs`

```rust
// 直接复用 Sui 的 Object 结构
use sui_types::object::{Object, MoveObject, Owner};
use sui_types::base_types::{ObjectID, SequenceNumber};

// 不修改任何定义，确保完全兼容
```

**兼容性保证**：
- ✅ Phase 1 和 Phase 2 使用相同的 Object 结构
- ✅ 数据格式完全兼容，可无缝迁移
- ✅ ZK-Rollup 可以验证 Object 结构的正确性

#### 5.1.2 ObjectID 生成设计

**原则**：使用与 Sui 完全相同的 ObjectID 生成算法。

**算法**：
```rust
// 位置: crates/sui-types/src/base_types.rs
pub fn generate_object_id(tx_digest: &TransactionDigest, index: u64) -> ObjectID {
    // 算法与 Sui 完全一致
    ObjectID::from(hash(tx_digest, index))
}
```

**Phase 1 实现**：
```rust
// Phase 1: 从 Sequencer 分配的交易摘要生成
pub struct ObjectIDGenerator {
    // 单节点无需状态，但保留接口兼容性
}

impl ObjectIDGenerator {
    pub fn generate(&self, tx_digest: &TransactionDigest, index: u64) -> ObjectID {
        ObjectID::from(hash(tx_digest, index))
    }
}
```

**Phase 2 兼容性**：
- ✅ Sui 共识：算法完全相同，直接兼容
- ✅ ZK-Rollup：ID 生成算法不变，只需在 ZK 电路中验证

### 5.2 版本控制设计（Phase 1 简化 vs Phase 2 完整）

#### 5.2.1 Lamport 时间戳算法

**原则**：使用与 Sui 完全相同的 Lamport 时间戳算法。

**算法**：
```rust
// 公式: max(input_versions) + 1
pub fn lamport_timestamp(
    input_versions: &[SequenceNumber],
    receiving_versions: &[SequenceNumber],
) -> SequenceNumber {
    let max = input_versions.iter()
        .chain(receiving_versions.iter())
        .max()
        .copied()
        .unwrap_or(SequenceNumber::MIN);
    max + 1
}
```

**位置**：参考 `sui-execution/latest/sui-adapter/src/temporary_store.rs`、`crates/sui-types/src/transaction.rs:4530`

#### 5.2.2 Phase 1 版本管理器（简化实现）

**设计**：
```rust
// Phase 1: 单节点版本号分配（简化实现）
pub struct Phase1VersionManager {
    // 单节点无需并发控制，但保留接口兼容性
}

impl Phase1VersionManager {
    /// 分配新版本号（Lamport 时间戳算法）
    pub fn assign_version(
        &self,
        input_versions: &[SequenceNumber],
        receiving_versions: &[SequenceNumber],
    ) -> SequenceNumber {
        // 算法与 Sui 完全一致
        lamport_timestamp(input_versions, receiving_versions)
    }
}
```

**特点**：
- ✅ 算法与 Sui 完全相同
- ✅ 单节点实现简单，性能优异
- ✅ 数据结构完全兼容

#### 5.2.3 Phase 2 版本管理器（完整实现）

**设计**：
```rust
// Phase 2: 复用 Sui 的版本号分配逻辑
pub struct Phase2VersionManager {
    temporary_store: TemporaryStore,  // 复用 Sui 的实现
}

impl Phase2VersionManager {
    pub fn assign_version(
        &self,
        input_objects: &InputObjects,
        receiving_objects: &[ObjectRef],
    ) -> SequenceNumber {
        // 使用 Sui 的 lamport_timestamp 计算逻辑
        input_objects.lamport_timestamp(receiving_objects)
    }
}
```

**兼容性**：
- ✅ Phase 1 和 Phase 2 的算法完全相同
- ✅ Phase 1 的版本号序列在 Phase 2 中有效
- ✅ ZK-Rollup 可以验证版本号计算的正确性

#### 5.2.4 抽象层设计

**原则**：通过抽象层隔离 Phase 1 和 Phase 2 的实现差异。

**设计**：
```rust
// 抽象接口
pub trait VersionManager: Send + Sync {
    fn assign_version(
        &self,
        input_versions: &[SequenceNumber],
        receiving_versions: &[SequenceNumber],
    ) -> SequenceNumber;
}

// Phase 1 实现
pub struct Phase1VersionManager { /* ... */ }
impl VersionManager for Phase1VersionManager {
    fn assign_version(&self, input: &[SequenceNumber], receiving: &[SequenceNumber]) -> SequenceNumber {
        lamport_timestamp(input, receiving)
    }
}

// Phase 2 实现
pub struct Phase2VersionManager {
    temporary_store: TemporaryStore,
}
impl VersionManager for Phase2VersionManager {
    fn assign_version(&self, input: &[SequenceNumber], receiving: &[SequenceNumber]) -> SequenceNumber {
        // 复用 Sui 的实现
        self.temporary_store.lamport_timestamp(input, receiving)
    }
}
```

**优势**：
- ✅ 接口统一，代码复用
- ✅ Phase 1 到 Phase 2 的迁移只需替换实现
- ✅ 测试和验证更容易

### 5.3 ID 生成设计（兼容 Sui 算法）

**原则**：使用与 Sui 完全相同的 ObjectID 生成算法。

**实现**：
```rust
// Phase 1 和 Phase 2 使用相同的算法
pub struct ObjectIDGenerator;

impl ObjectIDGenerator {
    pub fn generate(&self, tx_digest: &TransactionDigest, index: u64) -> ObjectID {
        // 算法与 Sui 完全一致
        ObjectID::from(hash(tx_digest, index))
    }
}
```

**兼容性保证**：
- ✅ 相同交易和索引总是产生相同 ID
- ✅ Phase 1 生成的 ID 在 Phase 2 中有效
- ✅ ZK-Rollup 可以验证 ID 生成的正确性

### 5.4 抽象层设计（隔离 Phase 1 和 Phase 2）

**原则**：通过抽象层隔离 Phase 1 和 Phase 2 的实现差异。

**设计**：
```rust
// Object 状态管理器抽象
pub trait ObjectStateManager: Send + Sync {
    async fn create_object(&self, data: Vec<u8>, owner: Owner) -> Result<ObjectID>;
    async fn update_object(&self, object_id: ObjectID, data: Vec<u8>) -> Result<SequenceNumber>;
    async fn get_object(&self, object_id: ObjectID) -> Result<Option<Object>>;
}

// Phase 1 实现
pub struct Phase1ObjectStateManager {
    version_manager: Phase1VersionManager,
    storage: typed_store::ObjectStore,
}
impl ObjectStateManager for Phase1ObjectStateManager { /* ... */ }

// Phase 2 实现
pub struct Phase2ObjectStateManager {
    version_manager: Phase2VersionManager,
    storage: typed_store::ObjectStore,
}
impl ObjectStateManager for Phase2ObjectStateManager { /* ... */ }
```

**优势**：
- ✅ 接口统一，代码复用
- ✅ Phase 1 到 Phase 2 的迁移只需替换实现
- ✅ 测试和验证更容易

---

## 6. 存储层设计（Object 模型集成）⭐

> **本章节重点设计 Object 模型在各个存储模块中的应用，以及 Phase 1 和 Phase 2 的存储格式兼容性设计。**

### 6.1 账户 Object 设计

#### 6.1.1 账户 Object 结构

**设计**：使用 Owned Object 存储账户状态。

**结构**：
```rust
// 账户 Object 内容（BCS 编码）
pub struct AccountObject {
    // ObjectID（存储在 Object 元数据中，不在内容中）
    // id: ObjectID,  // Object 元数据字段
    
    // 账户字段
    owner: SuiAddress,           // 账户所有者（32 字节）
    subaccount_index: u32,       // 子账户编号（4 字节）
    
    // 资产持仓
    asset_balances: Vec<AssetBalance>,  // 资产余额列表
    
    // 永续仓位
    perpetual_positions: Vec<PerpetualPosition>,  // 仓位列表
    
    // 元数据
    last_update_timestamp: u64,  // 最后更新时间戳（8 字节）
}
```

**Object 元数据**：
- **Owner**: `AddressOwner(owner_address)`（账户所有者）
- **Version**: `SequenceNumber`（Lamport 时间戳）
- **ObjectID**: 从交易摘要生成

**存储**：
- **Phase 1**: typed-store (RocksDB) + 内存缓存
- **Phase 2**: 直接接入 Sui 共识，存储格式完全兼容

#### 6.1.2 账户 Object 生命周期

**创建**：
```rust
// 创建账户 Object
pub fn create_account_object(
    owner: SuiAddress,
    subaccount_index: u32,
    tx_digest: TransactionDigest,
    object_index: u64,
) -> Object {
    let object_id = generate_object_id(&tx_digest, object_index);
    let account_data = AccountObject {
        owner,
        subaccount_index,
        asset_balances: vec![],
        perpetual_positions: vec![],
        last_update_timestamp: current_timestamp(),
    };
    
    Object::new_move(
        MoveObject::new(ACCOUNT_OBJECT_TYPE, bcs::to_bytes(&account_data)?, object_id),
        Owner::AddressOwner(owner),
    )
}
```

**更新**：
```rust
// 更新账户 Object
pub fn update_account_object(
    object: &mut Object,
    new_data: AccountObject,
    version_manager: &dyn VersionManager,
) -> Result<SequenceNumber> {
    // 1. 分配新版本号（Lamport 时间戳）
    let new_version = version_manager.assign_version(
        &[object.version()],
        &[],
    );
    
    // 2. 更新 Object 内容
    object.data.try_as_move_mut()?.update_contents(
        bcs::to_bytes(&new_data)?,
        new_version,
    )?;
    
    Ok(new_version)
}
```

#### 6.1.3 账户 Object 查询

**查询当前状态**：
```rust
// 使用 ObjectID 查询
pub async fn get_account_object(
    object_id: ObjectID,
    storage: &dyn ObjectStateManager,
) -> Result<Option<Object>> {
    storage.get_object(object_id).await
}
```

**查询历史状态**（Phase 2 支持）：
```rust
// 使用 Versioned ID 查询历史状态
pub async fn get_account_object_at_version(
    object_id: ObjectID,
    version: SequenceNumber,
    storage: &dyn ObjectStateManager,
) -> Result<Option<Object>> {
    storage.get_object_at_version(object_id, version).await
}
```

### 6.2 资产 Object 设计

#### 6.2.1 资产 Object 结构

**设计**：使用 Immutable Object 存储资产定义。

**结构**：
```rust
// 资产 Object 内容（BCS 编码）
pub struct AssetObject {
    // ObjectID（存储在 Object 元数据中，不在内容中）
    
    // 资产字段
    asset_id: u32,              // 资产编号（4 字节）
    symbol: String,             // 符号（如 "USDC"）
    decimals: i8,               // 精度指数（1 字节，如 -6）
    atomic_resolution: i8,      // 原子分辨率（1 字节）
    
    // 元数据
    created_timestamp: u64,     // 创建时间戳（8 字节）
}
```

**Object 元数据**：
- **Owner**: `Immutable`（不可变对象）
- **Version**: 冻结时的版本号（之后不再变化）
- **ObjectID**: 从交易摘要生成

**存储**：
- **Phase 1**: typed-store (RocksDB)
- **Phase 2**: 保持不变，完全兼容

#### 6.2.2 资产 Object 生命周期

**创建**（Immutable Object）：
```rust
// 创建资产 Object（Immutable）
pub fn create_asset_object(
    asset_id: u32,
    symbol: String,
    decimals: i8,
    tx_digest: TransactionDigest,
    object_index: u64,
) -> Object {
    let object_id = generate_object_id(&tx_digest, object_index);
    let asset_data = AssetObject {
        asset_id,
        symbol,
        decimals,
        atomic_resolution: decimals,
        created_timestamp: current_timestamp(),
    };
    
    // 创建 Immutable Object
    Object::new_immutable(
        MoveObject::new(ASSET_OBJECT_TYPE, bcs::to_bytes(&asset_data)?, object_id),
    )
}
```

**查询**（Immutable Object 不支持更新）：
```rust
// 查询资产 Object（Immutable）
pub async fn get_asset_object(
    object_id: ObjectID,
    storage: &dyn ObjectStateManager,
) -> Result<Option<Object>> {
    storage.get_object(object_id).await
}
```

### 6.3 订单 Object 设计（可选）

#### 6.3.1 订单 Object 结构（Phase 2 可选）

**设计**：Phase 1 优先使用内存订单簿，Phase 2 可选择持久化到 Object。

**Phase 1 方案**：
- ✅ 使用内存订单簿（DashMap + BTreeMap）
- ✅ 性能优先，延迟 < 10μs
- ❌ 不持久化到 Object

**Phase 2 可选方案**：
- ⚠️ 可选择持久化到 Object（Shared Object）
- ⚠️ 需要共识层，延迟增加
- ✅ 数据格式兼容

**结构**（Phase 2 可选）：
```rust
// 订单 Object 内容（BCS 编码，Phase 2 可选）
pub struct OrderObject {
    // ObjectID（存储在 Object 元数据中，不在内容中）
    
    // 订单字段
    order_id: u64,              // 订单 ID（8 字节）
    user_id: u64,               // 用户 ID（8 字节）
    market_id: u32,             // 市场 ID（4 字节）
    price: u64,                 // 价格（8 字节，fixed-point）
    size: u64,                  // 数量（8 字节）
    side: Side,                 // 方向（1 字节）
    order_type: OrderType,      // 类型（1 字节）
    timestamp: u64,             // 时间戳（8 字节）
}
```

**Object 元数据**（Phase 2 可选）：
- **Owner**: `Shared`（共享对象，需要共识）
- **Version**: `SequenceNumber`（Lamport 时间戳）
- **ObjectID**: 从交易摘要生成

**存储**：
- **Phase 1**: 内存订单簿（不持久化到 Object）
- **Phase 2**: 可选 Shared Object 或保持内存

### 6.4 存储格式兼容性

#### 6.4.1 数据结构兼容性

**原则**：Phase 1 和 Phase 2 使用**完全相同的 Object 数据结构格式**。

**实现**：
- ✅ 直接复用 `sui-types/src/object.rs` 的定义
- ✅ 不修改任何字段或编码格式
- ✅ 确保 Phase 1 和 Phase 2 的数据可以无缝迁移

**验证**：
```rust
// Phase 1 和 Phase 2 使用相同的 Object 结构
use sui_types::object::Object;  // 直接复用

// 数据格式完全兼容
let object_v1: Object = load_from_phase1_storage();
let object_v2: Object = load_from_phase2_storage();
// 结构相同，可以互操作
```

#### 6.4.2 存储层兼容性

**原则**：Phase 1 和 Phase 2 使用**相同的存储层接口**。

**实现**：
```rust
// 存储层抽象接口
pub trait ObjectStore: Send + Sync {
    async fn insert_object(&self, object: Object) -> Result<()>;
    async fn get_object(&self, object_id: ObjectID) -> Result<Option<Object>>;
    async fn update_object(&self, object: Object) -> Result<()>;
}

// Phase 1 实现
pub struct Phase1ObjectStore {
    db: typed_store::ObjectStore,  // RocksDB
}
impl ObjectStore for Phase1ObjectStore { /* ... */ }

// Phase 2 实现
pub struct Phase2ObjectStore {
    db: typed_store::ObjectStore,  // RocksDB（相同实现）
    // 额外：接入 Sui 共识层
}
impl ObjectStore for Phase2ObjectStore { /* ... */ }
```

**兼容性保证**：
- ✅ Phase 1 和 Phase 2 使用相同的存储层接口
- ✅ 数据格式完全兼容
- ✅ Phase 1 的数据可以直接迁移到 Phase 2

---

## 7. 数据流设计

### 7.1 订单提交流程

```
用户提交订单
    ↓
API Layer (JSON-RPC)
    ↓
Sequencer Layer (序列号分配)
    ↓
Execution Layer (执行层)
    ├─ Object State Manager (获取账户 Object)
    ├─ Risk Engine (风控检查)
    ├─ Matching Engine (撮合)
    │   └─ 内存订单簿 (DashMap + BTreeMap)
    └─ Object State Manager (更新账户 Object)
        ├─ 版本号分配 (Lamport 时间戳)
        └─ 状态更新
    ↓
Storage Layer (存储层)
    ├─ WAL (Write-Ahead Log)
    └─ typed-store (RocksDB, Object 持久化)
    ↓
Event Layer (事件发布)
    ├─ Order Event (订单事件)
    └─ Trade Event (成交事件)
```

### 7.2 账户查询流程

```
用户查询账户
    ↓
API Layer (JSON-RPC)
    ↓
Storage Layer (存储层)
    ├─ Memory Cache (内存缓存)
    │   └─ 命中 → 返回
    └─ typed-store (RocksDB)
        └─ 获取账户 Object
    ↓
Object State Manager (Object 状态管理器)
    └─ 解析 Object 内容
    ↓
返回账户数据
```

---

## 8. 接口设计

### 8.1 Object 状态管理器接口

```rust
// Object 状态管理器抽象接口
pub trait ObjectStateManager: Send + Sync {
    /// 创建 Object
    async fn create_object(
        &self,
        data: Vec<u8>,
        owner: Owner,
        tx_digest: TransactionDigest,
        object_index: u64,
    ) -> Result<ObjectID>;
    
    /// 更新 Object
    async fn update_object(
        &self,
        object_id: ObjectID,
        data: Vec<u8>,
    ) -> Result<SequenceNumber>;
    
    /// 获取 Object（当前版本）
    async fn get_object(&self, object_id: ObjectID) -> Result<Option<Object>>;
    
    /// 获取 Object（历史版本，Phase 2 支持）
    async fn get_object_at_version(
        &self,
        object_id: ObjectID,
        version: SequenceNumber,
    ) -> Result<Option<Object>>;
}
```

### 8.2 版本管理器接口

```rust
// 版本管理器抽象接口
pub trait VersionManager: Send + Sync {
    /// 分配新版本号（Lamport 时间戳算法）
    fn assign_version(
        &self,
        input_versions: &[SequenceNumber],
        receiving_versions: &[SequenceNumber],
    ) -> SequenceNumber;
}
```

---

## 9. 可观测性设计

### 9.1 监控指标

**复用 Sui 的监控指标**：
- ✅ `mysten-metrics`（Prometheus 集成）
- ✅ 自定义指标（Object 操作、版本号分配等）

**关键指标**：
- Object 创建/更新/查询延迟
- 版本号分配延迟
- Object 存储大小
- 内存缓存命中率

### 9.2 事件系统

**复用 Sui 的事件系统**：
- ✅ `sui-types/src/event.rs`（事件接口）
- ✅ Object 事件（创建、更新、删除）
- ✅ 账户事件（余额变化、仓位变化）

---

## 附录 A：参考资料

### A.1 业务需求

- DEX 完整业务需求：`prd/DEX完整业务需求.md`

### A.2 技术调研

- DEX 执行层技术调研：`exec_layer/01_research.md`
- Object 模型和 FastPath 使用性分析：`exec_layer/01_research.md` 第 4 章

### A.3 Sui 机制分析

- Sui Object 模型：`../sui/sui_object.md`
- Sui 架构文档：`../sui/sui_arch.md`

### A.4 已有设计文档（二期参考）

- Sui DEX 架构：`arch/sui_dex_arch.md`
- Sui DEX 技术方案：`tech/sui_dex_tech.md`

---

**文档版本**：v1.0  
**最后更新**：2026-01-08  
**审核状态**：待评审

