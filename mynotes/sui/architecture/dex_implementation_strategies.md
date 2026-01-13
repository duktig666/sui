# 基于 Sui 自研 DEX 专用链的技术路线分析

本文档分析基于 Sui 自研 DEX 专用链的三种技术实现方式,为技术选型和架构设计提供决策依据。

---

## 一、背景与目标

### 1.1 DEX 性能需求

| 指标 | 目标值 | 说明 |
|------|--------|------|
| **TPS** | 10,000+ | 支持高频交易,与中心化交易所竞争 |
| **延迟** | <100ms | 订单提交到确认的端到端延迟 |
| **吞吐量** | 持续高负载 | 避免拥堵时性能下降 |
| **并发度** | 高 | 多个交易对可并行处理 |

### 1.2 Sui 现状分析 (基于代码探索)

**性能现状**:

| 指标 | Sui 现状 | 代码依据 | 瓶颈分析 |
|------|---------|---------|---------|
| **TPS** | ~5,000 | Shared 对象限制 | 共识吞吐量,执行串行化 |
| **延迟** | 200-400ms | `consensus_adapter.rs:246-275` Mysticeti | 共识延迟 |
| **并发度** | CPU 核心数 | `execution_driver.rs:33` Semaphore | 锁粒度粗 |

**架构特征**:

**交易执行路径** (`sui-core/src/authority.rs`):
```
用户提交 → 判断路径
  ├─ Owned Objects → Fastpath (跳过共识,<10ms)
  └─ Shared Objects → Consensus Path (Mysticeti, 200-400ms)
       ↓
ExecutionScheduler 统一调度
       ↓
execution_driver 线程池 (并发度 = num_cpus)
       ↓
sui-execution Executor::execute_transaction_to_effects
       ↓
Move VM 执行
```

**关键约束**:
- ✅ **Fastpath**: Owned objects 可跳过共识,延迟 <10ms
- ❌ **Consensus Path**: Shared objects 必须走共识,延迟 200-400ms
- ❌ **执行并发**: 受 `Semaphore::new(num_cpus::get())` 限制

**性能瓶颈定位** (代码级):

1. **共识延迟** (`consensus_adapter.rs:246-275`):
   - Mysticeti BFT 共识延迟 200-400ms
   - Shared objects 必须走共识,无法绕过

2. **执行并行度** (`execution_driver.rs:33`):
   ```rust
   let limit = Arc::new(Semaphore::new(num_cpus::get()));
   ```
   - 并发度受 CPU 核心数限制
   - 不同交易对无法充分并行

3. **存储写入** (`authority.rs:1910-1944`):
   - 每个交易写入多个 RocksDB 表
   - 顺序写入,无批量优化

---

## 二、方案 1: Fork Sui 修改 (深度定制)

### 2.1 架构设计

**总体架构**:
```
┌──────────────────────────────────────────────────────────────┐
│                    Forked Sui 节点                            │
│  ┌────────────────────────────────────────────────────────┐  │
│  │              sui-types (修改)                          │  │
│  │  pub enum TransactionKind {                           │  │
│  │      ProgrammableTransaction,                          │  │
│  │      DexTransaction(DexTransaction), ← 新增           │  │
│  │      ...                                              │  │
│  │  }                                                     │  │
│  └────────────────────────────────────────────────────────┘  │
│                          ↓                                    │
│  ┌────────────────────────────────────────────────────────┐  │
│  │            sui-core (修改)                             │  │
│  │  AuthorityState {                                      │  │
│  │      dex_engine: Arc<DexMatchingEngine>, ← 新增       │  │
│  │      ...                                               │  │
│  │  }                                                     │  │
│  └────────────────────────────────────────────────────────┘  │
│                          ↓                                    │
│  ┌────────────────────────────────────────────────────────┐  │
│  │         sui-execution (修改)                           │  │
│  │  match transaction_kind {                              │  │
│  │      DexTransaction(dex_tx) => {                       │  │
│  │          execute_dex_matching(...)  ← 新增分支        │  │
│  │      }                                                  │  │
│  │      ProgrammableTransaction(pt) => { ... }            │  │
│  │  }                                                     │  │
│  └────────────────────────────────────────────────────────┘  │
│                          ↓                                    │
│  ┌────────────────────────────────────────────────────────┐  │
│  │         DexMatchingEngine (新增)                       │  │
│  │  - 内存订单簿 (lock-free data structures)              │  │
│  │  - 原生 Rust 撮合算法                                  │  │
│  │  - 批量执行优化                                         │  │
│  └────────────────────────────────────────────────────────┘  │
│                          ↓                                    │
│  ┌────────────────────────────────────────────────────────┐  │
│  │         sui-storage (适配)                             │  │
│  │  - DEX 状态集成到 checkpoint                           │  │
│  │  - 订单数据持久化                                       │  │
│  └────────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────────┘
```

### 2.2 核心修改点 (基于代码探索)

#### 修改点 1: 扩展 TransactionKind

**文件**: `crates/sui-types/src/transaction.rs:458`

**原代码**:
```rust
pub enum TransactionKind {
    ProgrammableTransaction(ProgrammableTransaction),
    ChangeEpoch(ChangeEpoch),
    Genesis(GenesisTransaction),
    ConsensusCommitPrologue(ConsensusCommitPrologue),
    // ...
}
```

**修改后**:
```rust
pub enum TransactionKind {
    ProgrammableTransaction(ProgrammableTransaction),

    // ✅ 新增: DEX 专用交易类型
    DexTransaction(DexTransaction),

    ChangeEpoch(ChangeEpoch),
    Genesis(GenesisTransaction),
    // ...
}

// 新增 DEX 交易结构
pub struct DexTransaction {
    pub operation: DexOperation,
    pub gas: GasData,
}

pub enum DexOperation {
    PlaceLimitOrder {
        orderbook_id: ObjectID,
        side: OrderSide,
        price: u64,
        quantity: u64,
        coin: ObjectRef,
    },
    CancelOrder {
        orderbook_id: ObjectID,
        order_id: OrderID,
    },
    BatchMatch {
        orderbook_id: ObjectID,
        orders: Vec<OrderRef>,
    },
}
```

**影响范围**:
- `transaction.rs` 中所有 `match transaction_kind` 的地方需要添加新分支
- 序列化/反序列化逻辑需要适配

