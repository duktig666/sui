# DEX 自研开发模块依赖分析

本文档聚焦于 Sui 平台上开发去中心化交易所 (DEX) 所需理解的核心模块及其依赖关系,为 DEX 自研项目提供技术参考。

---

## 一、DEX 开发模块优先级分类

### 高优先级模块 (必须深入理解)

这些模块是 DEX 开发的核心基础,必须深入理解其设计和实现。

| 模块 | 功能 | 为何重要 | Cargo.toml 路径 |
|------|------|---------|----------------|
| **sui-types** | 核心类型定义 | 定义 Object、Transaction、Event、Signature 等所有基础数据结构 | `crates/sui-types/Cargo.toml` |
| **sui-transaction-builder** | 交易构建器 | 构建 DEX 交易(下单、取消、成交)的工具 | `crates/sui-transaction-builder/Cargo.toml` |
| **sui-storage** | 存储层 | 理解对象存储机制,用于 DEX 数据持久化 | `crates/sui-storage/Cargo.toml` |
| **sui-indexer-alt** 系列 | 新索引器框架 | 索引 DEX 事件、查询订单状态、构建订单簿 | `crates/sui-indexer-alt*/Cargo.toml` |
| **sui-framework** | Move 标准库 | 提供 Coin、Balance、Transfer 等标准 Move 模块 | `crates/sui-framework/Cargo.toml` |

### 中优先级模块 (建议理解)

这些模块对 DEX 开发有辅助作用,建议理解其基本原理。

| 模块 | 功能 | 用途 | Cargo.toml 路径 |
|------|------|------|----------------|
| **sui-json-rpc** | JSON-RPC API | 提供查询接口,供前端和客户端访问 | `crates/sui-json-rpc/Cargo.toml` |
| **sui-graphql-rpc** | GraphQL API | 灵活的查询接口,适合复杂查询场景 | `crates/sui-graphql-rpc/Cargo.toml` |
| **sui-sdk** | Rust SDK | 客户端开发参考,理解如何与链交互 | `crates/sui-sdk/Cargo.toml` |
| **sui-data-ingestion-core** | 数据摄取 | 从检查点中读取数据,用于索引器 | `crates/sui-data-ingestion-core/Cargo.toml` |
| **sui-execution** | 执行层 | 理解 Move 合约如何执行,Gas 如何计费 | `sui-execution/Cargo.toml` |

### 低优先级模块 (可选了解)

这些模块对 DEX 核心功能不是必需的,但在特定场景下有用。

