# DEX 基于 Sui 技术方案设计文档

> **版本**: v1.0  
> **日期**: 2025-01-XX  
> **状态**: Draft  
> **设计方法**: Architect-driven  
> **参考**: [`mynotes/plan/dex_use_sui_plan_cursor.md`](../../../plan/dex_use_sui_plan_cursor.md)

---

## 📋 目录

1. [技术栈选择](#1-技术栈选择)
2. [代码结构设计](#2-代码结构设计)
3. [核心算法设计](#3-核心算法设计)
4. [数据结构设计](#4-数据结构设计)
5. [性能优化方案](#5-性能优化方案)
6. [安全方案设计](#6-安全方案设计)
7. [测试策略](#7-测试策略)
8. [实施细节](#8-实施细节)

---

## 1. 技术栈选择

### 1.1 核心技术栈

| 组件 | 技术选型 | 版本 | 理由 |
|-----|---------|------|------|
| **语言** | Rust | 1.75+ | 性能、内存安全、并发 |
| **异步运行时** | Tokio | 1.35+ | 高性能异步 I/O |
| **序列化** | BCS | - | Sui 标准，确定性 |
| **数据库** | RocksDB | 8.0+ | 高性能 KV 存储 |
| **网络框架** | Anemo | - | Sui P2P 网络（复用） |
| **RPC 框架** | Tonic | 0.10+ | gRPC 实现 |
| **并发数据结构** | DashMap | 5.5+ | 分片锁 HashMap |
| **序列化库** | serde | 1.0+ | 通用序列化 |

### 1.2 Sui 依赖版本

```toml
[dependencies]
sui-core = { git = "https://github.com/your-org/sui-dex-fork.git", branch = "dex-fork" }
sui-types = { git = "https://github.com/your-org/sui-dex-fork.git", branch = "dex-fork" }
sui-execution = { git = "https://github.com/your-org/sui-dex-fork.git", branch = "dex-fork" }
typed-store = { git = "https://github.com/your-org/sui-dex-fork.git", branch = "dex-fork" }
mysten-network = { git = "https://github.com/your-org/sui-dex-fork.git", branch = "dex-fork" }
```

### 1.3 开发工具

- **构建工具**: Cargo
- **代码格式化**: rustfmt
- **Lint**: clippy
- **测试框架**: Rust 标准库 + proptest
- **性能分析**: perf, flamegraph
- **内存分析**: valgrind, heaptrack

---

## 2. 代码结构设计

### 2.1 项目结构

```
dex-l1/
├── Cargo.toml                    # Workspace 配置
├── README.md
├── crates/
│   ├── dex-sequencer/           # 定序器
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── gateway.rs       # 订单网关
│   │   │   ├── sequencer.rs     # 核心定序逻辑
│   │   │   ├── publisher.rs     # 序列发布
│   │   │   ├── network.rs       # 网络层封装
│   │   │   └── config.rs        # 配置
│   │   └── tests/
│   │
│   ├── dex-engine/              # 撮合引擎
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── orderbook.rs     # 订单簿
│   │   │   ├── matcher.rs       # 撮合算法
│   │   │   ├── balance.rs       # 余额管理
│   │   │   ├── risk.rs          # 风险检查
│   │   │   └── types.rs         # 类型定义
│   │   └── tests/
│   │
│   ├── dex-storage/             # 存储层
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── memory.rs        # 内存缓存
│   │   │   ├── wal.rs           # WAL 日志
│   │   │   ├── snapshot.rs      # 快照管理
│   │   │   ├── rocksdb.rs       # RocksDB 封装
│   │   │   └── store.rs         # 统一存储接口
│   │   └── tests/
│   │
│   ├── dex-precompile/          # Precompile 钩子
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── router.rs        # 路由逻辑
│   │   │   ├── handler.rs       # 调用处理
│   │   │   ├── converter.rs     # 类型转换
│   │   │   └── types.rs
│   │   └── tests/
│   │
│   ├── dex-types/               # 类型定义
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── order.rs         # 订单类型
│   │   │   ├── market.rs        # 市场类型
│   │   │   ├── account.rs       # 账户类型
│   │   │   └── sequence.rs      # 序列类型
│   │   └── tests/
│   │
│   ├── dex-rpc/                 # RPC 扩展
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── api.rs           # API 实现
│   │   │   ├── websocket.rs     # WebSocket 服务
│   │   │   └── handlers.rs     # 请求处理
│   │   └── tests/
│   │
│   └── dex-framework/            # Move 框架
│       ├── Move.toml
│       └── sources/
│           ├── dex.move         # 核心模块
│           ├── account.move     # 账户模块
│           └── market.move     # 市场模块
│
└── sui-fork/                    # Sui fork (git submodule)
    └── (Sui 仓库)
```

### 2.2 模块依赖关系

```
dex-rpc
  ├── dex-sequencer
  │     ├── dex-engine
  │     │     ├── dex-types
  │     │     └── dex-storage
  │     └── dex-types
  └── dex-types

dex-precompile
  ├── dex-engine
  │     ├── dex-types
  │     └── dex-storage
  └── sui-execution (fork)

dex-sequencer
  ├── dex-engine
  ├── dex-storage
  ├── dex-types
  └── mysten-network (Sui)

dex-engine
  ├── dex-types
  ├── dex-storage
  └── dashmap

dex-storage
  ├── dex-types
  └── typed-store (Sui)
```

### 2.3 关键文件说明

#### 2.3.1 Sequencer 核心

**`crates/dex-sequencer/src/sequencer.rs`**:
- 序列号分配逻辑
- FIFO 队列管理
- 批次聚合
- 确认收集

**`crates/dex-sequencer/src/gateway.rs`**:
- 订单接收和验证
- 限流控制
- 请求路由

#### 2.3.2 Matching Engine 核心

**`crates/dex-engine/src/orderbook.rs`**:
- 订单簿数据结构
- 价格层级管理
- 订单索引

**`crates/dex-engine/src/matcher.rs`**:
- 撮合算法实现
- 价格-时间优先逻辑
- 部分成交处理

#### 2.3.3 Precompile 核心

**`crates/dex-precompile/src/router.rs`**:
- DEX 调用检测
- 路由决策
- 参数提取

**`crates/dex-precompile/src/handler.rs`**:
- 函数调用处理
- 类型转换
- 结果返回

---

## 3. 核心算法设计

### 3.1 撮合算法

#### 3.1.1 价格-时间优先算法

```rust
pub fn match_order(
    orderbook: &mut Orderbook,
    incoming_order: Order,
) -> Result<MatchResult> {
    let mut match_result = MatchResult::new();
    let mut remaining_qty = incoming_order.quantity;
    
    // 获取对手方订单簿
    let opposite_side = match incoming_order.side {
        Side::Buy => &mut orderbook.asks,
        Side::Sell => &mut orderbook.bids,
    };
    
    // 价格匹配检查
    while remaining_qty > 0 {
        let best_price_level = match opposite_side.first_entry() {
            Some(entry) => entry,
            None => break, // 无对手方，挂单
        };
        
        let best_price = *best_price_level.key();
        
        // 价格匹配条件
        let price_match = match incoming_order.side {
            Side::Buy => incoming_order.price >= best_price,
            Side::Sell => incoming_order.price <= best_price,
        };
        
        if !price_match {
            break; // 价格不匹配，挂单
        }
        
        // 时间优先撮合
        let price_level = best_price_level.get_mut();
        while let Some(mut opposite_order) = price_level.orders.pop_front() {
            let trade_qty = remaining_qty.min(opposite_order.quantity);
            
            // 执行成交
            match_result.add_trade(Trade {
                taker_order_id: incoming_order.id,
                maker_order_id: opposite_order.id,
                price: best_price,
                quantity: trade_qty,
                timestamp: now(),
            });
            
            // 更新数量
            remaining_qty -= trade_qty;
            opposite_order.quantity -= trade_qty;
            
            // 如果对手方订单未完全成交，放回队列
            if opposite_order.quantity > 0 {
                price_level.orders.push_front(opposite_order);
                break;
            }
            
            // 如果当前订单已完全成交，退出
            if remaining_qty == 0 {
                break;
            }
        }
        
        // 如果价格层级为空，移除
        if price_level.orders.is_empty() {
            best_price_level.remove();
        }
    }
    
    // 如果还有剩余数量，挂单
    if remaining_qty > 0 {
        incoming_order.quantity = remaining_qty;
        orderbook.insert_order(incoming_order)?;
    }
    
    Ok(match_result)
}
```

#### 3.1.2 性能优化

**1. 内存预分配**:
```rust
// 预分配 VecDeque 容量
let mut orders = VecDeque::with_capacity(1000);
```

**2. 零拷贝优化**:
```rust
// 使用引用而非克隆
fn match_order<'a>(
    orderbook: &'a mut Orderbook,
    order: &'a Order,
) -> MatchResult<'a> {
    // ...
}
```

**3. SIMD 优化** (未来):
```rust
#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

// 使用 SIMD 指令批量比较价格
unsafe {
    let prices = _mm256_loadu_si256(...);
    // ...
}
```

### 3.2 序列号分配算法

#### 3.2.1 原子序列号分配

```rust
pub struct SequenceAllocator {
    counter: AtomicU64,
    last_confirmed: Arc<RwLock<u64>>,
}

impl SequenceAllocator {
    pub fn allocate(&self) -> SequenceNumber {
        // 原子递增
        let seq = self.counter.fetch_add(1, Ordering::SeqCst);
        SequenceNumber::new(seq)
    }
    
    pub fn confirm(&self, seq: SequenceNumber) {
        let mut last = self.last_confirmed.write().unwrap();
        if seq.value() > *last {
            *last = seq.value();
        }
    }
}
```

#### 3.2.2 批次聚合算法

```rust
pub struct BatchAggregator {
    pending: VecDeque<Transaction>,
    batch_time_window: Duration,
    batch_size_threshold: usize,
    last_batch_time: Instant,
}

impl BatchAggregator {
    pub fn add_transaction(&mut self, tx: Transaction) -> Option<Batch> {
        self.pending.push_back(tx);
        
        let should_create_batch = 
            self.pending.len() >= self.batch_size_threshold ||
            self.last_batch_time.elapsed() >= self.batch_time_window;
        
        if should_create_batch {
            Some(self.create_batch())
        } else {
            None
        }
    }
    
    fn create_batch(&mut self) -> Batch {
        let transactions: Vec<_> = self.pending.drain(..).collect();
        self.last_batch_time = Instant::now();
        Batch::new(transactions)
    }
}
```

### 3.3 风险计算算法

#### 3.3.1 净抵押品计算

```rust
pub fn calculate_net_collateral(
    account: &Account,
    prices: &PriceOracle,
) -> Result<u64> {
    let mut total_value = 0u64;
    
    // 计算资产价值
    for position in &account.asset_positions {
        let asset_price = prices.get_price(position.asset_id)?;
        let value = position.quantity * asset_price;
        total_value += value;
    }
    
    // 计算仓位净值
    for position in &account.perpetual_positions {
        let mark_price = prices.get_mark_price(position.market_id)?;
        let entry_price = position.entry_price;
        let pnl = if position.side == Side::Long {
            (mark_price - entry_price) * position.size
        } else {
            (entry_price - mark_price) * position.size
        };
        total_value += pnl;
    }
    
    Ok(total_value)
}
```

#### 3.3.2 保证金计算

```rust
pub fn calculate_margin_requirement(
    positions: &[Position],
    market_configs: &HashMap<MarketID, MarketConfig>,
) -> Result<u64> {
    let mut total_imr = 0u64;
    let mut total_mmr = 0u64;
    
    for position in positions {
        let config = market_configs.get(&position.market_id)
            .ok_or(RiskError::MarketNotFound)?;
        
        // 初始保证金 (IMR)
        let imr = position.notional_value() * config.initial_margin_rate;
        total_imr += imr;
        
        // 维持保证金 (MMR)
        let mmr = position.notional_value() * config.maintenance_margin_rate;
        total_mmr += mmr;
    }
    
    // OIMF 调整
    let oimf_multiplier = calculate_oimf_multiplier(positions, market_configs)?;
    total_imr = (total_imr as f64 * oimf_multiplier) as u64;
    
    Ok(total_imr)
}
```

### 3.4 两阶段执行算法

#### 3.4.1 存款流程

```rust
pub async fn execute_deposit(
    sequencer: &DexSequencer,
    deposit_tx: Transaction,
) -> Result<DepositResult> {
    // Phase 1: Move VM 执行
    let move_result = sequencer.move_vm.execute(
        &deposit_tx,
        ExecutionMode::Signing,
    ).await?;
    
    // 验证 Coin 锁定
    let coin_locked = move_result
        .effects
        .locked_objects
        .contains(&deposit_tx.coin_id);
    
    if !coin_locked {
        return Err(ExecutionError::CoinNotLocked);
    }
    
    // Phase 2: DEX 余额更新
    let dex_result = sequencer.matching_engine
        .update_balance(
            deposit_tx.account_id,
            deposit_tx.asset_id,
            deposit_tx.amount,
        )
        .await?;
    
    // 原子提交
    sequencer.storage.commit_batch(vec![
        move_result.into_batch(),
        dex_result.into_batch(),
    ]).await?;
    
    Ok(DepositResult {
        sequence: sequencer.allocate_sequence(),
        balance: dex_result.new_balance,
    })
}
```

---

## 4. 数据结构设计

### 4.1 订单数据结构

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Order {
    pub id: OrderID,
    pub account_id: AccountID,
    pub subaccount_id: u32,
    pub market_id: MarketID,
    pub side: Side,
    pub order_type: OrderType,
    pub price: Price,
    pub quantity: u64,
    pub remaining_quantity: u64,
    pub timestamp: u64,
    pub sequence: SequenceNumber,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Buy,
    Sell,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrderType {
    Limit,
    Market,
    IOC,        // Immediate or Cancel
    PostOnly,   // Post Only
    FOK,        // Fill or Kill
}
```

### 4.2 订单簿数据结构

```rust
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

pub struct OrderRef {
    price: Price,
    side: Side,
    level_index: usize,  // 在 PriceLevel.orders 中的索引
}
```

### 4.3 账户数据结构

```rust
pub struct Account {
    pub address: SuiAddress,
    pub subaccounts: HashMap<u32, SubAccount>,
}

pub struct SubAccount {
    pub subaccount_id: u32,
    pub margin_mode: MarginMode,
    
    // 资产持仓
    pub asset_positions: HashMap<AssetID, AssetPosition>,
    
    // 永续仓位
    pub perpetual_positions: HashMap<PositionKey, PerpetualPosition>,
    
    // 订单列表
    pub active_orders: HashSet<OrderID>,
}

pub struct AssetPosition {
    pub asset_id: AssetID,
    pub quantity: u64,  // in quantums
}

pub struct PerpetualPosition {
    pub market_id: MarketID,
    pub side: Side,
    pub size: u64,
    pub entry_price: Price,
    pub mark_price: Price,
    pub unrealized_pnl: i64,
}
```

### 4.4 序列数据结构

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SequenceBatch {
    pub sequence_range: (SequenceNumber, SequenceNumber),
    pub transactions: Vec<SequencedTransaction>,
    pub digest: SequenceDigest,
    pub timestamp: u64,
    pub leader_signature: Signature,
}

pub struct SequencedTransaction {
    pub sequence: SequenceNumber,
    pub transaction: Transaction,
    pub execution_result: ExecutionResult,
}
```

---

## 5. 性能优化方案

### 5.1 内存优化

#### 5.1.1 对象池

```rust
pub struct OrderPool {
    pool: Vec<Order>,
    capacity: usize,
}

impl OrderPool {
    pub fn get(&mut self) -> Order {
        self.pool.pop().unwrap_or_else(|| Order::default())
    }
    
    pub fn put(&mut self, order: Order) {
        if self.pool.len() < self.capacity {
            order.reset();
            self.pool.push(order);
        }
    }
}
```

#### 5.1.2 内存对齐

```rust
#[repr(align(64))]  // Cache line alignment
pub struct Orderbook {
    // ...
}
```

### 5.2 CPU 优化

#### 5.2.1 CPU 亲和性

```rust
use core_affinity::CoreId;

pub fn set_cpu_affinity(core_id: CoreId) {
    core_affinity::set_for_current(core_id);
}
```

#### 5.2.2 无锁数据结构

```rust
use crossbeam::queue::SegQueue;

pub struct LockFreeOrderQueue {
    queue: SegQueue<Order>,
}

impl LockFreeOrderQueue {
    pub fn push(&self, order: Order) {
        self.queue.push(order);
    }
    
    pub fn pop(&self) -> Option<Order> {
        self.queue.pop()
    }
}
```

### 5.3 I/O 优化

#### 5.3.1 批量写入

```rust
pub struct BatchWriter {
    buffer: Vec<WriteOp>,
    batch_size: usize,
}

impl BatchWriter {
    pub fn write(&mut self, op: WriteOp) -> Result<()> {
        self.buffer.push(op);
        
        if self.buffer.len() >= self.batch_size {
            self.flush()?;
        }
        
        Ok(())
    }
    
    fn flush(&mut self) -> Result<()> {
        // 批量写入 RocksDB
        let batch = self.buffer.drain(..).collect();
        self.db.write_batch(batch)?;
        Ok(())
    }
}
```

#### 5.3.2 Group Commit

```rust
pub struct GroupCommit {
    pending_writes: Arc<Mutex<Vec<WriteOp>>>,
    commit_interval: Duration,
}

impl GroupCommit {
    pub async fn start_commit_loop(&self) {
        let mut interval = tokio::time::interval(self.commit_interval);
        
        loop {
            interval.tick().await;
            
            let writes = {
                let mut pending = self.pending_writes.lock().unwrap();
                pending.drain(..).collect()
            };
            
            if !writes.is_empty() {
                self.db.write_batch(writes).await?;
            }
        }
    }
}
```

### 5.4 网络优化

#### 5.4.1 消息压缩

```rust
use snappy;

pub fn compress_message(msg: &[u8]) -> Vec<u8> {
    snappy::compress(msg)
}

pub fn decompress_message(compressed: &[u8]) -> Vec<u8> {
    snappy::decompress(compressed).unwrap()
}
```

#### 5.4.2 连接复用

```rust
pub struct ConnectionPool {
    connections: Arc<DashMap<PeerID, Connection>>,
    max_connections: usize,
}

impl ConnectionPool {
    pub async fn get_connection(&self, peer: PeerID) -> Result<Connection> {
        if let Some(conn) = self.connections.get(&peer) {
            return Ok(conn.clone());
        }
        
        // 创建新连接
        let conn = self.create_connection(peer).await?;
        self.connections.insert(peer, conn.clone());
        Ok(conn)
    }
}
```

---

## 6. 安全方案设计

### 6.1 签名验证

```rust
pub fn verify_transaction_signature(
    tx: &Transaction,
    public_key: &PublicKey,
) -> Result<()> {
    let message = tx.to_signable_message();
    let signature = &tx.signature;
    
    public_key.verify(&message, signature)
        .map_err(|_| SecurityError::InvalidSignature)
}
```

### 6.2 输入验证

```rust
pub fn validate_order(order: &Order) -> Result<()> {
    // 价格验证
    if order.price == 0 || order.price > MAX_PRICE {
        return Err(ValidationError::InvalidPrice);
    }
    
    // 数量验证
    if order.quantity == 0 || order.quantity > MAX_QUANTITY {
        return Err(ValidationError::InvalidQuantity);
    }
    
    // 市场验证
    if !MARKETS.contains(&order.market_id) {
        return Err(ValidationError::InvalidMarket);
    }
    
    Ok(())
}
```

### 6.3 重放攻击防护

```rust
pub struct ReplayProtection {
    seen_nonces: Arc<DashSet<Nonce>>,
    nonce_window: Duration,
}

impl ReplayProtection {
    pub fn check_nonce(&self, nonce: Nonce) -> Result<()> {
        if self.seen_nonces.contains(&nonce) {
            return Err(SecurityError::ReplayAttack);
        }
        
        self.seen_nonces.insert(nonce);
        Ok(())
    }
}
```

### 6.4 访问控制

```rust
pub struct AccessControl {
    allowed_addresses: Arc<DashSet<SuiAddress>>,
    rate_limits: Arc<DashMap<SuiAddress, RateLimiter>>,
}

impl AccessControl {
    pub fn check_access(&self, address: &SuiAddress) -> Result<()> {
        if !self.allowed_addresses.contains(address) {
            return Err(SecurityError::AccessDenied);
        }
        
        // 检查限流
        let limiter = self.rate_limits
            .entry(*address)
            .or_insert_with(|| RateLimiter::new(1000, Duration::from_secs(1)));
        
        limiter.check()
            .map_err(|_| SecurityError::RateLimitExceeded)
    }
}
```

---

## 7. 测试策略

### 7.1 单元测试

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_match_order_basic() {
        let mut orderbook = Orderbook::new();
        let buy_order = Order::new_buy(50000, 1);
        let sell_order = Order::new_sell(50000, 1);
        
        orderbook.insert_order(buy_order.clone());
        let result = orderbook.match_order(sell_order).unwrap();
        
        assert_eq!(result.trades.len(), 1);
        assert_eq!(result.trades[0].price, 50000);
    }
}
```

### 7.2 集成测试

```rust
#[tokio::test]
async fn test_deposit_flow() {
    let sequencer = create_test_sequencer().await;
    let deposit_tx = create_deposit_transaction();
    
    let result = sequencer.process_transaction(deposit_tx).await.unwrap();
    
    assert!(result.sequence > 0);
    assert_eq!(result.balance, 1000);
}
```

### 7.3 性能测试

```rust
#[tokio::test]
async fn test_matching_performance() {
    let mut engine = MatchingEngine::new();
    let order = create_test_order();
    
    let start = Instant::now();
    for _ in 0..10000 {
        engine.match_order(order.clone()).unwrap();
    }
    let duration = start.elapsed();
    
    assert!(duration.as_micros() < 100000); // < 100ms for 10k orders
}
```

### 7.4 压力测试

```rust
#[tokio::test]
async fn test_high_throughput() {
    let sequencer = create_test_sequencer().await;
    let mut handles = vec![];
    
    // 启动 100 个并发客户端
    for _ in 0..100 {
        let sequencer = sequencer.clone();
        let handle = tokio::spawn(async move {
            for _ in 0..1000 {
                let tx = create_random_transaction();
                sequencer.process_transaction(tx).await.unwrap();
            }
        });
        handles.push(handle);
    }
    
    // 等待所有任务完成
    for handle in handles {
        handle.await.unwrap();
    }
}
```

---

## 8. 实施细节

### 8.1 构建配置

**`Cargo.toml` (Workspace)**:
```toml
[workspace]
members = [
    "crates/dex-sequencer",
    "crates/dex-engine",
    "crates/dex-storage",
    "crates/dex-precompile",
    "crates/dex-types",
    "crates/dex-rpc",
]

[profile.release]
opt-level = 3
lto = true
codegen-units = 1
```

### 8.2 特性标志

```rust
// Cargo.toml
[features]
default = []
simd = ["dex-engine/simd"]
metrics = ["dex-sequencer/metrics", "dex-engine/metrics"]
```

### 8.3 日志配置

```rust
use tracing::{info, error, warn};

pub fn init_logging() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .init();
}
```

### 8.4 监控指标

```rust
use prometheus::{Counter, Histogram, Registry};

pub struct Metrics {
    pub orders_processed: Counter,
    pub match_latency: Histogram,
    pub sequence_latency: Histogram,
}

impl Metrics {
    pub fn register(registry: &Registry) -> Self {
        Self {
            orders_processed: Counter::new("orders_processed", "Total orders processed")
                .expect("metric can be created"),
            match_latency: Histogram::with_opts(
                HistogramOpts::new("match_latency", "Order matching latency")
                    .buckets(vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0]),
            ).expect("metric can be created"),
            sequence_latency: Histogram::with_opts(
                HistogramOpts::new("sequence_latency", "Sequencing latency")
                    .buckets(vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0]),
            ).expect("metric can be created"),
        }
    }
}
```

### 8.5 错误处理

```rust
#[derive(Debug, thiserror::Error)]
pub enum DexError {
    #[error("Invalid order: {0}")]
    InvalidOrder(String),
    
    #[error("Insufficient balance: {0}")]
    InsufficientBalance(u64),
    
    #[error("Market not found: {0}")]
    MarketNotFound(MarketID),
    
    #[error("Risk check failed: {0}")]
    RiskCheckFailed(String),
    
    #[error("Storage error: {0}")]
    StorageError(#[from] StorageError),
    
    #[error("Network error: {0}")]
    NetworkError(#[from] NetworkError),
}
```

---

## 9. 开发流程

### 9.1 代码规范

- **命名**: 使用 snake_case (函数、变量) 和 PascalCase (类型)
- **文档**: 所有公共 API 必须有文档注释
- **错误处理**: 使用 `Result<T, E>` 而非 panic
- **测试覆盖率**: 目标 80%+

### 9.2 Git 工作流

```
main (生产分支)
  ├── develop (开发分支)
  │     ├── feature/sequencer
  │     ├── feature/matching-engine
  │     └── feature/storage
  └── release/v1.0
```

### 9.3 CI/CD 流程

```yaml
# .github/workflows/ci.yml
name: CI

on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v2
      - uses: actions-rs/cargo@v1
        with:
          command: test
          
  lint:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v2
      - uses: actions-rs/cargo@v1
        with:
          command: clippy
          
  format:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v2
      - uses: actions-rs/cargo@v1
        with:
          command: fmt
          args: -- --check
```

---

## 10. 总结

### 10.1 技术栈总结

- **语言**: Rust (性能、安全)
- **异步**: Tokio (高并发)
- **存储**: RocksDB (高性能)
- **网络**: Anemo (Sui P2P)
- **序列化**: BCS (确定性)

### 10.2 关键设计

- **撮合算法**: 价格-时间优先，< 10μs
- **序列分配**: 原子递增，FIFO 排序
- **存储**: 内存 + WAL + RocksDB 三层
- **安全**: 签名验证、输入验证、重放防护

### 10.3 性能目标

- **撮合延迟**: < 10μs (单笔)
- **端到端延迟**: < 50ms (P99)
- **吞吐量**: 10万+ TPS
- **内存占用**: < 20GB (1000 markets)

---

**文档版本**: v1.0  
**最后更新**: 2025-01-XX  
**维护者**: DEX 技术团队

