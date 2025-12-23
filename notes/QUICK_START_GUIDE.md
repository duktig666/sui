# Sui 代码仓库快速理解指南 | Sui Codebase Quick Start Guide

> **目标**: 通过关键代码入口和可执行测试快速理解 Sui 区块链架构
> **Goal**: Quickly understand Sui blockchain architecture through key code entry points and executable tests

---

## 📑 目录 | Table of Contents

0. [前置准备](#0-前置准备--prerequisites)
1. [核心执行流程](#1-核心执行流程--core-execution-flow)
2. [按模块理解](#2-按模块理解--understanding-by-module)
3. [测试驱动学习路径](#3-测试驱动学习路径--test-driven-learning-path)
4. [快速上手建议](#4-快速上手建议--quick-start-recommendations)

---

## 0. 前置准备 | Prerequisites

### 0.1 必需工具 | Required Tools

在开始之前，需要安装以下工具:

#### **Rust 工具链** | **Rust Toolchain**
```bash
# 安装 Rust (如果还没有)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 验证安装
rustc --version
cargo --version
```

#### **cargo-nextest** (快速测试运行器)
```bash
# 安装 cargo-nextest
cargo install cargo-nextest --locked

# 验证安装
cargo nextest --version
# 输出: cargo-nextest 0.9.115 (或更高版本)
```

**为什么需要 nextest?**
- ⚡ 比 `cargo test` 快 3-10 倍
- 📊 更好的测试结果展示
- 🔧 Sui 项目推荐使用

#### **可选工具** | **Optional Tools**

```bash
# clippy (代码检查)
rustup component add clippy

# rustfmt (代码格式化)
rustup component add rustfmt

# Sui CLI (本地开发)
cargo install --locked --git https://github.com/MystenLabs/sui.git --branch main sui
```

### 0.2 验证环境 | Verify Environment

```bash
# 克隆仓库 (如果还没有)
git clone https://github.com/MystenLabs/sui.git
cd sui

# 检查代码是否可以编译
cargo check -p sui-core

# 运行快速测试验证环境
cargo nextest run -p sui-types --lib base_types

# 如果看到 "test result: ok" 说明环境配置正确 ✅
```

### 0.3 常见问题 | Common Issues

**问题 1**: `error: no such command: nextest`
```bash
# 解决: 安装 cargo-nextest
cargo install cargo-nextest --locked
```

**问题 2**: 编译超时
```bash
# 解决: 使用 -p 选择特定包，或增加超时
cargo check -p sui-core --timeout 600
```

**问题 3**: 测试失败 (simtest 相关)
```bash
# 解决: 使用正确的测试命令
# ❌ 错误: cargo nextest run -p consensus-core
# ✅ 正确: cargo simtest -p consensus-core
```

---

## 1. 核心执行流程 | Core Execution Flow

### 1.1 交易生命周期 | Transaction Lifecycle

```
Client → JSON-RPC API → TransactionOrchestrator → AuthorityState → Execution
                                                        ↓
                                                   Consensus (if needed)
                                                        ↓
                                                   Checkpoint → Finality
```

#### **阶段 1: 交易提交 | Phase 1: Transaction Submission**

**入口代码** | **Entry Point**:
```
crates/sui-json-rpc/src/transaction_execution_api.rs:43
└─ struct TransactionExecutionApi
   └─ execute_transaction_block() - 接收客户端交易 | Receives client transactions
```

**关键方法** | **Key Methods**:
- `prepare_execute_transaction_block()` - 准备执行参数
- `execute_transaction_internal()` - 内部执行逻辑

**测试入口** | **Test Entry**:
```bash
# 测试交易执行 API
cargo nextest run -p sui-json-rpc transaction_execution

# E2E 测试交易编排器
cargo nextest run -p sui-e2e-tests transaction_orchestrator_tests
```

---

#### **阶段 2: 交易编排 | Phase 2: Transaction Orchestration**

**入口代码** | **Entry Point**:
```
crates/sui-core/src/transaction_orchestrator.rs:62
└─ struct TransactionOrchestrator
   ├─ execute_transaction_block() - 协调交易执行流程
   ├─ quorum_driver() - 驱动法定人数达成
   └─ validator_state: AuthorityState - 验证器状态
```

**执行流程**:
1. **本地验证** - 检查签名、gas、对象锁
2. **选择路径**:
   - **快速路径** (Owned Objects): 直接执行，无需共识
   - **共识路径** (Shared Objects): 发送到 Consensus

**关键代码路径**:
```rust
// transaction_orchestrator.rs:120+
pub async fn execute_transaction_block(
    &self,
    request: ExecuteTransactionRequestV3,
) -> Result<ExecuteTransactionResponseV3, QuorumDriverError>
```

**测试入口** | **Test Entry**:
```bash
# 测试事务协调器
cargo nextest run -p sui-core transaction_orchestrator

# 测试法定人数驱动
cargo nextest run -p sui-core quorum_driver
```

---

#### **阶段 3: 验证器状态处理 | Phase 3: Authority State Processing**

**入口代码** | **Entry Point**:
```
crates/sui-core/src/authority.rs:1
└─ struct AuthorityState - 核心验证器状态机
   ├─ handle_transaction() - 处理单个交易
   ├─ handle_certificate() - 处理已签名证书
   └─ execute_certificate() - 执行证书并产生效果
```

**关键组件**:
```rust
// authority.rs 中的关键结构
pub struct AuthorityState {
    pub name: AuthorityName,                    // 验证器名称
    pub committee_store: Arc<CommitteeStore>,   // 委员会信息
    pub execution_cache: Arc<ExecutionCache>,   // 执行缓存
    pub epoch_store: ArcSwap<AuthorityPerEpochStore>, // Epoch 存储
    // ... 更多字段
}
```

**关键方法**:
- `authority.rs:500+` - `handle_transaction()` - 接收和验证交易
- `authority.rs:800+` - `execute_certificate()` - 执行已达成共识的交易
- `authority.rs:1200+` - `handle_node_sync_certificate()` - 处理同步的证书

**测试入口** | **Test Entry**:
```bash
# Authority 核心测试 (30+ 测试套件)
cargo nextest run -p sui-core --lib authority_tests

# 批量交易测试
cargo nextest run -p sui-core batch_transaction_tests

# 共享对象测试
cargo nextest run -p sui-e2e-tests shared_objects
```

---

#### **阶段 4: 共识集成 | Phase 4: Consensus Integration**

**入口代码** | **Entry Point**:
```
crates/sui-core/src/consensus_adapter.rs:1
└─ struct ConsensusAdapter - 连接 AuthorityState 和 Consensus
   └─ submit() - 提交交易到共识层

crates/sui-core/src/consensus_handler.rs:1
└─ struct ConsensusHandler - 处理共识输出
   └─ handle_consensus_output() - 处理共识决定的交易
```

**共识流程**:
```
AuthorityState (Shared Object Tx)
    ↓ submit()
ConsensusAdapter
    ↓
Mysticeti Consensus (DAG-based BFT)
    ↓ consensus output
ConsensusHandler
    ↓ handle_consensus_output()
AuthorityState.execute_certificate()
```

**关键方法**:
- `consensus_adapter.rs:200+` - `submit()` - 提交到共识
- `consensus_handler.rs:100+` - `handle_consensus_output()` - 处理输出
- `consensus_handler.rs:300+` - `process_consensus_transactions()` - 批量处理

**测试入口** | **Test Entry**:
```bash
# 共识适配器测试
cargo nextest run -p sui-core consensus_adapter

# 共识处理器测试
cargo nextest run -p sui-core consensus_handler

# 完整共识集成测试
cargo nextest run -p sui-core consensus_tests
```

---

#### **阶段 5: Move 执行 | Phase 5: Move Execution**

**入口代码** | **Entry Point**:
```
sui-execution/latest/sui-adapter/src/adapter.rs:1
└─ 执行 Move 字节码
   ├─ execute() - 主执行入口
   ├─ load_module() - 加载 Move 模块
   └─ verify_and_execute() - 验证并执行
```

**执行适配器结构**:
```rust
// 核心函数
pub fn new_move_vm(
    natives: NativeFunctionTable,
    protocol_config: &ProtocolConfig,
) -> Result<MoveVM>

pub fn new_native_extensions(
    child_resolver: &dyn ChildObjectResolver,
    input_objects: BTreeMap<ObjectID, InputObject>,
    // ...
) -> NativeContextExtensions
```

**关键步骤**:
1. **加载对象** - 从存储读取输入对象
2. **验证字节码** - Move 字节码验证器
3. **执行交易** - Move VM 执行
4. **Gas 计量** - 跟踪 gas 消耗
5. **效果生成** - 创建 TransactionEffects

**测试入口** | **Test Entry**:
```bash
# Move 集成测试
cargo nextest run -p sui-core move_integration_tests

# Move 包测试
cargo nextest run -p sui-core move_package_tests

# 可编程交易块测试
cargo nextest run -p sui-adapter programmable_transactions

# 完整 Move 适配器测试
SUI_SKIP_SIMTESTS=1 cargo nextest run -p sui-adapter
```

---

#### **阶段 6: Checkpoint 和最终性 | Phase 6: Checkpoint & Finality**

**入口代码** | **Entry Point**:
```
crates/sui-core/src/checkpoints/checkpoint_executor/mod.rs:1
└─ struct CheckpointExecutor - 执行 checkpoints
   └─ execute_checkpoint() - 执行单个 checkpoint

crates/sui-core/src/checkpoints/mod.rs:1
└─ struct CheckpointService - 创建 checkpoints
   └─ make_checkpoint() - 构建新 checkpoint
```

**Checkpoint 流程**:
```
Executed Transactions
    ↓
CheckpointBuilder (每个 epoch)
    ↓ 累积交易
CheckpointService.make_checkpoint()
    ↓ 签名
Broadcast to validators
    ↓ 收集签名 (quorum)
Certified Checkpoint
    ↓
Persist to storage
```

**测试入口** | **Test Entry**:
```bash
# Checkpoint 执行器测试
cargo nextest run -p sui-core checkpoint_executor

# E2E checkpoint 测试
cargo nextest run -p sui-e2e-tests checkpoint_tests

# Checkpoint 服务测试
cargo nextest run -p sui-core checkpoints
```

---

## 2. 按模块理解 | Understanding by Module

### 2.1 共识层 (Mysticeti) | Consensus Layer

**主入口** | **Main Entry**:
```
consensus/core/src/core.rs:60
└─ struct Core - Mysticeti 共识核心
   ├─ add_blocks() - 添加新区块到 DAG
   ├─ try_commit() - 尝试提交区块
   └─ process_blocks() - 处理待处理区块
```

**核心组件**:
```
consensus/core/src/
├─ core.rs (167KB!) - 主共识逻辑
├─ dag_state.rs - DAG 状态管理
├─ block_manager.rs - 区块管理器
├─ commit_finalizer.rs - 提交最终化
├─ leader_schedule.rs - Leader 选举
├─ linearizer.rs - 交易排序
└─ universal_committer/ - 通用提交器
```

**关键概念**:
- **DAG** (Directed Acyclic Graph) - 区块组织成 DAG 而非链
- **Leader-based commits** - 每轮选举 leader，leader 区块触发提交
- **Pipelined** - 多轮并行处理

**测试入口** | **Test Entry**:
```bash
# 共识核心单元测试 (必须用 simtest!)
cargo simtest -p consensus-core

# 随机化测试 (压力测试)
cargo simtest -p consensus-core randomized_tests

# 特定组件测试
cargo simtest -p consensus-core universal_committer_tests
cargo simtest -p consensus-core dag_state
```

**学习路径**:
1. 阅读 `consensus/README.md` - 理解 Mysticeti 协议
2. 运行 `cargo simtest -p consensus-core` 查看所有测试
3. 阅读 `core.rs:60-200` 理解 Core 结构
4. 查看 `block_manager.rs` 理解区块如何添加到 DAG
5. 研究 `universal_committer/` 理解如何决定提交

---

### 2.2 类型系统 | Type System

**主入口** | **Main Entry**:
```
crates/sui-types/src/
├─ base_types.rs - 基础类型 (ObjectID, TransactionDigest, etc.)
├─ transaction.rs - Transaction 结构
├─ object.rs - Object 模型
├─ effects/ - TransactionEffects (v1, v2)
└─ messages_consensus.rs - 共识消息类型
```

**核心类型**:
```rust
// base_types.rs 中
pub struct ObjectID(pub AccountAddress);
pub struct TransactionDigest(pub [u8; 32]);
pub struct SuiAddress(pub AccountAddress);

// object.rs 中
pub struct Object {
    pub data: Data,
    pub owner: Owner,
    pub previous_transaction: TransactionDigest,
    pub storage_rebate: u64,
}

// transaction.rs 中
pub struct Transaction {
    pub data: TransactionData,
    pub signatures: Vec<GenericSignature>,
}
```

**测试入口** | **Test Entry**:
```bash
# 类型系统测试
SUI_SKIP_SIMTESTS=1 cargo nextest run -p sui-types --lib

# 对象模型测试
cargo nextest run -p sui-types object

# 交易类型测试
cargo nextest run -p sui-types transaction

# 效果测试
cargo nextest run -p sui-types effects
```

---

### 2.3 存储层 | Storage Layer

**主入口** | **Main Entry**:
```
crates/sui-core/src/authority/authority_store.rs:1
└─ struct AuthorityStore - 主存储抽象
   ├─ get_object() - 读取对象
   ├─ insert_transaction() - 写入交易
   └─ insert_checkpoint() - 写入 checkpoint

crates/sui-storage/src/
└─ 通用存储抽象
```

**存储结构**:
```
RocksDB (底层)
    ↑
typed-store (类型安全包装)
    ↑
AuthorityStore (业务逻辑)
    ↑
ExecutionCache (缓存层)
```

**关键存储表**:
- `objects` - 对象存储
- `transactions` - 交易数据
- `effects` - 交易效果
- `checkpoints` - Checkpoint 数据
- `epochs` - Epoch 信息

**测试入口** | **Test Entry**:
```bash
# Authority store 测试
cargo nextest run -p sui-core authority_store

# 存储层测试
cargo nextest run -p sui-storage

# 执行缓存测试
cargo nextest run -p sui-core execution_cache
```

---

### 2.4 网络层 | Network Layer

**主入口** | **Main Entry**:
```
crates/sui-network/src/
├─ api.rs - 网络 API 定义
├─ discovery/ - 节点发现
└─ state_sync/ - 状态同步

external-crates/mysten-network/src/
└─ Mysten 网络协议栈
```

**网络组件**:
- **anemo** - 自定义网络框架
- **State sync** - 同步区块链状态
- **Discovery** - P2P 节点发现
- **Randomness** - 随机性分发

**测试入口** | **Test Entry**:
```bash
# 网络层测试
cargo nextest run -p sui-network

# 状态同步测试
cargo nextest run -p sui-core state_sync

# Anemo 基准测试
cargo nextest run -p anemo
```

---

### 2.5 RPC 层 | RPC Layer

**主入口** | **Main Entry**:
```
crates/sui-json-rpc/src/
├─ api.rs - RPC API 定义
├─ read_api.rs - 读取 API (查询对象、交易)
├─ transaction_execution_api.rs - 交易执行 API
├─ coin_api.rs - 代币 API
└─ governance_api.rs - 治理 API

crates/sui-graphql-rpc/src/
└─ GraphQL API 实现
```

**API 层次**:
```
Client
    ↓
JSON-RPC / GraphQL
    ↓
API Implementation
    ↓
AuthorityState / TransactionOrchestrator
```

**测试入口** | **Test Entry**:
```bash
# JSON-RPC 测试
cargo nextest run -p sui-json-rpc

# GraphQL 测试
cargo nextest run -p sui-graphql-rpc

# E2E RPC 测试
cargo nextest run -p sui-e2e-tests rpc
```

---

### 2.6 Move 框架 | Move Framework

**主入口** | **Main Entry**:
```
crates/sui-framework/packages/
├─ move-stdlib/ - Move 标准库
│   └─ sources/ (vector.move, option.move, etc.)
├─ sui-framework/ - Sui 框架
│   └─ sources/ (coin.move, object.move, transfer.move, etc.)
├─ sui-system/ - 系统合约
│   └─ sources/ (sui_system.move, validator.move, staking_pool.move)
└─ deepbook/ - DeepBook DEX
    └─ sources/ (clob_v2.move, custodian.move)
```

**关键模块**:
```move
// sui-framework/sources/object.move
public fun new(ctx: &mut TxContext): UID

// sui-framework/sources/transfer.move
public fun transfer<T: key>(obj: T, recipient: address)
public fun share_object<T: key>(obj: T)

// sui-framework/sources/coin.move
public fun create<T>(...)
```

**测试入口** | **Test Entry**:
```bash
# 构建框架 (检查是否有错误)
cargo build -p sui-framework

# Move 包发布测试
cargo nextest run -p sui-core move_package_publish_tests

# Move 包升级测试
cargo nextest run -p sui-core move_package_upgrade_tests

# 运行 Move 单元测试
cd crates/sui-framework/packages/sui-framework
sui move test
```

---

### 2.7 索引器 | Indexer

**主入口** | **Main Entry**:
```
crates/sui-indexer-alt/src/
├─ pipeline/ - 索引管道
├─ handlers/ - 各种数据处理器
│   ├─ obj_info.rs - 对象信息索引
│   ├─ tx_affected_objects.rs - 受影响对象
│   └─ ev_emit_mod.rs - 事件索引
└─ models/ - PostgreSQL 模型
```

**索引流程**:
```
Checkpoint Stream
    ↓
Pipeline (并行处理)
    ↓
Handlers (20+ 专用处理器)
    ↓
PostgreSQL Database
    ↑
GraphQL / JSON-RPC Queries
```

**测试入口** | **Test Entry**:
```bash
# 索引器测试
cargo nextest run -p sui-indexer-alt

# 旧索引器测试
cargo nextest run -p sui-indexer
```

---

## 3. 测试驱动学习路径 | Test-Driven Learning Path

### 3.1 快速测试 (< 1分钟) | Quick Tests

理解基本概念:

```bash
# 1. 基础类型系统 (10秒)
cargo nextest run -p sui-types --lib base_types

# 2. 对象模型 (15秒)
cargo nextest run -p sui-types object_tests

# 3. 交易结构 (15秒)
cargo nextest run -p sui-types transaction_tests

# 4. 加密和签名 (20秒)
cargo nextest run -p sui-types crypto_tests
```

### 3.2 中等测试 (1-5分钟) | Medium Tests

理解核心流程:

```bash
# 1. Authority 基础测试 (2分钟)
cargo nextest run -p sui-core --lib authority_tests::test_handle_transfer_transaction

# 2. 交易执行流程 (3分钟)
cargo nextest run -p sui-core --lib execution_driver_tests

# 3. 共享对象处理 (4分钟)
cargo nextest run -p sui-core --lib shared_object

# 4. Gas 机制 (2分钟)
cargo nextest run -p sui-core --lib gas_tests
```

### 3.3 深度测试 (5-15分钟) | Deep Tests

理解完整系统:

```bash
# 1. E2E 交易测试 (10分钟)
cargo nextest run -p sui-e2e-tests transaction

# 2. Checkpoint 完整流程 (8分钟)
cargo nextest run -p sui-e2e-tests checkpoint_tests

# 3. 共识集成测试 (12分钟, 必须用 simtest!)
cargo simtest -p sui-core consensus_tests

# 4. 完整的权限聚合器测试 (10分钟)
cargo nextest run -p sui-core authority_aggregator_tests
```

### 3.4 专家级测试 (15分钟+) | Expert Tests

深入理解架构:

```bash
# 1. 共识随机化测试 (20分钟+, 压力测试)
cargo simtest -p consensus-core randomized_tests

# 2. Move 集成测试套件 (30分钟)
cargo nextest run -p sui-core move_integration_tests

# 3. 完整 E2E 测试套件 (45分钟+)
cargo simtest -p sui-e2e-tests

# 4. 整个仓库单元测试 (60分钟+, 需要高配置)
SUI_SKIP_SIMTESTS=1 cargo nextest run --timeout 600
```

---

## 4. 快速上手建议 | Quick Start Recommendations

### 4.1 第一天: 理解基础 | Day 1: Understand Basics

**阅读清单** (2-3小时):
```
1. README.md - 项目概览
2. CLAUDE.md - 开发指南
3. consensus/README.md - 共识协议
4. sui-execution/README.md - 执行版本管理
```

**代码阅读** (2-3小时):
```
1. crates/sui-types/src/base_types.rs - 基础类型
2. crates/sui-types/src/object.rs - 对象模型
3. crates/sui-types/src/transaction.rs - 交易结构
4. crates/sui-core/src/authority.rs:1-300 - Authority 结构
```

**运行测试**:
```bash
# 验证环境配置
cargo check -p sui-core

# 运行快速测试
cargo nextest run -p sui-types --lib
```

---

### 4.2 第二天: 理解交易流程 | Day 2: Understand Transaction Flow

**跟踪完整流程**:

1. **启动点**: 阅读 `transaction_execution_api.rs`
2. **编排**: 阅读 `transaction_orchestrator.rs`
3. **处理**: 阅读 `authority.rs` 的 `handle_transaction()`
4. **执行**: 阅读 `sui-adapter/src/adapter.rs`

**运行相关测试**:
```bash
# 跟着测试理解流程
cargo nextest run -p sui-core authority_tests::test_handle_transfer_transaction
cargo nextest run -p sui-core execution_driver_tests
cargo nextest run -p sui-e2e-tests transaction_orchestrator_tests
```

**实验**:
```bash
# 启动本地测试网络
sui start

# 在另一个终端发送交易
sui client transfer --to <address> --object-id <object> --gas-budget 10000000

# 观察日志理解流程
```

---

### 4.3 第三天: 理解共识 | Day 3: Understand Consensus

**重点阅读**:
```
1. consensus/README.md - Mysticeti 协议详解
2. consensus/core/src/core.rs:60-200 - Core 结构
3. consensus/core/src/block_manager.rs - 区块管理
4. crates/sui-core/src/consensus_adapter.rs - 共识适配
5. crates/sui-core/src/consensus_handler.rs - 共识处理
```

**运行测试理解**:
```bash
# 共识核心测试
cargo simtest -p consensus-core --lib

# 共识集成测试
cargo nextest run -p sui-core consensus_adapter
cargo nextest run -p sui-core consensus_handler

# 观察 DAG 构建
cargo simtest -p consensus-core dag_state
```

---

### 4.4 第四天: 理解 Move 执行 | Day 4: Understand Move Execution

**重点阅读**:
```
1. sui-execution/README.md - 执行版本管理
2. sui-execution/latest/sui-adapter/src/adapter.rs - Move 适配器
3. sui-execution/latest/sui-move-natives/src/ - 原生函数
4. crates/sui-framework/packages/sui-framework/sources/ - 框架代码
```

**运行测试**:
```bash
# Move 适配器测试
SUI_SKIP_SIMTESTS=1 cargo nextest run -p sui-adapter

# Move 集成测试
cargo nextest run -p sui-core move_integration_tests

# 包管理测试
cargo nextest run -p sui-core move_package_publish_tests
```

**Move 代码实验**:
```bash
# 创建示例 Move 包
cd /tmp
sui move new my_module

# 编写简单模块并测试
# 然后发布到本地网络
sui client publish --gas-budget 100000000
```

---

### 4.5 第五天: 理解存储和索引 | Day 5: Understand Storage & Indexing

**重点阅读**:
```
1. crates/sui-core/src/authority/authority_store.rs - 存储抽象
2. crates/sui-storage/src/ - 存储实现
3. crates/sui-core/src/execution_cache/ - 执行缓存
4. crates/sui-indexer-alt/src/ - 索引器架构
```

**运行测试**:
```bash
# 存储测试
cargo nextest run -p sui-storage
cargo nextest run -p sui-core authority_store

# 执行缓存测试
cargo nextest run -p sui-core execution_cache

# 索引器测试
cargo nextest run -p sui-indexer-alt
```

---

### 4.6 第六-七天: E2E 测试和系统集成 | Day 6-7: E2E Tests & System Integration

**运行完整测试套件**:
```bash
# Day 6: E2E 测试
cargo simtest -p sui-e2e-tests transaction_orchestrator_tests
cargo simtest -p sui-e2e-tests checkpoint_tests
cargo simtest -p sui-e2e-tests shared_objects_tests

# Day 7: 系统压力测试
cargo simtest -p consensus-core randomized_tests
cargo nextest run -p sui-benchmark
```

**阅读实际案例**:
```
1. crates/sui-e2e-tests/tests/ - 所有 E2E 测试
2. examples/ - 示例 dApps
3. crates/sui-benchmark/src/ - 性能基准测试
```

---

## 5. 调试技巧 | Debugging Tips

### 5.1 启用详细日志 | Enable Verbose Logging

```bash
# 运行测试时启用 trace 日志
RUST_LOG=trace cargo nextest run -p sui-core authority_tests

# 只看特定模块日志
RUST_LOG=sui_core::authority=debug cargo nextest run

# 共识详细日志
RUST_LOG=consensus_core::core=trace cargo simtest -p consensus-core
```

### 5.2 使用 Rust 调试器 | Use Rust Debugger

```bash
# 使用 rust-lldb (macOS) 或 rust-gdb (Linux)
rust-lldb target/debug/deps/sui_core-<hash>

# 设置断点
(lldb) b sui_core::authority::handle_transaction
(lldb) run

# 或使用 VS Code 的 CodeLLDB 扩展
```

### 5.3 查看测试输出 | View Test Output

```bash
# 显示测试的 stdout/stderr
cargo nextest run --nocapture -p sui-core authority_tests

# 显示失败测试的完整输出
cargo nextest run --failure-output immediate-final
```

### 5.4 性能分析 | Performance Profiling

```bash
# 使用 flamegraph
cargo install flamegraph
sudo cargo flamegraph --bin sui-node

# 使用 perf
perf record -g target/release/sui-node
perf report
```

---

## 6. 关键概念速查 | Key Concepts Cheat Sheet

### 对象模型 | Object Model

- **Owned Objects**: 单一所有者，可并行处理
- **Shared Objects**: 多方共享，需要共识排序
- **Immutable Objects**: 不可变，可并行读取
- **ObjectID**: 全局唯一标识符
- **Version**: 每次修改递增

### 交易类型 | Transaction Types

- **Transfer**: 转移对象所有权
- **Publish**: 发布 Move 包
- **Call**: 调用 Move 函数
- **PTB** (Programmable Transaction Block): 组合多个操作

### 共识 | Consensus

- **DAG**: 有向无环图组织区块
- **Leader**: 每轮选举的提交触发者
- **Quorum**: 2f+1 验证器签名
- **Fast Path**: Owned objects 跳过共识

### Gas | Gas

- **Gas Budget**: 用户愿意支付的最大 gas
- **Gas Price**: Gas 单位价格
- **Storage Rebate**: 删除对象时退还
- **Computation Cost**: 执行成本
- **Storage Cost**: 存储成本

---

## 7. 常用命令汇总 | Common Commands Summary

```bash
# === 构建 ===
cargo build -p sui-core                    # 构建核心 crate
cargo check                                # 快速检查所有代码

# === 测试 ===
SUI_SKIP_SIMTESTS=1 cargo nextest run     # 单元测试 (跳过模拟测试)
cargo simtest -p sui-e2e-tests             # E2E 模拟测试
cargo nextest run -p sui-core --lib        # 只运行库测试

# === 代码质量 ===
./scripts/lint.sh                          # 完整 lint (格式化 + clippy)
cargo fmt --all                            # 格式化
cargo xclippy                              # Clippy lint

# === 本地开发 ===
sui start                                  # 启动本地网络
sui client                                 # 客户端命令
sui move build                             # 构建 Move 包
sui move test                              # 运行 Move 测试

# === 依赖管理 ===
cargo tree -p sui-core                     # 查看依赖树
cargo udeps                                # 查找未使用依赖

# === 性能 ===
cargo build --release -p sui-node          # Release 构建
cargo bench -p sui-benchmark               # 运行基准测试
```

---

## 8. 关键文件索引 | Key Files Index

### 核心执行 | Core Execution
- `crates/sui-node/src/lib.rs:1` - 节点入口
- `crates/sui-core/src/authority.rs:1` - 验证器状态机
- `crates/sui-core/src/transaction_orchestrator.rs:62` - 交易编排器
- `sui-execution/latest/sui-adapter/src/adapter.rs:1` - Move 执行

### 共识 | Consensus
- `consensus/core/src/core.rs:60` - Mysticeti 核心
- `consensus/core/src/block_manager.rs:1` - 区块管理
- `crates/sui-core/src/consensus_adapter.rs:1` - 共识适配器
- `crates/sui-core/src/consensus_handler.rs:1` - 共识处理器

### 类型 | Types
- `crates/sui-types/src/base_types.rs:1` - 基础类型
- `crates/sui-types/src/object.rs:1` - 对象模型
- `crates/sui-types/src/transaction.rs:1` - 交易类型
- `crates/sui-types/src/effects/mod.rs:1` - 交易效果

### API
- `crates/sui-json-rpc/src/transaction_execution_api.rs:43` - 交易执行 API
- `crates/sui-json-rpc/src/read_api.rs:1` - 读取 API
- `crates/sui-graphql-rpc/src/server/mod.rs:1` - GraphQL 服务器

### 存储 | Storage
- `crates/sui-core/src/authority/authority_store.rs:1` - Authority 存储
- `crates/sui-storage/src/lib.rs:1` - 存储抽象
- `crates/sui-core/src/execution_cache/mod.rs:1` - 执行缓存

### Checkpoints
- `crates/sui-core/src/checkpoints/checkpoint_executor/mod.rs:1` - Checkpoint 执行
- `crates/sui-core/src/checkpoints/mod.rs:1` - Checkpoint 服务

### Move Framework
- `crates/sui-framework/packages/sui-framework/sources/object.move` - 对象操作
- `crates/sui-framework/packages/sui-framework/sources/transfer.move` - 转移操作
- `crates/sui-framework/packages/sui-framework/sources/coin.move` - 代币操作
- `crates/sui-framework/packages/sui-system/sources/sui_system.move` - 系统合约

---

## 9. 进阶主题 | Advanced Topics

### 9.1 Protocol Config

**位置**: `crates/sui-protocol-config/src/lib.rs`

协议配置管理特性开关和参数，支持无分叉升级。

### 9.2 Execution Versioning

**位置**: `sui-execution/README.md`

理解如何在不分叉的情况下升级执行逻辑。

### 9.3 Mysticeti 深入

**论文**: https://arxiv.org/pdf/2310.14821
**代码**: `consensus/core/`

深入理解 DAG-based BFT 共识。

### 9.4 zkLogin

**位置**: `crates/sui-types/src/authenticator.rs`

理解零知识证明登录机制。

### 9.5 Bridge

**位置**: `bridge/`

学习跨链桥接实现 (Sui ↔ Ethereum)。

### 9.6 DeepBook

**位置**: `crates/sui-framework/packages/deepbook/`

研究链上订单簿 DEX 实现。

---

## 10. 学习资源 | Learning Resources

### 官方文档
- Sui 文档: https://docs.sui.io
- Move Book: https://move-book.com
- Sui Move by Example: https://examples.sui.io

### 代码示例
- `examples/` - 仓库中的示例
- Sui Developer Hub: https://sui.io/developers

### 社区
- Discord: https://discord.gg/sui
- Forum: https://forums.sui.io
- GitHub Issues: https://github.com/MystenLabs/sui/issues

---

## 11. 贡献指南 | Contributing Guidelines

开发前必读:
1. `CLAUDE.md` - 开发命令和约定
2. `CONTRIBUTING.md` - 贡献指南
3. 运行 `./scripts/lint.sh` 确保代码质量
4. 所有测试必须通过

关键规则:
- ❌ **不要禁用测试**
- ❌ **不要使用 `#[allow(dead_code)]`**
- ✅ **运行完整 lint**: `cargo xclippy && cargo fmt`
- ✅ **测试通过**: `SUI_SKIP_SIMTESTS=1 cargo nextest run`

---

**祝学习愉快! Happy Learning!** 🚀

如有问题，请参考源码注释或在 Discord/Forum 提问。