---

#### 修改点 2: 执行器集成 DEX 引擎

**文件**: `sui-execution/v1/sui-adapter/src/execution_engine.rs:59`

**原代码**:
```rust
pub fn execute_transaction_to_effects<Mode: ExecutionMode>(
    store: &dyn BackingStore,
    // ...
    transaction_kind: TransactionKind,
    // ...
) -> (...) {
    let mut temporary_store = TemporaryStore::new(...);

    let (gas_cost_summary, execution_result) = execute_transaction::<Mode>(
        &mut temporary_store,
        transaction_kind,
        // ...
    );

    // ...
}
```

**修改后**:
```rust
pub fn execute_transaction_to_effects<Mode: ExecutionMode>(
    store: &dyn BackingStore,
    dex_engine: Option<&Arc<DexMatchingEngine>>,  // ← 新增参数
    // ...
    transaction_kind: TransactionKind,
    // ...
) -> (...) {
    // ✅ DEX 交易走专用引擎
    if let TransactionKind::DexTransaction(dex_tx) = &transaction_kind {
        return execute_dex_transaction(
            store,
            dex_engine.expect("DexEngine required"),
            dex_tx,
            gas_status,
            transaction_digest,
            epoch_id,
            epoch_timestamp_ms,
            protocol_config,
            metrics,
        );
    }

    // 原有逻辑
    let mut temporary_store = TemporaryStore::new(...);
    // ...
}

// ✅ 新增: DEX 交易执行函数
fn execute_dex_transaction(
    store: &dyn BackingStore,
    dex_engine: &DexMatchingEngine,
    dex_tx: &DexTransaction,
    mut gas_status: SuiGasStatus,
    transaction_digest: TransactionDigest,
    epoch_id: &EpochId,
    epoch_timestamp_ms: u64,
    protocol_config: &ProtocolConfig,
    metrics: Arc<LimitsMetrics>,
) -> (
    InnerTemporaryStore,
    SuiGasStatus,
    TransactionEffects,
    Vec<ExecutionTiming>,
    Result<(), ExecutionError>,
) {
    // 原生 Rust 撮合逻辑
    match &dex_tx.operation {
        DexOperation::PlaceLimitOrder { orderbook_id, side, price, quantity, coin } => {
            // 1. 检查订单合法性
            // 2. 锁定用户资产 (coin)
            // 3. 插入订单簿
            // 4. 尝试立即撮合
            // 5. 生成 TransactionEffects
            dex_engine.place_order(orderbook_id, side, price, quantity, coin)
        }
        DexOperation::CancelOrder { orderbook_id, order_id } => {
            // 1. 验证订单所有权
            // 2. 从订单簿移除
            // 3. 退还锁定资产
            dex_engine.cancel_order(orderbook_id, order_id)
        }
        DexOperation::BatchMatch { orderbook_id, orders } => {
            // 批量撮合优化
            dex_engine.batch_match(orderbook_id, orders)
        }
    }
}
```

---

#### 修改点 3: Authority 集成 DEX 引擎

**文件**: `crates/sui-core/src/authority.rs:903`

**原代码**:
```rust
pub struct AuthorityState {
    name: AuthorityName,
    secret: StableSyncAuthoritySigner,
    input_loader: TransactionInputLoader,
    execution_cache_trait_pointers: ExecutionCacheTraitPointers,
    // ...
}
```

**修改后**:
```rust
pub struct AuthorityState {
    name: AuthorityName,
    secret: StableSyncAuthoritySigner,
    input_loader: TransactionInputLoader,
    execution_cache_trait_pointers: ExecutionCacheTraitPointers,

    // ✅ 新增: DEX 撮合引擎
    dex_engine: Option<Arc<DexMatchingEngine>>,

    // ...
}

impl AuthorityState {
    // 在 execute_certificate 中传递 dex_engine
    fn execute_certificate(
        &self,
        // ...
    ) -> ExecutionOutput<...> {
        self.executor.execute_transaction_to_effects(
            store,
            self.dex_engine.as_ref(),  // ← 传递 DEX 引擎
            // ...
            transaction_kind,
            // ...
        )
    }
}
```

---

#### 修改点 4: 批量执行优化

**文件**: `crates/sui-core/src/consensus_handler.rs:99`

**原代码**:
```rust
impl ConsensusHandler {
    fn handle_consensus_output(
        &mut self,
        consensus_output: ConsensusCommit,
    ) {
        for tx in parse_transactions(&consensus_output) {
            self.schedule_execution(tx);
        }
    }
}
```

**修改后**:
```rust
impl ConsensusHandler {
    fn handle_consensus_output(
        &mut self,
        consensus_output: ConsensusCommit,
    ) {
        let transactions = parse_transactions(&consensus_output);

        // ✅ 批量提取 DEX 交易
        let (dex_txs, other_txs): (Vec<_>, Vec<_>) =
            transactions.into_iter().partition(|tx| {
                matches!(tx.kind(), TransactionKind::DexTransaction(_))
            });

        // ✅ 批量执行 DEX 交易 (单次原子操作)
        if !dex_txs.is_empty() {
            self.execute_dex_batch(dex_txs);
        }

        // 正常处理其他交易
        for tx in other_txs {
            self.schedule_execution(tx);
        }
    }

    // ✅ 新增: 批量执行 DEX 交易
    fn execute_dex_batch(&mut self, transactions: Vec<VerifiedExecutableTransaction>) {
        // 按 orderbook_id 分组
        let mut orderbooks: HashMap<ObjectID, Vec<DexTransaction>> = HashMap::new();
        for tx in transactions {
            if let TransactionKind::DexTransaction(dex_tx) = tx.kind() {
                let orderbook_id = dex_tx.orderbook_id();
                orderbooks.entry(orderbook_id).or_default().push(dex_tx.clone());
            }
        }

        // 并行执行不同订单簿的交易
        orderbooks.par_iter().for_each(|(orderbook_id, txs)| {
            self.state.dex_engine.as_ref().unwrap().batch_execute(orderbook_id, txs);
        });
    }
}
```

---

### 2.3 DexMatchingEngine 设计

**新增文件**: `crates/sui-core/src/dex_engine.rs`

