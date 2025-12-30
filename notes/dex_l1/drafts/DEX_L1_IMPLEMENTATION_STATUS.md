# DEX L1 区块链实现状态

> 基于 Sui Fork 的高性能 DEX Layer 1 区块链

## 项目目标

| 指标 | 目标值 | 实现方式 |
|-----|-------|---------|
| 撮合延迟 | < 50ms | 中心化 Sequencer + 原生 Rust 引擎 |
| 吞吐量 | 100,000 TPS | 绕过 Move VM，原生撮合 |
| 软确认 | < 100ms | Sequencer 直接确认 |
| 硬确认 | < 500ms | 2f+1 验证者签名 |

---

## 已完成工作

### Phase 1: 核心基础设施 ✅ 完成

#### 1. dex-types (6 tests)
**路径**: `crates/dex-types/`

核心类型定义:
- `Order` - 订单结构 (id, account, market, side, price, quantity, etc.)
- `Trade` - 成交记录
- `Balance` - 账户余额 (available, locked)
- `Position` - 持仓信息 (永续合约)
- `Market` - 市场配置
- `DexEvent` - 事件类型 (OrderPlaced, OrderCancelled, TradeExecuted, etc.)
- `Price`, `Quantity` - 定点数类型
- `Side`, `OrderType`, `TimeInForce`, `OrderStatus` - 枚举类型

```rust
// 核心结构示例
pub struct Order {
    pub id: OrderId,
    pub account: AccountId,
    pub market: MarketId,
    pub side: Side,
    pub price: Price,
    pub quantity: Quantity,
    pub filled_quantity: Quantity,
    pub order_type: OrderType,
    pub time_in_force: TimeInForce,
    pub status: OrderStatus,
    pub sequence: SequenceNumber,
    pub timestamp: Timestamp,
    // ...
}
```

---

#### 2. dex-engine (39 tests)
**路径**: `crates/dex-engine/`

原生 Rust 撮合引擎:

**核心组件**:
- `DexEngine` - 主引擎，管理多个市场
- `Orderbook` - 价格-时间优先订单簿 (BTreeMap)
- `PriceLevel` - 价格层级，维护同价格订单队列

**关键方法**:
```rust
impl DexEngine {
    pub fn new(event_buffer_size: usize) -> Self;
    pub fn register_market(&self, market: Market) -> DexResult<()>;
    pub fn place_order(...) -> DexResult<(Order, Vec<Trade>)>;
    pub fn cancel_order(&self, market_id: &str, order_id: OrderId) -> DexResult<Order>;
    pub fn deposit(&self, account: &AccountId, asset: &str, amount: u64);
    pub fn withdraw(&self, account: &AccountId, asset: &str, amount: u64) -> DexResult<()>;
    pub fn get_orderbook(&self, market_id: &str) -> DexResult<OrderbookSnapshot>;
    pub fn get_balance(&self, account: &AccountId, asset: &str) -> Balance;
}
```

**数据结构**:
```
Orderbook
├── bids: BTreeMap<BidPrice, PriceLevel>  // 买单 (降序)
├── asks: BTreeMap<AskPrice, PriceLevel>  // 卖单 (升序)
└── order_index: HashMap<OrderId, OrderLocation>  // O(1) 查找

PriceLevel
├── price: Price
├── orders: VecDeque<Order>  // FIFO 队列
└── total_quantity: Quantity
```

---

#### 3. dex-sequencer (19 tests)
**路径**: `crates/dex-sequencer/`

中心化交易排序器:

**核心组件**:
- `DexSequencer` - 主排序器
- `SequenceBatch` - 批量交易
- `Committee` - 验证者委员会
- `LeaderSchedule` - Leader 选举调度

**关键方法**:
```rust
impl DexSequencer {
    pub fn new(config: SequencerConfig) -> (Self, Receiver<SequenceRequest>);
    pub fn become_leader(&self, starting_sequence: SequenceNumber);
    pub fn step_down(&self);
    pub fn is_leader(&self) -> bool;
    pub async fn submit_transaction(&self, tx: DexTransaction) -> Result<SequenceResponse>;
    pub async fn run_sequencer_loop(&self, rx: Receiver, shutdown: Receiver);
    pub fn subscribe_batches(&self) -> broadcast::Receiver<SequenceBatch>;
}
```

