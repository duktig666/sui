# Token Chain - 架构设计文档

本文档详细描述 Token Chain 的系统架构、核心组件和设计决策。

---

## 📐 架构概览

### 系统层次结构

```
┌─────────────────────────────────────────────────────────┐
│                    Application Layer                     │
│  ┌──────────────┐  ┌────────────┐  ┌─────────────────┐ │
│  │ CLI Client   │  │ HTTP API   │  │  JSON-RPC API   │ │
│  └──────────────┘  └────────────┘  └─────────────────┘ │
└────────────────────────────┬────────────────────────────┘
                             │
┌────────────────────────────┴────────────────────────────┐
│                      RPC Layer                           │
│  ┌────────────────────────────────────────────────────┐ │
│  │  RPC Server (jsonrpsee)                           │ │
│  │  - submitTransaction  - getBalance                │ │
│  │  - getNonce           - getStatus                 │ │
│  └────────────────────────────────────────────────────┘ │
└────────────────────────────┬────────────────────────────┘
                             │
┌────────────────────────────┴────────────────────────────┐
│                     Node Layer                           │
│  ┌────────────────────────────────────────────────────┐ │
│  │  TokenChainNode                                    │ │
│  │  - Transaction submission                          │ │
│  │  - State queries                                   │ │
│  │  - Lifecycle management                            │ │
│  └───────────┬──────────────────────┬─────────────────┘ │
└─────────────┼──────────────────────┼───────────────────┘
              │                      │
      ┌───────┴───────┐      ┌──────┴──────┐
      │               │      │             │
┌─────┴─────┐   ┌─────┴─────┐
│ Execution │   │ Consensus │
│   Layer   │   │   Layer   │
│           │   │           │
│ ┌───────┐ │   │ ┌───────┐ │
│ │ Token │ │   │ │Mysti- │ │
│ │ Exec- │ │   │ │ ceti  │ │
│ │ utor  │ │   │ │Adapter│ │
│ └───┬───┘ │   │ └───────┘ │
│     │     │   │           │
│ ┌───┴───┐ │   └───────────┘
│ │ State │ │
│ │Manager│ │
│ └───────┘ │
└───────────┘
      │
┌─────┴────────────────┐
│   Storage Layer      │
│  ┌────────────────┐  │
│  │ In-Memory      │  │
│  │ HashMap        │  │
│  │ (Future:       │  │
│  │  RocksDB)      │  │
│  └────────────────┘  │
└──────────────────────┘
```

### 核心模块

| 模块 | 职责 | 文件 |
|------|------|------|
| **Types** | 类型定义、序列化 | `types.rs` |
| **Executor** | 交易执行、状态管理 | `executor.rs` |
| **Node** | 节点核心、组件协调 | `node.rs` |
| **RPC** | JSON-RPC API 服务 | `rpc.rs` |
| **Error** | 错误定义、处理 | `error.rs` |
| **Main** | 程序入口、配置 | `main.rs` |

---

## 🏗️ 核心组件详解

### 1. Types Layer - 类型层

#### 职责
- 定义核心数据结构
- 提供序列化/反序列化
- 实现类型转换和验证

#### 关键类型

**Address (地址)**
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Address(pub [u8; 32]);
```

**设计考虑**：
- 32字节固定长度（与以太坊兼容）
- 实现 `Hash` 和 `Eq` 可作为 HashMap key
- 支持从字符串创建（测试便利）

**Transaction (交易)**
```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Transaction {
    Transfer {
        from: Address,
        to: Address,
        amount: u64,
        nonce: u64,
    },
    Mint {
        to: Address,
        amount: u64,
    },
}
```

**设计考虑**：
- 使用 Rust enum 表达不同交易类型
- Transfer 包含 nonce 防止重放攻击
- Mint 简化设计（生产需要权限控制）

**Account (账户)**
```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Account {
    pub balance: u64,
    pub nonce: u64,
}
```

**设计考虑**：
- 最小状态设计
- `Default` trait 简化初始化
- `u64` 足够大（最大 ~18.4 quintillion）

**State (全局状态)**
```rust
pub type State = HashMap<Address, Account>;
```

**设计考虑**：
- O(1) 查找复杂度
- 内存高效
- 易于扩展（未来可切换到持久化存储）

---

### 2. Execution Layer - 执行层

#### TokenExecutor

**核心职责**：
- 执行交易并更新状态
- 验证交易合法性
- 追踪状态变更历史

**架构**：
```rust
pub struct TokenExecutor {
    state: State,                        // 当前状态
    execution_history: Vec<ExecutionResult>,  // 历史记录
}
```

**状态管理**：
```
┌──────────────────────────────────────┐
│         TokenExecutor                │
├──────────────────────────────────────┤
│  state: HashMap<Address, Account>    │
│  ┌────────────┬────────────┐         │
│  │  alice     │  Account   │         │
│  │            │  - balance │         │
│  │            │  - nonce   │         │
│  ├────────────┼────────────┤         │
│  │  bob       │  Account   │         │
│  └────────────┴────────────┘         │
│                                      │
│  execution_history:                  │
│  ┌────────────────────────────────┐ │
│  │  [ExecutionResult]             │ │
│  │  - tx_hash                     │ │
│  │  - success                     │ │
│  │  - state_changes               │ │
│  └────────────────────────────────┘ │
└──────────────────────────────────────┘
```

**执行流程**：

```
                    execute_batch(txs)
                          │
                          ▼
              ┌────────────────────────┐
              │  for each transaction  │
              └──────────┬─────────────┘
                         │
          ┌──────────────┴──────────────┐
          │                             │
    ┌─────▼──────┐              ┌──────▼──────┐
    │  Transfer  │              │    Mint     │
    └─────┬──────┘              └──────┬──────┘
          │                             │
    ┌─────▼────────────────┐     ┌─────▼──────┐
    │ 1. Validate nonce    │     │ 1. Get acct│
    │ 2. Check balance     │     │ 2. Add bal │
    │ 3. Deduct from       │     │ 3. Record  │
    │ 4. Add to            │     └────────────┘
    │ 5. Increment nonce   │
    │ 6. Record changes    │
    └──────────────────────┘
              │
              ▼
     ┌────────────────┐
     │ ExecutionResult│
     │  - tx_hash     │
     │  - success     │
     │  - changes     │
     └────────────────┘
              │
              ▼
        Store in history
