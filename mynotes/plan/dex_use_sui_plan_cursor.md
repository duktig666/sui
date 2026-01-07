# DEX 基于 Sui 开发规划 - 第一阶段

> **版本**: v1.0  
> **日期**: 2025-01-XX  
> **状态**: Draft  
> **目标**: 基于 Sui 开发高性能 DEX，第一阶段使用中心化定序器

---

## 📋 目录

1. [概述](#1-概述)
2. [第一阶段架构设计](#2-第一阶段架构设计)
3. [与 Sui 的结合点](#3-与-sui-的结合点)
4. [使用的 Sui 模块和特性](#4-使用的-sui-模块和特性)
5. [DEX 业务需求与 Sui 结合分析](#5-dex-业务需求与-sui-结合分析)
6. [Sui 网络层集成方案](#6-sui-网络层集成方案)
7. [集成方式选择](#7-集成方式选择)
8. [实现思路](#8-实现思路)
9. [关键技术决策](#9-关键技术决策)
10. [实施计划](#10-实施计划)

---

## 1. 概述

### 1.1 项目目标

基于 Sui 区块链开发一个高性能去中心化交易所（DEX），第一阶段采用**中心化定序器**架构，实现：
- **撮合延迟**: < 50ms (P99)
- **吞吐量**: 10万+ TPS
- **完全兼容**: Sui 生态和 Move 智能合约

### 1.2 设计参考

主要参考文档：
- [`notes/dex_l1/drafts/dex-plan.md`](../../notes/dex_l1/drafts/dex-plan.md) - 核心架构设计
- [`notes/dex_l1/DEX_L1_DESIGN_SUMMARY.md`](../../notes/dex_l1/DEX_L1_DESIGN_SUMMARY.md) - 设计总结
- [`notes/dex_l1/docs/01-REQUIREMENTS.md`](../../notes/dex_l1/docs/01-REQUIREMENTS.md) - 需求规格
- [`notes/dex_l1/docs/07-MOVE-INTEGRATION-DESIGN.md`](../../notes/dex_l1/docs/07-MOVE-INTEGRATION-DESIGN.md) - Move 集成设计
- [`mynotes/dex/prd/DEX完整业务需求.md`](../../dex/prd/DEX完整业务需求.md) - 完整业务需求
- [`notes/SUI_NETWORK_PROPAGATION_ANALYSIS.md`](../../notes/SUI_NETWORK_PROPAGATION_ANALYSIS.md) - Sui 网络层分析

### 1.3 架构演进路径

```
Phase 1 (当前): 中心化 Sequencer
    ↓
Phase 2 (未来): 多节点轮换 Sequencer (复用 Sui 验证者网络)
    ↓
Phase 3 (未来): 类似 HyperEVM，使用 Sui 共识
```

---

## 2. 第一阶段架构设计

### 2.1 整体架构图

```
┌─────────────────────────────────────────────────────────────────┐
│                      DEX L1 Architecture (Phase 1)            │
├─────────────────────────────────────────────────────────────────┤
│  Client Layer                                                   │
│  └── Sui SDK/Wallets ── JSON-RPC ── WebSocket                  │
├─────────────────────────────────────────────────────────────────┤
│  Sequencer Layer (NEW - 中心化)                                 │
│  └── Order Gateway → Tx Sequencer → Sequence Publisher         │
│      (< 5ms)          (FIFO)         (DA Layer)                │
├─────────────────────────────────────────────────────────────────┤
│  Native DEX Engine (NEW)                                       │
│  └── Order Manager → Matching Engine → Risk Engine             │
│      (Rust Native)    (< 10us/match)   (Margin/Liquidation)    │
├─────────────────────────────────────────────────────────────────┤
│  Modified Sui Execution Layer                                  │
│  └── DEX Precompile → Move VM → Balance Manager               │
│      (Bypass VM)      (Non-DEX)  (Fast Path)                   │
├─────────────────────────────────────────────────────────────────┤
│  Sui Storage Layer (复用)                                      │
│  └── Orderbook State → Balance Cache → RocksDB                │
│      (In-Memory)       (DashMap)       (Sui typed-store)      │
└─────────────────────────────────────────────────────────────────┘
```

### 2.2 交易路径分类

| 交易类型 | 执行路径 | 延迟 | 说明 |
|---------|---------|-----|------|
| **DEX 订单** | Sequencer → Native Engine | < 50ms | 纯撮合，绕过 Move VM |
| **存取款** | Sequencer → Move VM → Native Engine | < 100ms | 需要 Move VM 处理 Coin 对象 |
| **其他交易** | Mysticeti → Move VM | ~600ms | 标准 Sui 交易路径 |

### 2.3 核心组件

1. **Sequencer (定序器)**
   - 中心化单节点（Phase 1）
   - FIFO 排序，分配全局序列号
   - 热备份支持（故障切换 < 100ms）

2. **Matching Engine (撮合引擎)**
   - 原生 Rust 实现
   - 价格-时间优先算法
   - 内存订单簿（BTreeMap）

3. **DEX Precompile (预编译钩子)**
   - 拦截特定 Move 调用
   - 路由到原生执行路径
   - 保持 Move 接口兼容

4. **Storage Layer (存储层)**
   - 复用 Sui 的 RocksDB 存储
   - 内存缓存 + WAL + 快照

---

## 3. 与 Sui 的结合点

### 3.1 核心结合点

#### 3.1.1 交易路由层
**位置**: `crates/sui-core/src/authority.rs`

**结合方式**:
- 在 `handle_transaction()` 中添加 DEX 交易检测
- 识别 DEX 交易后路由到 Sequencer，而非 Mysticeti 共识
- 保持标准 Sui 交易的原有路径不变

**关键代码位置**:
```rust
// crates/sui-core/src/authority.rs
impl AuthorityState {
    pub async fn handle_transaction(&self, tx: Transaction) -> Result<...> {
        // 检测是否为 DEX 交易
        if self.is_dex_transaction(&tx)? {
            // 路由到 DEX Sequencer
            return self.submit_to_dex_sequencer(tx).await;
        }
        
        // 标准 Sui 交易路径
        // ... 原有逻辑
    }
}
```

#### 3.1.2 执行层集成
**位置**: `sui-execution/latest/sui-adapter/src/execution_engine.rs`

**结合方式**:
- 在 `execute_transaction_to_effects()` 中添加 Precompile 检测
- DEX 交易直接调用原生引擎，绕过 Move VM
- 非 DEX 交易正常走 Move VM

**关键代码位置**:
```rust
// sui-execution/latest/sui-adapter/src/execution_engine.rs
pub fn execute_transaction_to_effects(...) -> Result<...> {
    // 检测是否为 DEX Precompile
    if is_dex_precompile(&transaction_kind)? {
        // 调用原生 DEX 引擎
        return dex_engine.execute_native(tx).await;
    }
    
    // 标准 Move VM 执行
    // ... 原有逻辑
}
```

#### 3.1.3 存储层复用
**位置**: `crates/sui-core/src/authority/authority_store.rs`

**结合方式**:
- 复用 Sui 的 `AuthorityStore` 和 `typed-store`
- DEX 状态存储在独立的列族（Column Family）
- 利用 Sui 的 WAL 和 Checkpoint 机制

#### 3.1.4 网络层复用
**位置**: `crates/sui-network/`

**结合方式**:
- 复用 Sui 的 P2P 网络（anemo）
- Sequencer 通过现有网络广播序列
- 验证者通过现有网络确认序列

#### 3.1.5 RPC 层扩展
**位置**: `crates/sui-json-rpc/`

**结合方式**:
- 扩展 Sui JSON-RPC API
- 添加 DEX 专用接口（下单、撤单、查询订单簿等）
- 保持向后兼容

---

## 4. 使用的 Sui 模块和特性

### 4.1 核心模块清单

| 模块 | 路径 | 用途 | 修改程度 |
|-----|------|-----|---------|
| **sui-types** | `crates/sui-types/` | 基础类型定义（Transaction, Object, etc.） | 复用 |
| **sui-core** | `crates/sui-core/` | Authority 状态机、交易处理 | **修改** |
| **sui-execution** | `sui-execution/latest/` | Move VM 执行层 | **修改** |
| **sui-storage** | `crates/sui-storage/` | 存储抽象和缓存 | 复用 |
| **typed-store** | `crates/typed-store/` | RocksDB 封装 | 复用 |
| **sui-json-rpc** | `crates/sui-json-rpc/` | RPC API 服务器 | **扩展** |
| **sui-network** | `crates/sui-network/` | P2P 网络层 | 复用 |
| **sui-framework** | `crates/sui-framework/` | Move 标准库 | **扩展** |

### 4.2 关键特性使用

#### 4.2.1 对象模型 (Object Model)
**用途**: 管理链上资产（Coin 对象）

**使用场景**:
- 存款：用户将 `Coin<T>` 转移到 DEX 托管账户
- 取款：从托管账户释放 `Coin<T>` 给用户
- 余额证明：通过对象版本号验证余额

**关键代码**:
```rust
// 使用 Sui 的对象模型
use sui_types::object::{Object, Owner};
use sui_types::base_types::{ObjectID, ObjectRef};

// 存款：锁定 Coin 对象
pub fn deposit_coin(coin: Object) -> Result<()> {
    // 转移到 DEX 托管账户（Shared Object）
    // ...
}
```

#### 4.2.2 FastPath 机制
**用途**: 拥有对象交易跳过共识

**适配**:
- DEX 订单交易：类似 FastPath，但走 Sequencer 而非直接执行
- 存取款交易：需要 Move VM 处理，但仍走 Sequencer 保证顺序

#### 4.2.3 执行调度器 (Execution Scheduler)
**用途**: 并行执行非冲突交易

**适配**:
- DEX 订单：Sequencer 已排序，无需调度
- 非 DEX 交易：继续使用原有调度器

#### 4.2.4 存储层 (Storage Layer)
**用途**: 持久化状态

**使用**:
- **RocksDB**: 通过 `typed-store` 存储 DEX 状态
- **WAL**: 使用 Sui 的 WAL 机制保证持久化
- **Checkpoint**: 定期创建快照，支持快速恢复

**关键代码**:
```rust
// 复用 Sui 的存储层
use typed_store::rocks::{DBMap, DBBatch};

// 定义 DEX 专用表
pub struct DexStore {
    orders: DBMap<OrderID, Order>,
    balances: DBMap<AccountID, Balance>,
    // ...
}
```

#### 4.2.5 Move 框架集成
**用途**: 提供 Move 接口

**实现**:
- 创建 `dex-framework` Move 包
- 定义 `place_order`, `cancel_order`, `deposit`, `withdraw` 等函数
- Precompile 拦截这些调用，路由到原生引擎

**Move 代码示例**:
```move
// dex-framework/sources/dex.move
module dex::dex {
    public entry fun place_order(...) {
        // Precompile 拦截，实际由原生引擎执行
        abort 0
    }
}
```

#### 4.2.6 事件系统 (Event System)
**用途**: 发布交易事件

**使用**:
- 复用 Sui 的事件机制
- 发布订单创建、成交、撤单等事件
- 支持索引器订阅

#### 4.2.7 Gas 机制
**用途**: 交易费用计算

**适配**:
- DEX 订单：使用固定 Gas 或按撮合次数计费
- 存取款：使用标准 Sui Gas 计算
- 非 DEX 交易：使用原有 Gas 机制

---

## 5. DEX 业务需求与 Sui 结合分析

> 参考文档: [`mynotes/dex/prd/DEX完整业务需求.md`](../../dex/prd/DEX完整业务需求.md)

基于完整业务需求文档，分析各模块与 Sui 的结合程度：

### 5.1 功能模块分类

#### 5.1.1 完全复用 Sui（无需自行开发）

| 模块 | Sui 组件 | 说明 |
|-----|---------|------|
| **账户地址系统** | `sui-types::base_types::SuiAddress` | 直接使用 Sui 地址作为用户标识 |
| **资产对象模型** | `sui-types::object::Object` | 使用 Sui Coin 对象管理链上资产 |
| **交易签名验证** | `sui-types::crypto::*` | 复用 Sui 的签名算法和验证逻辑 |
| **事件系统** | `sui-types::event::*` | 使用 Sui 事件机制发布交易事件 |
| **Gas 机制** | `sui-execution/.../gas_charger.rs` | 复用 Sui Gas 计算（存取款场景） |
| **Checkpoint 机制** | `sui-core/src/checkpoints/` | 用于状态快照和恢复 |

#### 5.1.2 部分复用 Sui（需要扩展）

| 模块 | Sui 组件 | 扩展内容 | 开发量 |
|-----|---------|---------|--------|
| **存储层** | `typed-store` (RocksDB) | 添加 DEX 专用表（订单、余额、持仓） | 中 |
| **RPC API** | `sui-json-rpc` | 扩展 DEX 专用接口（下单、撤单、查询订单簿） | 中 |
| **执行层** | `sui-execution/.../execution_engine.rs` | 添加 Precompile 钩子 | 中 |
| **网络层** | `mysten-network` (anemo) | 添加 Sequencer 消息类型 | 小 |
| **Move 框架** | `sui-framework` | 创建 `dex-framework` Move 包 | 中 |

#### 5.1.3 完全自行开发（Rust 原生）

| 模块 | 原因 | 技术栈 |
|-----|------|--------|
| **撮合引擎** | 性能要求 < 10μs，Move VM 无法满足 | Rust + BTreeMap |
| **订单簿管理** | 需要内存级性能，复杂数据结构 | Rust + DashMap |
| **Sequencer** | 中心化定序逻辑，Sui 无对应组件 | Rust + 异步 |
| **风险引擎** | 保证金计算、清算逻辑 | Rust |
| **资金费率计算** | 复杂的金融计算逻辑 | Rust |
| **清算引擎** | 破产价计算、去杠杆机制 | Rust |
| **手续费计算** | 多层级费率、推荐人返佣 | Rust |
| **Vault 机制** | 股份系统、收益分配 | Rust |

### 5.2 详细模块分析

#### 5.2.1 账户模块（部分复用）

**复用 Sui**:
- ✅ 用户地址：直接使用 `SuiAddress`
- ✅ 子账户标识：`(SuiAddress, SubAccountID)` 组合

**自行开发**:
- ❌ 子账户结构：资产持仓列表、永续仓位列表
- ❌ 保证金模式：全仓/逐仓逻辑
- ❌ 仓位余额机制：逐仓模式的专属余额管理

**实现方式**:
```rust
// 复用 Sui 地址
use sui_types::base_types::SuiAddress;

// 自行实现子账户
pub struct SubAccount {
    address: SuiAddress,        // 复用
    sub_account_id: u32,        // 自行定义
    asset_positions: Vec<AssetPosition>,  // 自行实现
    perpetual_positions: Vec<PerpetualPosition>,  // 自行实现
}
```

#### 5.2.2 资产模块（部分复用）

**复用 Sui**:
- ✅ Coin 对象：使用 `Coin<T>` 管理链上资产
- ✅ 对象转移：使用 `transfer::transfer()` 进行存取款

**自行开发**:
- ❌ Quantums 精度系统：内部计量单位转换
- ❌ 资产定义：资产编号、符号、精度配置

**实现方式**:
```rust
// 复用 Sui Coin
use sui::coin::Coin;

// 自行实现精度转换
pub struct Asset {
    asset_id: u8,
    symbol: String,
    quantum_resolution: i8,  // 如 USDC: -6
}

impl Asset {
    pub fn to_quantums(&self, amount: f64) -> u64 {
        (amount * 10_f64.powi(-self.quantum_resolution as i32)) as u64
    }
}
```

#### 5.2.3 风险控制模块（完全自行开发）

**原因**: 复杂的金融计算逻辑，需要极致性能

**自行开发内容**:
- ❌ 净抵押品计算（NC）
- ❌ 初始保证金要求（IMR）
- ❌ 维持保证金要求（MMR）
- ❌ OIMF 机制（开仓量保证金调整）
- ❌ 账户健康状态判断

**实现方式**:
```rust
pub struct RiskEngine {
    // 完全 Rust 原生实现
    pub fn calculate_nc(&self, account: &Account) -> u64 { ... }
    pub fn calculate_imr(&self, positions: &[Position]) -> u64 { ... }
    pub fn calculate_mmr(&self, positions: &[Position]) -> u64 { ... }
}
```

#### 5.2.4 撮合结算模块（完全自行开发）

**原因**: 性能要求 < 10μs，必须原生实现

**自行开发内容**:
- ❌ 订单簿数据结构（BTreeMap + HashMap）
- ❌ 价格-时间优先撮合算法
- ❌ 订单类型处理（Limit, IOC, Post-Only, etc.）
- ❌ 成交结算逻辑
- ❌ 手续费计算

**实现方式**:
```rust
pub struct Orderbook {
    bids: BTreeMap<Reverse<Price>, VecDeque<Order>>,
    asks: BTreeMap<Price, VecDeque<Order>>,
    order_index: HashMap<OrderID, OrderRef>,
}

impl Orderbook {
    pub fn match_order(&mut self, order: Order) -> MatchResult {
        // < 10μs 撮合算法
    }
}
```

#### 5.2.5 资金费率模块（完全自行开发）

**自行开发内容**:
- ❌ 溢价采样（每分钟）
- ❌ 资金费率计算（每小时）
- ❌ 资金费率索引更新
- ❌ 资金费结算

#### 5.2.6 清算模块（完全自行开发）

**自行开发内容**:
- ❌ 清算触发条件判断
- ❌ 破产价格计算
- ❌ 可成交价格计算
- ❌ 保险基金机制
- ❌ 去杠杆机制

#### 5.2.7 手续费与收入分成（完全自行开发）

**自行开发内容**:
- ❌ 费率层级系统
- ❌ 推荐人机制
- ❌ 收入分成计算

#### 5.2.8 协议 Vault 机制（完全自行开发）

**自行开发内容**:
- ❌ 股份系统
- ❌ 股份解锁期
- ❌ 收益分配

### 5.3 结合度总结

| 类别 | 模块数量 | 开发量 | 说明 |
|-----|---------|--------|------|
| **完全复用** | 6 | 0% | 直接使用 Sui 组件 |
| **部分复用** | 5 | 30-50% | 扩展 Sui 组件 |
| **完全自行开发** | 8 | 100% | Rust 原生实现 |

**总体评估**:
- **复用率**: 约 40%（基础设施层）
- **自行开发率**: 约 60%（业务逻辑层）
- **优势**: 基础设施稳定可靠，专注业务逻辑开发

---

## 6. Sui 网络层集成方案

> 参考文档: [`notes/SUI_NETWORK_PROPAGATION_ANALYSIS.md`](../../notes/SUI_NETWORK_PROPAGATION_ANALYSIS.md)

### 6.1 Sui 网络层架构

Sui 采用多层网络架构：

```
┌─────────────────────────────────────────────┐
│   Application Layer (Validator Service)     │  ← gRPC/Tonic
├─────────────────────────────────────────────┤
│   Domain Services Layer                     │
│   ├─ Discovery Service (节点发现)            │
│   ├─ State Sync Service (状态同步)          │
│   └─ Randomness Service (随机性)             │
├─────────────────────────────────────────────┤
│   Network Abstraction Layer                 │
│   (Mysten Network)                          │  ← 编解码、连接监控
├─────────────────────────────────────────────┤
│   P2P Framework Layer                       │
│   (Anemo)                                   │  ← QUIC-based P2P
├─────────────────────────────────────────────┤
│   Transport Layer (QUIC/TLS)                │
└─────────────────────────────────────────────┘
```

### 6.2 Tonic/Anemo 的作用

#### 6.2.1 Tonic (gRPC)

**用途**: 验证者之间的 RPC 通信

**解决的问题**:
- ✅ 类型安全的 RPC 接口定义
- ✅ 流式传输支持
- ✅ 连接管理和重试机制
- ✅ 超时和错误处理

**在 DEX 中的应用**:
```rust
// 定义 Sequencer RPC 服务
#[tonic::async_trait]
pub trait SequencerService: Send + Sync {
    async fn submit_order(
        &self,
        request: Request<OrderRequest>,
    ) -> Result<Response<OrderResponse>, Status>;
    
    async fn get_sequence_status(
        &self,
        request: Request<SequenceStatusRequest>,
    ) -> Result<Response<SequenceStatus>, Status>;
}
```

#### 6.2.2 Anemo (P2P 网络)

**用途**: 验证者之间的 P2P 通信

**解决的问题**:
- ✅ 节点发现和连接管理
- ✅ 消息广播（一对多）
- ✅ 连接复用（单连接多流）
- ✅ 自动重连和故障恢复

**关键特性**:
- **QUIC 协议**: 1-RTT 握手，多路复用
- **BCS 序列化**: 确定性序列化
- **Snappy 压缩**: 减少带宽 30-50%

### 6.3 第一阶段网络层使用方案

#### 6.3.1 架构设计

```
┌─────────────────────────────────────────────────────────────┐
│                    Phase 1 Network Architecture              │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌──────────────┐         ┌──────────────┐                  │
│  │  Main Node   │         │  Standby     │                  │
│  │ (Sequencer)  │         │  Nodes       │                  │
│  │              │         │              │                  │
│  │ ┌──────────┐ │         │ ┌──────────┐ │                  │
│  │ │Sequencer │ │         │ │Receiver  │ │                  │
│  │ │  Active  │ │         │ │ Passive  │ │                  │
│  │ └────┬─────┘ │         │ └────┬─────┘ │                  │
│  │      │       │         │      │       │                  │
│  └──────┼───────┘         └──────┼───────┘                  │
│         │                        │                          │
│         │  ┌─────────────────────┘                          │
│         │  │                                                 │
│         ▼  ▼                                                 │
│  ┌─────────────────────────────────────────┐                │
│  │      Sui Network Layer (Anemo)          │                │
│  │  • Order forwarding (主→从)             │                │
│  │  • Sequence broadcast (主→从)           │                │
│  │  • Heartbeat (主↔从)                    │                │
│  │  • Confirmation (从→主)                 │                │
│  └─────────────────────────────────────────┘                │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

#### 6.3.2 消息流程

**场景 1: 客户端提交订单到从节点**

```
Client → Standby Node (via JSON-RPC)
    ↓
Standby Node 检测到 DEX 交易
    ↓
通过 Anemo P2P 转发到 Main Node (Sequencer)
    ↓
Main Node 处理并返回序列号
    ↓
Standby Node 返回给 Client
```

**场景 2: 主节点广播序列**

```
Main Node (Sequencer) 处理订单
    ↓
分配序列号，执行撮合
    ↓
通过 Anemo.broadcast() 广播 SequenceBatch
    ↓
所有 Standby Nodes 接收并确认
    ↓
收集 2f+1 确认后，写入 DA 层
```

**场景 3: 心跳检测**

```
Main Node 定期发送 Heartbeat (via Anemo)
    ↓
Standby Nodes 接收并记录
    ↓
如果超时未收到，触发故障检测
    ↓
选举新的 Main Node
```

#### 6.3.3 关键实现

**1. 复用 Anemo 网络**

```rust
use anemo::Network;
use mysten_network::codec::anemo::BcsSnappyCodec;

pub struct DexSequencerNetwork {
    network: Arc<Network>,
}

impl DexSequencerNetwork {
    /// 转发订单到主节点
    pub async fn forward_to_leader(
        &self,
        leader: AuthorityIndex,
        order: Order,
    ) -> Result<SequenceNumber> {
        let peer = self.network.peer(leader)?;
        let response = peer
            .rpc(DexSequencerServiceClient::submit_order)
            .send(order)
            .await?;
        Ok(response.sequence_number)
    }
    
    /// 广播序列批次
    pub async fn broadcast_sequence_batch(
        &self,
        batch: SequenceBatch,
    ) -> Result<()> {
        // 使用 Anemo 的广播能力
        self.network.broadcast(batch).await?;
        Ok(())
    }
}
```

**2. 定义 Sequencer 消息类型**

```rust
// 复用 Sui 的 BCS 序列化
use sui_types::base_types::TransactionDigest;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct OrderRequest {
    pub transaction: Transaction,
    pub timestamp: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SequenceBatch {
    pub sequence_range: (u64, u64),
    pub orders: Vec<SequencedOrder>,
    pub digest: SequenceDigest,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Heartbeat {
    pub leader: AuthorityIndex,
    pub last_sequence: u64,
    pub timestamp: u64,
}
```

**3. 从节点接收逻辑**

```rust
pub struct StandbyNode {
    network: Arc<Network>,
    sequencer_client: Arc<DexSequencerClient>,
    engine: Arc<MatchingEngine>,
}

impl StandbyNode {
    /// 处理转发来的订单
    pub async fn handle_forwarded_order(&self, order: OrderRequest) {
        // 转发到主节点
        let seq = self.sequencer_client
            .forward_to_leader(order)
            .await?;
        
        // 返回给客户端
        Ok(seq)
    }
    
    /// 接收并重放序列批次
    pub async fn handle_sequence_batch(&self, batch: SequenceBatch) {
        // 1. 验证主节点签名
        self.verify_leader_signature(&batch)?;
        
        // 2. 确定性重放
        self.engine.replay_batch(&batch).await?;
        
        // 3. 发送确认
        self.send_confirmation(batch.sequence_range).await?;
    }
}
```

### 6.4 网络层优势

**复用 Sui 网络层的优势**:

1. **成熟稳定**: Sui 网络层经过生产验证
2. **性能优化**: QUIC 协议、Snappy 压缩、连接复用
3. **自动重连**: 网络故障自动恢复
4. **类型安全**: BCS 序列化保证确定性
5. **减少开发**: 无需自己实现 P2P 网络

**第一阶段使用场景**:

| 场景 | 使用组件 | 说明 |
|-----|---------|------|
| 订单转发 | Anemo P2P | 从节点转发订单到主节点 |
| 序列广播 | Anemo.broadcast() | 主节点广播序列到所有从节点 |
| 心跳检测 | Anemo RPC | 主从节点心跳通信 |
| 确认收集 | Anemo RPC | 从节点发送确认到主节点 |
| 故障切换 | Anemo + Leader Schedule | 检测主节点故障并切换 |

---

## 7. 集成方式选择

### 7.1 方案对比

#### 方案 A: SDK 方式引入（推荐）

**架构**:
```
独立 DEX 项目
├── Cargo.toml
│   └── sui-sdk = { git = "...", branch = "dex-fork" }
├── crates/
│   ├── dex-sequencer/
│   ├── dex-engine/
│   └── dex-storage/
└── 修改 sui-sdk 部分代码（fork）
```

**优点**:
- ✅ **清晰边界**: DEX 代码和 Sui 代码分离
- ✅ **独立发布**: 可以独立版本管理和发布
- ✅ **灵活修改**: Fork Sui 后可以修改必要部分
- ✅ **易于维护**: 代码结构清晰，职责分明
- ✅ **测试隔离**: DEX 测试不影响 Sui 核心测试

**缺点**:
- ⚠️ **依赖管理**: 需要维护 Sui fork 的更新
- ⚠️ **版本同步**: Sui 升级时需要同步更新

**适用场景**: ✅ **推荐用于第一阶段**

#### 方案 B: 直接在 Sui 仓库开发

**架构**:
```
sui/ (Sui 仓库)
├── crates/
│   ├── sui-core/ (修改)
│   ├── sui-execution/ (修改)
│   ├── dex-sequencer/ (新增)
│   ├── dex-engine/ (新增)
│   └── dex-storage/ (新增)
```

**优点**:
- ✅ **深度集成**: 可以深度修改 Sui 核心
- ✅ **统一测试**: 所有测试在一个仓库
- ✅ **版本一致**: 无需担心版本不匹配

**缺点**:
- ❌ **代码混乱**: DEX 代码和 Sui 代码混在一起
- ❌ **维护困难**: 难以区分 DEX 和 Sui 的职责
- ❌ **升级困难**: Sui 升级时容易产生冲突
- ❌ **发布复杂**: 需要维护整个 Sui 仓库

**适用场景**: ❌ **不推荐用于第一阶段**

### 7.2 推荐方案：SDK 方式 + Fork

#### 7.2.1 具体实现

**1. Fork Sui 仓库**

```bash
# Fork Sui 到自己的组织
git clone https://github.com/MystenLabs/sui.git
cd sui
git remote add dex-fork https://github.com/your-org/sui-dex-fork.git

# 创建 dex-fork 分支
git checkout -b dex-fork
```

**2. 修改 Sui 必要部分**

需要修改的文件（最小化）:
- `crates/sui-core/src/authority.rs` - 添加 DEX 路由
- `sui-execution/latest/sui-adapter/src/execution_engine.rs` - 添加 Precompile
- `crates/sui-json-rpc/src/lib.rs` - 扩展 API

**3. 创建独立 DEX 项目**

```bash
# 创建新项目
cargo new dex-l1 --workspace
cd dex-l1

# 创建 crates
cargo new --lib crates/dex-sequencer
cargo new --lib crates/dex-engine
cargo new --lib crates/dex-storage
```

**4. 配置依赖**

```toml
# Cargo.toml (workspace)
[workspace]
members = [
    "crates/dex-sequencer",
    "crates/dex-engine",
    "crates/dex-storage",
]

# crates/dex-sequencer/Cargo.toml
[dependencies]
sui-core = { git = "https://github.com/your-org/sui-dex-fork.git", branch = "dex-fork" }
sui-types = { git = "https://github.com/your-org/sui-dex-fork.git", branch = "dex-fork" }
mysten-network = { git = "https://github.com/your-org/sui-dex-fork.git", branch = "dex-fork" }
```

#### 7.2.2 项目结构

```
dex-l1/
├── Cargo.toml (workspace)
├── crates/
│   ├── dex-sequencer/      # 定序器
│   ├── dex-engine/         # 撮合引擎
│   ├── dex-storage/        # 存储层
│   ├── dex-types/          # 类型定义
│   └── dex-rpc/            # RPC 扩展
├── sui-fork/               # Sui fork 子模块（可选）
│   └── (git submodule)
└── README.md
```

#### 7.2.3 优势总结

1. **代码清晰**: DEX 业务逻辑独立，Sui 基础设施复用
2. **易于维护**: 修改 Sui 部分时只需更新 fork
3. **灵活升级**: 可以选择性升级 Sui 版本
4. **团队协作**: DEX 团队专注业务逻辑，Sui 团队维护基础设施

---

## 8. 实现思路

### 5.1 整体实现流程

```
1. 创建新 Crates
   ├── crates/dex-sequencer/     # 定序器
   ├── crates/dex-engine/        # 撮合引擎
   ├── crates/dex-storage/       # 存储层
   └── crates/dex-framework/      # Move 框架

2. 修改 Sui 核心
   ├── crates/sui-core/src/authority.rs          # 添加路由
   ├── sui-execution/.../execution_engine.rs    # 添加 Precompile
   └── crates/sui-json-rpc/                      # 扩展 API

3. 集成测试
   ├── 单元测试
   ├── 集成测试
   └── 性能测试
```

### 5.2 详细实现步骤

#### 步骤 1: 创建 DEX Sequencer

**文件**: `crates/dex-sequencer/src/lib.rs`

**功能**:
- 接收交易请求
- FIFO 排序，分配全局序列号
- 广播序列到验证者
- 收集确认（2f+1）

**关键实现**:
```rust
pub struct DexSequencer {
    sequence_counter: AtomicU64,
    pending_queue: UnboundedReceiver<Transaction>,
    network: Arc<AuthorityNetwork>,  // 复用 Sui 网络
    engine: Arc<MatchingEngine>,
}

impl DexSequencer {
    pub async fn process_transaction(&self, tx: Transaction) -> Result<SequenceNumber> {
        // 1. 分配序列号
        let seq = self.sequence_counter.fetch_add(1, Ordering::SeqCst);
        
        // 2. 执行交易（调用撮合引擎）
        let result = self.engine.execute(tx.clone(), seq).await?;
        
        // 3. 广播序列
        self.broadcast_sequence(seq, tx, result).await?;
        
        // 4. 等待确认（异步）
        Ok(seq)
    }
}
```

#### 步骤 2: 创建 DEX 撮合引擎

**文件**: `crates/dex-engine/src/lib.rs`

**功能**:
- 内存订单簿管理
- 价格-时间优先撮合
- 余额管理
- 风险检查

**关键实现**:
```rust
pub struct MatchingEngine {
    orderbooks: DashMap<MarketID, Orderbook>,  // 分片锁
    balances: DashMap<AccountID, Balance>,      // 账户余额
}

pub struct Orderbook {
    bids: BTreeMap<Reverse<Price>, VecDeque<Order>>,  // 买单（降序）
    asks: BTreeMap<Price, VecDeque<Order>>,          // 卖单（升序）
    order_index: HashMap<OrderID, OrderRef>,
}

impl MatchingEngine {
    pub fn match_order(&self, order: Order) -> Result<MatchResult> {
        let mut orderbook = self.orderbooks.get_mut(&order.market_id)?;
        
        // 价格-时间优先撮合
        // ...
        
        Ok(MatchResult { ... })
    }
}
```

#### 步骤 3: 修改 Authority 路由

**文件**: `crates/sui-core/src/authority.rs`

**修改点**:
```rust
impl AuthorityState {
    /// 检测是否为 DEX 交易
    fn is_dex_transaction(&self, tx: &Transaction) -> Result<bool> {
        if let Some(call) = tx.as_programmable() {
            for cmd in &call.commands {
                if let Command::MoveCall(mc) = cmd {
                    // 检查是否为 DEX 包地址
                    if mc.package == DEX_PACKAGE_ID {
                        return Ok(true);
                    }
                }
            }
        }
        Ok(false)
    }
    
    /// 提交到 DEX Sequencer
    async fn submit_to_dex_sequencer(&self, tx: Transaction) -> Result<...> {
        let sequencer = self.dex_sequencer.as_ref().ok_or(...)?;
        sequencer.process_transaction(tx).await
    }
}
```

#### 步骤 4: 实现 Precompile 钩子

**文件**: `sui-execution/latest/sui-adapter/src/execution_engine.rs`

**修改点**:
```rust
pub fn execute_transaction_to_effects(...) -> Result<...> {
    // 检测 DEX Precompile
    if let Some(dex_call) = extract_dex_precompile(&transaction_kind)? {
        // 调用原生 DEX 引擎
        let dex_engine = get_dex_engine()?;
        return dex_engine.execute_native(dex_call).await;
    }
    
    // 标准 Move VM 执行
    // ... 原有逻辑
}
```

#### 步骤 5: 扩展 JSON-RPC API

**文件**: `crates/sui-json-rpc/src/dex_api.rs` (新建)

**功能**:
- `dex_place_order` - 下单
- `dex_cancel_order` - 撤单
- `dex_get_orderbook` - 查询订单簿
- `dex_get_balance` - 查询余额

**实现**:
```rust
pub struct DexApi {
    sequencer: Arc<DexSequencer>,
}

#[async_trait]
impl DexApi {
    pub async fn place_order(&self, params: PlaceOrderParams) -> Result<OrderID> {
        // 构建交易
        let tx = self.build_place_order_tx(params).await?;
        
        // 提交到 Sequencer
        let seq = self.sequencer.process_transaction(tx).await?;
        
        Ok(seq.into())
    }
}
```

#### 步骤 6: 创建 Move 框架

**目录**: `crates/dex-framework/packages/dex-framework/`

**Move 代码**:
```move
module dex::dex {
    use sui::coin::{Self, Coin};
    use sui::transfer;
    use sui::tx_context::{Self, TxContext};

    /// 下单（Precompile 拦截）
    public entry fun place_order<B, Q>(
        market_id: vector<u8>,
        side: u8,
        price: u64,
        quantity: u64,
        ctx: &mut TxContext,
    ) {
        // Precompile 会拦截此调用
        abort 0
    }
    
    /// 存款（Hybrid: Move + Native）
    public entry fun deposit<T>(
        coin: Coin<T>,
        ctx: &mut TxContext,
    ) {
        // Precompile 拦截:
        // 1. Move: 锁定 Coin 到托管账户
        // 2. Native: 更新 DEX 余额
        abort 0
    }
}
```

#### 步骤 7: 集成存储层

**文件**: `crates/dex-storage/src/lib.rs`

**实现**:
```rust
use typed_store::rocks::{DBMap, DBBatch};

pub struct DexStore {
    // 复用 Sui 的存储层
    orders: DBMap<OrderID, Order>,
    balances: DBMap<AccountID, Balance>,
    trades: DBMap<TradeID, Trade>,
}

impl DexStore {
    pub fn write_batch(&self, batch: DexBatch) -> Result<()> {
        let mut db_batch = self.orders.batch();
        
        // 写入订单
        db_batch.insert_batch(&self.orders, batch.orders)?;
        
        // 写入余额
        db_batch.insert_batch(&self.balances, batch.balances)?;
        
        // 原子提交
        db_batch.write()?;
        
        Ok(())
    }
}
```

---

## 9. 关键技术决策

### 6.1 为什么使用中心化 Sequencer？

**原因**:
1. **性能要求**: Mysticeti 共识延迟 ~600ms，无法达到 50ms 目标
2. **简化实现**: Phase 1 先实现核心功能，后续再扩展
3. **可扩展**: 为 Phase 2 的多节点轮换预留接口

**风险缓解**:
- 热备份 Sequencer（故障切换 < 100ms）
- DA 层持久化（防止数据丢失）
- 2f+1 验证者确认（保证最终一致性）

### 6.2 为什么使用原生 Rust 引擎？

**原因**:
1. **性能**: Move VM 执行开销大，无法达到 10万 TPS
2. **控制**: 需要细粒度的性能优化（SIMD、无锁设计等）
3. **兼容**: 通过 Precompile 保持 Move 接口兼容

**权衡**:
- ✅ 性能极致
- ✅ 完全控制
- ⚠️ 需要严格测试和审计

### 6.3 如何保证原子性？

**两阶段执行模型**:

1. **Signing Phase**:
   - 计算效果
   - 创建取款锁
   - **不修改余额**

2. **Certificate Execution**:
   - 验证 2f+1 证书
   - **正式 Commit** 状态变更

**形式化不变量**:
```
托管账户余额 == Σ(用户DEX余额) + Σ(Pending存入)
```

### 6.4 如何保证顺序一致性？

**Sequencer 机制**:
- 全局单调递增序列号
- FIFO 排序
- 所有验证者按相同顺序重放

**故障恢复**:
- 从 DA 层获取最后确认序列号
- 新 Sequencer 从该序列号继续

---

## 10. 实施计划

### 7.1 Phase 1 里程碑

| 里程碑 | 任务 | 预计时间 |
|-------|------|---------|
| **M1: 基础设施** | 创建 Crates、基础类型定义 | 1 周 |
| **M2: Sequencer** | 实现中心化定序器 | 2 周 |
| **M3: 撮合引擎** | 实现内存订单簿和撮合算法 | 2 周 |
| **M4: Sui 集成** | 修改 Authority、Precompile | 2 周 |
| **M5: Move 框架** | 创建 dex-framework 包 | 1 周 |
| **M6: 存储层** | 集成 RocksDB、WAL | 1 周 |
| **M7: RPC API** | 扩展 JSON-RPC | 1 周 |
| **M8: 测试** | 单元测试、集成测试、性能测试 | 2 周 |

**总计**: 约 12 周（3 个月）

### 7.2 开发优先级

**P0 (必须)**:
- Sequencer 核心功能
- 撮合引擎基础功能
- Authority 路由修改
- Precompile 钩子

**P1 (重要)**:
- 存储层集成
- Move 框架
- RPC API 扩展
- 基础测试

**P2 (可选)**:
- 性能优化
- 监控和日志
- 文档完善

### 7.3 技术风险

| 风险 | 影响 | 缓解措施 |
|-----|------|---------|
| Sequencer 单点故障 | 高 | 热备份 + 快速故障切换 |
| 数据一致性 | 高 | 2f+1 确认 + WAL |
| Move 兼容性 | 中 | Precompile 桥接，完整测试 |
| 性能不达标 | 中 | 持续优化，SIMD、无锁设计 |
| 安全性 | 高 | 代码审计、形式化验证 |

---

## 8. 参考文档

### 8.1 设计文档
- [`notes/dex_l1/drafts/dex-plan.md`](../../notes/dex_l1/drafts/dex-plan.md) - 核心架构设计
- [`notes/dex_l1/DEX_L1_DESIGN_SUMMARY.md`](../../notes/dex_l1/DEX_L1_DESIGN_SUMMARY.md) - 设计总结
- [`notes/dex_l1/docs/01-REQUIREMENTS.md`](../../notes/dex_l1/docs/01-REQUIREMENTS.md) - 需求规格
- [`notes/dex_l1/docs/02-ARCHITECTURE-OVERVIEW.md`](../../notes/dex_l1/docs/02-ARCHITECTURE-OVERVIEW.md) - 架构总览
- [`notes/dex_l1/docs/07-MOVE-INTEGRATION-DESIGN.md`](../../notes/dex_l1/docs/07-MOVE-INTEGRATION-DESIGN.md) - Move 集成设计

### 8.2 Sui 架构文档
- [`notes/SUI_ARCHITECTURE_REPORT.md`](../../notes/SUI_ARCHITECTURE_REPORT.md) - Sui 架构研究报告
- [`notes/QUICK_START_GUIDE.md`](../../notes/QUICK_START_GUIDE.md) - Sui 快速开始指南

### 8.3 相关研究
- [`notes/research/consensus/`](../../notes/research/consensus/) - 共识层研究
- [`notes/docs/dex-appchain-architecture-v*.md`](../../notes/docs/) - DEX 架构演进

---

## 9. 总结

第一阶段的核心思路是：

1. **复用 Sui 基础设施**: 存储、网络、类型系统、RPC 框架
2. **最小化修改**: 只在关键路径添加路由和 Precompile
3. **原生性能**: 使用 Rust 原生引擎绕过 Move VM
4. **保持兼容**: 通过 Precompile 保持 Move 接口兼容

这样可以在保持 Sui 生态兼容性的同时，实现 CEX 级的交易性能。

---

## 11. 总结

### 11.1 核心要点

1. **复用 Sui 基础设施** (约 40%)
   - 存储层（RocksDB）、网络层（Anemo/Tonic）、类型系统、事件系统
   - 减少开发量，提高稳定性

2. **自行开发业务逻辑** (约 60%)
   - 撮合引擎、风险控制、清算、资金费率等
   - 需要极致性能，必须 Rust 原生实现

3. **网络层集成方案**
   - 复用 Sui 的 Anemo P2P 网络
   - 主节点通过 Anemo 广播序列
   - 从节点通过 Anemo 转发订单和确认

4. **集成方式选择**
   - **推荐**: SDK 方式 + Fork Sui
   - 代码清晰、易于维护、灵活升级

### 11.2 关键决策

| 决策项 | 选择 | 理由 |
|-------|------|------|
| **集成方式** | SDK + Fork | 代码清晰，易于维护 |
| **网络层** | 复用 Anemo | 成熟稳定，性能优化 |
| **存储层** | 复用 typed-store | 经过验证，减少开发 |
| **执行路径** | Precompile 钩子 | 保持 Move 兼容性 |
| **撮合引擎** | Rust 原生 | 性能要求 < 10μs |

### 11.3 下一步行动

1. **Fork Sui 仓库**，创建 `dex-fork` 分支
2. **创建独立 DEX 项目**，配置 Sui SDK 依赖
3. **实现核心组件**：Sequencer、Matching Engine、Storage
4. **集成测试**：验证与 Sui 的集成点
5. **性能测试**：验证延迟和吞吐量目标

---

**文档版本**: v1.1  
**最后更新**: 2025-01-XX  
**维护者**: DEX 开发团队