**交易类型**:
```rust
pub enum DexTransaction {
    PlaceOrder { account, market, side, price, quantity, ... },
    CancelOrder { account, order_id },
    CancelAllOrders { account, market },
    Deposit { account, asset, amount, deposit_tx_ref },
    Withdraw { account, asset, amount, destination },
}
```

**配置**:
```rust
pub struct SequencerConfig {
    pub max_batch_size: usize,        // 默认 1000
    pub max_batch_latency: Duration,  // 默认 10ms
    pub tx_buffer_size: usize,        // 默认 100,000
    pub heartbeat_interval: Duration, // 默认 25ms
    pub heartbeat_timeout: Duration,  // 默认 50ms
}
```

---

#### 4. dex-storage (18 tests)
**路径**: `crates/dex-storage/`

持久化存储层:

**核心组件**:
- `DexStorage` - 主存储接口
- `WalWriter` - Write-Ahead Log 写入器
- `SnapshotManager` - 快照管理器
- `StateCache` - 内存状态缓存 (DashMap)

**WAL 条目类型**:
```rust
pub enum WalEntry {
    OrderAdded { sequence, order },
    OrderRemoved { sequence, order_id, market },
    OrderUpdated { sequence, order },
    TradeExecuted { sequence, trade },
    BalanceUpdated { sequence, account, asset, available, locked },
    MarketAdded { sequence, market },
    Checkpoint { sequence },
}
```

**关键方法**:
```rust
impl DexStorage {
    pub fn new(config: StorageConfig) -> Result<Self>;
    pub fn append(&self, entry: WalEntry) -> Result<()>;
    pub fn flush(&self) -> Result<()>;
    pub fn recover(&self) -> Result<SequenceNumber>;
    pub fn cache(&self) -> Arc<StateCache>;
}
```

**快照结构**:
```rust
pub struct StateSnapshot {
    pub sequence: SequenceNumber,
    pub timestamp: u64,
    pub markets: HashMap<MarketId, Market>,
    pub orders: HashMap<OrderId, Order>,
    pub balances: HashMap<(AccountId, String), Balance>,
    pub positions: HashMap<(AccountId, MarketId), Position>,
}
```

---

#### 5. dex-integration (19 tests)
**路径**: `crates/dex-integration/`

Sui Authority 集成层:

**核心组件**:
- `DexService` - 主服务，协调 Engine + Sequencer + Storage
- `DexServiceHandle` - 异步操作句柄
- `DexTransactionClassifier` - 交易分类器
- `DexPrecompile` - Move VM 桥接

**服务架构**:
```rust
pub struct DexService {
    config: DexConfig,
    engine: Arc<RwLock<DexEngine>>,
    sequencer: Arc<DexSequencer>,
    storage: Arc<DexStorage>,
    state: Arc<ServiceState>,
}

impl DexService {
    pub fn new(config: DexConfig) -> DexResult<Self>;
    pub fn start(self) -> DexServiceHandle;
}

impl DexServiceHandle {
    pub async fn execute(&self, request: DexTransactionRequest) -> DexResult<DexExecutionResult>;
    pub async fn stats(&self) -> DexResult<DexStats>;
    pub async fn shutdown(&self) -> DexResult<()>;
}
```

**交易分类**:
```rust
pub enum TransactionType {
    DexOrder,    // 原生 DEX 路径 (< 50ms)
    DexTransfer, // 混合路径 (< 100ms)
    Standard,    // Mysticeti 共识 (~600ms)
}

impl DexTransactionClassifier {
    pub fn classify(&self, module: &str, function: &str) -> TransactionType;
    pub fn requires_native_path(&self, module: &str, function: &str) -> bool;
}
```

**Precompile 函数**:
```rust
pub enum DexPrecompileFunction {
    PlaceOrder = 1,
    CancelOrder = 2,
    PlaceMarketOrder = 3,
    ModifyOrder = 4,
    GetOrder = 5,
    GetOrderbook = 6,
    GetBalance = 7,
    GetPosition = 8,
    Deposit = 9,
    Withdraw = 10,
}
```

---

## 测试统计