```

#### ExecutionEngine Trait 实现

```rust
#[async_trait]
impl ExecutionEngine for TokenExecutor {
    type Transaction = Transaction;
    type State = State;
    type Output = BatchOutput;

    async fn execute_batch(&mut self, txs: Vec<Transaction>)
        -> Result<BatchOutput, ExecutionError>
    {
        // 批量执行逻辑
    }

    async fn validate(&self, tx: &Transaction)
        -> Result<(), ExecutionError>
    {
        // 验证逻辑
    }
}
```

**关键方法**：

1. **execute_transfer** - 转账执行
   ```
   Inputs: from, to, amount, nonce
   Checks:
     - Nonce matches
     - Balance sufficient
   Effects:
     - Deduct from sender
     - Add to receiver
     - Increment nonce
   ```

2. **execute_mint** - 铸币执行
   ```
   Inputs: to, amount
   Checks: (None in current version)
   Effects:
     - Add to recipient
   ```

3. **validate** - 预验证
   ```
   Purpose: Fast-fail before consensus
   Checks:
     - Account exists
     - Nonce correct
     - Balance sufficient
   ```

---

### 3. Consensus Layer - 共识层

#### MysticetiAdapter

**集成方式**：
```rust
pub struct TokenChainNode {
    executor: Arc<Mutex<TokenExecutor>>,
    consensus: Arc<Mutex<MysticetiAdapter<TokenExecutor>>>,
}
```

**交互流程**：
```
    Client
      │
      │ submitTransaction
      ▼
  TokenChainNode
      │
      ├─► Validate (TokenExecutor)
      │       │
      │       ▼
      │   Check nonce
      │   Check balance
      │       │
      ├───────┘
      │
      ├─► Submit (MysticetiAdapter)
      │       │
      │       ▼
      │   Add to consensus
      │   Wait for ordering
      │       │
      ├───────┘
      │
      ├─► Execute (TokenExecutor)
      │       │
      │       ▼
      │   Apply state changes
      │       │
      └───────┘
      │
      ▼
   Return TxHash
```

**共识保证**：
- ✅ 交易顺序一致性
- ✅ 拜占庭容错（BFT）
- ✅ 最终确定性

---

### 4. Node Layer - 节点层

#### TokenChainNode

**组件协调**：
```
┌────────────────────────────────────────┐
│        TokenChainNode                  │
├────────────────────────────────────────┤
│                                        │
│  config: NodeConfig                    │
│  ┌───────────────────────────────┐    │
│  │ node_id: 0                    │    │
│  │ rpc_addr: "127.0.0.1:9000"    │    │
│  │ consensus: {...}              │    │
│  └───────────────────────────────┘    │
│                                        │
│  executor: Arc<Mutex<TokenExecutor>>   │
│  ┌───────────────────────────────┐    │
│  │  Manages state                │    │
│  │  Executes transactions        │    │
│  └───────────────────────────────┘    │
│                                        │
│  consensus: Arc<Mutex<Mysticeti>>      │
│  ┌───────────────────────────────┐    │
│  │  Orders transactions          │    │
│  │  Provides finality            │    │
│  └───────────────────────────────┘    │
│                                        │
│  running: Arc<Mutex<bool>>             │
│                                        │
└────────────────────────────────────────┘
```

**生命周期管理**：
```
    new()
      │
      ▼
  Created
      │
      │ start()
      ▼
  Running ◄──┐
      │      │
      │      │ (serving requests)
      │      │
      │ stop()
      ▼
  Stopped
