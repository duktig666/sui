# DEX 基于 Sui 架构设计文档

> **版本**: v1.0  
> **日期**: 2025-01-XX  
> **状态**: Draft  
> **设计方法**: Architect-driven  
> **参考**: [`mynotes/plan/dex_use_sui_plan_cursor.md`](../../../plan/dex_use_sui_plan_cursor.md)

---

## 📋 目录

1. [架构概述](#1-架构概述)
2. [系统分层架构](#2-系统分层架构)
3. [核心组件设计](#3-核心组件设计)
4. [数据流设计](#4-数据流设计)
5. [接口设计](#5-接口设计)
6. [部署架构](#6-部署架构)
7. [扩展性设计](#7-扩展性设计)
8. [参考架构对比](#8-参考架构对比)

---

## 1. 架构概述

### 1.1 设计目标

基于 Sui 区块链构建高性能 DEX，第一阶段采用中心化 Sequencer 架构：

- **性能目标**: 撮合延迟 < 50ms (P99)，吞吐量 10万+ TPS
- **兼容性**: 完全兼容 Sui 生态和 Move 智能合约
- **可靠性**: 热备份 Sequencer，故障切换 < 100ms
- **可扩展**: 为 Phase 2 多节点轮换预留接口

### 1.2 架构原则

1. **分层解耦**: 清晰的层次边界，降低耦合度
2. **复用优先**: 最大化复用 Sui 基础设施（约 40%）
3. **性能优先**: 关键路径使用 Rust 原生实现
4. **渐进演进**: Phase 1 → Phase 2 → Phase 3 平滑过渡

### 1.3 参考架构

- **dYdX v4 (Cosmos)**: 链下撮合、链上结算的混合模型
- **Sui 架构**: 对象中心模型、FastPath、并行执行
- **设计文档**: [`mynotes/dex/arch/DYDX_Cosmos_DEX架构.docx`](DYDX_Cosmos_DEX架构.docx)

---

## 2. 系统分层架构

### 2.1 整体分层架构

```
┌─────────────────────────────────────────────────────────────────────┐
│                        DEX L1 系统架构                               │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  ┌───────────────────────────────────────────────────────────────┐ │
│  │  Layer 1: 客户端层 (Client Layer)                             │ │
│  │  • Sui SDK / Wallets                                          │ │
│  │  • JSON-RPC API (扩展)                                        │ │
│  │  • WebSocket (实时推送)                                        │ │
│  └───────────────────────────────────────────────────────────────┘ │
│                              │                                       │
│                              ▼                                       │
│  ┌───────────────────────────────────────────────────────────────┐ │
│  │  Layer 2: API 网关层 (API Gateway Layer)                      │ │
│  │  • 交易路由 (DEX vs Standard)                                  │ │
│  │  • 请求验证和限流                                              │ │
│  │  • 负载均衡                                                    │ │
│  └───────────────────────────────────────────────────────────────┘ │
│                              │                                       │
│                    ┌─────────┴─────────┐                            │
│                    │                   │                            │
│                    ▼                   ▼                            │
│  ┌──────────────────────┐  ┌──────────────────────┐              │
│  │  Layer 3a: DEX 路径   │  │  Layer 3b: Sui 路径   │              │
│  │  (Fast Path)         │  │  (Standard Path)      │              │
│  │                      │  │                      │              │
│  │  ┌────────────────┐ │  │  ┌────────────────┐ │              │
│  │  │ Sequencer      │ │  │  │ Mysticeti       │ │              │
│  │  │ (中心化)       │ │  │  │ Consensus       │ │              │
│  │  └────────────────┘ │  │  └────────────────┘ │              │
│  │         │           │  │         │           │              │
│  │         ▼           │  │         ▼           │              │
│  │  ┌────────────────┐ │  │  ┌────────────────┐ │              │
│  │  │ Matching       │ │  │  │ Move VM        │ │              │
│  │  │ Engine         │ │  │  │ Execution      │ │              │
│  │  │ (Native Rust)  │ │  │  │                │ │              │
│  │  └────────────────┘ │  │  └────────────────┘ │              │
│  └──────────────────────┘  └──────────────────────┘              │
│                    │                   │                            │
│                    └─────────┬─────────┘                            │
│                              ▼                                       │
│  ┌───────────────────────────────────────────────────────────────┐ │
│  │  Layer 4: 执行层 (Execution Layer)                              │ │
│  │  • DEX Precompile (钩子)                                       │ │
│  │  • Move VM (存取款、非 DEX 交易)                                │ │
│  │  • 两阶段执行 (原子性保证)                                      │ │
│  └───────────────────────────────────────────────────────────────┘ │
│                              │                                       │
│                              ▼                                       │
│  ┌───────────────────────────────────────────────────────────────┐ │
│  │  Layer 5: 状态管理层 (State Management Layer)                  │ │
│  │  • 内存状态 (Orderbook, Balances)                              │ │
│  │  • 状态缓存 (DashMap, ShardedLruCache)                         │ │
│  │  • 持久化存储 (RocksDB via typed-store)                        │ │
│  └───────────────────────────────────────────────────────────────┘ │
│                              │                                       │
│                              ▼                                       │
│  ┌───────────────────────────────────────────────────────────────┐ │
│  │  Layer 6: 基础设施层 (Infrastructure Layer)                    │ │
│  │  • Sui Network (Anemo P2P)                                    │ │
│  │  • Sui Storage (typed-store, RocksDB)                         │ │
│  │  • Sui Types (Transaction, Object, etc.)                      │ │
│  └───────────────────────────────────────────────────────────────┘ │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### 2.2 层次职责

#### Layer 1: 客户端层
- **职责**: 用户交互、交易构建、签名
- **组件**: Sui SDK、钱包、Web 前端
- **不负责**: 业务逻辑、状态管理

#### Layer 2: API 网关层
- **职责**: 请求路由、验证、限流、负载均衡
- **组件**: JSON-RPC 服务器、WebSocket 服务器
- **关键**: 区分 DEX 交易和标准 Sui 交易

#### Layer 3: 交易处理层
- **职责**: 交易排序和执行
- **组件**: 
  - **3a (DEX 路径)**: Sequencer + Matching Engine
  - **3b (Sui 路径)**: Mysticeti Consensus + Move VM

#### Layer 4: 执行层
- **职责**: 交易执行、状态转换
- **组件**: DEX Precompile、Move VM、两阶段执行器

#### Layer 5: 状态管理层
- **职责**: 状态存储、缓存、持久化
- **组件**: 内存订单簿、余额缓存、RocksDB

#### Layer 6: 基础设施层
- **职责**: 网络、存储、类型系统
- **组件**: Sui 网络层、存储层、类型系统（复用）

---

## 3. 核心组件设计

### 3.1 Sequencer (定序器)

#### 3.1.1 组件职责

- **交易排序**: FIFO 排序，分配全局序列号
- **批次聚合**: 基于时间或大小的批次聚合
- **序列广播**: 通过 Anemo 网络广播序列
- **确认收集**: 收集 2f+1 验证者确认

#### 3.1.2 架构设计

```
┌─────────────────────────────────────────────────────────────┐
│                    Sequencer 组件架构                        │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌────────────────────────────────────────────────────────┐ │
│  │  Order Gateway                                         │ │
│  │  • 接收订单请求                                        │ │
│  │  • 参数验证                                            │ │
│  │  • 限流控制                                            │ │
│  └────────────────────────────────────────────────────────┘ │
│                         │                                    │
│                         ▼                                    │
│  ┌────────────────────────────────────────────────────────┐ │
│  │  Transaction Sequencer                                  │ │
│  │  • FIFO 队列管理                                       │ │
│  │  • 序列号分配 (AtomicU64)                              │ │
│  │  • 批次聚合 (时间/大小)                                 │ │
│  └────────────────────────────────────────────────────────┘ │
│                         │                                    │
│                         ▼                                    │
│  ┌────────────────────────────────────────────────────────┐ │
│  │  Sequence Publisher                                     │ │
│  │  • 签名序列批次                                         │ │
│  │  • Anemo 广播                                          │ │
│  │  • 确认收集器                                           │ │
│  └────────────────────────────────────────────────────────┘ │
│                         │                                    │
│                         ▼                                    │
│  ┌────────────────────────────────────────────────────────┐ │
│  │  DA Layer (可选)                                       │ │
│  │  • 序列持久化                                           │ │
│  │  • 故障恢复支持                                         │ │
│  └────────────────────────────────────────────────────────┘ │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

#### 3.1.3 数据结构

```rust
pub struct DexSequencer {
    // 序列号管理
    sequence_counter: AtomicU64,
    last_confirmed_sequence: Arc<RwLock<u64>>,
    
    // 队列管理
    pending_queue: UnboundedReceiver<Transaction>,
    batch_aggregator: BatchAggregator,
    
    // 网络层
    network: Arc<DexSequencerNetwork>,
    
    // 执行引擎
    matching_engine: Arc<MatchingEngine>,
    
    // 配置
    config: SequencerConfig,
}

pub struct SequencerConfig {
    pub batch_time_window_ms: u64,      // 5ms
    pub batch_size_threshold: usize,     // 1000
    pub heartbeat_interval_ms: u64,     // 50ms
    pub confirmation_timeout_ms: u64,   // 200ms
}
```

#### 3.1.4 关键流程

**序列号分配流程**:
```
1. 接收交易请求
2. 验证签名和格式
3. 分配序列号 (原子递增)
4. 加入批次聚合器
5. 达到阈值后创建批次
6. 签名并广播
7. 等待确认 (异步)
```

**故障恢复流程**:
```
1. 检测主节点故障 (心跳超时)
2. 从 DA 层获取最后确认序列号
3. 新主节点从该序列号继续
4. 广播 Leader 变更
5. 重放未确认序列
```

### 3.2 Matching Engine (撮合引擎)

#### 3.2.1 组件职责

- **订单簿管理**: 内存订单簿（BTreeMap + HashMap）
- **撮合算法**: 价格-时间优先撮合
- **余额管理**: 账户余额更新
- **风险检查**: 保证金验证、仓位限制

#### 3.2.2 架构设计

```
┌─────────────────────────────────────────────────────────────┐
│                  Matching Engine 组件架构                    │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌────────────────────────────────────────────────────────┐ │
│  │  Order Manager                                          │ │
│  │  • 订单验证                                             │ │
│  │  • 订单索引管理                                         │ │
│  │  • 订单生命周期                                         │ │
│  └────────────────────────────────────────────────────────┘ │
│                         │                                    │
│                         ▼                                    │
│  ┌────────────────────────────────────────────────────────┐ │
│  │  Matching Engine Core                                   │ │
│  │  • 价格-时间优先算法                                    │ │
│  │  • 订单簿操作 (BTreeMap)                               │ │
│  │  • 撮合逻辑 (< 10μs)                                    │ │
│  └────────────────────────────────────────────────────────┘ │
│                         │                                    │
│         ┌───────────────┴───────────────┐                   │
│         │                               │                   │
│         ▼                               ▼                   │
│  ┌──────────────┐            ┌──────────────┐              │
│  │ Balance      │            │ Risk Engine  │              │
│  │ Manager      │            │              │              │
│  │ • 余额更新   │            │ • 保证金检查 │              │
│  │ • 锁定管理   │            │ • 仓位限制   │              │
│  └──────────────┘            └──────────────┘              │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

#### 3.2.3 数据结构

```rust
pub struct MatchingEngine {
    // 订单簿 (分片锁，支持并行)
    orderbooks: DashMap<MarketID, Orderbook>,
    
    // 账户余额 (分片锁)
    balances: DashMap<AccountID, Balance>,
    
    // 风险引擎
    risk_engine: Arc<RiskEngine>,
    
    // 配置
    config: EngineConfig,
}

pub struct Orderbook {
    // 买单 (价格降序)
    bids: BTreeMap<Reverse<Price>, PriceLevel>,
    
    // 卖单 (价格升序)
    asks: BTreeMap<Price, PriceLevel>,
    
    // 订单索引 (O(1) 查找)
    order_index: HashMap<OrderID, OrderRef>,
    
    // 市场配置
    market_config: MarketConfig,
}

pub struct PriceLevel {
    price: Price,
    orders: VecDeque<Order>,  // 时间优先队列
    total_quantity: u64,
}
```

#### 3.2.4 撮合算法

**价格-时间优先算法**:

```
1. 接收订单
2. 查找对手方订单簿
3. 价格匹配检查
   - 买单: price >= best_ask
   - 卖单: price <= best_bid
4. 时间优先匹配
   - 相同价格按时间顺序成交
5. 部分成交处理
   - 更新订单数量
   - 继续匹配或挂单
6. 更新订单簿
7. 更新余额
8. 生成成交事件
```

### 3.3 DEX Precompile (预编译钩子)

#### 3.3.1 组件职责

- **调用拦截**: 检测 DEX 包地址的 Move 调用
- **路由决策**: 路由到原生引擎或 Move VM
- **参数转换**: Move 参数转换为 Rust 类型
- **结果转换**: Rust 结果转换为 Move 类型

#### 3.3.2 架构设计

```
┌─────────────────────────────────────────────────────────────┐
│                  Precompile 组件架构                         │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  Move Transaction                                           │
│         │                                                    │
│         ▼                                                    │
│  ┌────────────────────────────────────────────────────────┐ │
│  │  Transaction Router                                     │ │
│  │  • 解析 MoveCall                                       │ │
│  │  • 检查包地址 (0xDEX)                                  │ │
│  │  • 提取函数名和参数                                    │ │
│  └────────────────────────────────────────────────────────┘ │
│         │                                                    │
│    ┌────┴────┐                                              │
│    │         │                                              │
│    ▼         ▼                                              │
│  ┌──────┐  ┌──────────┐                                     │
│  │ DEX  │  │ Standard │                                     │
│  │ Path │  │ Path     │                                     │
│  └──────┘  └──────────┘                                     │
│    │                                                        │
│    ▼                                                        │
│  ┌────────────────────────────────────────────────────────┐ │
│  │  Precompile Handler                                    │ │
│  │  • 参数解码 (BCS)                                      │ │
│  │  • 调用原生引擎                                        │ │
│  │  • 结果编码                                            │ │
│  └────────────────────────────────────────────────────────┘ │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

#### 3.3.3 实现细节

```rust
pub struct DexPrecompile {
    package_id: ObjectID,
    engine: Arc<MatchingEngine>,
}

impl DexPrecompile {
    pub fn is_dex_call(&self, call: &MoveCall) -> bool {
        call.package == self.package_id
    }
    
    pub async fn handle_call(
        &self,
        call: MoveCall,
        context: &ExecutionContext,
    ) -> Result<ExecutionResult> {
        match call.function.as_str() {
            "place_order" => self.handle_place_order(call.args, context).await,
            "cancel_order" => self.handle_cancel_order(call.args, context).await,
            "deposit" => self.handle_deposit(call.args, context).await,
            "withdraw" => self.handle_withdraw(call.args, context).await,
            _ => Err(PrecompileError::UnknownFunction),
        }
    }
}
```

### 3.4 Storage Layer (存储层)

#### 3.4.1 组件职责

- **内存缓存**: 热数据内存存储（订单簿、余额）
- **WAL 持久化**: 写前日志，保证持久化
- **快照管理**: 定期快照，支持快速恢复
- **RocksDB 存储**: 通过 typed-store 持久化

#### 3.4.2 架构设计

```
┌─────────────────────────────────────────────────────────────┐
│                    Storage Layer 架构                       │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌────────────────────────────────────────────────────────┐ │
│  │  Memory Layer (L1 Cache)                               │ │
│  │  • Orderbook State (DashMap)                           │ │
│  │  • Balance Cache (DashMap)                             │ │
│  │  • Position Cache (DashMap)                            │ │
│  └────────────────────────────────────────────────────────┘ │
│                         │                                    │
│                         ▼                                    │
│  ┌────────────────────────────────────────────────────────┐ │
│  │  WAL Layer (Write-Ahead Log)                           │ │
│  │  • 顺序写入日志                                         │ │
│  │  • Group Commit (批量 fsync)                            │ │
│  │  • 崩溃恢复支持                                         │ │
│  └────────────────────────────────────────────────────────┘ │
│                         │                                    │
│                         ▼                                    │
│  ┌────────────────────────────────────────────────────────┐ │
│  │  Snapshot Layer                                         │ │
│  │  • 定期快照 (LZ4 压缩)                                  │ │
│  │  • 增量快照支持                                         │ │
│  │  • 快速恢复 (< 5min RTO)                                │ │
│  └────────────────────────────────────────────────────────┘ │
│                         │                                    │
│                         ▼                                    │
│  ┌────────────────────────────────────────────────────────┐ │
│  │  RocksDB Layer (via typed-store)                       │ │
│  │  • 持久化存储                                           │ │
│  │  • 列族隔离 (DEX 专用)                                  │ │
│  │  • 事务支持                                             │ │
│  └────────────────────────────────────────────────────────┘ │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

#### 3.4.3 数据表设计

```rust
pub struct DexStore {
    // 订单表
    orders: DBMap<OrderID, Order>,
    
    // 余额表
    balances: DBMap<AccountID, Balance>,
    
    // 持仓表
    positions: DBMap<PositionKey, Position>,
    
    // 成交表
    trades: DBMap<TradeID, Trade>,
    
    // 市场配置表
    markets: DBMap<MarketID, MarketConfig>,
    
    // 序列号表 (用于恢复)
    sequences: DBMap<SequenceNumber, SequenceBatch>,
}
```

### 3.5 Risk Engine (风险引擎)

#### 3.5.1 组件职责

- **保证金计算**: NC、IMR、MMR 计算
- **账户健康度**: 健康状态判断
- **OIMF 机制**: 开仓量保证金调整
- **清算触发**: 清算条件检测

#### 3.5.2 架构设计

```
┌─────────────────────────────────────────────────────────────┐
│                    Risk Engine 架构                         │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌────────────────────────────────────────────────────────┐ │
│  │  Collateral Calculator                                 │ │
│  │  • 净抵押品 (NC) 计算                                  │ │
│  │  • 资产价值评估                                        │ │
│  │  • 仓位净值计算                                        │ │
│  └────────────────────────────────────────────────────────┘ │
│                         │                                    │
│                         ▼                                    │
│  ┌────────────────────────────────────────────────────────┐ │
│  │  Margin Calculator                                     │ │
│  │  • 初始保证金 (IMR) 计算                               │ │
│  │  • 维持保证金 (MMR) 计算                               │ │
│  │  • OIMF 调整                                           │ │
│  └────────────────────────────────────────────────────────┘ │
│                         │                                    │
│                         ▼                                    │
│  ┌────────────────────────────────────────────────────────┐ │
│  │  Health Monitor                                       │ │
│  │  • 账户健康状态判断                                    │ │
│  │  • 清算触发检测                                        │ │
│  │  • 风险预警                                            │ │
│  └────────────────────────────────────────────────────────┘ │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

---

## 4. 数据流设计

### 4.1 DEX 订单完整流程

```
┌─────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐
│ Client  │    │ API      │    │ Sequencer│    │ Matching │    │ Storage  │
│         │    │ Gateway  │    │          │    │ Engine   │    │          │
└────┬────┘    └────┬─────┘    └────┬─────┘    └────┬─────┘    └────┬─────┘
     │               │               │               │               │
     │ 1. Place     │               │               │               │
     │    Order     │               │               │               │
     │─────────────>│               │               │               │
     │               │               │               │               │
     │               │ 2. Route to   │               │               │
     │               │    Sequencer  │               │               │
     │               │──────────────>│               │               │
     │               │               │               │               │
     │               │               │ 3. Assign    │               │
     │               │               │    SeqNo     │               │
     │               │               │──────────────┼───────────────┼──┐
     │               │               │               │               │  │
     │               │               │               │ 4. Match      │  │
     │               │               │               │    Order       │  │
     │               │               │               │<───────────────┼──┘
     │               │               │               │               │
     │               │               │               │ 5. Update     │
     │               │               │               │    Balance    │
     │               │               │               │──────────────>│
     │               │               │               │               │
     │               │               │ 6. Broadcast  │               │
     │               │               │    Sequence   │               │
     │               │               │──────────────┼───────────────┼──┐
     │               │               │               │               │  │
     │               │               │               │ 7. Persist     │  │
     │               │               │               │    to WAL     │  │
     │               │               │               │<───────────────┼──┘
     │               │               │               │               │
     │               │ 8. Soft ACK   │               │               │
     │               │<──────────────│               │               │
     │ 9. Response   │               │               │               │
     │<──────────────│               │               │               │
     │               │               │               │               │
```

**时间线**:
- T+0ms: 订单到达
- T+5ms: 序列号分配
- T+10ms: 撮合完成
- T+15ms: 状态更新
- T+20ms: 广播序列
- T+30ms: WAL 写入
- T+50ms: 软确认返回

### 4.2 存取款流程 (Hybrid Path)

```
┌─────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐
│ Client  │    │ Sequencer│    │ Move VM  │    │ Matching │    │ Storage  │
│         │    │          │    │          │    │ Engine   │    │          │
└────┬────┘    └────┬─────┘    └────┬─────┘    └────┬─────┘    └────┬─────┘
     │               │               │               │               │
     │ 1. Deposit   │               │               │               │
     │    Tx        │               │               │               │
     │─────────────>│               │               │               │
     │               │               │               │               │
     │               │ 2. Assign    │               │               │
     │               │    SeqNo     │               │               │
     │               │───────────────┼───────────────┼───────────────┼──┐
     │               │               │               │               │  │
     │               │               │ 3. Lock Coin │               │  │
     │               │               │    (Move VM)  │               │  │
     │               │               │<──────────────┼───────────────┼──┘
     │               │               │               │               │
     │               │               │               │ 4. Update    │
     │               │               │               │    DEX Balance│
     │               │               │               │──────────────>│
     │               │               │               │               │
     │               │ 5. Broadcast  │               │               │
     │               │    Sequence   │               │               │
     │               │───────────────┼───────────────┼───────────────┼──┐
     │               │               │               │               │  │
     │               │               │               │ 6. Persist     │  │
     │               │               │               │    to WAL     │  │
     │               │               │               │<──────────────┼──┘
     │               │               │               │               │
     │ 7. Confirmed  │               │               │               │
     │<──────────────│               │               │               │
     │               │               │               │               │
```

**关键点**:
- 两阶段执行：先 Move VM 锁定，再 DEX 更新余额
- 原子性保证：两者在同一交易上下文内完成

### 4.3 网络层数据流

```
┌──────────────┐         ┌──────────────┐         ┌──────────────┐
│ Main Node    │         │ Standby Node │         │ Standby Node │
│ (Sequencer)  │         │              │         │              │
└──────┬───────┘         └──────┬───────┘         └──────┬───────┘
       │                        │                        │
       │ 1. Process Order       │                        │
       │────────────────────────┼────────────────────────┼──┐
       │                        │                        │  │
       │                        │ 2. Forward Order      │  │
       │                        │<───────────────────────┼──┘
       │                        │                        │
       │ 3. Broadcast Sequence │                        │
       │────────────────────────>│                        │
       │────────────────────────────────────────────────>│
       │                        │                        │
       │                        │ 4. Verify & Replay    │
       │                        │────────────────────────┼──┐
       │                        │                        │  │
       │                        │ 5. Send Confirmation  │  │
       │<────────────────────────│                        │  │
       │<────────────────────────────────────────────────│  │
       │                        │                        │  │
       │ 6. Collect 2f+1        │                        │  │
       │    Confirmations       │                        │  │
       │────────────────────────┼────────────────────────┼──┘
       │                        │                        │
```

---

## 5. 接口设计

### 5.1 JSON-RPC API 扩展

#### 5.1.1 DEX 专用接口

```json
// 下单
{
  "jsonrpc": "2.0",
  "method": "dex_place_order",
  "params": {
    "market_id": "BTC-USD",
    "side": "buy",
    "order_type": "limit",
    "price": 50000,
    "quantity": 0.1,
    "subaccount_id": 0
  },
  "id": 1
}

// 撤单
{
  "jsonrpc": "2.0",
  "method": "dex_cancel_order",
  "params": {
    "order_id": "0x1234...",
    "subaccount_id": 0
  },
  "id": 2
}

// 查询订单簿
{
  "jsonrpc": "2.0",
  "method": "dex_get_orderbook",
  "params": {
    "market_id": "BTC-USD",
    "depth": 20
  },
  "id": 3
}

// 查询余额
{
  "jsonrpc": "2.0",
  "method": "dex_get_balance",
  "params": {
    "address": "0xabc...",
    "subaccount_id": 0,
    "asset_id": 0
  },
  "id": 4
}
```

#### 5.1.2 WebSocket 接口

```rust
// 订阅订单簿更新
{
  "method": "subscribe",
  "params": {
    "channel": "orderbook",
    "market_id": "BTC-USD"
  }
}

// 订阅成交事件
{
  "method": "subscribe",
  "params": {
    "channel": "trades",
    "market_id": "BTC-USD"
  }
}

// 订阅账户更新
{
  "method": "subscribe",
  "params": {
    "channel": "account",
    "address": "0xabc...",
    "subaccount_id": 0
  }
}
```

### 5.2 Move 接口设计

#### 5.2.1 dex-framework 模块

```move
module dex::dex {
    use sui::coin::{Self, Coin};
    use sui::transfer;
    use sui::tx_context::{Self, TxContext};
    
    // 下单 (Precompile 拦截)
    public entry fun place_order<B, Q>(
        market_id: vector<u8>,
        side: u8,            // 0: Buy, 1: Sell
        order_type: u8,      // 0: Limit, 1: Market, ...
        price: u64,
        quantity: u64,
        subaccount_id: u32,
        ctx: &mut TxContext,
    ) {
        // Precompile 拦截，实际由原生引擎执行
        abort 0
    }
    
    // 撤单 (Precompile 拦截)
    public entry fun cancel_order(
        order_id: vector<u8>,
        subaccount_id: u32,
        ctx: &mut TxContext,
    ) {
        abort 0
    }
    
    // 存款 (Hybrid: Move + Native)
    public entry fun deposit<T>(
        coin: Coin<T>,
        subaccount_id: u32,
        ctx: &mut TxContext,
    ) {
        // Precompile 拦截:
        // 1. Move: 锁定 Coin 到托管账户
        // 2. Native: 更新 DEX 余额
        abort 0
    }
    
    // 取款 (Hybrid: Native + Move)
    public entry fun withdraw<T>(
        amount: u64,
        subaccount_id: u32,
        ctx: &mut TxContext,
    ) {
        // Precompile 拦截:
        // 1. Native: 检查并扣减 DEX 余额
        // 2. Move: 释放 Coin 给用户
        abort 0
    }
}
```

### 5.3 内部接口设计

#### 5.3.1 Sequencer 接口

```rust
pub trait Sequencer: Send + Sync {
    /// 处理交易
    async fn process_transaction(
        &self,
        tx: Transaction,
    ) -> Result<SequenceNumber>;
    
    /// 获取序列状态
    async fn get_sequence_status(
        &self,
        seq: SequenceNumber,
    ) -> Result<SequenceStatus>;
    
    /// 广播序列批次
    async fn broadcast_batch(
        &self,
        batch: SequenceBatch,
    ) -> Result<()>;
}
```

#### 5.3.2 Matching Engine 接口

```rust
pub trait MatchingEngine: Send + Sync {
    /// 撮合订单
    fn match_order(
        &self,
        order: Order,
        sequence: SequenceNumber,
    ) -> Result<MatchResult>;
    
    /// 撤单
    fn cancel_order(
        &self,
        order_id: OrderID,
    ) -> Result<()>;
    
    /// 查询订单簿
    fn get_orderbook(
        &self,
        market_id: MarketID,
        depth: usize,
    ) -> Result<OrderbookSnapshot>;
}
```

---

## 6. 部署架构

### 6.1 节点部署架构

```
┌─────────────────────────────────────────────────────────────┐
│                    Phase 1 部署架构                          │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌────────────────────────────────────────────────────────┐ │
│  │  Main Node (Sequencer)                                │ │
│  │  • 中心化定序器                                        │ │
│  │  • 撮合引擎                                            │ │
│  │  • 状态管理                                            │ │
│  │  • 网络: 10Gbps                                        │ │
│  │  • CPU: 32 cores                                       │ │
│  │  • RAM: 256GB                                          │ │
│  └────────────────────────────────────────────────────────┘ │
│                         │                                    │
│         ┌───────────────┴───────────────┐                   │
│         │                               │                   │
│         ▼                               ▼                   │
│  ┌──────────────┐            ┌──────────────┐              │
│  │ Standby Node │            │ Standby Node │              │
│  │              │            │              │              │
│  │ • 订单接收   │            │ • 订单接收   │              │
│  │ • 序列重放   │            │ • 序列重放   │              │
│  │ • 状态同步   │            │ • 状态同步   │              │
│  │ • 故障切换   │            │ • 故障切换   │              │
│  └──────────────┘            └──────────────┘              │
│         │                               │                   │
│         └───────────────┬───────────────┘                   │
│                         │                                    │
│                         ▼                                    │
│  ┌────────────────────────────────────────────────────────┐ │
│  │  Sui Network (Anemo P2P)                              │ │
│  │  • QUIC 协议                                           │ │
│  │  • 自动重连                                             │ │
│  │  • 消息广播                                             │ │
│  └────────────────────────────────────────────────────────┘ │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

### 6.2 存储部署

```
┌─────────────────────────────────────────────────────────────┐
│                    Storage 部署架构                         │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌────────────────────────────────────────────────────────┐ │
│  │  Memory (RAM)                                          │ │
│  │  • Orderbook: ~10GB (1000 markets)                    │ │
│  │  • Balances: ~5GB (1M accounts)                        │ │
│  │  • Positions: ~2GB                                    │ │
│  └────────────────────────────────────────────────────────┘ │
│                         │                                    │
│                         ▼                                    │
│  ┌────────────────────────────────────────────────────────┐ │
│  │  WAL (NVMe SSD)                                        │ │
│  │  • 顺序写入，~100MB/s                                  │ │
│  │  • Group Commit (每 5ms)                               │ │
│  └────────────────────────────────────────────────────────┘ │
│                         │                                    │
│                         ▼                                    │
│  ┌────────────────────────────────────────────────────────┐ │
│  │  RocksDB (NVMe SSD)                                    │ │
│  │  • 持久化存储，~500GB                                  │ │
│  │  • 列族隔离                                            │ │
│  │  • 压缩优化                                            │ │
│  └────────────────────────────────────────────────────────┘ │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

---

## 7. 扩展性设计

### 7.1 Phase 1 → Phase 2 演进

#### 7.1.1 多节点轮换 Sequencer

**当前 (Phase 1)**:
- 单一中心化 Sequencer
- 热备份支持

**演进 (Phase 2)**:
- 复用 Sui Leader Schedule
- 轮换 Sequencer Leader
- 故障自动切换

**接口预留**:
```rust
pub trait SequencerLeaderSchedule {
    fn current_leader(&self, timestamp: u64) -> AuthorityIndex;
    fn next_leader(&self, failed: AuthorityIndex) -> AuthorityIndex;
}
```

#### 7.1.2 性能扩展

**水平扩展**:
- 多市场并行处理
- 分片订单簿
- 负载均衡

**垂直扩展**:
- SIMD 优化
- CPU 亲和性
- 内存池优化

### 7.2 容量规划

| 指标 | Phase 1 | Phase 2 | Phase 3 |
|-----|---------|---------|---------|
| **TPS** | 10万 | 50万 | 100万+ |
| **延迟 (P99)** | < 50ms | < 30ms | < 20ms |
| **市场数量** | 100 | 500 | 1000+ |
| **账户数量** | 100万 | 1000万 | 1亿+ |

---

## 8. 参考架构对比

### 8.1 与 dYdX v4 (Cosmos) 对比

| 特性 | dYdX v4 | DEX on Sui (Phase 1) |
|-----|---------|---------------------|
| **共识** | CometBFT | 中心化 Sequencer |
| **撮合** | MemClob (内存) | Native Rust Engine |
| **结算** | Cosmos SDK | Sui Move VM |
| **存储** | IAVL Tree | RocksDB + 内存缓存 |
| **网络** | Tendermint P2P | Anemo P2P (复用) |
| **延迟** | ~100ms | < 50ms |
| **TPS** | ~2,000 | 10万+ |

### 8.2 与 Sui 标准路径对比

| 特性 | Sui Standard | DEX Fast Path |
|-----|-------------|---------------|
| **共识** | Mysticeti | Sequencer |
| **执行** | Move VM | Native Rust |
| **延迟** | ~600ms | < 50ms |
| **TPS** | ~2,000 | 10万+ |
| **适用** | 通用交易 | DEX 订单 |

---

## 9. 架构决策记录 (ADR)

### ADR-001: 选择中心化 Sequencer

**状态**: 已决定  
**上下文**: Phase 1 需要快速实现，性能优先  
**决策**: 使用中心化 Sequencer，后续演进为多节点轮换  
**后果**: 
- ✅ 快速实现，性能优异
- ⚠️ 单点故障风险（通过热备份缓解）

### ADR-002: 使用原生 Rust 引擎

**状态**: 已决定  
**上下文**: 性能要求 < 10μs，Move VM 无法满足  
**决策**: 使用 Rust 原生实现撮合引擎  
**后果**:
- ✅ 极致性能
- ⚠️ 需要严格测试和审计

### ADR-003: SDK 方式集成

**状态**: 已决定  
**上下文**: 需要清晰代码边界，易于维护  
**决策**: 采用 SDK 方式 + Fork Sui  
**后果**:
- ✅ 代码清晰，易于维护
- ⚠️ 需要维护 Sui fork

---

## 10. 总结

### 10.1 架构特点

1. **分层清晰**: 6 层架构，职责分明
2. **路径分离**: DEX Fast Path 和 Sui Standard Path
3. **复用优先**: 约 40% 复用 Sui 基础设施
4. **性能优先**: 关键路径原生实现

### 10.2 关键设计

- **Sequencer**: 中心化定序，FIFO 排序
- **Matching Engine**: 原生 Rust，< 10μs 撮合
- **Precompile**: 保持 Move 兼容性
- **Storage**: 内存 + WAL + RocksDB 三层

### 10.3 演进路径

Phase 1 (当前) → Phase 2 (多节点) → Phase 3 (Sui 共识)

---

**文档版本**: v1.0  
**最后更新**: 2025-01-XX  
**维护者**: DEX 架构团队