| Crate | 测试数量 | 状态 |
|-------|---------|-----|
| dex-types | 6 | ✅ 全部通过 |
| dex-engine | 39 | ✅ 全部通过 |
| dex-sequencer | 19 | ✅ 全部通过 |
| dex-storage | 18 | ✅ 全部通过 |
| dex-integration | 19 | ✅ 全部通过 |
| **总计** | **101** | ✅ |

运行测试:
```bash
cargo test -p dex-types -p dex-engine -p dex-sequencer -p dex-storage -p dex-integration
```

---

## 待完成工作

### Phase 2: Sui Authority 集成 🔲 待开始

#### 2.1 修改 sui-core/src/authority.rs

**目标**: 添加 DEX 交易路由逻辑

```rust
// 需要添加的代码
impl AuthorityState {
    /// 检测是否为 DEX 交易
    fn is_dex_transaction(&self, tx: &Transaction) -> bool {
        let classifier = DexTransactionClassifier::new();
        // 解析交易中的 Move 调用
        for command in tx.commands() {
            if let Command::MoveCall(call) = command {
                if classifier.requires_native_path(&call.module, &call.function) {
                    return true;
                }
            }
        }
        false
    }

    /// 处理交易 - 添加 DEX 分流
    pub async fn handle_transaction(&self, tx: Transaction) -> Result<TransactionEffects> {
        if self.is_dex_transaction(&tx) {
            // DEX 原生路径
            return self.handle_dex_transaction(tx).await;
        }

        // 标准 Mysticeti 共识路径
        self.handle_consensus_transaction(tx).await
    }

    /// DEX 交易处理
    async fn handle_dex_transaction(&self, tx: Transaction) -> Result<TransactionEffects> {
        let request = self.parse_dex_request(&tx)?;
        let result = self.dex_service.execute(request).await?;
        self.create_effects_from_dex_result(result)
    }
}
```

**修改文件**:
- `crates/sui-core/src/authority.rs`
- `crates/sui-core/src/authority/mod.rs`

---

#### 2.2 修改 sui-execution

**目标**: 添加 DEX Precompile 钩子

```rust
// sui-execution/latest/sui-adapter/src/execution_engine.rs

impl ExecutionEngine {
    pub fn execute_transaction_to_effects(&self, tx: &Transaction) -> Result<Effects> {
        // 检查是否为 DEX precompile 调用
        if let Some(precompile_call) = self.extract_dex_precompile(tx) {
            return self.execute_dex_precompile(precompile_call);
        }

        // 标准 Move VM 执行
        self.execute_move(tx)
    }

    fn execute_dex_precompile(&self, call: PrecompileCall) -> Result<Effects> {
        let precompile = DexPrecompile::new(self.dex_engine.clone());
        let result = precompile.execute(call.args);
        self.convert_precompile_result(result)
    }
}
```

**修改文件**:
- `sui-execution/latest/sui-adapter/src/execution_engine.rs`
- `sui-execution/latest/sui-adapter/src/programmable_transactions/execution.rs`

---

#### 2.3 修改 sui-node

**目标**: 初始化 DEX 服务

```rust
// crates/sui-node/src/lib.rs

impl SuiNode {
    pub async fn start(config: NodeConfig) -> Result<Self> {
        // ... 现有初始化代码 ...

        // 初始化 DEX 服务
        let dex_service = if config.dex.enabled {
            let dex_config = DexConfig::from(&config.dex);
            let service = DexService::new(dex_config)?;
            Some(service.start())
        } else {
            None
        };

        // 将 DEX 服务注入 Authority
        authority.set_dex_service(dex_service);

        // ... 其余代码 ...
    }
}
```

**修改文件**:
- `crates/sui-node/src/lib.rs`
- `crates/sui-config/src/node.rs` (添加 DexConfig)

---

### Phase 3: Move Framework 🔲 待开始

#### 3.1 创建 dex-framework Move 包

**路径**: `crates/sui-framework/packages/dex-framework/`