```rust
use crossbeam_skiplist::SkipMap;  // Lock-free 数据结构
use parking_lot::RwLock;

pub struct DexMatchingEngine {
    // 内存订单簿 (按 orderbook_id 索引)
    orderbooks: Arc<DashMap<ObjectID, Arc<Orderbook>>>,

    // 持久化层
    storage: Arc<dyn DexStorage>,
}

pub struct Orderbook {
    id: ObjectID,

    // Buy orders: 价格从高到低
    bids: SkipMap<Price, VecDeque<Order>>,  // Lock-free

    // Sell orders: 价格从低到高
    asks: SkipMap<Price, VecDeque<Order>>,

    // 最新成交价
    last_price: AtomicU64,
}

pub struct Order {
    order_id: OrderID,
    trader: SuiAddress,
    side: OrderSide,
    price: u64,
    original_quantity: u64,
    filled_quantity: u64,
    locked_coin: ObjectRef,
    timestamp: u64,
}

impl DexMatchingEngine {
    pub fn place_order(
        &self,
        orderbook_id: &ObjectID,
        side: OrderSide,
        price: u64,
        quantity: u64,
        coin: ObjectRef,
    ) -> Result<TransactionEffects, ExecutionError> {
        let orderbook = self.orderbooks.get(orderbook_id)?;

        // 1. 尝试立即撮合
        let (filled_quantity, matched_orders) = match side {
            OrderSide::Buy => orderbook.match_buy(price, quantity),
            OrderSide::Sell => orderbook.match_sell(price, quantity),
        };

        // 2. 如果未完全成交,插入订单簿
        if filled_quantity < quantity {
            let remaining = quantity - filled_quantity;
            let order = Order {
                order_id: OrderID::new(),
                trader: coin.owner(),
                side,
                price,
                original_quantity: quantity,
                filled_quantity,
                locked_coin: coin,
                timestamp: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64,
            };

            match side {
                OrderSide::Buy => orderbook.bids.insert(price, order),
                OrderSide::Sell => orderbook.asks.insert(price, order),
            };
        }

        // 3. 生成 TransactionEffects
        self.generate_effects(filled_quantity, matched_orders)
    }

    pub fn batch_match(
        &self,
        orderbook_id: &ObjectID,
        orders: &[OrderRef],
    ) -> Vec<TransactionEffects> {
        // 批量撮合优化:
        // 1. 一次性收集所有买单和卖单
        // 2. 按价格排序
        // 3. 批量撮合
        // 4. 批量生成 effects
        // 5. 批量写入存储 (减少 I/O)

        // ... 实现
    }
}

impl Orderbook {
    fn match_buy(&self, max_price: u64, quantity: u64) -> (u64, Vec<OrderID>) {
        let mut filled = 0u64;
        let mut matched_orders = Vec::new();

        // 从 asks (卖单) 中寻找匹配
        for (price, orders) in self.asks.range(..=max_price) {
            for order in orders.iter() {
                let match_quantity = (quantity - filled).min(order.remaining_quantity());
                filled += match_quantity;
                matched_orders.push(order.order_id);

                if filled >= quantity {
                    return (filled, matched_orders);
                }
            }
        }

        (filled, matched_orders)
    }

    fn match_sell(&self, min_price: u64, quantity: u64) -> (u64, Vec<OrderID>) {
        // 类似 match_buy,但从 bids (买单) 匹配
        // ...
    }
}
```

---

### 2.4 性能优化策略

#### 优化 1: Fastpath 路径

**策略**: 订单使用 Owned objects,跳过共识

**实现**:
```rust
// 每个订单是独立的 Owned object
// Owner = trader address
// 下单时:
//   1. 创建 Order object (Owned by trader)
//   2. Fastpath 执行 (不走共识)
//   3. 索引器监听 OrderPlacedEvent
//   4. 撮合引擎在链下聚合订单簿

// 成交时:
//   1. 批量撮合计算结果
//   2. 批量提交到链上 (Shared orderbook object)
//   3. Consensus Path (但批量摊销成本)
```

**预期效果**:
- 下单延迟: <10ms (Fastpath)
- 成交延迟: 200-400ms (Consensus,但批量处理)
- TPS: 15,000+ (下单不走共识)

---

#### 优化 2: Sequencer 替代共识

**策略**: 使用中心化 Sequencer 替代 Mysticeti 共识

**实施点**: `crates/sui-core/src/consensus_adapter.rs`

**原代码**:
```rust
pub struct ConsensusAdapter {
    consensus_client: Arc<dyn ConsensusClient>,  // Mysticeti
    // ...
}
```

**修改后**:
```rust
pub struct ConsensusAdapter {
    consensus_client: Arc<dyn ConsensusClient>,  // Sequencer 或 Mysticeti (可配置)
    use_sequencer: bool,  // ← 配置开关
    // ...
}

impl ConsensusAdapter {
    pub async fn submit(&self, tx: &ConsensusTransaction) -> Result<()> {
        if self.use_sequencer {
            // ✅ 使用 Sequencer (中心化排序)
            self.sequencer_client.submit(tx).await
        } else {
            // 原有 Mysticeti 共识
            self.consensus_client.submit(tx).await
        }
    }
}

// ✅ 新增: Sequencer 客户端
struct SequencerClient {
    url: String,
}

impl SequencerClient {
    async fn submit(&self, tx: &ConsensusTransaction) -> Result<()> {
        // 1. 提交到中心化 Sequencer
        // 2. Sequencer 立即返回排序结果 (<50ms)
        // 3. 定期批量提交到去中心化共识 (最终性保证)

        let sequence_number = reqwest::post(format!("{}/submit", self.url))
            .json(tx)
            .send()
            .await?
            .json::<u64>()
            .await?;

        Ok(())
    }
}
```

**预期效果**:
- 延迟: <50ms (Sequencer 排序)
- TPS: 20,000+ (Sequencer 吞吐量)
- 最终性: 定期批量提交到共识 (如每 1000 笔或每 10 秒)

**权衡**:
- ✅ 极低延迟和高 TPS
- ❌ 引入中心化点 (Sequencer)
- ⚠️ 需要设计 Sequencer 容错机制

---

#### 优化 3: 并行执行调度

**策略**: 不同交易对独立队列,完全并行