```

**并发控制**：
- `Arc<Mutex<>>` 保证线程安全
- 异步锁避免阻塞
- 分离的状态管理

---

### 5. RPC Layer - RPC层

#### JSON-RPC Server

**架构**：
```
┌─────────────────────────────────────────┐
│         RPC Server (jsonrpsee)          │
├─────────────────────────────────────────┤
│                                         │
│  HTTP Listener (127.0.0.1:9000)        │
│         │                               │
│         ▼                               │
│  ┌──────────────────────────────┐      │
│  │  Request Router              │      │
│  │  - Parse JSON-RPC request    │      │
│  │  - Route to method handler   │      │
│  │  - Serialize response        │      │
│  └──────────────┬───────────────┘      │
│                 │                       │
│         ┌───────┴───────────────┐      │
│         │                       │      │
│    ┌────▼────┐          ┌──────▼────┐ │
│    │submit   │          │ getBalance│ │
│    │Trans-   │   ...    │ getNonce  │ │
│    │action   │          │ getStatus │ │
│    └────┬────┘          └──────┬────┘ │
│         │                      │       │
│         └──────────┬───────────┘       │
│                    │                   │
│                    ▼                   │
│          ┌──────────────────┐         │
│          │ RpcServerImpl    │         │
│          │  node: Arc<...>  │         │
│          └──────────────────┘         │
│                    │                   │
│                    ▼                   │
│           TokenChainNode               │
│                                         │
└─────────────────────────────────────────┘
```

**Trait 定义**：
```rust
#[rpc(server)]
pub trait TokenChainRpc {
    #[method(name = "submitTransaction")]
    async fn submit_transaction(&self, tx: Transaction)
        -> RpcResult<String>;

    #[method(name = "getBalance")]
    async fn get_balance(&self, address: Address)
        -> RpcResult<u64>;

    // ... more methods
}
```

**错误处理**：
```rust
async fn submit_transaction(&self, tx: Transaction) -> RpcResult<String> {
    self.node
        .submit_transaction(tx)
        .await
        .map(|hash| format!("0x{}", hex::encode(&hash.0)))
        .map_err(|e| ErrorObjectOwned::owned(1, e.to_string(), None::<()>))
}
```

---

## 🔄 数据流分析

### 交易提交流程

```
  Client
    │
    │ HTTP POST
    │ {
    │   "method": "submitTransaction",
    │   "params": [{"Transfer": {...}}]
    │ }
    ▼
┌─────────────────┐
│  RPC Server     │
│  (jsonrpsee)    │
└────────┬────────┘
         │ Parse & Route
         ▼
┌────────────────────┐
│ RpcServerImpl      │
│ submit_transaction │
└────────┬───────────┘
         │
         ▼
┌────────────────────────┐
│  TokenChainNode        │
│  submit_transaction()  │
└────────┬───────────────┘
         │
         ├─► 1. Check if running
         │   ┌─────────────┐
         │   │ running?    │
         │   └──────┬──────┘
         │          │ ✓
         │          ▼
         ├─► 2. Validate (pre-check)
         │   ┌──────────────────┐
         │   │ TokenExecutor    │
         │   │ validate(tx)     │
         │   └──────┬───────────┘
         │          │
         │          ├─► Check nonce
         │          ├─► Check balance
         │          └─► Return Ok/Err
         │          │ ✓
         │          ▼
         ├─► 3. Submit to consensus
         │   ┌──────────────────┐
         │   │ MysticetiAdapter │
         │   │ submit(tx)       │
         │   └──────┬───────────┘
         │          │
         │          ├─► Add to DAG
         │          ├─► Wait for order
         │          └─► Return TxId
         │          │ TxId
         │          ▼
         ├─► 4. Execute locally
         │   ┌──────────────────┐
         │   │ TokenExecutor    │
         │   │ execute_batch()  │
         │   └──────┬───────────┘
         │          │
         │          ├─► Apply state changes
         │          ├─► Update balances
         │          ├─► Increment nonce
         │          └─► Record in history
         │          │ BatchOutput
         │          ▼
         └─► 5. Return TxHash
             ┌──────────────┐
             │ TxHash       │
             │ 0xabcd...    │
             └──────┬───────┘
                    │
                    ▼
             ┌──────────────┐
             │ JSON Response│
             │ {"result":   │
             │  "0xabcd..."} │
             └──────┬───────┘
                    │
                    ▼
                 Client
```

### 状态查询流程

```
  Client
    │
    │ getBalance(address)
    ▼
  RPC Server
    │
    ▼