| 模块 | 功能 | 场景 |
|------|------|------|
| **sui-core** | 验证器核心 | 运行自己的验证器节点时需要 |
| **sui-node** | 验证器节点 | 部署完整节点时需要 |
| **consensus/** | 共识机制 | 理解交易最终性和排序机制 |
| **sui-bridge** | 跨链桥 | 支持跨链资产时需要 |
| **sui-benchmark** | 性能测试 | 压测 DEX 性能时需要 |

---

## 二、sui-types 核心子模块深度分析

`sui-types` 是 DEX 开发最重要的模块,定义了所有核心数据结构。

### 2.1 Object 模块 (对象模型)

**文件位置**: `crates/sui-types/src/object.rs` (1401 行)

#### 核心数据结构

| 类型 | 功能 | 行号参考 | DEX 应用场景 |
|------|------|---------|-------------|
| `Object` | 对象容器,Arc 包装的 ObjectInner | ~647 | 所有链上实体(订单、池子、用户资产) |
| `MoveObject` | Move 对象数据 | 54-66 | DEX 订单、流动性池 |
| `ObjectInner` | 对象内部结构 | 632-643 | 包含 data, owner, previous_tx, storage_rebate |
| `Owner` | 所有权枚举 | 500-522 | 决定对象的访问和并发性 |
| `MoveObjectType` | 对象类型 | 237-272 | 识别 Coin、Balance、自定义类型 |
| `Data` | 对象数据包装 | 423-429 | Move 对象或 Package |

#### Owner 枚举详解

```rust
pub enum Owner {
    AddressOwner(SuiAddress),           // 地址专属所有
    ObjectOwner(SuiAddress),            // 对象所有(父子关系)
    Shared { initial_shared_version },  // 共享对象(需要共识排序)
    Immutable,                          // 不可变
    ConsensusAddressOwner {             // 共识地址所有
        start_version: SequenceNumber,
        owner: SuiAddress,
    },
}
```

**对 DEX 设计的影响**:

| Owner 类型 | DEX 使用场景 | 执行路径 | 并发性 |
|-----------|-------------|---------|--------|
| **AddressOwner** | 用户持有的 Coin (如 USDC, SUI) | Fast Path (无需共识) | 高 (用户独占) |
| **Shared** | 订单簿、流动性池、全局配置 | Consensus Path (需共识排序) | 低 (全局锁) |
| **Immutable** | 价格预言机数据、历史快照 | Fast Path | 极高 (只读) |
| **ObjectOwner** | 子订单、嵌套结构 | 取决于父对象 | 中等 |
| **ConsensusAddressOwner** | 通过共识管理的用户资产 | Consensus Path | 中等 |

**DEX 架构选择**:
- **CLOB (中心化限价订单簿)**: 订单簿使用 `Shared` 对象,所有订单操作需共识排序
- **AMM (自动做市商)**: 流动性池使用 `Shared` 对象,swap 操作需共识
- **用户资产**: 用户钱包中的 Coin 使用 `AddressOwner`,充值/提现快速

#### 版本控制机制

**文件位置**: `crates/sui-types/src/base_types.rs` (2083 行)

| 类型 | 定义 | DEX 用途 |
|------|------|---------|
| `ObjectID` | 32 字节唯一标识符 | 订单 ID、池子 ID |
| `SequenceNumber` | u64 版本号(Lamport 时间戳) | 对象版本,防止双花 |
| `ObjectRef` | (ObjectID, SequenceNumber, ObjectDigest) | 引用特定版本的对象 |
| `ObjectDigest` | Blake2b256 哈希 | 对象内容完整性校验 |

**版本控制示例**:
```
订单对象初始创建:
ObjectRef = (order_id: 0x1234..., version: 1, digest: 0xabcd...)

订单部分成交后:
ObjectRef = (order_id: 0x1234..., version: 2, digest: 0xef01...)

订单完全成交(删除):
ObjectRef = (order_id: 0x1234..., version: 3, digest: 0x2345...)
```

---

### 2.2 Event 模块 (事件系统)

**文件位置**: `crates/sui-types/src/event.rs`

#### 核心数据结构

| 类型 | 功能 | DEX 用途 |
|------|------|---------|
| `EventEnvelope` | 事件容器 | 包装所有事件,带时间戳和交易摘要 |
| `EventID` | 事件唯一标识 | (tx_digest, event_seq) 定位特定事件 |
| `Event` | 具体事件类型 | 订单创建、成交、取消事件 |

#### DEX 事件设计示例

```rust
// 订单创建事件
pub struct OrderPlacedEvent {
    pub order_id: ObjectID,
    pub trader: SuiAddress,
    pub side: OrderSide,       // Buy / Sell
    pub price: u64,
    pub quantity: u64,
    pub timestamp: u64,
}

// 订单成交事件
pub struct OrderMatchedEvent {
    pub maker_order_id: ObjectID,
    pub taker_order_id: ObjectID,
    pub price: u64,
    pub quantity: u64,
    pub maker_filled: u64,
    pub taker_filled: u64,
    pub timestamp: u64,
}

// 订单取消事件
pub struct OrderCancelledEvent {
    pub order_id: ObjectID,
    pub trader: SuiAddress,
    pub remaining_quantity: u64,
    pub timestamp: u64,
}
```

**事件索引流程**:
```
Move 合约发出事件
       ↓
TransactionEffects 包含 events_digest
       ↓
索引器监听 EventEnvelope
       ↓
解析事件并存入数据库
       ↓
前端通过 WebSocket 订阅实时更新
```

---

### 2.3 Signature & Crypto 模块 (签名与密钥)

**文件位置**:
- `crates/sui-types/src/crypto.rs` (1827 行)
- `crates/sui-types/src/signature.rs` (13 行声明)
- `crates/sui-types/src/multisig.rs`

#### 密钥类型

```rust
// Authority (验证器) 使用 BLS12381 聚合签名
pub type AuthorityKeyPair = BLS12381KeyPair;
pub type AuthoritySignature = BLS12381Signature;
pub type AggregateAuthoritySignature = BLS12381AggregateSignature;

// 用户账户使用 Ed25519 签名
pub type AccountKeyPair = Ed25519KeyPair;
pub type AccountPublicKey = Ed25519PublicKey;

// 哈希算法
pub type DefaultHash = Blake2b256;
```

#### 签名类型

```rust
pub enum GenericSignature {
    Signature,               // 单签 (Ed25519, Secp256k1, Secp256r1)
    MultiSig,                // 多签 (m-of-n)
    MultiSigLegacy,          // 旧版多签
    ZkLoginAuthenticator,    // zkLogin (OAuth 登录)
    PasskeyAuthenticator,    // Passkey (WebAuthn)
}
```

**DEX 应用场景**:

| 签名类型 | DEX 场景 | 示例 |
|---------|---------|------|
| **Ed25519 单签** | 普通用户交易 | 用户下单、取消订单 |
| **MultiSig** | 多签钱包 | 机构账户、托管钱包 |
| **ZkLogin** | 社交登录 | 用户通过 Google 登录交易 |
| **Passkey** | 硬件密钥 | 高安全性交易 |
| **BLS12381 聚合签名** | 验证器认证 | CertifiedTransaction |

**交易签名流程**:
```
用户构造 TransactionData
       ↓
用 Ed25519 私钥签名 → Signature
       ↓
Envelope<SenderSignedData, EmptySignInfo> (Transaction)
       ↓
发送到验证器
       ↓
验证器验证签名后执行
       ↓
2f+1 验证器签名 → CertifiedTransaction
```

---

### 2.4 Transaction 模块 (交易结构)

**文件位置**: `crates/sui-types/src/transaction.rs` (4716 行,最大的文件)

#### 核心数据结构

| 类型 | 行号 | 功能 | DEX 用途 |
|------|------|------|---------|
| `TransactionKind` | 458 | 交易类型枚举 | ProgrammableTransaction 为主 |
| `ProgrammableTransaction` | 964 | 可编程交易块 (PTB) | DEX 所有操作 |
| `Command` | 982 | PTB 命令 | MoveCall, TransferObjects 等 |
| `Argument` | 1015 | 命令参数 | Input, Result, NestedResult |
| `ProgrammableMoveCall` | 1032 | Move 函数调用 | 调用 DEX 合约函数 |
| `CallArg` | 104 | 交易入参 | Pure, Object, FundsWithdrawal |
| `ObjectArg` | 130 | 对象参数 | ImmOrOwnedObject, SharedObject |
| `GasData` | 1923 | Gas 配置 | 指定 Gas Coin 和预算 |
| `TransactionData` | 1984 | 交易数据 (版本化) | TransactionDataV1 |
| `Transaction` | 3648 | 已签名交易 | Envelope<SenderSignedData> |
| `CertifiedTransaction` | 3712 | 已认证交易 | 2f+1 签名的交易 |

#### Programmable Transaction Blocks (PTB)

```rust
pub struct ProgrammableTransaction {
    pub inputs: Vec<CallArg>,      // 输入参数
    pub commands: Vec<Command>,    // 执行命令序列
}

pub enum Command {
    MoveCall(Box<ProgrammableMoveCall>),  // 调用 Move 函数
    TransferObjects(Vec<Argument>, Argument),  // 转移对象
    SplitCoins(Argument, Vec<Argument>),       // 拆分 Coin
    MergeCoins(Argument, Vec<Argument>),       // 合并 Coin
    Publish(Vec<Vec<u8>>, Vec<ObjectID>),      // 发布 Move 包
    MakeMoveVec(Option<TypeInput>, Vec<Argument>), // 创建 vector
    Upgrade(Vec<Vec<u8>>, Vec<ObjectID>, ObjectID, Argument), // 升级包
}

pub enum Argument {
    GasCoin,                    // Gas Coin
    Input(u16),                 // inputs[i]
    Result(u16),                // commands[i] 的结果
    NestedResult(u16, u16),     // commands[i] 的第 j 个返回值
}
```

#### DEX 交易示例

**场景 1: 限价买单**

```rust
ProgrammableTransaction {
    inputs: [
        Pure(bcs::to_bytes(&price)),        // Input(0): 价格
        Pure(bcs::to_bytes(&quantity)),     // Input(1): 数量
        Object(SharedObject {               // Input(2): 订单簿
            id: orderbook_id,
            initial_shared_version: 123,
            mutable: true,
        }),
        Object(ImmOrOwnedObject(            // Input(3): 用户 USDC
            user_usdc_coin_ref,
        )),
    ],
    commands: [
        // 1. 拆分所需的 USDC
        SplitCoins(
            Argument::Input(3),              // 用户 USDC
            vec![Argument::Input(1)]         // 数量
        ),
        // 2. 调用 DEX 下单函数
        MoveCall(ProgrammableMoveCall {
            package: dex_package_id,
            module: "orderbook",
            function: "place_limit_order",
            type_arguments: vec![USDC_TYPE, SUI_TYPE],
            arguments: vec![
                Argument::Input(2),          // orderbook
                Argument::Input(0),          // price
                Argument::Result(0),         // split 出的 USDC
            ],
        }),
    ],
}
```

**场景 2: 取消订单并退款**

```rust
ProgrammableTransaction {
    inputs: [
        Object(SharedObject {               // Input(0): 订单簿
            id: orderbook_id,
            ...
        }),
        Pure(bcs::to_bytes(&order_id)),     // Input(1): 订单 ID
    ],
    commands: [
        // 1. 取消订单,返回剩余 Coin
        MoveCall(ProgrammableMoveCall {
            package: dex_package_id,
            module: "orderbook",
            function: "cancel_order",
            type_arguments: vec![USDC_TYPE, SUI_TYPE],
            arguments: vec![
                Argument::Input(0),          // orderbook
                Argument::Input(1),          // order_id
            ],
        }),
        // 2. 转移退款到用户地址
        TransferObjects(
            vec![Argument::Result(0)],       // 取消订单返回的 Coin
            Argument::GasCoin,               // 目标地址(从 GasCoin owner 推导)
        ),
    ],
}
```

**PTB 的优势**:
- **原子性**: 所有命令要么全部成功,要么全部回滚
- **组合性**: 可链式调用多个函数,Result 作为后续 Input
- **Gas 效率**: 一次交易完成多个操作,节省 Gas
- **灵活性**: 支持动态参数和条件逻辑

---

### 2.5 Effects 模块 (交易效果)

**文件位置**:
- `crates/sui-types/src/effects/mod.rs` (475 行)
- `crates/sui-types/src/effects/effects_v2.rs` (31057 行)
- `crates/sui-types/src/effects/object_change.rs` (470 行)

#### 核心数据结构

```rust
pub enum TransactionEffects {
    V1(TransactionEffectsV1),
    V2(TransactionEffectsV2),  // 当前版本
}

pub struct TransactionEffectsV2 {
    pub status: ExecutionStatus,              // Success / Failure
    pub executed_epoch: EpochId,
    pub gas_used: GasCostSummary,
    pub transaction_digest: TransactionDigest,
    pub gas_object_index: Option<u32>,
    pub events_digest: Option<TransactionEventsDigest>,
    pub dependencies: Vec<TransactionDigest>,
    pub lamport_version: SequenceNumber,
    pub changed_objects: Vec<(ObjectID, EffectsObjectChange)>,
    pub unchanged_consensus_objects: Vec<(ObjectID, UnchangedConsensusKind)>,
    pub aux_data_digest: Option<EffectsAuxDataDigest>,
}

pub struct EffectsObjectChange {
    pub input_state: ObjectIn,      // NotExist / Exist(version, digest)
    pub output_state: ObjectOut,    // ObjectWrite / PackageWrite / DeleteOrWrap
    pub id_operation: IDOperation,  // Created / Mutated / Deleted / None
}

pub enum ExecutionStatus {
    Success,
    Failure {
        error: ExecutionFailureStatus,
        command: Option<u64>,
    },
}
```

#### DEX 应用场景

**场景: 订单部分成交**

```rust
TransactionEffectsV2 {
    status: Success,
    gas_used: GasCostSummary {
        computation_cost: 1_000_000,
        storage_cost: 500_000,
        storage_rebate: 200_000,
        non_refundable_storage_fee: 50_000,
    },
    changed_objects: [
        // 1. Maker 订单被修改(部分成交)
        (maker_order_id, EffectsObjectChange {
            input_state: Exist(version: 5, digest: 0xaaa...),
            output_state: ObjectWrite(version: 6, digest: 0xbbb...),
            id_operation: Mutated,
        }),
        // 2. Taker Coin 被消耗
        (taker_coin_id, EffectsObjectChange {
            input_state: Exist(version: 2, digest: 0xccc...),
            output_state: DeleteOrWrap,
            id_operation: Deleted,
        }),
        // 3. 新创建 Maker 收到的 Coin
        (new_coin_id_1, EffectsObjectChange {
            input_state: NotExist,
            output_state: ObjectWrite(version: 1, digest: 0xddd...),
            id_operation: Created,
        }),
        // 4. 新创建 Taker 收到的 Coin
        (new_coin_id_2, EffectsObjectChange {
            input_state: NotExist,
            output_state: ObjectWrite(version: 1, digest: 0xeee...),
            id_operation: Created,
        }),
        // 5. Gas Coin 被扣费
        (gas_coin_id, EffectsObjectChange {
            input_state: Exist(version: 10, digest: 0xfff...),
            output_state: ObjectWrite(version: 11, digest: 0x000...),
            id_operation: Mutated,
        }),
    ],
    events_digest: Some(event_digest),  // 包含 OrderMatchedEvent
    ...
}
```

**Effects 的用途**:
- **状态追踪**: 确认订单状态变化(部分成交、完全成交、取消)
- **余额更新**: 追踪新创建的 Coin,更新用户余额
- **Gas 计算**: 统计 DEX 操作的 Gas 成本
- **失败诊断**: 识别交易失败原因(余额不足、订单不存在等)

---

### 2.6 其他重要模块

| 文件 | 行数 | 核心功能 | DEX 用途 |
|------|------|---------|---------|
| `base_types.rs` | 2083 | ObjectID, SequenceNumber, SuiAddress, TransactionDigest | 所有基础类型 |
| `balance.rs` | 154 | Balance<T> 结构体 | 流动性池余额 |
| `coin.rs` | - | Coin<T> 结构体 | 用户持有的代币 |
| `dynamic_field.rs` | 633 | 动态字段存储 | 订单额外数据(价格、时间戳) |
| `gas.rs` | 12063 | Gas 计费模型 | 理解 Gas 成本 |
| `execution.rs` | 388 | ExecutionResults, SharedInput | 共享对象执行 |
| `execution_status.rs` | 473 | ExecutionStatus, ExecutionFailureStatus | 错误处理 |
| `digests.rs` | 1213 | 各种摘要类型 | ObjectDigest, TransactionDigest |
| `message_envelope.rs` | 458 | Envelope, VerifiedEnvelope | 消息包装和验证 |

---

## 三、其他核心模块依赖分析

### 3.1 sui-transaction-builder (交易构建器)

**Cargo.toml 路径**: `crates/sui-transaction-builder/Cargo.toml`

#### 直接依赖

```toml
[dependencies]
sui-json-rpc-types.workspace = true
sui-types.workspace = true
sui-json.workspace = true
sui-protocol-config.workspace = true

move-binary-format.workspace = true
move-core-types.workspace = true
```

#### 依赖深度

```
sui-transaction-builder
├─ sui-types (直接)
├─ sui-protocol-config (直接)
├─ sui-json
│  └─ sui-types
└─ sui-json-rpc-types
   └─ sui-types
```

**依赖深度**: 1-2 层(非常轻量)

**特点**:
- 不依赖 `sui-core` 或 `sui-storage`
- 适合客户端集成
- 提供高级 API 构建 PTB

#### 核心功能

```rust
// 构建 Move 调用
pub fn move_call(
    package_id: ObjectID,
    module: &str,
    function: &str,
    type_args: Vec<TypeTag>,
    call_args: Vec<CallArg>,
) -> Command;

// 构建转账
pub fn transfer_objects(
    objects: Vec<Argument>,
    recipient: SuiAddress,
) -> Command;

// 构建 Coin 拆分
pub fn split_coins(
    coin: Argument,
    amounts: Vec<u64>,
) -> Vec<Command>;
```

**DEX 使用示例**:

```rust
use sui_transaction_builder::TransactionBuilder;

let mut builder = TransactionBuilder::new(sender);

// 1. 构建下单交易
builder.move_call(
    dex_package,
    "orderbook",
    "place_limit_order",
    vec![type_arg!(USDC), type_arg!(SUI)],
    vec![
        CallArg::Pure(bcs::to_bytes(&price)?),
        CallArg::Pure(bcs::to_bytes(&quantity)?),
        CallArg::Object(ObjectArg::SharedObject {
            id: orderbook_id,
            initial_shared_version: 123,
            mutable: true,
        }),
        CallArg::Object(ObjectArg::ImmOrOwnedObject(usdc_coin_ref)),
    ],
);

let tx_data = builder.finish();
```

---

### 3.2 sui-storage (存储层)

**Cargo.toml 路径**: `crates/sui-storage/Cargo.toml`

#### 直接依赖

```toml
[dependencies]
sui-types.workspace = true
sui-json-rpc-types.workspace = true
sui-protocol-config.workspace = true
sui-config.workspace = true
typed-store.workspace = true

move-core-types.workspace = true
move-binary-format.workspace = true
move-bytecode-utils.workspace = true
```

#### 依赖深度

```
sui-storage
├─ sui-types (直接)
├─ sui-protocol-config (直接)
├─ typed-store (RocksDB 封装)
└─ sui-config
```

**依赖深度**: 1-2 层

**特点**:
- 不依赖 `sui-core`,保持分层架构
- 使用 RocksDB 作为后端
- 管理对象存储、检查点、事务日志

#### 核心功能

**对象存储**:
```rust
// 读取对象
pub fn get_object(&self, object_id: &ObjectID) -> Option<Object>;

// 写入对象
pub fn insert_object(&mut self, object: Object);

// 删除对象
pub fn remove_object(&mut self, object_id: &ObjectID);
```

**检查点管理**:
```rust
// 读取检查点
pub fn get_checkpoint(&self, seq: CheckpointSequenceNumber) -> Option<Checkpoint>;

// 写入检查点
pub fn insert_checkpoint(&mut self, checkpoint: Checkpoint);
```

**DEX 应用**:
- 本地节点存储订单对象
- 检查点同步 DEX 状态
- 历史数据查询

---

### 3.3 sui-indexer-alt 系列 (新索引器框架)

#### 架构概览

```
sui-indexer-alt (主程序)
├─ sui-indexer-alt-framework (核心框架)
│  ├─ 数据摄取 (从检查点读取)
│  ├─ 数据处理 (解析事件和对象)
│  └─ 数据存储 (写入 PostgreSQL)
├─ sui-indexer-alt-schema (数据库 schema)
├─ sui-indexer-alt-reader (查询接口)
├─ sui-indexer-alt-jsonrpc (JSON-RPC 接口)
└─ sui-indexer-alt-graphql (GraphQL 接口)
```

#### sui-indexer-alt-framework 依赖

**Cargo.toml 路径**: `crates/sui-indexer-alt-framework/Cargo.toml`

```toml
[dependencies]
sui-field-count.workspace = true
sui-futures.workspace = true
sui-indexer-alt-framework-store-traits.workspace = true
sui-indexer-alt-metrics.workspace = true
sui-rpc.workspace = true
sui-sdk-types.workspace = true
sui-rpc-api.workspace = true
sui-storage.workspace = true
sui-types.workspace = true

[dependencies.sui-pg-db]
workspace = true
optional = true
```

**依赖深度**:
```
sui-indexer-alt-framework
├─ sui-types (直接)
├─ sui-storage (直接)
│  └─ sui-types
├─ sui-rpc-api
│  └─ sui-sdk-types
└─ sui-pg-db (可选)
```

**依赖深度**: 2-3 层

**特点**:
- 不依赖 `sui-core`,轻量级
- 模块化设计,职责清晰
- 支持 PostgreSQL,查询灵活

#### DEX 索引器设计

**数据流**:
```
Sui 节点
  ↓ (检查点)
sui-data-ingestion-core (读取检查点)
  ↓
sui-indexer-alt-framework (解析数据)
  ↓ (提取)
- 订单对象 (Object)
- 订单事件 (OrderPlacedEvent, OrderMatchedEvent)
- 余额变化 (BalanceChange)
  ↓ (存储)
PostgreSQL
  ↓ (查询)
sui-indexer-alt-reader
  ↓ (接口)
JSON-RPC / GraphQL
  ↓
DEX 前端
```

**数据库 Schema 示例**:

```sql
-- 订单表
CREATE TABLE dex_orders (
    order_id BYTEA PRIMARY KEY,
    trader BYTEA NOT NULL,
    orderbook_id BYTEA NOT NULL,
    side TEXT NOT NULL,  -- 'buy' / 'sell'
    price BIGINT NOT NULL,
    original_quantity BIGINT NOT NULL,
    filled_quantity BIGINT NOT NULL,
    status TEXT NOT NULL,  -- 'open' / 'partial' / 'filled' / 'cancelled'
    created_tx BYTEA NOT NULL,
    created_checkpoint BIGINT NOT NULL,
    updated_tx BYTEA,
    updated_checkpoint BIGINT,
    INDEX idx_trader (trader),
    INDEX idx_orderbook (orderbook_id),
    INDEX idx_status (status)
);

-- 成交历史表
CREATE TABLE dex_trades (
    id SERIAL PRIMARY KEY,
    maker_order_id BYTEA NOT NULL,
    taker_order_id BYTEA NOT NULL,
    price BIGINT NOT NULL,
    quantity BIGINT NOT NULL,
    tx_digest BYTEA NOT NULL,
    checkpoint BIGINT NOT NULL,
    timestamp BIGINT NOT NULL,
    INDEX idx_maker (maker_order_id),
    INDEX idx_taker (taker_order_id),
    INDEX idx_checkpoint (checkpoint),
    INDEX idx_timestamp (timestamp)
);

-- 订单簿快照表
CREATE TABLE dex_orderbook_snapshots (
    orderbook_id BYTEA NOT NULL,
    checkpoint BIGINT NOT NULL,
    bids JSONB NOT NULL,  -- [{price, quantity}, ...]
    asks JSONB NOT NULL,
    PRIMARY KEY (orderbook_id, checkpoint)
);
```

**查询接口示例**:

```rust
// 查询用户订单
pub async fn get_user_orders(
    &self,
    trader: SuiAddress,
    status: Option<OrderStatus>,
) -> Vec<Order>;

// 查询订单簿深度
pub async fn get_orderbook_depth(
    &self,
    orderbook_id: ObjectID,
    depth: usize,
) -> OrderbookDepth;

// 查询成交历史
pub async fn get_trade_history(
    &self,
    orderbook_id: ObjectID,
    start_time: u64,
    end_time: u64,
) -> Vec<Trade>;
```

---

### 3.4 sui-json-rpc (JSON-RPC API)

**Cargo.toml 路径**: `crates/sui-json-rpc/Cargo.toml`

#### 直接依赖 (重型)

```toml
[dependencies]
sui-core.workspace = true  # ⚠️ 重型依赖
sui-display.workspace = true
sui-storage.workspace = true
sui-types.workspace = true
sui-json.workspace = true
sui-json-rpc-api.workspace = true
sui-name-service.workspace = true
sui-protocol-config.workspace = true
sui-config.workspace = true
sui-json-rpc-types.workspace = true
sui-transaction-builder.workspace = true
```

**依赖深度**:
```
sui-json-rpc
├─ sui-core (直接) ⚠️
│  ├─ sui-execution
│  ├─ sui-framework
│  ├─ sui-storage
│  └─ consensus-*
├─ sui-storage (直接)
│  └─ sui-types
├─ sui-types (直接)
└─ sui-transaction-builder
   └─ sui-types
```

**依赖深度**: 2-4 层(重型模块)

**特点**:
- 直接依赖 `sui-core`,包含完整验证器逻辑
- 适合全节点 RPC 服务
- 不适合轻量级客户端

**DEX 相关 RPC 方法**:

```rust
// 查询对象 (订单、池子)
sui_getObject(object_id: ObjectID, options: ObjectDataOptions) -> Object

// 查询多个对象
sui_multiGetObjects(object_ids: Vec<ObjectID>) -> Vec<Object>

// 查询事件 (订单事件)
sui_queryEvents(query: EventFilter, cursor: EventID, limit: usize) -> EventPage

// 发送交易
sui_executeTransactionBlock(
    tx_bytes: Base64,
    signatures: Vec<Base64>,
    options: TransactionBlockResponseOptions,
) -> TransactionBlockResponse

// 模拟交易 (预估 Gas)
sui_dryRunTransactionBlock(tx_bytes: Base64) -> DryRunTransactionBlockResponse
```

---

## 四、依赖策略建议

### 4.1 最小依赖集 (核心类型)

适用于:轻量级客户端、SDK、工具

```toml
[dependencies]
sui-types = { workspace = true }
sui-protocol-config = { workspace = true }
sui-json = { workspace = true }
move-core-types = { workspace = true }
```

**用途**:
- 解析链上数据
- 构造基础数据结构
- 验证签名

---

### 4.2 扩展依赖集 (+ 交易构建)

适用于:交易客户端、钱包

```toml
[dependencies]
sui-types = { workspace = true }
sui-protocol-config = { workspace = true }
sui-json = { workspace = true }
sui-transaction-builder = { workspace = true }  # 新增
sui-json-rpc-types = { workspace = true }       # 新增
move-core-types = { workspace = true }
```

**用途**:
- 构建 PTB
- 下单、取消订单
- 转账、Swap

---

### 4.3 完整依赖集 (+ RPC 服务 / 索引器)

适用于:全节点、索引器、后端服务

```toml
[dependencies]
sui-types = { workspace = true }
sui-protocol-config = { workspace = true }
sui-storage = { workspace = true }              # 新增
sui-indexer-alt-framework = { workspace = true }  # 新增
sui-indexer-alt-schema = { workspace = true }     # 新增
sui-pg-db = { workspace = true }                  # 新增
sui-data-ingestion-core = { workspace = true }    # 新增
```

**用途**:
- 索引 DEX 事件
- 提供 RPC 查询
- 构建订单簿快照

---

### 4.4 应避免的依赖

❌ **不要依赖**:
- `sui-core` - 引入完整验证器逻辑,除非运行全节点
- `sui-execution` - 执行层,通常不需要直接依赖
- `consensus-*` - 共识层,只在运行验证器时需要

**原因**:
- 依赖链深,编译慢
- 引入不必要的功能
- 增加二进制体积

---

## 五、架构模式分析

### 5.1 对象模型架构

#### Owned vs Shared 对象对 DEX 的影响

| 对象类型 | 优势 | 劣势 | DEX 场景 |
|---------|------|------|---------|
| **Owned (AddressOwner)** | Fast Path 执行,高并发,低延迟 | 只能单用户访问 | 用户钱包 Coin、个人订单历史 |
| **Shared** | 多用户访问,全局状态 | Consensus Path,低并发,高延迟 | 订单簿、流动性池、全局配置 |
| **Immutable** | 高并发读,零冲突 | 无法修改 | 价格快照、历史记录 |

#### 架构选择

**方案 1: 全局订单簿 (Shared Object)**

```
订单簿 (Shared)
├─ 买单队列: Vec<Order>
└─ 卖单队列: Vec<Order>

所有交易都访问同一个 Shared 对象
→ 需要共识排序
→ TPS 受限于共识延迟 (~400ms)
```

**优势**: 实现简单,订单簿一致性强
**劣势**: TPS 低,延迟高

**方案 2: 用户订单对象 (Owned) + 索引器聚合**

```
每个订单是独立的 Owned 对象
订单对象 owner = 用户地址

索引器监听订单事件
→ 聚合构建订单簿视图
→ 匹配引擎在链下运行
→ 成交结果上链

所有订单操作 Fast Path
→ TPS 高,延迟低
```

**优势**: 高 TPS,低延迟,可扩展
**劣势**: 实现复杂,需要链下匹配引擎

**方案 3: 混合架构**

```
全局流动性池 (Shared) - 用于 AMM Swap
订单簿索引 (链下) - 用于限价单
结算池 (Shared) - 用于批量结算

用户提交订单 → Fast Path (Owned 对象)
匹配引擎聚合 → 批量结算上链 (Shared 对象)
```

**优势**: 平衡 TPS 和一致性
**劣势**: 架构复杂度最高

---

### 5.2 事件系统架构

#### 事件发布 → 索引 → 查询流程

```
Move 合约
  ↓ emit event
TransactionEffects.events_digest
  ↓
检查点 (Checkpoint)
  ↓ 数据摄取
sui-data-ingestion-core
  ↓ 解析 EventEnvelope
sui-indexer-alt-framework
  ↓ 写入数据库
PostgreSQL
  ↓ 查询
sui-indexer-alt-reader
  ↓ API
JSON-RPC / GraphQL / WebSocket
  ↓
DEX 前端
```

#### 实时订阅

```rust
// WebSocket 订阅订单簿更新
ws.subscribe("orderbook_updates", {
    orderbook_id: "0x1234...",
});

// 后端推送
ws.on_event(|event: OrderMatchedEvent| {
    ws.send({
        type: "trade",
        price: event.price,
        quantity: event.quantity,
        timestamp: event.timestamp,
    });
});
```

---

### 5.3 交易执行架构

#### Programmable Transactions 支持复杂 DEX 操作

**原子化操作示例**:

```rust
// 场景: 下单 + 自动成交 + 退款
ProgrammableTransaction {
    inputs: [...],
    commands: [
        // 1. 拆分 Coin
        SplitCoins(user_coin, amount),

        // 2. 下限价单
        MoveCall("place_order", [...]),

        // 3. 检查是否立即成交
        MoveCall("try_match", [Result(1)]),

        // 4. 如果有剩余,退款
        TransferObjects([Result(2)], user_address),
    ],
}
```

**优势**:
- 一次交易完成多步操作
- 节省 Gas
- 避免中间状态

---

### 5.4 索引器架构对比

#### 旧架构: sui-deepbook-indexer

```
sui-deepbook-indexer
├─ sui-indexer-builder (框架)
├─ sui-data-ingestion-core (数据摄取)
└─ 特定于 DeepBook 的逻辑

特点:
- 单体架构
- 与 DeepBook 强耦合
- 扩展性较差
```

#### 新架构: sui-indexer-alt 系列

```
sui-indexer-alt (主程序)
├─ sui-indexer-alt-framework (通用框架)
│  ├─ 数据摄取
│  ├─ 数据处理
│  └─ 存储抽象
├─ sui-indexer-alt-schema (schema 定义)
├─ sui-indexer-alt-reader (查询层)
├─ sui-indexer-alt-jsonrpc (JSON-RPC 接口)
└─ sui-indexer-alt-graphql (GraphQL 接口)

特点:
- 模块化设计
- 职责清晰分离
- 易于定制和扩展
- 支持多种接口
```

**推荐**: DEX 自研使用 `sui-indexer-alt` 架构

---

## 六、DEX 自研建议

### 6.1 技术栈选择

| 组件 | 推荐方案 | 依赖模块 |
|------|---------|---------|
| **智能合约** | Sui Move | `sui-framework` |
| **交易构建** | Rust SDK | `sui-transaction-builder`, `sui-types` |
| **后端索引器** | sui-indexer-alt 框架 | `sui-indexer-alt-framework`, `sui-pg-db` |
| **数据库** | PostgreSQL | - |
| **RPC 接口** | 自定义 JSON-RPC / GraphQL | `sui-indexer-alt-reader` |
| **前端** | React + TypeScript | `@mysten/sui.js` |
| **实时推送** | WebSocket | - |

---

### 6.2 开发路线图

**阶段 1: 核心理解 (1-2 周)**
- [ ] 深入学习 `sui-types` 核心模块
- [ ] 理解 Object 模型和 Owner 机制
- [ ] 掌握 Programmable Transactions
- [ ] 熟悉事件系统

**阶段 2: 合约开发 (2-4 周)**
- [ ] 设计订单数据结构
- [ ] 实现限价单逻辑
- [ ] 实现撮合引擎
- [ ] 编写单元测试

**阶段 3: 索引器开发 (2-3 周)**
- [ ] 基于 `sui-indexer-alt-framework` 构建索引器
- [ ] 定义数据库 schema
- [ ] 实现事件监听和解析
- [ ] 实现查询接口

**阶段 4: 前端开发 (3-4 周)**
- [ ] 交易构建和签名
- [ ] 订单簿可视化
- [ ] K 线图和深度图
- [ ] 实时更新 (WebSocket)

**阶段 5: 测试和优化 (2-3 周)**
- [ ] 端到端测试
- [ ] 压力测试
- [ ] Gas 优化
- [ ] 性能调优

---

### 6.3 关键文件清单

#### 核心依赖文件

| 文件 | 用途 |
|------|------|
| `crates/sui-types/Cargo.toml` | 核心类型依赖 |
| `crates/sui-types/src/object.rs` | Object 模型定义 |
| `crates/sui-types/src/transaction.rs` | Transaction 结构 |
| `crates/sui-types/src/event.rs` | Event 定义 |
| `crates/sui-types/src/effects/effects_v2.rs` | TransactionEffects |
| `crates/sui-transaction-builder/Cargo.toml` | 交易构建器依赖 |
| `crates/sui-indexer-alt-framework/Cargo.toml` | 索引器框架依赖 |
| `crates/sui-indexer-alt-schema/Cargo.toml` | Schema 定义依赖 |

#### 参考实现文件

| 文件 | 参考内容 |
|------|---------|
| `crates/sui-framework/packages/sui-framework/sources/coin.move` | Coin 标准实现 |
| `crates/sui-framework/packages/sui-framework/sources/balance.move` | Balance 实现 |
| `crates/sui-deepbook-indexer/src/main.rs` | DeepBook 索引器实现(参考) |

---

## 七、总结

DEX 自研开发的核心依赖策略:

1. **基础层**: 深入理解 `sui-types` 的 Object、Transaction、Event、Effects 模块
2. **工具层**: 使用 `sui-transaction-builder` 构建交易
3. **数据层**: 基于 `sui-indexer-alt` 系列构建索引器
4. **避免重型依赖**: 不依赖 `sui-core` 和 `sui-execution`,保持轻量级

**关键设计决策**:
- **对象模型**: Owned vs Shared,影响 TPS 和延迟
- **事件驱动**: 利用事件系统实现实时更新
- **PTB**: 利用 Programmable Transactions 实现原子化操作
- **模块化索引器**: 参考 `sui-indexer-alt` 架构,职责清晰

通过理解这些核心模块的依赖关系和设计原理,可以构建高性能、可扩展的 Sui DEX 系统。
