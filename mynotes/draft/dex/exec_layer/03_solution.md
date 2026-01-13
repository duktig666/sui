# DEX 执行层方案设计文档

> **版本**: v1.0  
> **日期**: 2026-01-08  
> **状态**: 方案设计文档  
> **目标**: 基于架构设计的单节点 DEX 执行层实现方案  
> **参考**: `exec_layer/02_architecture.md`、`exec_layer/01_research.md`、`tech/sui_dex_tech.md`（二期参考）

---

## 目录

1. [执行摘要](#1-执行摘要)
2. [Phase 1 实现方案](#2-phase-1-实现方案)
3. [核心技术实现](#3-核心技术实现)
4. [Object 模型和 FastPath 实现方案](#4-object-模型和-fastpath-实现方案) ⭐ **重点章节**
5. [Phase 2 演进路径](#5-phase-2-演进路径) ⭐ **重点章节**
6. [实施计划](#6-实施计划)
7. [风险与缓解](#7-风险与缓解)

---

## 1. 执行摘要

本文档定义了**单节点 DEX 执行层**的实现方案，聚焦 Phase 1（单节点验证）的具体实现步骤，同时提供 Phase 2（共识集成）的演进路径。

**核心实现策略**：
1. **Object 模型优先**：使用 Sui Object 模型存储账户、资产等状态，数据结构完全兼容
2. **Phase 1 简化实现**：版本管理器、ID 生成器等使用简化实现，但算法相同
3. **Phase 2 平滑演进**：通过抽象层设计，Phase 1 到 Phase 2 的迁移只需替换实现
4. **性能优先**：内存订单簿、原生引擎、无锁并发，达到性能目标

**关键里程碑**：
- **Milestone 1（1-2 个月）**：Object 模型集成、版本管理器实现
- **Milestone 2（2-3 个月）**：撮合引擎实现、性能优化
- **Milestone 3（3-4 个月）**：完整功能、性能测试
- **Phase 2（4-6 个月）**：共识集成、去中心化验证

---

## 2. Phase 1 实现方案

### 2.1 整体实现策略

**技术栈**：
- **语言**: Rust
- **存储**: typed-store (RocksDB) + 内存缓存
- **Object 模型**: `sui-types/src/object.rs`（直接复用）
- **版本控制**: Lamport 时间戳算法（Phase 1 简化实现）
- **序列化**: bcs
- **事件**: `sui-types/src/event.rs`（复用接口）

**实现原则**：
1. ✅ **直接复用 Sui 组件**：Object 数据结构、存储层、事件系统
2. ✅ **算法兼容**：版本号算法、ID 生成算法与 Sui 完全相同
3. ✅ **接口抽象**：通过抽象层隔离 Phase 1 和 Phase 2 的实现差异
4. ✅ **性能优先**：内存订单簿、原生引擎、无锁并发

### 2.2 模块实现顺序

| 阶段 | 模块 | 优先级 | 预计时间 |
|-----|------|--------|---------|
| **Phase 1.1** | Object 模型集成 | P0 | 2 周 |
| **Phase 1.1** | 版本管理器（Phase 1） | P0 | 1 周 |
| **Phase 1.1** | ID 生成器 | P0 | 1 周 |
| **Phase 1.2** | 账户 Object 实现 | P0 | 2 周 |
| **Phase 1.2** | 资产 Object 实现 | P0 | 1 周 |
| **Phase 1.2** | 存储层集成 | P0 | 2 周 |
| **Phase 1.3** | Sequencer 实现 | P0 | 2 周 |
| **Phase 1.3** | 撮合引擎实现 | P0 | 3 周 |
| **Phase 1.3** | 风控引擎实现 | P0 | 2 周 |
| **Phase 1.4** | API 层实现 | P0 | 2 周 |
| **Phase 1.4** | 事件系统集成 | P1 | 1 周 |
| **Phase 1.4** | 监控指标集成 | P1 | 1 周 |

---

## 3. 核心技术实现

### 3.1 Sequencer 实现

**参考**：`tech/sui_dex_tech.md` 第 1 节。

**核心功能**：
- 全局序列号分配（`[Epoch:16][Counter:48]`）
- 批次聚合（5ms 或 1000 tx）
- 交易排序

**实现要点**：
- 单节点 Sequencer（Phase 1）
- 序列号格式与 Phase 2 兼容
- 批次结构可扩展

### 3.2 撮合引擎实现

**参考**：`tech/sui_dex_tech.md` 第 2 节。

**核心功能**：
- 内存订单簿（DashMap + BTreeMap）
- 价格-时间优先撮合
- 性能目标：< 10μs 单次撮合

**实现要点**：
- 无锁并发（市场间并行）
- SIMD 优化（价格比较）
- 内存池（订单对象池）

### 3.3 存储层实现

**参考**：`tech/sui_dex_tech.md` 第 3 节。

**核心功能**：
- WAL（Write-Ahead Log）
- typed-store (RocksDB)
- 内存缓存

**实现要点**：
- Object 持久化
- Group Commit（批量 fsync）
- 快照机制（可选）

---

## 4. Object 模型和 FastPath 实现方案 ⭐

> **本章节重点提供 Object 模型和 FastPath 在 Phase 1 的具体实现方案，以及 Phase 2 的演进路径。**

### 4.1 Phase 1 实现方案

#### 4.1.1 版本管理器实现（Phase 1）

**设计**：使用 Lamport 时间戳算法，但使用简化实现（单节点）。

**代码示例**：

```rust
// Phase 1: 单节点版本号分配（简化实现）
use sui_types::base_types::SequenceNumber;
use std::sync::atomic::{AtomicU64, Ordering};

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
        let max = input_versions.iter()
            .chain(receiving_versions.iter())
            .map(|v| v.value())
            .max()
            .unwrap_or(0);
        
        SequenceNumber::from(max + 1)
    }
}

// 测试用例
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_lamport_timestamp() {
        let manager = Phase1VersionManager::new();
        
        // 测试 Lamport 时间戳算法
        let input = vec![
            SequenceNumber::from(5),
            SequenceNumber::from(3),
        ];
        let receiving = vec![];
        
        let version = manager.assign_version(&input, &receiving);
        assert_eq!(version, SequenceNumber::from(6));  // max(5, 3) + 1 = 6
    }
}
```

**特点**：
- ✅ 算法与 Sui 完全相同
- ✅ 单节点实现简单，性能优异
- ✅ 数据结构完全兼容

#### 4.1.2 ObjectID 生成器实现

**设计**：使用与 Sui 完全相同的 ObjectID 生成算法。

**代码示例**：

```rust
// Phase 1: ObjectID 生成器
use sui_types::base_types::{ObjectID, TransactionDigest};
use sui_types::crypto::sha3_hash;

pub struct ObjectIDGenerator;

impl ObjectIDGenerator {
    /// 生成 ObjectID（算法与 Sui 完全一致）
    pub fn generate(
        &self,
        tx_digest: &TransactionDigest,
        object_index: u64,
    ) -> ObjectID {
        // 算法与 Sui 完全一致
        // 从交易摘要和对象索引生成 ObjectID
        let hash_input = bcs::to_bytes(&(tx_digest, object_index)).unwrap();
        let hash = sha3_hash(&hash_input);
        ObjectID::from(hash)
    }
}

// 测试用例
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_object_id_generation() {
        let generator = ObjectIDGenerator;
        let tx_digest = TransactionDigest::random();
        let index = 0;
        
        let object_id1 = generator.generate(&tx_digest, index);
        let object_id2 = generator.generate(&tx_digest, index);
        
        // 相同输入产生相同 ID（确定性）
        assert_eq!(object_id1, object_id2);
        
        // 不同输入产生不同 ID
        let object_id3 = generator.generate(&tx_digest, index + 1);
        assert_ne!(object_id1, object_id3);
    }
}
```

**特点**：
- ✅ 算法与 Sui 完全相同
- ✅ 确定性：相同输入产生相同 ID
- ✅ Phase 2 兼容性：ID 生成算法不变

#### 4.1.3 Object 状态管理器实现（Phase 1）

**设计**：封装 Object 的创建、更新、查询操作，使用 Phase 1 的版本管理器。

**代码示例**：

```rust
// Phase 1: Object 状态管理器
use sui_types::object::{Object, MoveObject, Owner};
use sui_types::base_types::{ObjectID, SequenceNumber, TransactionDigest};

pub struct Phase1ObjectStateManager {
    version_manager: Phase1VersionManager,
    object_store: typed_store::ObjectStore,
}

impl Phase1ObjectStateManager {
    /// 创建 Object
    pub async fn create_object(
        &self,
        data: Vec<u8>,
        owner: Owner,
        tx_digest: TransactionDigest,
        object_index: u64,
    ) -> Result<ObjectID> {
        // 1. 生成 ObjectID
        let object_id = ObjectIDGenerator.generate(&tx_digest, object_index);
        
        // 2. 创建 MoveObject
        let move_object = MoveObject::new(
            OBJECT_TYPE_TAG,
            data,
            object_id,
        );
        
        // 3. 创建 Object（初始版本为 1）
        let object = Object::new_move(move_object, owner)?;
        
        // 4. 存储到 typed-store
        self.object_store.insert_object(object).await?;
        
        Ok(object_id)
    }
    
    /// 更新 Object
    pub async fn update_object(
        &self,
        object_id: ObjectID,
        new_data: Vec<u8>,
    ) -> Result<SequenceNumber> {
        // 1. 获取现有 Object
        let mut object = self.object_store
            .get_object(object_id)
            .await?
            .ok_or(Error::ObjectNotFound)?;
        
        // 2. 分配新版本号（Lamport 时间戳）
        let new_version = self.version_manager.assign_version(
            &[object.version()],
            &[],
        );
        
        // 3. 更新 Object 内容
        object.data.try_as_move_mut()?.update_contents(
            new_data,
            new_version,
        )?;
        
        // 4. 存储到 typed-store
        self.object_store.update_object(object).await?;
        
        Ok(new_version)
    }
    
    /// 获取 Object（当前版本）
    pub async fn get_object(&self, object_id: ObjectID) -> Result<Option<Object>> {
        self.object_store.get_object(object_id).await
    }
}
```

**特点**：
- ✅ 封装 Object 操作，接口统一
- ✅ 使用 Phase 1 的版本管理器
- ✅ 存储格式完全兼容

#### 4.1.4 账户 Object 实现示例

**设计**：使用 Owned Object 存储账户状态。

**代码示例**：

```rust
// 账户 Object 内容结构
use bcs;
use sui_types::base_types::SuiAddress;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountObject {
    owner: SuiAddress,
    subaccount_index: u32,
    asset_balances: Vec<AssetBalance>,
    perpetual_positions: Vec<PerpetualPosition>,
    last_update_timestamp: u64,
}

// 账户 Object 管理器
pub struct AccountObjectManager {
    object_state_manager: Arc<dyn ObjectStateManager>,
}

impl AccountObjectManager {
    /// 创建账户 Object
    pub async fn create_account(
        &self,
        owner: SuiAddress,
        subaccount_index: u32,
        tx_digest: TransactionDigest,
        object_index: u64,
    ) -> Result<ObjectID> {
        let account_data = AccountObject {
            owner,
            subaccount_index,
            asset_balances: vec![],
            perpetual_positions: vec![],
            last_update_timestamp: current_timestamp(),
        };
        
        let data = bcs::to_bytes(&account_data)?;
        let owner_enum = Owner::AddressOwner(owner);
        
        self.object_state_manager.create_object(
            data,
            owner_enum,
            tx_digest,
            object_index,
        ).await
    }
    
    /// 更新账户 Object
    pub async fn update_account(
        &self,
        object_id: ObjectID,
        new_data: AccountObject,
    ) -> Result<SequenceNumber> {
        let data = bcs::to_bytes(&new_data)?;
        self.object_state_manager.update_object(object_id, data).await
    }
    
    /// 获取账户 Object
    pub async fn get_account(
        &self,
        object_id: ObjectID,
    ) -> Result<Option<AccountObject>> {
        let object = self.object_state_manager.get_object(object_id).await?;
        
        match object {
            Some(obj) => {
                let data = obj.data.try_as_move()?.contents();
                let account_data = bcs::from_bytes(&data)?;
                Ok(Some(account_data))
            }
            None => Ok(None),
        }
    }
}
```

**特点**：
- ✅ 使用 Owned Object 存储账户状态
- ✅ 数据结构完全兼容
- ✅ 版本控制使用 Lamport 时间戳

#### 4.1.5 单节点 FastPath 简化实现

**设计**：Phase 1 单节点直接执行，无需 2f+1 签名。

**代码示例**：

```rust
// Phase 1: 单节点 FastPath（简化实现）
pub struct Phase1FastPath {
    version_manager: Phase1VersionManager,
    object_state_manager: Arc<dyn ObjectStateManager>,
    execution_engine: Arc<dyn ExecutionEngine>,
}

impl Phase1FastPath {
    /// 单节点直接执行（无需签名收集）
    pub async fn execute_transaction(
        &self,
        tx: Transaction,
    ) -> Result<TransactionEffects> {
        // 1. 检查是否为 Owned Objects（核心思想）
        let has_shared = tx.shared_input_objects().next().is_some();
        if has_shared {
            return Err(Error::SharedObjectNotSupported);
        }
        
        // 2. 单节点直接执行（无需 2f+1 签名）
        let effects = self.execution_engine.execute_locally(tx).await?;
        
        // 3. 版本号分配（Lamport 时间戳）
        let input_versions: Vec<SequenceNumber> = effects
            .modified_objects()
            .iter()
            .map(|obj| obj.version())
            .collect();
        
        let new_version = self.version_manager.assign_version(
            &input_versions,
            &[],
        );
        
        // 4. 更新状态
        self.object_state_manager.update_objects(effects, new_version).await?;
        
        Ok(effects)
    }
}
```

**特点**：
- ✅ 核心思想可用（拥有对象无需共识）
- ✅ 单节点实现简单，性能优异
- ✅ 数据结构完全兼容

### 4.2 Phase 2 演进方案

#### 4.2.1 Sui 共识集成路径

**策略**：直接复用 Sui 的版本管理器实现，接入 Sui 共识层。

**代码示例**：

```rust
// Phase 2: 复用 Sui 的版本管理器实现
use sui_execution_latest::adapter::temporary_store::TemporaryStore;
use sui_types::transaction::InputObjects;

pub struct Phase2VersionManager {
    temporary_store: TemporaryStore,  // 复用 Sui 的实现
}

impl VersionManager for Phase2VersionManager {
    fn assign_version(
        &self,
        input_objects: &InputObjects,
        receiving_objects: &[ObjectRef],
    ) -> SequenceNumber {
        // 使用 Sui 的 lamport_timestamp 计算逻辑
        input_objects.lamport_timestamp(receiving_objects)
    }
}

// Phase 2: 完整 FastPath 实现
pub struct Phase2FastPath {
    version_manager: Phase2VersionManager,
    authority_state: AuthorityState,  // 复用 Sui 的实现
}

impl Phase2FastPath {
    /// 完整 FastPath 实现（2f+1 签名）
    pub async fn execute_transaction(
        &self,
        tx: Transaction,
    ) -> Result<TransactionEffects> {
        // 1. 检查是否为 Owned Objects
        let has_shared = tx.shared_input_objects().next().is_some();
        
        if has_shared {
            // 共识路径：提交到 Mysticeti
            self.authority_state.consensus_adapter.submit(tx).await?;
        } else {
            // FastPath：收集 2f+1 签名
            let certificate = self.authority_state
                .aggregate_signatures(tx)
                .await?;
            
            // 执行交易
            self.authority_state.execute_certificate(certificate).await?;
        }
    }
}
```

**特点**：
- ✅ 直接复用 Sui 的实现
- ✅ 数据结构完全兼容
- ✅ 版本号算法相同

#### 4.2.2 ZK-Rollup 集成路径

**策略**：使用相同的 Object 数据结构，在 ZK 电路中验证状态转换。

**代码示例**：

```rust
// Phase 2: ZK-Rollup 集成（数据结构兼容）
pub struct ZKVersionManager {
    // ZK 电路验证版本号计算的正确性
}

impl VersionManager for ZKVersionManager {
    fn assign_version(
        &self,
        input_versions: &[SequenceNumber],
        receiving_versions: &[SequenceNumber],
    ) -> SequenceNumber {
        // 算法相同：Lamport 时间戳
        lamport_timestamp(input_versions, receiving_versions)
    }
    
    /// 生成 ZK 证明
    fn prove_version_assignment(
        &self,
        input_versions: &[SequenceNumber],
        receiving_versions: &[SequenceNumber],
        output_version: SequenceNumber,
    ) -> ZKProof {
        // 在 ZK 电路中证明版本号计算的正确性
        zk_circuit::prove_lamport_timestamp(
            input_versions,
            receiving_versions,
            output_version,
        )
    }
}
```

**特点**：
- ✅ Object 数据结构格式兼容
- ✅ 版本号算法相同
- ✅ ZK 证明状态转换的正确性

### 4.3 兼容性验证方案

#### 4.3.1 数据结构兼容性测试

**测试用例**：

```rust
#[cfg(test)]
mod compatibility_tests {
    use super::*;
    
    #[test]
    fn test_object_structure_compatibility() {
        // Phase 1 创建的 Object
        let object_v1 = create_phase1_object();
        
        // Phase 2 应该能够读取 Phase 1 的 Object
        let object_v2 = load_phase2_object(object_v1.id());
        
        // 结构应该相同
        assert_eq!(object_v1.data, object_v2.data);
        assert_eq!(object_v1.owner, object_v2.owner);
    }
    
    #[test]
    fn test_version_algorithm_compatibility() {
        // Phase 1 版本管理器
        let manager_v1 = Phase1VersionManager::new();
        
        // Phase 2 版本管理器
        let manager_v2 = Phase2VersionManager::new();
        
        let input = vec![
            SequenceNumber::from(5),
            SequenceNumber::from(3),
        ];
        let receiving = vec![];
        
        // 算法应该相同
        let version_v1 = manager_v1.assign_version(&input, &receiving);
        let version_v2 = manager_v2.assign_version(&input, &receiving);
        
        assert_eq!(version_v1, version_v2);  // 应该相同
    }
    
    #[test]
    fn test_object_id_generation_compatibility() {
        // Phase 1 ID 生成器
        let generator_v1 = ObjectIDGenerator;
        
        // Phase 2 ID 生成器（使用 Sui 的实现）
        let generator_v2 = SuiObjectIDGenerator;
        
        let tx_digest = TransactionDigest::random();
        let index = 0;
        
        // 算法应该相同
        let id_v1 = generator_v1.generate(&tx_digest, index);
        let id_v2 = generator_v2.generate(&tx_digest, index);
        
        assert_eq!(id_v1, id_v2);  // 应该相同
    }
}
```

#### 4.3.2 迁移测试

**测试用例**：

```rust
#[cfg(test)]
mod migration_tests {
    use super::*;
    
    #[tokio::test]
    async fn test_phase1_to_phase2_migration() {
        // 1. Phase 1 创建账户 Object
        let object_id = create_phase1_account().await?;
        
        // 2. Phase 2 读取 Phase 1 的 Object
        let object = load_phase2_object(object_id).await?;
        
        // 3. 验证数据结构
        assert!(object.is_some());
        let account_data: AccountObject = bcs::from_bytes(&object.data)?;
        assert_eq!(account_data.owner, expected_owner);
        
        // 4. Phase 2 更新 Object
        let new_version = update_phase2_object(object_id, new_data).await?;
        
        // 5. 验证版本号递增
        assert!(new_version > object.version());
    }
}
```

---

## 5. Phase 2 演进路径 ⭐

> **本章节重点描述 Phase 1 到 Phase 2 的演进路径，包括 Sui DAG 集成和 ZK-Rollup 集成两种方案。**

### 5.1 Sui DAG 集成路径（推荐）

#### 5.1.1 集成策略

**优势**：
- ✅ 直接复用 Sui 成熟的共识层
- ✅ Object 模型完全兼容，无需数据迁移
- ✅ 版本号算法相同，无缝切换
- ✅ Mysticeti 低延迟（< 400ms）

**挑战**：
- ⚠️ 需要适配 Sui 的 Shared Object 模型
- ⚠️ Checkpoint 等待时间优化
- ⚠️ Fork 维护成本

#### 5.1.2 实施步骤

**Step 1: 版本管理器迁移**

```rust
// 替换 Phase 1 的版本管理器
let version_manager = Phase2VersionManager::new(
    temporary_store,  // 复用 Sui 的实现
);

// 接口相同，只需替换实现
engine.set_version_manager(version_manager);
```

**Step 2: Object 状态管理器迁移**

```rust
// 替换 Phase 1 的 Object 状态管理器
let object_state_manager = Phase2ObjectStateManager::new(
    authority_store,  // 复用 Sui 的存储层
);

// 接口相同，只需替换实现
engine.set_object_state_manager(object_state_manager);
```

**Step 3: FastPath 启用**

```rust
// 启用完整 FastPath（2f+1 签名）
let fast_path = Phase2FastPath::new(
    authority_state,  // 复用 Sui 的 AuthorityState
);

// 替换 Phase 1 的简化 FastPath
engine.set_fast_path(fast_path);
```

**Step 4: 共识层集成**

```rust
// 接入 Sui Mysticeti 共识
let consensus_adapter = MysticetiAdapter::new(
    committee,
    network,
);

engine.set_consensus_adapter(consensus_adapter);
```

#### 5.1.3 兼容性保证

**数据结构兼容性**：
- ✅ Phase 1 和 Phase 2 使用相同的 Object 结构
- ✅ 数据格式完全兼容，可无缝迁移

**算法兼容性**：
- ✅ 版本号算法相同（Lamport 时间戳）
- ✅ ID 生成算法相同
- ✅ 版本号序列在 Phase 2 中有效

**接口兼容性**：
- ✅ 通过抽象层设计，接口统一
- ✅ Phase 1 到 Phase 2 的迁移只需替换实现

### 5.2 ZK-Rollup 集成路径

#### 5.2.1 集成策略

**优势**：
- ✅ 完全去中心化，无需信任 Sequencer
- ✅ Object 数据结构格式兼容
- ✅ 版本号算法相同，可验证

**挑战**：
- ⚠️ ZK 电路设计复杂
- ⚠️ 证明生成时间开销
- ⚠️ 状态转换验证成本

#### 5.2.2 实施步骤

**Step 1: ZK 电路设计**

```rust
// ZK 电路：验证 Lamport 时间戳计算
pub fn prove_lamport_timestamp(
    input_versions: &[SequenceNumber],
    receiving_versions: &[SequenceNumber],
    output_version: SequenceNumber,
) -> ZKProof {
    // 在 ZK 电路中证明：
    // output_version == max(input_versions, receiving_versions) + 1
    zk_circuit::prove_max_increment(
        input_versions,
        receiving_versions,
        output_version,
    )
}
```

**Step 2: 状态转换验证**

```rust
// ZK 电路：验证 Object 状态转换
pub fn prove_object_update(
    old_object: Object,
    new_object: Object,
    new_version: SequenceNumber,
) -> ZKProof {
    // 在 ZK 电路中证明：
    // 1. 版本号计算正确（Lamport 时间戳）
    // 2. 状态转换逻辑正确
    // 3. 所有权验证
    zk_circuit::prove_object_transition(
        old_object,
        new_object,
        new_version,
    )
}
```

**Step 3: Rollup 集成**

```rust
// ZK-Rollup 状态管理器
pub struct ZKRollupStateManager {
    zk_prover: ZKProver,
    state_store: StateStore,
}

impl ZKRollupStateManager {
    /// 提交批次到 Rollup
    pub async fn submit_batch(
        &self,
        transactions: Vec<Transaction>,
    ) -> Result<ZKProof> {
        // 1. 执行交易
        let effects = self.execute_transactions(transactions).await?;
        
        // 2. 生成 ZK 证明
        let proof = self.zk_prover.prove_state_transition(effects).await?;
        
        // 3. 提交到 L1
        self.submit_to_l1(proof).await?;
        
        Ok(proof)
    }
}
```

#### 5.2.3 兼容性保证

**数据结构兼容性**：
- ✅ Object 数据结构格式兼容
- ✅ 版本号算法相同

**算法兼容性**：
- ✅ Lamport 时间戳算法相同
- ✅ ID 生成算法相同
- ✅ ZK 电路可验证算法的正确性

### 5.3 路径对比

| 特性 | Sui DAG 集成 | ZK-Rollup 集成 | 推荐 |
|-----|------------|--------------|------|
| **去中心化程度** | ⭐⭐⭐⭐ 中等（2f+1 验证者） | ⭐⭐⭐⭐⭐ 完全去中心化 | ZK-Rollup |
| **延迟** | ⭐⭐⭐⭐ < 400ms | ⭐⭐⭐ ~1-2s（证明生成） | Sui DAG |
| **吞吐量** | ⭐⭐⭐⭐ 200K+ TPS | ⭐⭐⭐⭐ 100K+ TPS | Sui DAG |
| **Object 模型兼容性** | ⭐⭐⭐⭐⭐ 完全兼容 | ⭐⭐⭐⭐ 格式兼容 | Sui DAG |
| **实施复杂度** | ⭐⭐⭐ 中等 | ⭐⭐ 高 | Sui DAG |
| **维护成本** | ⭐⭐⭐ 中等（Fork 维护） | ⭐⭐⭐⭐ 低（独立实现） | ZK-Rollup |

**推荐**：**Sui DAG 集成**（优先推荐）

**理由**：
1. ✅ Object 模型完全兼容，无需数据迁移
2. ✅ 版本号算法相同，无缝切换
3. ✅ 延迟更低（< 400ms vs ~1-2s）
4. ✅ 实施复杂度较低

---

## 6. 实施计划

### 6.1 Phase 1 实施计划（3-4 个月）

| 阶段 | 模块 | 时间 | 交付物 |
|-----|------|------|--------|
| **Phase 1.1** | Object 模型集成 | 2 周 | Object 数据结构集成、版本管理器实现 |
| **Phase 1.1** | ID 生成器实现 | 1 周 | ObjectID 生成器、测试用例 |
| **Phase 1.2** | 账户 Object 实现 | 2 周 | 账户 Object 管理器、CRUD 接口 |
| **Phase 1.2** | 资产 Object 实现 | 1 周 | 资产 Object 管理器、CRUD 接口 |
| **Phase 1.2** | 存储层集成 | 2 周 | typed-store 集成、WAL 实现 |
| **Phase 1.3** | Sequencer 实现 | 2 周 | Sequencer、序列号分配、批次聚合 |
| **Phase 1.3** | 撮合引擎实现 | 3 周 | 内存订单簿、撮合算法、性能优化 |
| **Phase 1.3** | 风控引擎实现 | 2 周 | 保证金计算、风控检查 |
| **Phase 1.4** | API 层实现 | 2 周 | JSON-RPC、WebSocket、限流 |
| **Phase 1.4** | 事件系统集成 | 1 周 | 事件发布、订阅接口 |
| **Phase 1.4** | 监控指标集成 | 1 周 | Prometheus 指标、自定义指标 |
| **Phase 1.5** | 性能测试 | 2 周 | 性能测试报告、优化建议 |
| **Phase 1.5** | 兼容性测试 | 1 周 | Phase 1 到 Phase 2 兼容性测试 |

**总计**：约 20 周（5 个月）

### 6.2 Phase 2 实施计划（4-6 个月）

| 阶段 | 模块 | 时间 | 交付物 |
|-----|------|------|--------|
| **Phase 2.1** | Sui 共识集成（推荐） | 4 周 | 版本管理器迁移、FastPath 启用 |
| **Phase 2.1** | Object 状态管理器迁移 | 2 周 | Phase 2 Object 状态管理器 |
| **Phase 2.1** | 共识层集成 | 4 周 | Mysticeti 共识集成、测试 |
| **Phase 2.2** | 多节点测试 | 4 周 | 多节点测试网、性能测试 |
| **Phase 2.2** | 数据迁移 | 2 周 | Phase 1 到 Phase 2 数据迁移工具 |
| **Phase 2.3** | 主网准备 | 4 周 | 安全审计、文档完善 |

**总计**：约 20 周（5 个月）

---

## 7. 风险与缓解

### 7.1 技术风险

| 风险 | 影响 | 概率 | 缓解措施 |
|-----|------|------|---------|
| **性能目标无法达成** | 高 | 中 | 提前进行性能原型验证 |
| **Object 模型兼容性问题** | 中 | 低 | 使用完全相同的数据结构和算法 |
| **Phase 1 到 Phase 2 迁移复杂** | 中 | 中 | 抽象层设计，接口统一 |
| **版本号算法不一致** | 高 | 低 | 使用相同的 Lamport 时间戳算法 |
| **ID 生成冲突** | 中 | 低 | 使用相同的哈希算法 |

### 7.2 实施风险

| 风险 | 影响 | 概率 | 缓解措施 |
|-----|------|------|---------|
| **开发进度延迟** | 中 | 中 | 模块化开发，并行实施 |
| **Sui Fork 维护成本高** | 中 | 高 | 最小化修改，模块化设计 |
| **Phase 2 集成复杂度** | 高 | 中 | Phase 1 抽象层设计，接口统一 |

---

## 附录 A：参考资料

### A.1 技术调研

- DEX 执行层技术调研：`exec_layer/01_research.md`
- Object 模型和 FastPath 使用性分析：`exec_layer/01_research.md` 第 4 章

### A.2 架构设计

- DEX 执行层架构设计：`exec_layer/02_architecture.md`
- Object 模型集成设计：`exec_layer/02_architecture.md` 第 5 章

### A.3 Sui 机制分析

- Sui Object 模型：`../sui/sui_object.md`
- Sui 架构文档：`../sui/sui_arch.md`

### A.4 已有设计文档（二期参考）

- Sui DEX 技术方案：`tech/sui_dex_tech.md`

---

**文档版本**：v1.0  
**最后更新**：2026-01-08  
**审核状态**：待评审