**实施点**: `crates/sui-core/src/execution_driver.rs`

**原代码**:
```rust
let limit = Arc::new(Semaphore::new(num_cpus::get()));

for _ in 0..num_cpus::get() {
    tokio::spawn(async move {
        loop {
            let permit = limit.acquire().await;
            execute_transaction();
        }
    });
}
```

**修改后**:
```rust
// ✅ DEX 专用调度器
struct DexExecutionScheduler {
    // 为每个交易对分配独立队列
    orderbook_queues: DashMap<ObjectID, mpsc::UnboundedReceiver<Tx>>,

    // 并行度不受 CPU 核心数限制
}

impl DexExecutionScheduler {
    fn schedule(&self, tx: VerifiedExecutableTransaction) {
        if let TransactionKind::DexTransaction(dex_tx) = tx.kind() {
            let orderbook_id = dex_tx.orderbook_id();

            // 发送到对应订单簿的队列
            self.orderbook_queues.get(&orderbook_id)
                .unwrap()
                .send(tx)
                .await;
        }
    }

    fn start(&self) {
        // 为每个订单簿启动独立执行器
        for (orderbook_id, mut rx) in self.orderbook_queues.iter() {
            tokio::spawn(async move {
                while let Some(tx) = rx.recv().await {
                    execute_dex_transaction(tx);
                }
            });
        }
    }
}
```

**预期效果**:
- 不同交易对完全并行,无锁竞争
- 并发度提升 10x+
- 单一交易对内部仍保证顺序性

---

### 2.5 实施路径

**阶段 1: 基础集成 (4-6 周)**
1. Fork Sui 仓库到自己的组织
2. 修改 `TransactionKind` 添加 `DexTransaction`
3. 在 `sui-execution` 添加执行分支
4. 实现简单的内存订单簿 (单线程版本)
5. 端到端测试: 交易提交 → 执行 → 效果生成

**阶段 2: 撮合引擎优化 (6-8 周)**
1. 使用 lock-free 数据结构 (crossbeam-skiplist)
2. 实现高效撮合算法
3. 添加批量执行逻辑
4. 集成到 `ConsensusHandler`

**阶段 3: 性能优化 (4-6 周)**
1. 实现 Sequencer (可选)
2. 优化执行调度器 (并行队列)
3. 优化存储层 (批量写入)
4. 压力测试,调优

**阶段 4: 生产就绪 (8-12 周)**
1. Checkpoint 集成测试
2. Epoch 切换测试
3. 安全审计
4. 文档和工具链

**总计**: 22-32 周 (~5-8 月)

---

### 2.6 性能分析

**理论 TPS**:
- Fastpath 下单: 15,000 TPS
- Consensus 成交: 5,000 TPS (批量优化后)
- **综合**: 20,000+ TPS

**理论延迟**:
- 下单 (Fastpath): <10ms
- 成交 (Consensus): 200-400ms
- 使用 Sequencer: <50ms

**瓶颈识别**:
- ✅ 已解决: 绕过 Move VM,原生执行
- ✅ 已解决: 批量撮合,减少共识次数
- ⚠️ 仍存在: Mysticeti 共识延迟 (需 Sequencer 替代)

---

### 2.7 优缺点总结

#### 优势

- ✅ **最大灵活性**: 可任意修改所有层级 (类型、执行、共识)
- ✅ **性能潜力极高**: 理论 TPS 20,000+, 延迟 <50ms (Sequencer)
- ✅ **保留 Sui 生态**: Move 合约仍可用,工具链兼容
- ✅ **深度优化空间**: 可实现 Sequencer、批量执行、并行调度等

#### 劣势

- ❌ **维护成本高**: 需持续同步上游更新 (~每月 1 周工作量)
- ❌ **分叉风险**: 协议升级时需要适配,可能出现兼容性问题
- ❌ **初始开发成本**: 5-8 月开发周期
- ❌ **团队要求高**: 需要深入理解 Sui 内部实现

#### 风险

- ⚠️ **Checkpoint 集成**: DEX 状态需要正确持久化到 checkpoint
- ⚠️ **Epoch 切换**: Epoch 边界时 DEX 引擎状态一致性
- ⚠️ **上游分叉**: Sui 大版本升级可能导致大量冲突
- ⚠️ **安全漏洞**: 自定义代码可能引入安全问题

---

## 三、方案 2: 依赖集成 (组装自定义链)

### 3.1 架构设计

**总体架构**:
```
┌──────────────────────────────────────────────────────────────┐
│                 my-dex-chain (新项目)                         │
│                                                               │
│  Cargo.toml                                                   │
│  ┌─────────────────────────────────────────────────────┐     │
│  │ [dependencies]                                      │     │
│  │ sui-types = { workspace = true }          ← 直接依赖│     │
│  │ sui-storage = { workspace = true }        ← 直接依赖│     │
│  │ sui-execution = { workspace = true }      ← 直接依赖│     │
│  │ sui-framework = { workspace = true }      ← 直接依赖│     │
│  │ consensus-core = { workspace = true }     ← 直接依赖│     │
│  │                                                      │     │
│  │ my-sui-core = { path = "../forked-sui-core" }      │     │
│  │                                            ↑ Fork   │     │
│  └─────────────────────────────────────────────────────┘     │
│                                                               │
│  src/                                                         │
│  ├─ main.rs (自定义节点入口)                                  │
│  │    ├─ 参考 sui-node/src/main.rs                          │
│  │    ├─ 初始化 AuthorityState (使用 forked sui-core)        │
│  │    ├─ 启动 Consensus                                      │
│  │    ├─ 启动 DEX Engine                                     │
│  │    └─ 启动 RPC 服务                                       │
│  │                                                            │
│  ├─ dex_engine.rs (DEX 撮合引擎)                              │
│  │    └─ 与方案 1 相同的实现                                 │
│  │                                                            │
│  └─ dex_executor.rs (执行器包装)                              │
│       └─ 包装 sui-execution,添加 DEX 路由                    │
└──────────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────────┐
│              forked-sui-core (独立仓库)                       │
│  Fork from: https://github.com/MystenLabs/sui/crates/sui-core│
│                                                               │
│  修改点:                                                      │
│  - authority.rs: 添加 dex_engine 字段                        │
│  - execution_driver.rs: 调用自定义执行器                      │
└──────────────────────────────────────────────────────────────┘
```