```move
// sources/orderbook.move
module dex::orderbook {
    use sui::object::{Self, UID};
    use sui::tx_context::TxContext;

    /// 下单 - 调用原生 precompile
    public entry fun place_order(
        market: &Market,
        side: u8,
        price: u64,
        quantity: u64,
        order_type: u8,
        ctx: &mut TxContext
    ) {
        // 触发 DEX precompile
        native_place_order(market.id, side, price, quantity, order_type, ctx)
    }

    /// 取消订单
    public entry fun cancel_order(
        market: &Market,
        order_id: vector<u8>,
        ctx: &mut TxContext
    ) {
        native_cancel_order(market.id, order_id, ctx)
    }

    // Native 函数声明
    native fun native_place_order(
        market_id: vector<u8>,
        side: u8,
        price: u64,
        quantity: u64,
        order_type: u8,
        ctx: &mut TxContext
    );

    native fun native_cancel_order(
        market_id: vector<u8>,
        order_id: vector<u8>,
        ctx: &mut TxContext
    );
}
```

```move
// sources/perpetual.move
module dex::perpetual {
    /// 开仓
    public entry fun open_position(
        market: &Market,
        side: u8,
        size: u64,
        leverage: u8,
        ctx: &mut TxContext
    );

    /// 平仓
    public entry fun close_position(
        market: &Market,
        position_id: vector<u8>,
        ctx: &mut TxContext
    );

    /// 添加保证金
    public entry fun add_margin(
        market: &Market,
        position_id: vector<u8>,
        amount: u64,
        ctx: &mut TxContext
    );
}
```

```move
// sources/vault.move
module dex::vault {
    use sui::coin::Coin;
    use sui::balance::Balance;

    /// DEX 资金托管
    struct Vault<phantom T> has key {
        id: UID,
        balance: Balance<T>,
    }

    /// 存款
    public entry fun deposit<T>(
        vault: &mut Vault<T>,
        coin: Coin<T>,
        ctx: &mut TxContext
    );

    /// 取款
    public entry fun withdraw<T>(
        vault: &mut Vault<T>,
        amount: u64,
        ctx: &mut TxContext
    ): Coin<T>;
}
```

---

### Phase 4: 永续合约 🔲 待开始

#### 4.1 创建 dex-perpetuals crate

**路径**: `crates/dex-perpetuals/`

```rust
// src/lib.rs
pub struct PerpetualEngine {
    positions: DashMap<(AccountId, MarketId), Position>,
    funding_rates: DashMap<MarketId, FundingRate>,
    insurance_fund: DashMap<String, u64>,
}

impl PerpetualEngine {
    /// 开仓
    pub fn open_position(
        &self,
        account: AccountId,
        market: MarketId,
        side: Side,
        size: Quantity,
        leverage: u8,
        entry_price: Price,
    ) -> DexResult<Position>;

    /// 平仓
    pub fn close_position(
        &self,
        account: AccountId,
        market: MarketId,
        size: Quantity,
        exit_price: Price,
    ) -> DexResult<(Position, i64)>; // (updated position, pnl)

    /// 计算资金费率
    pub fn calculate_funding_rate(&self, market: &MarketId) -> FundingRate;

    /// 应用资金费率
    pub fn apply_funding(&self, market: &MarketId, timestamp: Timestamp) -> Vec<FundingPayment>;

    /// 检查清算
    pub fn check_liquidations(&self, market: &MarketId, mark_price: Price) -> Vec<Liquidation>;

    /// 执行清算
    pub fn liquidate(&self, position: &Position, mark_price: Price) -> DexResult<LiquidationResult>;
}
```

**资金费率**:
```rust
pub struct FundingRate {
    pub market: MarketId,
    pub rate: i64,           // 基点 (可正可负)
    pub timestamp: Timestamp,
    pub next_funding_time: Timestamp,
}

// 计算公式
// Funding Rate = Premium + Clamp(Interest - Premium, -0.05%, 0.05%)
// Premium = (Mark Price - Index Price) / Index Price
```

**清算逻辑**:
```rust
pub struct LiquidationEngine {
    maintenance_margin_rate: u64,  // 0.5% = 50 bps
    liquidation_penalty: u64,      // 1% = 100 bps
}

impl LiquidationEngine {
    pub fn is_liquidatable(&self, position: &Position, mark_price: Price) -> bool {
        let margin_ratio = self.calculate_margin_ratio(position, mark_price);
        margin_ratio < self.maintenance_margin_rate
    }

    pub fn execute_liquidation(&self, position: Position, mark_price: Price) -> LiquidationResult {
        // 1. 计算剩余保证金
        // 2. 扣除清算惩罚
        // 3. 剩余归还用户，不足由保险基金补充
    }
}
```

