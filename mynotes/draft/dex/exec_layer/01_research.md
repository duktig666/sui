# DEX 执行层技术调研报告

> **版本**: v1.0  
> **日期**: 2026-01-08  
> **状态**: 技术调研文档  
> **目标**: 基于 Rust 实现高性能、低延迟 DEX 执行层，对标 HyperLiquid  
> **参考**: `prd/DEX完整业务需求.md`、`research/01-DEX执行层技术调研.md`

---

## 目录

1. [执行摘要](#1-执行摘要)
2. [目标对标分析](#2-目标对标分析)
3. [参考项目深度分析](#3-参考项目深度分析)
4. [Object 模型和 FastPath 使用性分析](#4-object-模型和-fastpath-使用性分析) ⭐ **重点章节**
5. [技术栈选型](#5-技术栈选型)
6. [关键技术路径](#6-关键技术路径)
7. [风险评估与缓解](#7-风险评估与缓解)
8. [调研结论与建议](#8-调研结论与建议)

---

## 1. 执行摘要

本调研旨在探索如何使用 Rust 实现一个**单节点高性能 DEX 执行层**，作为第一阶段的技术验证。基于对 DYDX、Sui、Reth 三个主要参考项目的深入分析，我们识别出了关键的技术路径和可复用的设计模式。

**核心结论**：
- **性能目标可实现**：单节点撮合引擎可达 < 10μs 撮合延迟，200K+ TPS 吞吐量
- **Sui 可提供 80% 基础设施**：网络层、存储层、调度器、事件系统等均可复用
- **原生 Rust 引擎是关键**：绕过 Move VM 执行是达成性能目标的必要条件
- **Object 模型可部分复用**：数据结构、版本控制、存储层可直接使用，但 FastPath 需适配

**关键问题解答**：
1. **Object 模型在第一阶段是否可以使用？** ✅ **部分可用**：数据结构、版本控制、存储层完全可用；FastPath 核心思想可用但需简化
2. **哪些模块可以使用 Object 模型？** ✅ **账户、资产、可选订单存储**：Owned Object 可直接使用，Shared Object 需共识层
3. **如何兼容第二阶段？** ✅ **数据格式和算法兼容**：使用相同的 Object 数据结构、ID 生成算法、版本号算法

---

## 2. 目标对标分析

### 2.1 HyperLiquid 性能基准

根据公开数据和社区测试，HyperLiquid 的性能指标：

| 指标 | HyperLiquid | 我们的目标 | 差距分析 |
|------|-------------|-----------|----------|
| **撮合延迟** | 1-2ms (P50) | < 10μs | 需要原生引擎优化 |
| **端到端延迟** | 20-50ms | < 50ms | 可达成 |
| **吞吐量** | 100K+ TPS | 200K TPS | 需要并行优化 |
| **订单簿深度** | 实时 | 实时 | 内存数据结构 |
| **确认时间** | 软确认 < 50ms<br>硬确认 ~1s | 软确认 < 50ms<br>硬确认 < 100ms | 需要异步验证 |

**关键差异点**：
- HyperLiquid 使用自有 L1 链，完全控制执行层
- 我们第一阶段是单节点验证，不涉及共识层复杂性
- 后续阶段需考虑共识接入方式（Sui DAG 或 ZK-Rollup）

### 2.2 HyperLiquid 架构特点

根据白皮书和技术分析：

```
HyperLiquid 架构（推测）：
┌────────────────────────────────────────────┐
│  Client Layer (API/WebSocket)              │
└────────────────────────────────────────────┘
              ↓
┌────────────────────────────────────────────┐
│  Sequencer Layer (排序器)                  │
│  - 单一排序节点保证顺序                    │
│  - 生成全局序列号                          │
└────────────────────────────────────────────┘
              ↓
┌────────────────────────────────────────────┐
│  Native Matching Engine (原生撮合引擎)    │
│  - Rust/C++ 实现                           │
│  - 内存订单簿 (BTreeMap/Skip List)         │
│  - 无锁并发设计                            │
└────────────────────────────────────────────┘
              ↓
┌────────────────────────────────────────────┐
│  Storage Layer (存储层)                    │
│  - WAL (顺序写入)                          │
│  - RocksDB (持久化)                        │
└────────────────────────────────────────────┘
              ↓
┌────────────────────────────────────────────┐
│  Consensus Layer (共识层)                  │
│  - HotStuff BFT 变种                       │
│  - 异步验证器确认                          │
└────────────────────────────────────────────┘
```

**核心设计理念**：
1. **中心化排序 + 去中心化验证**：性能与安全的平衡
2. **软确认 + 硬确认两阶段**：用户体验优先
3. **内存状态 + 异步持久化**：极致性能
4. **原生引擎**：无虚拟机开销

---

## 3. 参考项目深度分析

### 3.1 DYDX（基于 Cosmos SDK）

#### 3.1.1 不可复用的原因

| 组件 | 原因 | 替代方案 |
|-----|------|---------|
| ABCI 架构 | 强制区块前/后逻辑，无法绕过 | 自研执行流程 |
| Tendermint 共识 | 出块时间 > 1s | Sui Mysticeti (< 400ms) |
| 链下订单簿 + 链上结算 | 状态分裂，复杂度高 | 内存状态统一管理 |
| Cosmos SDK 框架 | 过度通用化，DEX 特化不足 | 专用 DEX 引擎 |

**详细分析**：参考 `research/01-DEX执行层技术调研.md` 第 3.1 节。

### 3.2 Sui 区块链

#### 3.2.1 Sui 可复用组件清单

| 组件 | 文件位置 | 复用价值 | 集成难度 |
|-----|---------|---------|---------|
| **Tonic Network** | `consensus/core/src/network/` | ⭐⭐⭐⭐⭐ 高性能 P2P | 低（直接导入） |
| **typed-store** | `crates/typed-store/` | ⭐⭐⭐⭐⭐ 持久化存储 | 低（新增 DEX 表） |
| **shared-crypto** | `crates/shared-crypto/` | ⭐⭐⭐⭐ 签名验证 | 低（直接使用） |
| **mysten-metrics** | `crates/mysten-metrics/` | ⭐⭐⭐⭐ 监控指标 | 低（直接使用） |
| **Event System** | `crates/sui-types/src/event.rs` | ⭐⭐⭐ 事件发布 | 低（复用接口） |
| **Object 数据结构** | `crates/sui-types/src/object.rs` | ⭐⭐⭐⭐⭐ **完全可用** | 低（直接复用） |
| **版本控制机制** | `sui-execution/latest/sui-adapter/src/temporary_store.rs` | ⭐⭐⭐⭐ **可用但需适配** | 中（算法相同） |

**详细分析**：参考 `research/01-DEX执行层技术调研.md` 第 3.2 节。

#### 3.2.2 DeepBook 性能瓶颈

**为什么 DeepBook 慢？**

1. **必须走共识路径**：Pool 是 Shared Object，所有订单需要 Mysticeti 排序
2. **Move VM 执行开销**：Gas 计量、Critbit Tree 操作、内存分配
3. **Checkpoint 等待时间**：平均 ~1 秒，最差 ~2 秒

**优化路径**：
- ❌ 不使用 Shared Object → 无需共识（但失去一致性）
- ✅ 不使用 Move VM → 原生 Rust 引擎（保持一致性）
- ✅ 自定义 Sequencer → 绕过 Checkpoint 等待

**详细分析**：参考 `research/01-DEX执行层技术调研.md` 第 3.2.3 节。

### 3.3 Reth（高性能 Ethereum 客户端）

#### 3.3.1 可借鉴的设计模式

**复用价值**：⭐⭐⭐（中等）

**可借鉴点**：
- 架构设计理念可借鉴
- 存储优化技术可参考（MDBX 零拷贝、分片存储）
- 并行执行策略可适配（冲突检测、无冲突并行）

**不推荐直接集成**：
- Reth 是完整的 Ethereum 客户端，过于庞大
- EVM 相关组件对 DEX 无用
- 不如直接使用 Sui 的成熟组件

**详细分析**：参考 `research/01-DEX执行层技术调研.md` 第 3.3 节。

---

## 4. Object 模型和 FastPath 使用性分析 ⭐

> **本章节重点回答以下关键问题**：
> 1. Object 模型和 FastPath 在第一阶段是否可以使用？
> 2. 哪些模块可以使用 Object 模型？
> 3. 如何使用可以兼容第二阶段的 ZK 或共识？
> 4. Object 模型与 Sui 共识的绑定关系分析

### 4.1 Object 模型在第一阶段的使用性

#### 4.1.1 数据结构分析

**核心数据结构**：参考 `../sui/sui_object.md` 第 2 节。

**位置**：`crates/sui-types/src/object.rs`

```rust
pub struct Object {
    pub data: Data,                        // Move Object 或 Package
    pub owner: Owner,                      // AddressOwner / ObjectOwner / Shared / Immutable
    pub previous_transaction: TransactionDigest,
    pub storage_rebate: u64,
}

pub struct MoveObject {
    pub type_: MoveObjectType,
    pub version: SequenceNumber,           // Lamport 时间戳
    pub contents: Vec<u8>,                 // BCS 编码
}
```

**使用性评估**：

| 组件 | 第一阶段可用性 | 依赖关系 | 兼容性 |
|-----|------------|---------|--------|
| **Object 数据结构** | ✅ **完全可用** | 无依赖，纯数据结构 | 完全兼容 |
| **ObjectID** | ✅ **完全可用** | 可从交易摘要生成，无需共识 | 完全兼容 |
| **SequenceNumber** | ✅ **完全可用** | 只是 u64，可自己管理 | 完全兼容 |
| **Owner 枚举** | ✅ **完全可用** | 纯数据结构 | 完全兼容 |
| **MoveObject** | ✅ **完全可用** | 纯数据结构 | 完全兼容 |

**结论**：Object 数据结构**完全独立于共识层**，可以在第一阶段直接使用。

#### 4.1.2 ObjectID 生成机制

**位置**：`crates/sui-types/src/base_types.rs`

**生成算法**：
```rust
// 伪代码
object_id = hash(
    transaction_digest,    // 交易的摘要（32 字节）
    object_index           // 交易中创建的第几个对象
)
```

**特点**：
- ✅ **全局唯一**：基于交易摘要和计数器
- ✅ **确定性**：相同交易和索引总是产生相同 ID
- ✅ **不依赖共识**：只需要交易摘要即可生成

**第一阶段使用方案**：
```rust
// Phase 1: 单节点，从 Sequencer 分配的交易摘要生成
pub fn generate_object_id(tx_digest: &TransactionDigest, index: u64) -> ObjectID {
    ObjectID::from(hash(tx_digest, index))
}
```

**第二阶段兼容性**：
- Sui 共识：算法完全相同，直接兼容
- ZK-Rollup：ID 生成算法不变，只需在 ZK 电路中验证

**结论**：ObjectID 生成机制**完全可用且兼容**。

#### 4.1.3 版本控制机制（Lamport 时间戳）

**位置**：`sui-execution/latest/sui-adapter/src/temporary_store.rs`、`crates/sui-types/src/transaction.rs`

**算法**：
```rust
// 位置: crates/sui-types/src/transaction.rs:4530
pub fn lamport_timestamp(&self, receiving_objects: &[ObjectRef]) -> SequenceNumber {
    let input_versions = self.objects.iter()
        .map(|obj| obj.version())
        .chain(receiving_objects.iter().map(|ref| ref.1));
    
    SequenceNumber::lamport_increment(input_versions)
}

// 公式: max(input_versions) + 1
```

**核心特性**：
- ✅ **不依赖全局计数器**：只需输入对象的版本号
- ✅ **支持并行执行**：不同对象可并行分配版本
- ✅ **因果顺序检测**：版本号反映依赖关系

**第一阶段使用方案**：

```rust
// Phase 1: 单节点版本号分配（简化实现）
pub struct SingleNodeVersionManager {
    // 单节点无需并发控制，但保留接口兼容性
}

impl SingleNodeVersionManager {
    /// 分配新版本号（Lamport 时间戳算法）
    pub fn assign_version(
        &self,
        input_versions: &[SequenceNumber],
        receiving_versions: &[SequenceNumber],
    ) -> SequenceNumber {
        let max_version = input_versions.iter()
            .chain(receiving_versions.iter())
            .max()
            .copied()
            .unwrap_or(SequenceNumber::MIN);
        max_version + 1
    }
}
```

**第二阶段兼容性**：

```rust
// Phase 2 (Sui 共识): 复用 Sui 的版本号分配逻辑
pub struct SuiVersionManager {
    temporary_store: TemporaryStore,
}

impl SuiVersionManager {
    pub fn assign_version(&self, input_objects: &InputObjects) -> SequenceNumber {
        // 使用 Sui 的 lamport_timestamp 计算逻辑
        input_objects.lamport_timestamp(&receiving_objects)
    }
}
```

**Phase 2 (ZK-Rollup)**：
- 使用相同的 Lamport 时间戳算法
- ZK 证明不改变数据结构，只证明状态转换的正确性
- 版本号计算在 ZK 电路中验证

**结论**：版本控制机制**可用且兼容**，算法相同，只需实现细节不同。

#### 4.1.4 依赖关系分析

**Object 模型的核心依赖**：

```
Object 数据结构
    ├─ ObjectID（无依赖，从交易摘要生成）
    ├─ SequenceNumber（无依赖，Lamport 时间戳算法）
    ├─ Owner（无依赖，纯枚举）
    └─ Data（无依赖，BCS 编码）

版本号分配
    ├─ Lamport 时间戳算法（无依赖，纯数学计算）
    └─ 输入对象版本号（需要对象状态，但单节点可管理）

FastPath 机制
    ├─ 2f+1 验证者签名（依赖共识层）❌
    └─ 拥有对象无需共识（核心思想可用）✅
```

**结论**：
- ✅ **数据结构层**：完全独立，无依赖
- ✅ **版本控制层**：算法独立，无依赖
- ⚠️ **FastPath 层**：部分依赖共识，但核心思想可用

### 4.2 FastPath 机制分析

#### 4.2.1 FastPath 核心思想

**参考**：`../sui/sui_arch.md` 第 6 节。

**核心思想**：
- **拥有对象（Owned Objects）无需共识**：用户独占的对象，无并发冲突
- **并行执行**：不同 Owned Object 的交易可完全并行
- **跳过共识排序**：直接执行，延迟 ~200ms（vs 共识路径 ~600ms）

**位置**：`crates/sui-core/src/authority.rs`

```rust
impl AuthorityState {
    pub async fn handle_transaction(&self, tx: Transaction) -> SuiResult {
        let has_shared_objects = tx.shared_input_objects().next().is_some();
        
        if has_shared_objects {
            // 共识路径: 提交到 Mysticeti
            self.consensus_adapter.submit(tx).await?;
        } else {
            // FastPath: 立即执行（需要 2f+1 签名）
            self.execute_certificate(tx).await?;
        }
    }
}
```

#### 4.2.2 FastPath 依赖分析

**FastPath 的完整流程**：

```
1. 客户端提交交易
   ↓
2. 广播到所有验证者
   ↓
3. 收集 2f+1 签名（依赖共识层）❌
   ↓
4. 形成证书（Certificate）
   ↓
5. 执行交易（核心逻辑可用）✅
   ↓
6. 返回结果
```

**第一阶段不可用的部分**：
- ❌ **2f+1 验证者签名**：单节点阶段无验证者网络
- ❌ **证书形成机制**：依赖多节点签名聚合

**第一阶段可用的部分**：
- ✅ **拥有对象无需共识的核心思想**：单节点直接执行
- ✅ **并行执行逻辑**：不同对象可并行处理
- ✅ **版本号分配算法**：Lamport 时间戳可用

#### 4.2.3 单节点 FastPath 简化方案

**Phase 1 简化实现**：

```rust
// Phase 1: 单节点 FastPath（简化）
pub struct SingleNodeFastPath {
    version_manager: SingleNodeVersionManager,
    state_store: StateStore,
}

impl SingleNodeFastPath {
    /// 单节点直接执行（无需签名收集）
    pub async fn execute_transaction(&self, tx: Transaction) -> Result<TransactionEffects> {
        // 1. 检查是否为 Owned Objects（核心思想）
        let has_shared = tx.shared_input_objects().next().is_some();
        if has_shared {
            return Err(Error::SharedObjectNotSupported);
        }
        
        // 2. 单节点直接执行（无需 2f+1 签名）
        let effects = self.execute_locally(tx).await?;
        
        // 3. 版本号分配（Lamport 时间戳）
        let new_version = self.version_manager.assign_version(
            &tx.input_versions(),
            &tx.receiving_versions(),
        );
        
        // 4. 更新状态
        self.state_store.update_objects(effects, new_version).await?;
        
        Ok(effects)
    }
}
```

**与 Sui FastPath 的对比**：

| 特性 | Sui FastPath | Phase 1 简化 FastPath | 兼容性 |
|-----|-------------|---------------------|--------|
| **对象类型检查** | ✅ 检查 Shared Objects | ✅ 相同逻辑 | 完全兼容 |
| **签名收集** | ✅ 2f+1 验证者签名 | ❌ 单节点无需签名 | 数据结构兼容 |
| **版本号分配** | ✅ Lamport 时间戳 | ✅ 相同算法 | 完全兼容 |
| **并行执行** | ✅ 对象级别并行 | ✅ 相同逻辑 | 完全兼容 |
| **数据结构** | ✅ Object 模型 | ✅ 相同结构 | 完全兼容 |

**结论**：FastPath 的**核心思想可用**，只需去掉签名收集部分，数据结构完全兼容。

### 4.3 可使用的模块清单

#### 4.3.1 完全可用的模块

| 模块 | 使用方式 | 兼容性 | 说明 |
|-----|---------|--------|------|
| **Object 数据结构** | 直接复用 `sui-types/src/object.rs` | ✅ 完全兼容 | 纯数据结构，无依赖 |
| **版本控制（Lamport 时间戳）** | 自己实现版本号分配逻辑 | ✅ 算法兼容 | 算法相同，实现细节不同 |
| **存储层（typed-store）** | 直接复用 `typed-store` | ✅ 完全兼容 | RocksDB 封装，无依赖 |
| **事件系统** | 直接复用 `sui-types/src/event.rs` | ✅ 完全兼容 | 事件接口，无依赖 |
| **ObjectID 生成** | 从交易摘要生成 | ✅ 完全兼容 | 算法相同 |

#### 4.3.2 部分可用的模块

| 模块 | 可用部分 | 不可用部分 | 适配方式 |
|-----|---------|-----------|---------|
| **网络层（Tonic Network）** | ✅ P2P 网络、压缩、HTTP/2 | ❌ 验证者签名聚合 | Phase 1 单节点，Phase 2 启用 |
| **FastPath 执行路径** | ✅ 核心思想、并行执行 | ❌ 2f+1 签名机制 | Phase 1 简化实现，Phase 2 启用 |
| **Leader Schedule** | ✅ 轮转算法 | ❌ 多节点选举 | Phase 1 单节点，Phase 2 启用 |

#### 4.3.3 不可用的模块

| 模块 | 原因 | 替代方案 |
|-----|------|---------|
| **Mysticeti 共识** | 需要多节点网络 | Phase 1 单节点，Phase 2 启用 |
| **2f+1 签名机制** | 需要验证者网络 | Phase 1 无需，Phase 2 启用 |
| **Checkpoint Service** | 依赖 Sui 框架 | Phase 1 自研快照机制 |

### 4.4 兼容第二阶段的设计原则

#### 4.4.1 数据结构兼容性

**原则**：使用**完全相同的 Object 数据结构格式**。

**实现**：
- 直接复用 `sui-types/src/object.rs` 的定义
- 不修改任何字段或编码格式
- 确保 Phase 1 和 Phase 2 的数据可以无缝迁移

**验证**：
```rust
// Phase 1 和 Phase 2 使用相同的 Object 结构
use sui_types::object::Object;  // 直接复用

// 数据格式完全兼容
let object_v1: Object = load_from_phase1_storage();
let object_v2: Object = load_from_phase2_storage();
// 结构相同，可以互操作
```

#### 4.4.2 ID 生成算法兼容性

**原则**：使用**完全相同的 ObjectID 生成算法**。

**实现**：
```rust
// Phase 1 和 Phase 2 使用相同的算法
pub fn generate_object_id(tx_digest: &TransactionDigest, index: u64) -> ObjectID {
    // 算法与 Sui 完全一致
    ObjectID::from(hash(tx_digest, index))
}
```

**兼容性保证**：
- ✅ 相同交易和索引总是产生相同 ID
- ✅ Phase 1 生成的 ID 在 Phase 2 中有效
- ✅ ZK-Rollup 可以验证 ID 生成的正确性

#### 4.4.3 版本号算法兼容性

**原则**：使用**完全相同的 Lamport 时间戳算法**。

**实现**：
```rust
// Phase 1: 简化实现，但算法相同
pub fn assign_version(input_versions: &[SequenceNumber]) -> SequenceNumber {
    let max = input_versions.iter().max().copied().unwrap_or(0);
    max + 1  // Lamport 时间戳算法
}

// Phase 2: 复用 Sui 的实现
// 算法完全相同，只是输入来源不同
```

**兼容性保证**：
- ✅ 算法相同：`max(input_versions) + 1`
- ✅ Phase 1 的版本号序列在 Phase 2 中有效
- ✅ ZK-Rollup 可以验证版本号计算的正确性

#### 4.4.4 抽象层设计

**原则**：通过**抽象层隔离 Phase 1 和 Phase 2 的实现差异**。

**设计**：

```rust
// 抽象接口
pub trait VersionManager {
    fn assign_version(
        &self,
        input_versions: &[SequenceNumber],
        receiving_versions: &[SequenceNumber],
    ) -> SequenceNumber;
}

pub trait ExecutionEngine {
    async fn execute_transaction(
        &self,
        tx: Transaction,
    ) -> Result<TransactionEffects>;
}

// Phase 1 实现
pub struct Phase1VersionManager { /* ... */ }
impl VersionManager for Phase1VersionManager { /* ... */ }

pub struct Phase1ExecutionEngine { /* ... */ }
impl ExecutionEngine for Phase1ExecutionEngine { /* ... */ }

// Phase 2 实现
pub struct Phase2VersionManager { /* Sui 实现 */ }
impl VersionManager for Phase2VersionManager { /* ... */ }

pub struct Phase2ExecutionEngine { /* Sui 实现 */ }
impl ExecutionEngine for Phase2ExecutionEngine { /* ... */ }
```

**优势**：
- ✅ 接口统一，代码复用
- ✅ Phase 1 到 Phase 2 的迁移只需替换实现
- ✅ 测试和验证更容易

#### 4.4.5 具体模块使用方案

**可以使用 Object 模型的模块**：

| 模块 | Phase 1 使用方案 | Phase 2 兼容方案 | 说明 |
|-----|----------------|----------------|------|
| **账户模块** | Owned Object 存储账户余额和仓位 | 直接接入 Sui 共识或 ZK | 数据结构兼容 |
| **资产模块** | Immutable Object 存储资产定义 | 保持不变 | 完全兼容 |
| **订单模块（可选）** | Phase 1 优先使用内存订单簿 | Phase 2 可选择持久化到 Object | 性能优先 |
| **存储层** | typed-store (RocksDB) | 保持不变 | 完全兼容 |

**不能直接使用 Object 模型的模块**：

| 模块 | Phase 1 方案 | Phase 2 方案 | 说明 |
|-----|------------|------------|------|
| **订单簿（Orderbook）** | 内存订单簿（DashMap） | 可选择 Shared Object 或保持内存 | 性能优先 |
| **撮合引擎** | 内存状态 + Sequencer 顺序 | Sui 共识或 ZK Rollup 保证顺序 | 需要全局顺序 |

---

## 5. 技术栈选型

基于以上分析，第一阶段（单节点执行层）的技术栈：

### 5.1 核心依赖

| 组件 | 选型 | 来源 | 理由 |
|-----|------|------|------|
| **网络层** | Tonic + anemo | Sui | 成熟的 P2P 网络，压缩优化 |
| **存储层** | typed-store (RocksDB) | Sui | 与 Sui 生态兼容，成熟稳定 |
| **Object 数据结构** | `sui-types/src/object.rs` | Sui | ⭐⭐⭐⭐⭐ **完全可用** |
| **序列化** | bcs | Sui | Sui 标准格式，高效 |
| **签名验证** | shared-crypto | Sui | Ed25519/BLS 支持 |
| **监控指标** | mysten-metrics | Sui | Prometheus 集成 |
| **并发数据结构** | dashmap | 第三方 | 无锁 HashMap |
| **异步运行时** | tokio | 第三方 | Rust 标准异步库 |
| **压缩** | lz4 | 第三方 | 快照压缩 (10:1) |

### 5.2 自研组件

| 组件 | 技术选型 | 性能目标 |
|-----|---------|---------|
| **撮合引擎** | Rust + BTreeMap + SIMD | < 10μs 单次撮合 |
| **风控引擎** | Rust（保证金计算） | < 1ms 验证 |
| **清算引擎** | Rust（价格监控） | < 5ms 触发 |
| **永续引擎** | Rust（资金费率） | < 10ms 结算 |
| **Sequencer** | Rust（序列号分配） | < 1ms 排序 |
| **版本管理器（Phase 1）** | Rust（Lamport 时间戳） | < 1μs 分配 |

---

## 6. 关键技术路径

### 6.1 Phase 1：单节点执行层验证

**目标**：验证 200K TPS 和 < 50ms 延迟可达性

```
Phase 1 架构（简化）：
┌────────────────────────────────────────┐
│  JSON-RPC API                          │
│  - 订单提交 (PlaceOrder)               │
│  - 订单取消 (CancelOrder)              │
│  - 查询接口 (GetOrderbook, GetBalance) │
└────────────────────────────────────────┘
              ↓
┌────────────────────────────────────────┐
│  Sequencer (单节点)                    │
│  - 全局序列号分配                      │
│  - 批次聚合 (5ms 或 1000 tx)           │
└────────────────────────────────────────┘
              ↓
┌────────────────────────────────────────┐
│  Native Matching Engine                │
│  - 市场并行撮合                        │
│  - 内存订单簿 (DashMap + BTreeMap)     │
└────────────────────────────────────────┘
              ↓
┌────────────────────────────────────────┐
│  Storage (Memory + WAL + Object 模型)  │
│  - 内存状态 (热数据)                   │
│  - WAL (fsync, RPO=0)                  │
│  - typed-store (Object 持久化)         │
└────────────────────────────────────────┘
```

**关键技术点**：
1. **内存订单簿**：BTreeMap（价格排序）+ VecDeque（时间队列）
2. **Object 模型集成**：账户、资产使用 Owned Object
3. **无锁并发**：DashMap 实现市场间并行
4. **批量处理**：5ms 聚合批次，减少锁竞争
5. **WAL 持久化**：Group Commit，每 100 条或 10ms 一次 fsync

### 6.2 Phase 2：接入共识层

**两条演进路径**：

**路径 A：Sui DAG 共识（推荐）**
- 优势：复用 Sui 成熟的共识层，Object 模型直接接入
- 实现：DEX 状态作为 Shared Object，Sequencer 输出提交到 Mysticeti
- 兼容性：✅ Object 数据结构、版本号算法完全兼容

**路径 B：ZK-Rollup**
- 优势：完全去中心化，无需信任 Sequencer
- 实现：Phase 1 的状态作为 Rollup 状态，ZK 证明状态转换
- 兼容性：✅ Object 数据结构、ID 生成、版本号算法均可验证

**详细分析**：参考 `research/01-DEX执行层技术调研.md` 第 5.2 节。

---

## 7. 风险评估与缓解

### 7.1 技术风险

| 风险 | 影响 | 概率 | 缓解措施 |
|-----|------|------|---------|
| **性能目标无法达成** | 高 | 中 | 提前进行性能原型验证 |
| **Object 模型兼容性问题** | 中 | 低 | 使用完全相同的数据结构和算法 |
| **Phase 1 到 Phase 2 迁移复杂** | 中 | 中 | 抽象层设计，接口统一 |
| **状态一致性 Bug** | 高 | 中 | 完善测试，形式化验证 |

### 7.2 Object 模型相关风险

| 风险 | 影响 | 概率 | 缓解措施 |
|-----|------|------|---------|
| **版本号分配算法不一致** | 高 | 低 | 使用相同的 Lamport 时间戳算法 |
| **ObjectID 生成冲突** | 中 | 低 | 使用相同的哈希算法 |
| **数据结构格式不兼容** | 高 | 低 | 直接复用 Sui 的数据结构定义 |

---

## 8. 调研结论与建议

### 8.1 核心结论

1. **DYDX 不可复用**：ABCI 架构限制无法突破，执行层效率低下
2. **Sui 是最佳基础**：80% 组件可复用，网络、存储、调度成熟可靠
3. **Object 模型可部分复用**：✅ 数据结构、版本控制、存储层完全可用；⚠️ FastPath 核心思想可用但需适配
4. **原生引擎是必选项**：绕过 Move VM 是达成性能目标的唯一路径
5. **单节点验证可行**：Phase 1 聚焦执行层性能，共识后置

### 8.2 Object 模型使用建议

**✅ 推荐使用**：
- Object 数据结构（完全复用）
- 版本控制机制（Lamport 时间戳算法）
- 存储层（typed-store）
- 账户、资产模块（Owned Object）

**⚠️ 谨慎使用**：
- FastPath 机制（核心思想可用，但需简化实现）
- 网络层（部分可用，Phase 2 启用）

**❌ 不建议使用**：
- Mysticeti 共识（Phase 1 不需要）
- 2f+1 签名机制（Phase 1 不需要）
- Shared Object（需要共识层）

### 8.3 技术路线建议

**第一阶段（3-4 个月）：单节点执行层**
- 目标：验证 200K TPS 和 < 50ms 延迟
- 技术栈：Rust + Sui 基础组件 + Object 模型 + 原生撮合引擎
- Object 模型：使用 Owned Object 存储账户和资产
- 交付物：可运行的单节点 DEX，性能测试报告

**第二阶段（4-6 个月）：共识集成**
- 目标：去中心化验证，硬确认 < 100ms
- 技术栈：Sui DAG 共识（或 ZK-Rollup 备选）
- Object 模型：直接接入 Sui 共识，数据结构完全兼容
- 交付物：多节点 DEX 测试网

### 8.4 下一步行动

1. **架构设计阶段**（下一步）：
   - 详细定义模块边界
   - 设计 Object 模型集成方案
   - 明确 Phase 1 和 Phase 2 的抽象层接口

2. **技术方案阶段**：
   - 撮合引擎详细设计
   - Object 模型存储方案设计
   - 版本管理器实现方案

3. **原型开发**：
   - 核心撮合引擎实现
   - Object 模型集成验证
   - 性能基准测试

---

## 附录 A：参考资料

### A.1 代码仓库

- Sui 主仓库：https://github.com/MystenLabs/sui
- DeepBook：`crates/sui-framework/packages/deepbook`

### A.2 技术文档

- Sui Object 模型：`../sui/sui_object.md`
- Sui 架构文档：`../sui/sui_arch.md`
- FastPath 性能分析：`notes/SUI_SIMPLE_TX_PERFORMANCE.md`
- DEX 完整业务需求：`prd/DEX完整业务需求.md`

### A.3 已有设计文档

- DEX 技术调研：`research/01-DEX执行层技术调研.md`
- Sui DEX 架构：`arch/sui_dex_arch.md`（作为二期参考）
- Sui DEX 技术方案：`tech/sui_dex_tech.md`（作为二期参考）

---

**文档版本**：v1.0  
**最后更新**：2026-01-08  
**审核状态**：待评审