### 3.2 依赖选择 (基于探索)

**直接依赖** (无需修改):

| Crate | 用途 | 为何无需修改 |
|-------|------|-------------|
| `sui-types` | 核心类型定义 | 只读,无需扩展 |
| `sui-storage` | 存储层 | 接口稳定,可直接使用 |
| `sui-framework` | Move 标准库 | 系统合约,无需修改 |
| `consensus-*` | Mysticeti 共识 | 黑盒使用 |
| `sui-json-rpc` | JSON-RPC API | 可选,或自定义 RPC |
| `sui-transaction-builder` | 交易构建 | 客户端工具 |

**需要 Fork 的模块**:

| Crate | 为何需要 Fork | 修改内容 |
|-------|--------------|---------|
| `sui-core` | 需要集成 DEX 引擎 | 添加 `dex_engine` 字段,修改执行逻辑 |
| `sui-execution` (可选) | 如需深度定制执行 | 添加 DEX 执行分支 |

### 3.3 实施路径

**步骤 1: 创建新项目 (1 周)**
```bash
# 创建 Cargo 工作区
cargo new my-dex-chain
cd my-dex-chain

# Cargo.toml
[workspace]
members = ["node", "dex-engine"]

[dependencies]
sui-types = { git = "https://github.com/MystenLabs/sui", tag = "mainnet-v1.18.0" }
sui-storage = { git = "https://github.com/MystenLabs/sui", tag = "mainnet-v1.18.0" }
sui-execution = { git = "https://github.com/MystenLabs/sui", tag = "mainnet-v1.18.0" }
sui-framework = { git = "https://github.com/MystenLabs/sui", tag = "mainnet-v1.18.0" }
consensus-core = { git = "https://github.com/MystenLabs/sui", tag = "mainnet-v1.18.0" }

# Fork sui-core
my-sui-core = { path = "../forked-sui-core" }
```

**步骤 2: Fork sui-core (1-2 周)**
```bash
# Fork Sui 仓库
git clone https://github.com/MystenLabs/sui
cd sui
git checkout mainnet-v1.18.0
git checkout -b my-dex-fork

# 只保留 crates/sui-core
git filter-branch --subdirectory-filter crates/sui-core -- --all

# 推送到自己的仓库
git remote add my-org https://github.com/my-org/forked-sui-core
git push my-org my-dex-fork
```

**修改 `forked-sui-core/src/authority.rs`**:
```rust
pub struct AuthorityState {
    // ... 原有字段

    // ✅ 新增
    dex_engine: Option<Arc<dyn DexEngine>>,  // trait object,解耦
}
```

**步骤 3: 实现 DEX 引擎 (4-6 周)**

在新项目中实现 `dex-engine/src/lib.rs`:
```rust
pub trait DexEngine: Send + Sync {
    fn place_order(&self, ...) -> Result<TransactionEffects>;
    fn cancel_order(&self, ...) -> Result<TransactionEffects>;
    fn batch_match(&self, ...) -> Vec<TransactionEffects>;
}

pub struct InMemoryDexEngine {
    orderbooks: DashMap<ObjectID, Arc<Orderbook>>,
}

impl DexEngine for InMemoryDexEngine {
    // 实现与方案 1 相同
}
```

**步骤 4: 自定义节点入口 (2-3 周)**

`node/src/main.rs`:
```rust
use my_sui_core::authority::AuthorityState;
use dex_engine::InMemoryDexEngine;

#[tokio::main]
async fn main() {
    // 1. 加载配置 (参考 sui-node)
    let config = load_config();

    // 2. 创建 DEX 引擎
    let dex_engine = Arc::new(InMemoryDexEngine::new());

    // 3. 创建 AuthorityState (forked sui-core)
    let authority = AuthorityState::new_with_dex_engine(
        config,
        Some(dex_engine.clone()),
    ).await?;

    // 4. 启动共识 (使用 consensus-core)
    let consensus_manager = ConsensusManager::new(...);
    consensus_manager.start().await?;

    // 5. 启动 RPC 服务
    let rpc_server = start_rpc_server(authority.clone());

    // 6. 运行节点
    tokio::select! {
        _ = authority.run() => {},
        _ = tokio::signal::ctrl_c() => {},
    }
}
```

**步骤 5: 集成测试 (2-3 周)**
- 单元测试: DEX 引擎撮合逻辑
- 集成测试: 节点启动 → 交易提交 → 执行 → 持久化
- E2E 测试: 多节点共识 → DEX 交易 → 状态同步

**总计**: 10-15 周 (~2.5-4 月)

---

### 3.4 性能分析

**理论 TPS**:
- 受 Sui 架构约束: ~8,000-10,000 TPS
- Shared objects 必须走共识: 瓶颈

**理论延迟**:
- 下单 (Fastpath): <10ms (如果使用 owned objects)
- 成交 (Consensus): 200-400ms

**优化空间**:
- ✅ 可使用 Fastpath (owned objects)
- ✅ 可实现批量撮合
- ❌ 无法替换共识层 (依赖 consensus-core)
- ❌ 无法深度修改执行调度

---

### 3.5 优缺点总结

#### 优势

- ✅ **开发周期短**: 2.5-4 月,比方案 1 快
- ✅ **易于升级**: 依赖 Sui crates,跟随上游演进
- ✅ **技术风险低**: 大部分功能依赖稳定模块
- ✅ **团队要求低**: 不需要深入理解所有 Sui 模块

#### 劣势

- ❌ **灵活性受限**: 无法深度修改共识层
- ❌ **性能受限**: TPS ~8,000-10,000,低于方案 1
- ❌ **仍需 Fork sui-core**: 维护成本未完全消除
- ❌ **扩展性差**: 难以实现 Sequencer 等深度优化

#### 风险

- ⚠️ **依赖兼容性**: Sui 版本升级时可能破坏 API
- ⚠️ **模块边界**: sui-core 依赖关系复杂,fork 后可能难以集成
- ⚠️ **生态变化**: Sui 架构调整可能导致方案失效

---

## 四、方案 3: 代码复制 (完全独立)

### 4.1 架构设计