---

### Phase 5: 高可用和生产加固 🔲 待开始

#### 5.1 Sequencer 故障切换

```rust
// crates/dex-sequencer/src/failover.rs

pub struct SequencerFailover {
    heartbeat_timeout: Duration,   // 50ms
    detection_window: Duration,    // 100ms
    leader_schedule: LeaderSchedule,
}

impl SequencerFailover {
    /// 监控 Leader 心跳
    pub async fn monitor_leader(&self) {
        loop {
            let leader = self.leader_schedule.current_leader();

            if !self.received_heartbeat(leader, self.heartbeat_timeout).await {
                // 广播故障检测
                self.broadcast_failure(leader).await;

                // 收集 2f+1 确认
                if self.collect_votes(leader).await >= self.quorum() {
                    self.switch_leader(leader).await;
                }
            }

            sleep(self.heartbeat_timeout / 2).await;
        }
    }

    /// 切换 Leader
    async fn switch_leader(&self, failed: ValidatorId) {
        let new_leader = self.leader_schedule.next_leader(failed);
        let last_seq = self.fetch_last_sequence_from_da().await;

        if self.is_me(new_leader) {
            self.sequencer.become_leader(last_seq + 1);
        }

        self.broadcast_leader_change(new_leader).await;
    }
}
```

#### 5.2 性能优化

- [ ] 订单簿内存布局优化 (cache-friendly)
- [ ] 批量撮合优化
- [ ] 零拷贝序列化
- [ ] SIMD 价格比较
- [ ] 预分配内存池

#### 5.3 监控和指标

```rust
pub struct DexMetrics {
    // 延迟指标
    pub order_latency_p50: Histogram,
    pub order_latency_p99: Histogram,
    pub matching_latency: Histogram,

    // 吞吐量指标
    pub orders_per_second: Counter,
    pub trades_per_second: Counter,
    pub batches_per_second: Counter,

    // 状态指标
    pub open_orders: Gauge,
    pub active_markets: Gauge,
    pub sequencer_queue_depth: Gauge,
}
```

---

## 文件结构

```
crates/
├── dex-types/              ✅ 完成
│   ├── Cargo.toml
│   └── src/
│       └── lib.rs          # 核心类型定义
│
├── dex-engine/             ✅ 完成
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs          # DexEngine 主逻辑
│       └── orderbook.rs    # 订单簿实现
│
├── dex-sequencer/          ✅ 完成
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs          # DexSequencer
│       ├── transaction.rs  # DexTransaction
│       └── leader.rs       # Leader 选举
│
├── dex-storage/            ✅ 完成
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs          # DexStorage
│       ├── wal.rs          # Write-Ahead Log
│       ├── snapshot.rs     # 快照管理
│       └── state.rs        # 状态缓存
│
├── dex-integration/        ✅ 完成
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs          # 错误类型和结果
│       ├── service.rs      # DexService
│       ├── config.rs       # DexConfig
│       ├── transaction.rs  # 交易分类
│       └── precompile.rs   # Move 桥接
│
├── dex-perpetuals/         🔲 待实现
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── funding.rs
│       └── liquidation.rs
│
└── dex-framework/          🔲 待实现
    └── sources/
        ├── orderbook.move
        ├── perpetual.move
        └── vault.move
```

---

## 依赖关系

```
dex-integration
├── dex-engine
│   └── dex-types
├── dex-sequencer
│   └── dex-types
├── dex-storage
│   └── dex-types
└── dex-types
```

---

## 运行命令

```bash
# 构建所有 DEX crate
cargo build -p dex-types -p dex-engine -p dex-sequencer -p dex-storage -p dex-integration

# 运行所有测试
cargo test -p dex-types -p dex-engine -p dex-sequencer -p dex-storage -p dex-integration

# 检查代码
cargo check -p dex-integration

# 格式化
cargo fmt --all
```

---

## 参考文档

- [Sui 架构文档](https://docs.sui.io/concepts/sui-architecture)
- [Mysticeti 共识](https://arxiv.org/abs/2310.14821)
- [原始设计方案](./witty-floating-hippo.md)

---

*最后更新: 2025-12-30*
