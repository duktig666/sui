# DEX AppChain 架构设计文档

**版本**: v1.0
**日期**: 2025-12-16
**作者**: Architecture Team
**状态**: Design Review

---

## 📋 目录

1. [项目概述](#1-项目概述)
2. [设计目标与原则](#2-设计目标与原则)
3. [整体架构](#3-整体架构)
4. [核心组件设计](#4-核心组件设计)
5. [数据流与交互](#5-数据流与交互)
6. [性能优化设计](#6-性能优化设计)
7. [接口设计](#7-接口设计)
8. [部署架构](#8-部署架构)
9. [关键技术决策](#9-关键技术决策)
10. [风险与挑战](#10-风险与挑战)

---

## 1. 项目概述

### 1.1 项目定位

DEX AppChain 是一个基于 Sui Mysticeti 共识的**最小化订单簿现货交易所原型**，主要目标是：
- ✅ 验证 Mysticeti 共识机制的实际性能
- ✅ 测试订单簿模型在 BFT 共识下的表现
- ✅ 为高频交易场景提供性能基准

### 1.2 非目标

本项目**不包括**：
- ❌ AMM 流动性池
- ❌ 复杂的订单类型（止损单、冰山单等）
- ❌ 跨链桥接
- ❌ 智能合约支持
- ❌ 生产级安全特性（本阶段）

### 1.3 核心功能

| 功能 | 说明 | 优先级 |
|-----|------|--------|
| 存款/提款 | 资产充值和提现 | P0 |
| 限价单 | 指定价格下单 | P0 |
| 市价单 | 即时成交 | P0 |
| 撤单 | 取消未成交订单 | P0 |
| 订单簿查询 | 深度数据 | P1 |
| 成交历史 | 最近成交记录 | P1 |

---

## 2. 设计目标与原则

### 2.1 性能目标

| 指标 | 目标值 | 说明 |
|-----|--------|-----|
| 撮合引擎 TPS | > 100,000 | 纯内存撮合性能 |
| 端到端 TPS | > 1,000 | 包含共识的完整流程 |
| 共识延迟 P50 | < 450ms | Mysticeti 理论延迟 |
| 共识延迟 P99 | < 600ms | 长尾延迟控制 |
| 订单簿查询 | < 100μs | 20档深度 |
| 内存占用 | < 200MB | 10万活跃订单 |

### 2.2 设计原则

1. **简单性优先**: 最小化功能，聚焦核心
2. **性能可测**: 每层都有独立的性能测试
3. **确定性执行**: 所有节点执行结果必须一致
4. **可观测性**: 完整的性能监控和日志
5. **模块化**: 清晰的层次划分，便于测试和优化

---

## 3. 整体架构

### 3.1 四层架构

```
┌─────────────────────────────────────────────────────────────┐
│                        API Layer                             │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
│  │  JSON-RPC    │  │  Metrics     │  │  Admin API   │      │
│  └──────────────┘  └──────────────┘  └──────────────┘      │
└─────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────┐
│                      Execution Layer                         │
│  ┌──────────────────────────────────────────────────────┐   │
│  │              DexExecutor                              │   │
│  │  (ExecutionEngine Trait Implementation)              │   │
│  │                                                       │   │
│  │  ┌─────────────┐  ┌──────────────┐  ┌────────────┐ │   │
│  │  │  Balance    │  │   Matching   │  │   Order    │ │   │
│  │  │  Manager    │  │    Engine    │  │   Manager  │ │   │
│  │  └─────────────┘  └──────────────┘  └────────────┘ │   │
│  └──────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────┐
│                     Consensus Layer                          │
│  ┌──────────────────────────────────────────────────────┐   │
│  │         consensus-framework                           │   │
│  │  ┌────────────────┐  ┌──────────────────────────┐   │   │
│  │  │ Mysticeti      │  │  ConsensusProtocol       │   │   │
│  │  │ Adapter        │  │  Trait                   │   │   │
│  │  └────────────────┘  └──────────────────────────┘   │   │
│  └──────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────┐
│                      Storage Layer                           │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
│  │   In-Memory  │  │  RocksDB     │  │  State       │      │
│  │   State      │  │  (Optional)  │  │  Snapshots   │      │
│  └──────────────┘  └──────────────┘  └──────────────┘      │
└─────────────────────────────────────────────────────────────┘
```

### 3.2 层次职责

#### 3.2.1 API Layer（接口层）
**职责**:
- 接收外部请求（JSON-RPC）
- 参数验证和序列化
- 返回查询结果
- 性能指标暴露

**不负责**:
- ❌ 业务逻辑处理
- ❌ 状态管理
- ❌ 共识参与

#### 3.2.2 Execution Layer（执行层）
**职责**:
- 交易验证（余额、签名、nonce）
- 订单撮合逻辑
- 状态转换执行
- 确定性执行保证

**关键特性**:
- ✅ 纯函数式状态转换
- ✅ 无外部依赖
- ✅ 可重放执行

#### 3.2.3 Consensus Layer（共识层）
**职责**:
- 交易排序（全局顺序）
- 拜占庭容错
- 最终性保证
- 节点间同步

**集成方式**:
- 通过 `consensus-framework` crate
- 实现 `ExecutionEngine` trait
- 复用 Mysticeti 核心

#### 3.2.4 Storage Layer（存储层）
**职责**:
- 状态持久化（可选）
- 快照管理
- 历史数据查询

**实现方式**:
- 主要：内存存储（HashMap）
- 可选：RocksDB 持久化

---

## 4. 核心组件设计

### 4.1 DexExecutor（执行器）

```rust
pub struct DexExecutor {
    /// 共享状态（需要线程安全）
    state: Arc<RwLock<DexState>>,

    /// 配置参数
    config: DexConfig,
}

/// DEX 状态（所有可变状态）
pub struct DexState {
    /// 用户余额: User -> Asset -> Balance
    balances: HashMap<Address, HashMap<AssetId, u64>>,

    /// 撮合引擎（核心）
    matching_engine: MatchingEngine,

    /// 交易历史（有限保留）
    recent_trades: VecDeque<Fill>,

    /// 全局统计
    stats: GlobalStats,
}
```

**设计要点**:
1. **状态不可变性**: 使用 RwLock 保护，读多写少
2. **批量执行**: `execute_batch` 一次性处理多笔交易
3. **原子性**: 要么全部成功，要么全部回滚

### 4.2 MatchingEngine（撮合引擎）

```rust
pub struct MatchingEngine {
    /// 所有交易对的订单簿
    orderbooks: HashMap<TradingPair, OrderBook>,

    /// 全局订单 ID 生成器
    next_order_id: u128,

    /// 订单索引: OrderId -> (Pair, Order)
    order_index: HashMap<OrderId, (TradingPair, Order)>,
}

pub struct OrderBook {
    pair: TradingPair,

    /// 买单（价格从高到低）
    bids: BTreeMap<u64, PriceLevel>,

    /// 卖单（价格从低到高）
    asks: BTreeMap<u64, PriceLevel>,

    /// 最新成交价
    last_price: Option<u64>,
}
```

**关键算法**:

1. **价格-时间优先**:
   ```
   优先级 = (价格优先级, 时间戳)
   买单: 价格越高越优先
   卖单: 价格越低越优先
   同价格: 时间越早越优先
   ```

2. **撮合流程**:
   ```
   新订单 → 选择对手盘 → 价格匹配检查 →
   数量匹配 → 生成成交记录 → 更新订单状态 →
   更新余额 → 返回结果
   ```

3. **性能优化**:
   - BTreeMap: O(log n) 插入和查找
   - 价格级别合并：同价格订单合并为一个 PriceLevel
   - 批量删除：成交后批量清理完成订单

### 4.3 OrderBook（订单簿）

**数据结构设计**:

```rust
pub struct PriceLevel {
    price: u64,
    orders: Vec<Order>,         // 该价格的所有订单
    total_quantity: u64,        // 总量（加速查询）
}
```

**为什么用 BTreeMap？**
- ✅ 自动排序（买单降序，卖单升序）
- ✅ O(log n) 插入和删除
- ✅ 范围查询高效（获取深度数据）
- ✅ 迭代器性能好

**内存布局优化**:
```
价格级别数: ~1000 (每个交易对)
每个级别平均订单数: ~10
单个订单大小: ~200 bytes
总内存: 1000 * 10 * 200 = 2MB (单个交易对)
```

### 4.4 Balance Manager（余额管理）

```rust
pub struct BalanceManager {
    /// 可用余额
    balances: HashMap<Address, HashMap<AssetId, u64>>,

    /// 冻结余额（挂单占用）
    frozen: HashMap<Address, HashMap<AssetId, u64>>,
}

impl BalanceManager {
    /// 冻结资金（下单时）
    pub fn freeze(&mut self, user: Address, asset: AssetId, amount: u64)
        -> Result<(), BalanceError>;

    /// 解冻资金（撤单时）
    pub fn unfreeze(&mut self, user: Address, asset: AssetId, amount: u64)
        -> Result<(), BalanceError>;

    /// 转移余额（成交时）
    pub fn transfer(&mut self, from: Address, to: Address, asset: AssetId, amount: u64)
        -> Result<(), BalanceError>;
}
```

**设计要点**:
1. **双重记账**: 可用余额 + 冻结余额
2. **原子操作**: 冻结、解冻、转移必须原子完成
3. **溢出检查**: 使用 `checked_add/sub` 防止溢出

---

## 5. 数据流与交互

### 5.1 完整交易流程

#### 场景 1: 限价单完全成交

```
客户端                API层              执行层              共识层              存储层
  │                    │                  │                   │                   │
  │  PlaceOrder(Buy)   │                  │                   │                   │
  ├───────────────────>│                  │                   │                   │
  │                    │  Submit Tx       │                   │                   │
  │                    ├─────────────────>│                   │                   │
  │                    │                  │  Consensus        │                   │
  │                    │                  ├──────────────────>│                   │
  │                    │                  │                   │  Order Tx         │
  │                    │                  │                   │  (Total Order)    │
  │                    │                  │                   │                   │
  │                    │                  │  Execute Batch    │                   │
  │                    │                  │<──────────────────┤                   │
  │                    │                  │                   │                   │
  │                    │  1. Check Balance│                   │                   │
  │                    │  2. Freeze Funds │                   │                   │
  │                    │  3. Match Order  │                   │                   │
  │                    │  4. Generate Fill│                   │                   │
  │                    │  5. Update Balance                   │                   │
  │                    │  6. Update State │                   │                   │
  │                    │                  ├──────────────────────────────────────>│
  │                    │                  │                   │    Update State   │
  │                    │                  │                   │                   │
  │  OrderId + Fills   │                  │                   │                   │
  │<───────────────────┤                  │                   │                   │
  │                    │                  │                   │                   │
```

**时间分解**:
- API 处理: ~1ms
- 共识延迟: ~400ms (P50)
- 执行时间: ~10μs
- **总延迟: ~401ms**

#### 场景 2: 市价单即时成交

```
1. 客户端提交市价单
2. API 层验证参数
3. 提交到共识层排队
4. 共识层确定全局顺序
5. 执行层处理:
   - 验证余额
   - 遍历对手盘订单簿
   - 匹配直到满足数量或无流动性
   - 生成成交记录
   - 更新双方余额
6. 返回成交结果
```

#### 场景 3: 撤单流程

```
1. 客户端提交撤单请求
2. 共识层排序
3. 执行层处理:
   - 查找订单
   - 检查订单所有者
   - 从订单簿移除
   - 解冻资金
   - 更新状态
4. 返回成功
```

### 5.2 批量处理优化

**批量执行示意**:
```rust
// 共识层提交 N 笔交易
let txs = vec![
    DexTransaction::PlaceOrder { ... },  // 100 笔
    DexTransaction::CancelOrder { ... }, // 50 笔
    DexTransaction::PlaceOrder { ... },  // 100 笔
];

// 执行层批量处理
let result = executor.execute_batch(txs).await?;

// 一次性提交状态
state.commit()?;
```

**优化效果**:
- 单笔处理: 1000 TPS → 需要 1000 次共识
- 批量处理: 1000 笔/批 → 只需 1 次共识
- **吞吐量提升: 100-1000x**

### 5.3 状态一致性保证

**ACID 保证**:
1. **原子性**: 批量交易要么全部成功，要么全部失败
2. **一致性**: 余额守恒，订单状态一致
3. **隔离性**: RwLock 保证并发隔离
4. **持久性**: （可选）RocksDB 持久化

**确定性执行**:
```rust
// 相同输入必须产生相同输出
fn execute(state: State, txs: Vec<Tx>) -> (State, Result) {
    // 纯函数，无随机数，无系统调用
    // 所有节点执行结果必须一致
}
```

---

## 6. 性能优化设计

### 6.1 撮合引擎优化

#### 6.1.1 数据结构选择

| 需求 | 方案 | 时间复杂度 | 原因 |
|-----|------|-----------|------|
| 价格排序 | BTreeMap | O(log n) | 自动排序 |
| 订单查找 | HashMap | O(1) | 快速定位 |
| 深度查询 | BTreeMap迭代 | O(k) | k=深度档数 |
| 订单插入 | Vec | O(1) 摊销 | 同价格少 |

#### 6.1.2 内存布局优化

```rust
// 避免：每个订单独立分配
struct Order { ... }  // 每次 malloc

// 优化：使用 Arena 分配器
struct OrderArena {
    orders: Vec<Order>,  // 预分配
    free_list: Vec<usize>,
}
```

#### 6.1.3 缓存友好设计

```rust
// 热路径数据紧凑排列
#[repr(C)]
struct PriceLevel {
    price: u64,           // 8 bytes
    total_quantity: u64,  // 8 bytes
    orders: Vec<Order>,   // 24 bytes
    // 总共 40 bytes，适合 cache line
}
```

### 6.2 并发控制优化

#### 6.2.1 读写锁分离

```rust
pub struct DexState {
    // 读多写少：用 RwLock
    balances: Arc<RwLock<BalanceState>>,

    // 写密集：用 Mutex
    matching_engine: Arc<Mutex<MatchingEngine>>,
}

// 查询余额（多个并发读）
let balance = state.balances.read().await;

// 执行交易（独占写）
let mut engine = state.matching_engine.lock().await;
```

#### 6.2.2 批量锁获取

```rust
// 避免：多次获取锁
for tx in txs {
    let mut state = self.state.lock().await;
    execute_one(tx);
}  // 锁竞争严重

// 优化：批量执行
let mut state = self.state.lock().await;
for tx in txs {
    execute_one(tx);
}  // 只获取一次锁
```

### 6.3 共识层优化

#### 6.3.1 批量提交

```rust
// 客户端批量提交
let txs = vec![tx1, tx2, tx3, ...];
consensus.submit_batch(txs).await?;

// 共识层批量排序
// Mysticeti 内部会将多笔交易打包到一个 block
```

#### 6.3.2 Pipeline 并行

```
┌────────┐    ┌────────┐    ┌────────┐
│Block 1 │───>│Block 2 │───>│Block 3 │
└────────┘    └────────┘    └────────┘
    │             │             │
    ↓             ↓             ↓
 Execute      Execute      Execute
  (并行)       (并行)       (并行)
```

### 6.4 内存优化

#### 6.4.1 历史数据淘汰

```rust
pub struct RecentTrades {
    trades: VecDeque<Fill>,
    max_size: usize,  // 默认 10000
}

impl RecentTrades {
    pub fn push(&mut self, fill: Fill) {
        if self.trades.len() >= self.max_size {
            self.trades.pop_front();  // FIFO 淘汰
        }
        self.trades.push_back(fill);
    }
}
```

#### 6.4.2 订单簿压缩

```rust
// 合并小订单
if order.quantity < MIN_QUANTITY {
    // 合并到价格级别
    level.total_quantity += order.quantity;
    // 不单独存储
}
```

---

## 7. 接口设计

### 7.1 RPC API 定义

```rust
#[rpc(server)]
pub trait DexRpc {
    // ========== 交易接口 ==========

    /// 存款
    #[method(name = "deposit")]
    async fn deposit(
        &self,
        user: Address,
        asset: AssetId,
        amount: u64,
    ) -> RpcResult<TxHash>;

    /// 提款
    #[method(name = "withdraw")]
    async fn withdraw(
        &self,
        user: Address,
        asset: AssetId,
        amount: u64,
    ) -> RpcResult<TxHash>;

    /// 下单（限价单/市价单）
    #[method(name = "placeOrder")]
    async fn place_order(
        &self,
        trader: Address,
        pair: TradingPair,
        side: OrderSide,
        order_type: OrderType,
        price: u64,      // 市价单填 0
        quantity: u64,
    ) -> RpcResult<OrderId>;

    /// 撤单
    #[method(name = "cancelOrder")]
    async fn cancel_order(
        &self,
        trader: Address,
        order_id: OrderId,
    ) -> RpcResult<bool>;

    // ========== 查询接口 ==========

    /// 查询余额
    #[method(name = "getBalance")]
    async fn get_balance(
        &self,
        user: Address,
        asset: AssetId,
    ) -> RpcResult<BalanceInfo>;

    /// 查询订单簿
    #[method(name = "getOrderBook")]
    async fn get_orderbook(
        &self,
        pair: TradingPair,
        depth: usize,  // 深度档数，如 20
    ) -> RpcResult<OrderBookSnapshot>;

    /// 查询订单状态
    #[method(name = "getOrder")]
    async fn get_order(
        &self,
        order_id: OrderId,
    ) -> RpcResult<Order>;

    /// 查询最近成交
    #[method(name = "getRecentTrades")]
    async fn get_recent_trades(
        &self,
        pair: TradingPair,
        limit: usize,
    ) -> RpcResult<Vec<Fill>>;

    // ========== 系统接口 ==========

    /// 获取节点状态
    #[method(name = "getStatus")]
    async fn get_status(&self) -> RpcResult<NodeStatus>;

    /// 获取性能指标
    #[method(name = "getMetrics")]
    async fn get_metrics(&self) -> RpcResult<Metrics>;
}
```

### 7.2 数据结构定义

```rust
/// 余额信息
#[derive(Debug, Serialize, Deserialize)]
pub struct BalanceInfo {
    pub available: u64,  // 可用余额
    pub frozen: u64,     // 冻结余额（挂单占用）
    pub total: u64,      // 总余额
}

/// 订单簿快照
#[derive(Debug, Serialize, Deserialize)]
pub struct OrderBookSnapshot {
    pub pair: TradingPair,
    pub bids: Vec<(u64, u64)>,  // [(price, quantity), ...]
    pub asks: Vec<(u64, u64)>,
    pub last_price: Option<u64>,
    pub timestamp: u64,
}

/// 成交记录
#[derive(Debug, Serialize, Deserialize)]
pub struct Fill {
    pub order_id: OrderId,
    pub trader: Address,
    pub pair: TradingPair,
    pub side: OrderSide,
    pub price: u64,
    pub quantity: u64,
    pub timestamp: u64,
}

/// 节点状态
#[derive(Debug, Serialize, Deserialize)]
pub struct NodeStatus {
    pub node_id: u32,
    pub running: bool,
    pub commit_index: u64,
    pub pending_txs: usize,
    pub uptime: u64,
}

/// 性能指标
#[derive(Debug, Serialize, Deserialize)]
pub struct Metrics {
    pub total_orders: u64,
    pub total_fills: u64,
    pub avg_matching_time_us: u64,
    pub orderbook_depth: HashMap<TradingPair, (usize, usize)>,
}
```

---

## 8. 部署架构

### 8.1 单节点部署

```
┌─────────────────────────────────────┐
│         DEX AppChain Node           │
│                                     │
│  ┌─────────────────────────────┐   │
│  │      RPC Server             │   │
│  │   (Port 9000)               │   │
│  └─────────────────────────────┘   │
│              ↓                      │
│  ┌─────────────────────────────┐   │
│  │      DexExecutor            │   │
│  └─────────────────────────────┘   │
│              ↓                      │
│  ┌─────────────────────────────┐   │
│  │   Consensus Framework       │   │
│  └─────────────────────────────┘   │
│              ↓                      │
│  ┌─────────────────────────────┐   │
│  │      Memory State           │   │
│  └─────────────────────────────┘   │
└─────────────────────────────────────┘
```

### 8.2 4节点测试网

```
┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐
│  Node 0  │  │  Node 1  │  │  Node 2  │  │  Node 3  │
│  :9000   │  │  :9001   │  │  :9002   │  │  :9003   │
└──────────┘  └──────────┘  └──────────┘  └──────────┘
      │              │              │              │
      └──────────────┴──────────────┴──────────────┘
                  Mysticeti Consensus
              (Byzantine Fault Tolerant)
```

**配置示例**:
```yaml
# node0.yaml
node_id: 0
rpc_port: 9000
consensus:
  committee_size: 4
  authority_index: 0
  authorities:
    - id: 0
      address: "127.0.0.1:10000"
    - id: 1
      address: "127.0.0.1:10001"
    - id: 2
      address: "127.0.0.1:10002"
    - id: 3
      address: "127.0.0.1:10003"
```

---

## 9. 关键技术决策

### 9.1 为什么选择内存存储？

**决策**: 初版使用纯内存存储，不做持久化

**理由**:
1. ✅ **简化实现**: 减少 RocksDB 集成复杂度
2. ✅ **性能最优**: 无 I/O 瓶颈
3. ✅ **聚焦目标**: 本项目重点是性能测试，非生产部署
4. ✅ **快速迭代**: 便于开发和调试

**风险**:
- ❌ 节点重启丢失状态
- ❌ 无法恢复历史数据

**缓解措施**:
- 可选支持 RocksDB（后期添加）
- 定期导出快照

### 9.2 为什么不支持智能合约？

**决策**: DEX 逻辑直接用 Rust 实现，不使用 Move/EVM

**理由**:
1. ✅ **性能**: Rust 原生代码比 VM 快 10-100x
2. ✅ **确定性**: 编译时保证，无运行时风险
3. ✅ **简单**: 无需集成 Move VM
4. ✅ **聚焦**: 本项目是性能测试，非通用平台

**对比**:
| 方案 | TPS | 延迟 | 灵活性 |
|-----|-----|-----|--------|
| Rust 原生 | 100K+ | 10μs | 低 |
| Move VM | ~10K | 100μs | 高 |
| EVM | ~1K | 1ms | 最高 |

### 9.3 为什么使用 BTreeMap？

**决策**: 订单簿用 `BTreeMap<Price, PriceLevel>`

**理由**:
1. ✅ **自动排序**: 买单降序，卖单升序
2. ✅ **范围查询**: 获取深度数据高效
3. ✅ **性能平衡**: O(log n) 可接受

**对比其他方案**:
| 方案 | 插入 | 查询 | 排序 | 评价 |
|-----|-----|-----|-----|-----|
| HashMap | O(1) | O(1) | 需手动 | ❌ 无法高效排序 |
| BTreeMap | O(log n) | O(log n) | 自动 | ✅ 平衡 |
| Vec | O(n) | O(n) | O(n log n) | ❌ 太慢 |
| SkipList | O(log n) | O(log n) | 自动 | ⚠️ 复杂度高 |

### 9.4 批量执行 vs 单笔执行

**决策**: 支持批量执行，单笔作为特例

**理由**:
1. ✅ **共识优化**: 减少共识轮次
2. ✅ **吞吐量**: 批量处理提升 100x+
3. ✅ **锁优化**: 减少锁竞争

**实现**:
```rust
// 单笔执行（退化为批量大小=1）
pub async fn execute(&mut self, tx: Tx) -> Result<Output> {
    self.execute_batch(vec![tx]).await
}

// 批量执行（核心实现）
pub async fn execute_batch(&mut self, txs: Vec<Tx>) -> Result<Output> {
    let mut state = self.state.lock().await;
    for tx in txs {
        // 处理每笔交易
    }
    Ok(output)
}
```

---

## 10. 风险与挑战

### 10.1 技术风险

| 风险 | 影响 | 可能性 | 缓解措施 |
|-----|-----|--------|---------|
| 共识延迟过高 | 端到端 TPS 达不到目标 | 中 | 批量提交优化 |
| 撮合引擎瓶颈 | 无法达到 100K TPS | 低 | BTreeMap 足够快 |
| 内存溢出 | 大量订单导致 OOM | 中 | 限制订单数量 |
| 状态不一致 | 节点间状态分叉 | 低 | 确定性执行测试 |

### 10.2 性能挑战

**挑战 1: 达到 1000 TPS**
- **瓶颈**: 共识延迟 ~400ms
- **理论上限**: 1 / 0.4s ≈ 2.5 TPS (单笔)
- **解决方案**: 批量提交（1批=1000笔）→ 1000 / 0.4s = 2500 TPS

**挑战 2: 撮合引擎 100K TPS**
- **目标**: 10μs/订单
- **瓶颈**: BTreeMap 插入
- **解决方案**:
  - 使用 Arena 分配器
  - 价格级别合并
  - 批量删除

**挑战 3: 内存占用 < 200MB**
- **问题**: 10万订单 × 200 bytes = 20MB
- **加上**: 订单簿索引、价格级别 ≈ 100MB
- **总计**: ~120MB ✅ 满足目标

### 10.3 开发挑战

| 挑战 | 难度 | 工作量 | 优先级 |
|-----|-----|--------|--------|
| 撮合引擎正确性 | 高 | 2-3天 | P0 |
| 共识集成 | 中 | 1-2天 | P0 |
| 性能测试框架 | 中 | 2天 | P0 |
| RPC API 实现 | 低 | 1天 | P1 |

---

## 附录

### A. 术语表

| 术语 | 英文 | 说明 |
|-----|-----|-----|
| 订单簿 | Order Book | 存储所有未成交订单的数据结构 |
| 价格级别 | Price Level | 同一价格的所有订单 |
| 撮合 | Matching | 买卖订单配对成交的过程 |
| 限价单 | Limit Order | 指定价格的订单 |
| 市价单 | Market Order | 按市场最优价立即成交 |
| 深度 | Depth | 订单簿的买卖挂单分布 |
| 成交 | Fill | 订单匹配成功的结果 |
| Taker | Taker | 主动成交方（发起新订单） |
| Maker | Maker | 被动成交方（已挂单） |

### B. 参考资料

1. **Mysticeti 论文**: https://arxiv.org/pdf/2310.14821
2. **Sui 共识源码**: `consensus/core/src/`
3. **BTreeMap 文档**: https://doc.rust-lang.org/std/collections/struct.BTreeMap.html
4. **订单簿算法**: CLOB (Central Limit Order Book)

### C. 性能基准参考

| 系统 | 类型 | TPS | 延迟 |
|-----|-----|-----|-----|
| Binance | CEX | ~100K | <10ms |
| Coinbase | CEX | ~10K | ~50ms |
| dYdX v4 | DEX (Cosmos) | ~2K | ~1s |
| Serum | DEX (Solana) | ~1K | ~400ms |
| **DEX AppChain** | **目标** | **>1K** | **~400ms** |

---

**文档版本**: v1.0
**最后更新**: 2025-12-16
**下次审查**: 开发完成后