**总体架构**:
```
┌──────────────────────────────────────────────────────────────┐
│                 my-dex-chain (完全独立仓库)                    │
│                                                               │
│  从 Sui 复制全部代码,重命名品牌                                │
│                                                               │
│  crates/                                                      │
│  ├─ mydex-types/      (← sui-types)                          │
│  ├─ mydex-core/       (← sui-core)                           │
│  ├─ mydex-storage/    (← sui-storage)                        │
│  ├─ mydex-execution/  (← sui-execution)                      │
│  ├─ mydex-framework/  (← sui-framework)                      │
│  ├─ mydex-node/       (← sui-node)                           │
│  └─ ... (其他 110+ crates)                                    │
│                                                               │
│  consensus/           (← Mysticeti,或替换为自定义共识)         │
│  external-crates/     (← Move 编译器,可选)                    │
│                                                               │
│  完全自主修改,无上游依赖                                       │
└──────────────────────────────────────────────────────────────┘
```

### 4.2 代码复制范围 (基于探索)

**必须复制** (核心功能):

| 目录 | 行数 | 用途 |
|------|------|------|
| `crates/sui-types/` | ~50,000 | 核心类型定义 |
| `crates/sui-core/` | ~80,000 | 验证器核心逻辑 |
| `crates/sui-storage/` | ~20,000 | 存储层 |
| `crates/sui-node/` | ~5,000 | 节点入口 |
| `sui-execution/` | ~30,000 | 执行层 (版本化) |
| `consensus/` | ~50,000 | Mysticeti 共识 |

**可选复制** (生态兼容):

| 目录 | 行数 | 用途 | 是否需要 |
|------|------|------|---------|
| `crates/sui-framework/` | ~15,000 | Move 标准库 | 如需 Move 兼容则需要 |
| `external-crates/move/` | ~200,000 | Move 编译器 | 如需 Move 开发则需要 |
| `crates/sui-json-rpc/` | ~30,000 | JSON-RPC API | 可自定义 RPC |
| `crates/sui-sdk/` | ~10,000 | Rust SDK | 可自定义 SDK |

**总代码量**: ~235,000 行 (必须) + ~255,000 行 (可选) = ~490,000 行

### 4.3 初始工作量

**阶段 1: 代码复制和重命名 (1 周)**
```bash
# 1. Clone Sui 仓库
git clone https://github.com/MystenLabs/sui my-dex-chain
cd my-dex-chain

# 2. 删除不需要的部分
rm -rf apps/ docs/ sdk/

# 3. 批量重命名 (脚本)
find . -type f -name "*.rs" -o -name "*.toml" | xargs sed -i 's/sui::/mydex::/g'
find . -type f -name "*.toml" | xargs sed -i 's/sui-/mydex-/g'

# 4. 更新 Cargo.toml
# 将所有 workspace.dependencies 改为本地路径
```

**阶段 2: 梳理内部依赖 (2 周)**
- 检查所有 `use sui::*` 引用,改为 `use mydex::*`
- 确保编译通过: `cargo build --all`
- 修复版本冲突和依赖循环

**阶段 3: 品牌化和文档 (1 周)**
- 更新 README、LICENSE
- 修改 genesis 配置
- 更新 RPC 接口文档

**阶段 4: 测试验证 (2 周)**
- 单元测试: `cargo test --all`
- 集成测试: 启动本地网络
- E2E 测试: 交易提交到执行

**总计**: 6 周 (~1.5 月)

### 4.4 性能优化空间

**完全自主,无约束**:

1. **替换共识为 Sequencer**:
   - 删除 `consensus/` 目录
   - 实现自定义 Sequencer
   - 预期延迟: <30ms

2. **重写存储层为内存优先**:
   - 使用 Redis/MemCached 替代 RocksDB
   - 异步持久化到磁盘
   - 预期 TPS: 30,000+

3. **优化执行调度器**:
   - 完全并行调度 (无锁)
   - GPU 加速签名验证

4. **自定义序列化格式**:
   - 替换 BCS 为更高效的格式
   - 减少序列化开销

**理论性能上限**:
- **TPS**: 30,000+
- **延迟**: <30ms

### 4.5 实施路径

**快速路径 (基于复制)**:
1. 复制代码 (1 周)
2. 重命名 (1 周)
3. 添加 DEX 引擎 (4-6 周)
4. 测试验证 (2 周)

**深度优化路径 (独立演进)**:
1. 快速路径 (8-9 周)
2. 替换共识为 Sequencer (4-6 周)
3. 优化存储层 (4-6 周)
4. 优化执行调度 (2-4 周)
5. 压力测试和调优 (4-6 周)

**总计**: 22-31 周 (~5.5-8 月)

### 4.6 优缺点总结

#### 优势

- ✅ **完全自主**: 无上游依赖,任意修改
- ✅ **极高灵活性**: 可重写任何模块
- ✅ **无维护成本**: 不需要同步上游更新
- ✅ **性能潜力最高**: 理论 TPS 30,000+, 延迟 <30ms

#### 劣势

- ❌ **与 Sui 生态脱钩**: 无法享受 Sui 工具链
- ❌ **无上游改进**: 错过 Sui 的 bug 修复和性能优化
- ❌ **Move 兼容性差**: 难以吸引 Sui Move 开发者
- ❌ **初始工作量巨大**: 6 周复制 + 5.5-8 月优化

#### 风险

- ⚠️ **技术债务累积**: 长期无上游同步,代码老化
- ⚠️ **安全漏洞**: 上游修复的漏洞不会自动同步
- ⚠️ **招聘困难**: 非标准技术栈,人才稀缺
- ⚠️ **生态孤立**: 无法融入 Sui 生态,难以获得社区支持

---

## 五、方案对比

### 5.1 对比矩阵