TokenChainNode
    │
    ▼
TokenExecutor
    │
    ├─► Read state
    │   HashMap.get(address)
    │
    ▼
  Return balance
```

**优势**：
- 无需共识参与
- O(1) 查找复杂度
- 即时响应

---

## 🔐 安全设计

### 1. Nonce 机制

**防止重放攻击**：
```
Transaction Sequence:
  tx1: Transfer(alice→bob, amount=100, nonce=0) ✓
  tx2: Transfer(alice→bob, amount=100, nonce=0) ✗ Rejected!
  tx3: Transfer(alice→bob, amount=100, nonce=1) ✓
```

**实现**：
```rust
if from_account.nonce != nonce {
    return Err("Invalid nonce");
}
// Execute...
from_account.nonce += 1;  // Increment after success
```

### 2. 余额检查

**防止透支**：
```rust
if from_account.balance < amount {
    return Err("Insufficient balance");
}
```

### 3. 原子性保证

**状态更新原子性**：
- 使用 Mutex 保证互斥访问
- 失败时不修改状态
- 成功后一次性提交所有变更

### 4. 输入验证

**地址验证**：
- 固定32字节长度
- 类型安全的 Address 包装

**金额验证**：
- u64 类型防止负数
- 上限检查防止溢出

---

## ⚡ 性能优化

### 1. 并发处理

**异步设计**：
```rust
#[tokio::main]
async fn main() {
    // All I/O operations are async
    node.start().await;
    rpc_server.serve().await;
}
```

**优势**：
- 高并发能力
- 低资源占用
- 非阻塞 I/O

### 2. 状态访问优化

**读写分离**（未来优化）：
```rust
// Current: Arc<Mutex<State>>
// Future: Arc<RwLock<State>>

let state = executor.read().await;  // Multiple readers
let balance = state.get(&address);
```

### 3. 批量处理

**批量执行**：
```rust
async fn execute_batch(&mut self, txs: Vec<Transaction>)
    -> Result<BatchOutput>
{
    // Process all transactions in one batch
    for tx in txs { ... }
}
```

**优势**：
- 减少锁开销
- 提高吞吐量

---

## 🔧 配置管理

### NodeConfig

```rust
pub struct NodeConfig {
    pub node_id: u32,
    pub rpc_addr: String,
    pub consensus: ConsensusNodeConfig,
}
```

**配置来源**：
1. 命令行参数
2. 配置文件 (YAML)
3. 默认值

**加载优先级**：
```
Command Line > Config File > Defaults
```

---

## 📊 监控与可观测性

### 日志级别

```rust
tracing::info!("Node started");
tracing::debug!("Executing transaction: {:?}", tx);
tracing::warn!("Invalid nonce detected");
tracing::error!("Failed to start consensus");
```

### 日志结构

```
2025-12-11T10:00:00.123Z  INFO  simple_token_chain::node: Starting node 0
2025-12-11T10:00:01.456Z  DEBUG simple_token_chain::executor: Executing batch of 5 transactions
2025-12-11T10:00:01.789Z  INFO  simple_token_chain::executor: Transfer: alice -> bob, amount: 300, nonce: 0
```

---

## 🚀 扩展性考虑

### 1. 持久化存储

**当前限制**：
- 内存存储 (HashMap)
- 重启后数据丢失

**未来方案**：
```rust
pub trait StateStore: Send + Sync {
    async fn get(&self, address: &Address) -> Result<Option<Account>>;
    async fn put(&mut self, address: Address, account: Account) -> Result<()>;
    async fn commit(&mut self) -> Result<()>;
}

pub struct RocksDBStore { ... }
impl StateStore for RocksDBStore { ... }
```

### 2. 多节点支持

**当前限制**：
- 单节点运行
- 简化的共识集成

**未来方案**：
- 4+ 节点委员会
- 真实的 Mysticeti 共识
- 网络通信层

### 3. 智能合约

**未来扩展**：
```rust
pub enum Transaction {
    Transfer { ... },
    Mint { ... },
    ContractCall {
        contract: Address,
        method: String,
        args: Vec<u8>,
        nonce: u64,
    },
}
```

---

## 🎯 设计原则总结

1. **模块化**: 清晰的职责分离
2. **可测试**: 所有组件可独立测试
3. **类型安全**: 利用 Rust 类型系统
4. **异步优先**: 高性能 I/O
5. **错误处理**: 完整的错误传播
6. **可扩展**: 易于添加新功能

---

## 📚 相关文档

- **快速开始**: [getting-started.md](getting-started.md)
- **API 参考**: [api-reference.md](api-reference.md)
- **研究总结**: [research-summary.md](research-summary.md)

---

**文档版本**: 1.0
**最后更新**: 2025-12-11