| 维度 | 方案 1: Fork Sui | 方案 2: 依赖集成 | 方案 3: 代码复制 |
|------|-----------------|-----------------|-----------------|
| **开发成本** | 高 (5-8 月) | 中 (2.5-4 月) | 极高 (6.5-9 月) |
| **维护成本** | 高 (~1 周/月同步) | 中 (~2 天/月升级) | 低 (无上游同步) |
| **灵活性** | 极高 (任意修改) | 中 (受依赖约束) | 极高 (完全自主) |
| **性能潜力** | 极高 (20,000+ TPS) | 中 (8,000-10,000 TPS) | 极高 (30,000+ TPS) |
| **生态兼容** | 高 (保持协议一致) | 高 (使用 Sui crates) | 低 (完全独立) |
| **技术风险** | 中 (分叉风险) | 低 (依赖稳定模块) | 高 (技术债务) |
| **团队要求** | 高 (深入理解 Sui) | 中 (理解接口) | 极高 (自主维护) |
| **上游受益** | 部分 (手动同步) | 完全 (依赖升级) | 无 (完全脱钩) |

### 5.2 性能对比

| 方案 | 预估 TPS | 预估延迟 | 瓶颈分析 | 优化空间 |
|------|---------|---------|---------|---------|
| **方案 1: Fork Sui** | 15,000-20,000 | <50ms (Sequencer) | Mysticeti 共识 | 可替换共识,批量执行 |
| **方案 2: 依赖集成** | 8,000-10,000 | 200-400ms | Sui 架构约束 | 只能 Fastpath 优化 |
| **方案 3: 代码复制** | 20,000-30,000 | <30ms | 无约束 | 完全自主优化 |

**基准参考** (对标 CEX):
- Hyperliquid: ~100,000 TPS, ~10ms 延迟 (中心化)
- dYdX V4: ~10,000 TPS, ~1s 延迟 (Cosmos SDK)
- Sui 原生: ~5,000 TPS, ~400ms 延迟

### 5.3 成本对比

| 成本项 | 方案 1 | 方案 2 | 方案 3 |
|-------|--------|--------|--------|
| **初始开发** | 5-8 月 | 2.5-4 月 | 6.5-9 月 |
| **年度维护** | 12 周 | 4 周 | 0 周 (但技术债务累积) |
| **团队规模** | 3-5 人 | 2-3 人 | 5-8 人 |
| **技术专家** | 需要 Sui 核心开发者 | 需要 Rust 开发者 | 需要区块链架构师 |

### 5.4 风险对比

| 风险项 | 方案 1 | 方案 2 | 方案 3 |
|-------|--------|--------|--------|
| **上游分叉** | 高 (协议升级冲突) | 低 (依赖兼容性) | 无 (完全独立) |
| **安全漏洞** | 中 (手动同步修复) | 低 (依赖自动修复) | 高 (无上游修复) |
| **生态脱钩** | 低 (保持兼容) | 低 (使用 Sui crates) | 高 (完全孤立) |
| **技术债务** | 中 (定期同步) | 低 (依赖升级) | 高 (长期累积) |
| **招聘困难** | 中 (Sui 开发者) | 低 (Rust 开发者) | 高 (非标技术栈) |

---

## 六、推荐方案

### 6.1 推荐策略: 混合方案 (方案 2 → 方案 1)

**理由**:
- **阶段 1**: 使用方案 2 (依赖集成) 快速验证 POC,验证性能瓶颈
- **阶段 2**: 如果性能不达标,升级到方案 1 (Fork Sui) 深度优化
- **平衡**: 开发速度 vs 性能目标

### 6.2 实施路线图

#### 阶段 1: POC 验证 (2.5-4 月)

**采用方案 2 (依赖集成)**:

**目标**:
- 验证 DEX 核心功能可行性
- 测量实际性能瓶颈
- 评估是否需要深度优化

**里程碑**:
1. **Week 1-2**: 创建新项目,配置依赖
2. **Week 3-6**: Fork sui-core,集成 DEX 引擎
3. **Week 7-10**: 实现内存订单簿和撮合算法
4. **Week 11-12**: 集成测试,性能基准测试
5. **Week 13-14**: 文档和 demo
6. **Week 15-16**: 压力测试,评估性能

**决策点**: 如果 POC 性能达到 8,000-10,000 TPS, 延迟 <200ms,可继续方案 2;否则升级到方案 1

---

#### 阶段 2: 性能优化 (如需要,3-4 月)

**升级到方案 1 (Fork Sui)**:

**目标**:
- 达到 15,000+ TPS
- 延迟降低到 <50ms
- 实现 Sequencer 或深度优化共识

**里程碑**:
1. **Week 17-20**: Fork 完整 Sui 仓库,迁移 POC 代码
2. **Week 21-24**: 修改 TransactionKind,集成 DEX 引擎
3. **Week 25-28**: 实现批量执行优化 (ConsensusHandler 层)
4. **Week 29-32**: 实现 Sequencer (可选) 或优化共识
5. **Week 33-36**: 优化执行调度器 (并行队列)
6. **Week 37-40**: 压力测试,性能调优

**目标指标**:
- TPS: 15,000+
- 延迟: <50ms (Sequencer) 或 <200ms (优化共识)

---

#### 阶段 3: 生产就绪 (3-6 月)

**目标**:
- 安全审计
- 主网部署
- 运维工具

**里程碑**:
1. **Week 41-44**: Checkpoint 集成测试
2. **Week 45-48**: Epoch 切换测试,边界情况覆盖
3. **Week 49-52**: 安全审计 (内部 + 外部)
4. **Week 53-56**: 测试网部署,社区测试
5. **Week 57-64**: 主网部署准备,运维工具开发
6. **Week 65+**: 主网启动,持续监控

---

### 6.3 备选方案

**如果追求极致性能** → 方案 3 (代码复制):
- 适用于: TPS 需求 >30,000, 延迟 <30ms
- 权衡: 与 Sui 生态完全脱钩,技术债务高

**如果追求快速上线** → 坚持方案 2 (依赖集成):
- 适用于: TPS 需求 8,000-10,000, 延迟 <200ms 可接受
- 权衡: 性能受限,难以深度优化

---

## 七、关键技术决策

### 7.1 DEX 引擎设计

**决策 1: Rust 原生实现 vs Move 合约**

| 方案 | 优势 | 劣势 | 推荐 |
|------|------|------|------|
| **Rust 原生** | 极高性能,绕过 VM | 升级需要节点硬分叉 | ✅ 推荐 (性能优先) |
| **Move 合约** | 可升级,生态兼容 | 性能受 VM 限制 | ❌ 不推荐 (性能不足) |

**决策 2: 订单簿内存结构**

推荐使用 **lock-free 数据结构**:
```rust
use crossbeam_skiplist::SkipMap;

pub struct Orderbook {
    bids: SkipMap<Price, VecDeque<Order>>,  // Lock-free
    asks: SkipMap<Price, VecDeque<Order>>,
}
```

**理由**:
- 避免锁竞争,支持高并发
- 价格排序天然支持 (SkipMap)
- 性能优于 BTreeMap + RwLock

**决策 3: 撮合算法**

推荐 **Price-Time Priority (价格-时间优先)**:
1. 买单从最高价开始撮合
2. 卖单从最低价开始撮合
3. 同价格按时间戳排序

**实现**:
```rust
impl Orderbook {
    fn match_buy(&self, max_price: u64, quantity: u64) -> Vec<Match> {
        let mut matches = Vec::new();
        let mut remaining = quantity;

        // 从 asks (卖单) 中寻找匹配,价格从低到高
        for entry in self.asks.range(..=max_price) {
            let (price, orders) = entry.pair();
            for order in orders.iter() {
                let match_qty = remaining.min(order.remaining_quantity());
                matches.push(Match { price: *price, quantity: match_qty });
                remaining -= match_qty;
                if remaining == 0 {
                    return matches;
                }
            }
        }
        matches
    }
}
```

---

### 7.2 共识优化

**决策 1: 保留 Mysticeti vs 替换 Sequencer**

| 方案 | 延迟 | TPS | 去中心化 | 推荐 |
|------|------|-----|---------|------|
| **Mysticeti (保留)** | 200-400ms | 5,000-10,000 | 完全去中心化 | ✅ 阶段 1 推荐 |
| **Sequencer (替换)** | <50ms | 20,000+ | 中心化 | ⚠️ 阶段 2 可选 |
| **混合模式** | <50ms (排序) + 批量共识 | 15,000+ | 部分去中心化 | ✅ 阶段 2 推荐 |

**混合模式架构**:
```
用户提交交易
    ↓
Sequencer (中心化排序,<50ms)
    ↓
批量聚合 (每 1000 笔或 10 秒)
    ↓
Mysticeti Consensus (最终性保证)
    ↓
执行和持久化
```

**优势**:
- 低延迟: Sequencer 立即返回排序结果
- 高 TPS: Sequencer 吞吐量无限制
- 去中心化: 定期批量提交到共识,保证最终性

**风险缓解**:
- Sequencer 容错: 多个 Sequencer 备份,主从切换
- 审计透明: Sequencer 日志公开,可验证

---

### 7.3 存储优化

**决策 1: 订单数据存储策略**

推荐 **内存索引 + 定期 checkpoint**:

```rust
pub struct DexStorage {
    // 内存索引 (热数据)
    active_orders: DashMap<OrderID, Order>,

    // 持久化层 (冷数据)
    rocksdb: Arc<RocksDB>,

    // Checkpoint 机制
    checkpoint_interval: Duration,
}

impl DexStorage {
    // 订单创建: 写入内存
    pub fn insert_order(&self, order: Order) {
        self.active_orders.insert(order.order_id, order);
    }

    // 订单成交: 从内存删除,异步持久化
    pub fn fulfill_order(&self, order_id: OrderID) {
        if let Some((_, order)) = self.active_orders.remove(&order_id) {
            tokio::spawn(async move {
                self.rocksdb.put(order_id, order).await;
            });
        }
    }

    // 定期 checkpoint: 批量写入磁盘
    pub async fn checkpoint(&self) {
        let snapshot: Vec<_> = self.active_orders.iter()
            .map(|entry| entry.clone())
            .collect();

        // 批量写入 RocksDB
        self.rocksdb.write_batch(snapshot).await;
    }
}
```

**优势**:
- 读写全在内存,极低延迟
- 批量持久化,减少 I/O
- Checkpoint 保证数据不丢失

**决策 2: 历史数据归档**

推荐 **冷热分离**:
- 热数据 (近 7 天): 内存 + RocksDB
- 冷数据 (>7 天): 归档到对象存储 (S3/OSS)

---

## 八、总结

### 8.1 三种方案总结

| 方案 | 适用场景 | 核心优势 | 核心劣势 |
|------|---------|---------|---------|
| **方案 1: Fork Sui** | 追求极致性能,可接受高维护成本 | 最大灵活性,性能潜力 20,000+ TPS | 维护成本高,分叉风险 |
| **方案 2: 依赖集成** | 快速上线,性能要求适中 | 开发快,易升级,风险低 | 性能受限 8,000-10,000 TPS |
| **方案 3: 代码复制** | 完全独立演进,无生态依赖 | 完全自主,性能潜力 30,000+ TPS | 生态脱钩,技术债务高 |

### 8.2 推荐方案

**推荐采用混合策略**: **方案 2 (依赖集成) → 方案 1 (Fork Sui)**

**理由**:
1. **阶段 1** (2.5-4 月): 使用方案 2 快速验证 POC
   - 低风险,快速验证技术可行性
   - 测量实际性能瓶颈

2. **阶段 2** (3-4 月): 如需要,升级到方案 1
   - 实现 Sequencer 或深度优化共识
   - 达到 15,000+ TPS, <50ms 延迟

3. **阶段 3** (3-6 月): 生产就绪
   - 安全审计,主网部署

**总周期**: 8.5-14 月

### 8.3 关键成功因素

- ✅ **技术团队**: 需要 Sui 核心开发经验 (至少 1 人)
- ✅ **性能目标**: 明确 TPS 和延迟目标,决定方案选择
- ✅ **生态兼容**: 评估是否需要与 Sui 生态兼容
- ✅ **维护能力**: 评估团队是否有能力长期维护 fork

### 8.4 风险提示

- ⚠️ **上游分叉风险**: Sui 协议升级可能导致冲突
- ⚠️ **性能不达预期**: 即使 fork,Mysticeti 共识仍是瓶颈
- ⚠️ **安全漏洞**: 自定义代码可能引入漏洞
- ⚠️ **生态孤立**: 过度定制可能脱离 Sui 生态

---

**文档生成时间**: 2026-01-13
**基于 Sui 版本**: mainnet-v1.18.0 (commit `8eeacbe6fc`)
**代码探索文件数**: 15+ 核心源文件
**分析完整度**: 代码级分析,包含行号引用
