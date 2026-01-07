# DEX L1 完整系统架构设计
> **基于 Sui Fork 的高性能去中心化交易所**

**版本**: v1.0
**日期**: 2026-01-07
**状态**: 架构设计文档
**参考**: `dex_use_sui_plan.md`, `DEX完整业务需求.md`, `DEX_L1_DESIGN_SUMMARY.md`

---

## 执行摘要

本文档定义了基于 Sui Fork 的 DEX L1 完整系统架构,采用 **中心化 Sequencer + 异步验证者确认** 模式,在不使用 Sui Mysticeti 共识的前提下实现:
- **< 50ms 端到端延迟**
- **≥ 200,000 TPS 吞吐量**
- **< 10μs 单次撮合延迟**

**核心策略**:
1. 复用 Sui 80% 基础设施 (Tonic Network, typed-store, Leader Schedule)
2. 原生 Rust 引擎绕过 Move VM
3. 轮转 Sequencer 实现高可用
4. 两阶段执行保证存取款原子性

---

## 目录

1. [整体架构](#1-整体架构)
2. [核心组件设计](#2-核心组件设计)
3. [数据流设计](#3-数据流设计)
4. [存储架构](#4-存储架构)
5. [网络拓扑](#5-网络拓扑)
6. [安全与可靠性](#6-安全与可靠性)
7. [扩展性设计](#7-扩展性设计)
8. [技术栈与依赖](#8-技术栈与依赖)
9. [部署架构](#9-部署架构)
10. [演进路线](#10-演进路线)

---

## 1. 整体架构

### 1.1 七层架构图

```
┌──────────────────────────────────────────────────────────────────────┐
│  Layer 7: Client Layer (客户端层)                                     │
│  ├─ Sui SDK/Wallets (复用 Sui 签名逻辑)                               │
│  ├─ Trading Bots (算法交易)                                           │
│  └─ Web/Mobile Apps (前端应用)                                        │
└──────────────────────────────────────────────────────────────────────┘
                              ↓ JSON-RPC / WebSocket
┌──────────────────────────────────────────────────────────────────────┐
│  Layer 6: API Gateway Layer (API 网关层)                              │
│  ├─ JSON-RPC Server (复用 sui-json-rpc)                              │
│  ├─ WebSocket Server (实时行情推送)                                   │
│  ├─ Rate Limiter (多层限流: IP/Account/Endpoint)                      │
│  └─ Signature Verifier (交易签名验证,复用 shared-crypto)               │
└──────────────────────────────────────────────────────────────────────┘
                              ↓ 分类路由
┌──────────────────────────────────────────────────────────────────────┐
│  Layer 5: Transaction Router (交易路由层)                             │
│  ├─ Transaction Classifier                                           │
│  │   ├─ is_dex_transaction() → Fast Path                            │
│  │   ├─ is_deposit_withdrawal() → Hybrid Path                       │
│  │   └─ is_standard_sui() → Standard Path (Mysticeti)               │
│  └─ Path Dispatcher                                                  │
└──────────────────────────────────────────────────────────────────────┘
        ↓ Fast             ↓ Hybrid            ↓ Standard
┌──────────────────┐  ┌─────────────────┐  ┌──────────────────────┐
│ DEX Fast Path    │  │ Hybrid Path     │  │ Standard Sui Path    │
│ (< 50ms)         │  │ (< 100ms)       │  │ (~600ms)             │
└──────────────────┘  └─────────────────┘  └──────────────────────┘
        ↓                     ↓                      ↓
┌──────────────────────────────────────────────────────────────────────┐
│  Layer 4: Sequencer Layer (排序器层)                                  │
│  ├─ DexSequencerService (主 Sequencer)                               │
│  │   ├─ Leader Election (复用 leader_schedule.rs)                   │
│  │   ├─ Sequence Assignment ([Epoch:16][Counter:48])                │
│  │   ├─ Batch Aggregation (5ms 或 1000 tx)                          │
│  │   └─ Broadcast via Tonic Network                                 │
│  ├─ Sequencer Health Monitor (50ms 心跳检测)                         │
│  └─ Failover Manager (< 100ms 故障切换)                              │
└──────────────────────────────────────────────────────────────────────┘
                              ↓ 广播批次
┌──────────────────────────────────────────────────────────────────────┐
│  Layer 3: Execution Layer (执行层)                                    │
│  ├─ Native DEX Engine (原生撮合引擎,< 10μs)                            │
│  │   ├─ MatchingEngine (BTreeMap + DashMap)                         │
│  │   ├─ RiskEngine (IMR/MMR 计算)                                    │
│  │   ├─ PerpetualEngine (资金费率)                                   │
│  │   └─ LiquidationEngine (清算逻辑)                                 │
│  ├─ Precompile Interceptor (拦截 0xDEX 包调用)                        │
│  └─ Move VM (仅用于存取款和标准 Sui 交易)                              │
└──────────────────────────────────────────────────────────────────────┘
                              ↓ 状态更新
┌──────────────────────────────────────────────────────────────────────┐
│  Layer 2: Storage Layer (存储层)                                      │
│  ├─ Memory Cache (DashMap, < 1μs)                                    │
│  │   ├─ Active Orderbooks (实时订单簿)                               │
│  │   ├─ Balance Cache (账户余额缓存)                                 │
│  │   └─ Position Cache (持仓缓存)                                    │
│  ├─ WAL (Write-Ahead Log, < 10ms)                                    │
│  │   ├─ Group Commit (批量 fsync)                                    │
│  │   └─ Sequence-based Replay (基于序列号回放)                       │
│  ├─ Snapshot (LZ4 压缩, RTO < 5min)                                  │
│  │   └─ Periodic Snapshot Creation (每小时或每 100 万交易)            │
│  └─ typed-store (RocksDB, 复用 Sui)                                  │
│      ├─ DexOrderbookStore                                            │
│      ├─ DexBalanceStore                                              │
│      ├─ DexPerpetualStore                                            │
│      └─ DexFundingRateStore                                          │
└──────────────────────────────────────────────────────────────────────┘
                              ↓ 持久化确认
┌──────────────────────────────────────────────────────────────────────┐
│  Layer 1: Consensus & Validation Layer (共识与验证层)                  │
│  ├─ 2f+1 Validators (异步验证批次)                                    │
│  │   ├─ Batch Validation (验证批次合法性)                            │
│  │   ├─ State Hash Verification (状态哈希验证)                       │
│  │   └─ fsync to Local WAL (持久化确认, RPO=0)                       │
│  ├─ Sui Checkpoint Service (复用检查点机制)                           │
│  └─ (可选) Phase 2 演进到 Mysticeti 共识                              │
└──────────────────────────────────────────────────────────────────────┘
```

### 1.2 架构设计原则

| 原则 | 说明 | 体现 |
|-----|------|-----|
| **性能优先** | 满足 < 50ms 延迟和 200K TPS | 原生 Rust 引擎 + 无锁并发 |
| **渐进式去中心化** | Phase 1 中心化,Phase 2 向去中心化演进 | Sequencer 轮转 + 2f+1 验证 |
| **复用成熟组件** | 80% 复用 Sui 基础设施 | Tonic Network, typed-store, Leader Schedule |
| **原子性安全** | 保证存取款一致性 | 两阶段执行 + 锁机制 |
| **可观测性** | 完整监控与调试能力 | 复用 mysten-metrics + 自定义指标 |
| **容错性** | 单点故障自动恢复 | 快速故障切换 (< 100ms) |

---

## 2. 核心组件设计

### 2.1 Transaction Router (交易路由器)

#### 2.1.1 功能定位
- 在 `AuthorityState` 入口拦截所有交易
- 根据交易类型分流到不同路径
- 确保分类准确性,避免错误路由

#### 2.1.2 分类逻辑

```rust
// /crates/sui-core/src/authority.rs

pub struct TransactionRouter {
    dex_sequencer: Arc<DexSequencerService>,
    dex_engine: Arc<RwLock<MatchingEngine>>,
}

impl TransactionRouter {
    /// 核心分类逻辑
    pub async fn route_transaction(
        &self,
        tx: Transaction,
    ) -> Result<TransactionResponse, SuiError> {
        // 1. 检查是否为纯 DEX 交易
        if self.is_dex_transaction(&tx) {
            return self.route_to_dex_fast_path(tx).await;
        }

        // 2. 检查是否为存取款 (涉及链上资产)
        if self.is_deposit_withdrawal(&tx) {
            return self.route_to_hybrid_path(tx).await;
        }

        // 3. 其他走标准 Sui 流程
        self.route_to_standard_path(tx).await
    }

    /// 判断是否为纯 DEX 交易
    fn is_dex_transaction(&self, tx: &Transaction) -> bool {
        tx.data()
            .intent_message()
            .value
            .kind()
            .input_objects()
            .iter()
            .all(|obj| {
                // 所有输入对象都来自 0xDEX 包
                obj.package() == Some(&DEX_PACKAGE_ADDRESS)
            })
    }

    /// 判断是否为存取款交易
    fn is_deposit_withdrawal(&self, tx: &Transaction) -> bool {
        // 检查是否同时涉及:
        // 1. 链上 Coin 对象 (如 USDC)
        // 2. DEX 账户余额修改
        let has_coin = tx.contains_coin_object();
        let has_dex_call = tx.calls_dex_package();
        has_coin && has_dex_call
    }
}
```

#### 2.1.3 路由矩阵

| 交易类型 | 检测条件 | 路由路径 | 延迟目标 |
|---------|---------|---------|---------|
| **纯 DEX 交易** | 仅涉及 0xDEX 包 | Fast Path (Sequencer → Native Engine) | < 50ms |
| **存款** | Coin 对象 + DEX 包调用 | Hybrid Path (Two-Phase) | < 100ms |
| **取款** | DEX 包调用 + 铸造 Coin | Hybrid Path (Two-Phase) | < 100ms |
| **标准 Sui 交易** | 无 DEX 包调用 | Standard Path (Mysticeti + Move VM) | ~600ms |

---

### 2.2 Sequencer Layer (排序器层)

#### 2.2.1 Sequencer 架构

```
┌─────────────────────────────────────────────────────────────┐
│             Sequencer Cluster (轮转 Leader)                 │
├─────────────────────────────────────────────────────────────┤
│  Leader Sequencer (当前 Epoch)                              │
│  ├─ Order Reception (订单接收)                              │
│  │   └─ Rate Limiting (速率限制: 10K req/s per IP)          │
│  ├─ Sequence Assignment (序列号分配)                        │
│  │   └─ [Epoch:16 bits][Counter:48 bits]                  │
│  ├─ Batch Aggregation (批次聚合)                            │
│  │   ├─ Time-based: 每 5ms 一批                            │
│  │   └─ Size-based: 每 1000 tx 一批                        │
│  ├─ Native Execution (原生执行)                             │
│  │   └─ Matching Engine (< 10μs per match)                │
│  ├─ Soft Confirmation (软确认)                              │
│  │   └─ 返回序列号和执行结果给客户端                         │
│  └─ Batch Broadcast (批次广播)                              │
│      └─ 通过 Tonic Network 发送到所有验证者                  │
├─────────────────────────────────────────────────────────────┤
│  Standby Sequencers (备用节点)                              │
│  ├─ Health Monitoring (每 50ms 心跳检测)                    │
│  ├─ State Sync (同步最新状态)                               │
│  └─ Failover Readiness (< 100ms 接管)                      │
└─────────────────────────────────────────────────────────────┘
```

#### 2.2.2 Leader 选举机制

**复用 Sui Leader Schedule**:

```rust
// /consensus/core/src/leader_schedule.rs

pub struct DexSequencerSchedule {
    /// 复用 Mysticeti 的 leader 调度逻辑
    inner: LeaderSchedule,

    /// Sequencer epoch 时长 (如 1 分钟,比共识 epoch 短)
    sequencer_epoch_duration: Duration,
}

impl DexSequencerSchedule {
    /// 基于时间戳的确定性轮转
    pub fn current_sequencer_leader(&self, timestamp: u64) -> AuthorityIndex {
        let epoch = timestamp / self.sequencer_epoch_duration.as_millis() as u64;
        let committee = self.inner.committee();

        // 权益加权随机选举 (与 Sui 一致)
        committee.leader_by_epoch(epoch)
    }

    /// 故障切换: 跳到下一个 leader
    pub fn next_leader(&self, failed_leader: AuthorityIndex) -> AuthorityIndex {
        self.inner.elect_leader_excluding(failed_leader)
    }
}
```

**选举特性**:
- **确定性**: 所有节点根据时间戳计算出相同 leader
- **权益加权**: 质押更多的验证者更有机会成为 Sequencer
- **快速轮转**: 1 分钟轮换一次,降低单点风险
- **故障检测**: 50ms 心跳超时,自动切换

#### 2.2.3 序列号设计

**结构**: `[Epoch:16 bits][Counter:48 bits]`

| 字段 | 位数 | 范围 | 说明 |
|-----|-----|------|-----|
| Epoch | 16 bits | 0 - 65,535 | Sequencer 轮换 epoch |
| Counter | 48 bits | 0 - 281 万亿 | 单 epoch 内序列号 |

**特性**:
- **全局唯一**: Epoch + Counter 组合保证唯一性
- **单调递增**: 便于检测间隙和回放
- **高容量**: 单 epoch (1 分钟) 支持 281 万亿交易 (远超 200K TPS)

#### 2.2.4 批次聚合策略

**混合触发条件** (满足任一即聚合):
1. **时间触发**: 距上次聚合 ≥ 5ms
2. **大小触发**: 累积交易数 ≥ 1000 笔
3. **优先级触发**: 出现高优先级交易 (如清算)

**批次结构**:
```rust
pub struct SequencedBatch {
    /// 批次 ID (递增)
    batch_id: u64,

    /// Sequencer epoch
    epoch: u16,

    /// 起始序列号
    start_sequence: u64,

    /// 交易列表 (已排序)
    transactions: Vec<SequencedTransaction>,

    /// 状态哈希 (执行后状态的 Merkle Root)
    state_hash: Hash,

    /// Sequencer 签名
    signature: Signature,
}
```

---

### 2.3 Matching Engine (撮合引擎)

#### 2.3.1 核心数据结构

```rust
// /crates/dex-engine/src/matching.rs

use dashmap::DashMap;
use std::collections::{BTreeMap, VecDeque};

/// 全局撮合引擎
pub struct MatchingEngine {
    /// 多市场管理 (支持市场间并行)
    markets: DashMap<MarketId, Orderbook>,

    /// 订单索引 (快速定位订单)
    order_index: DashMap<OrderId, OrderLocation>,

    /// 账户余额 (分片锁,支持账户间并行)
    balances: DashMap<AccountId, AccountBalance>,

    /// 风控引擎
    risk_engine: Arc<RiskEngine>,

    /// 永续合约引擎
    perpetual_engine: Arc<PerpetualEngine>,
}

/// 单市场订单簿
pub struct Orderbook {
    market_id: MarketId,

    /// 买单队列 (价格从高到低排序)
    bids: BTreeMap<Price, OrderQueue>,

    /// 卖单队列 (价格从低到高排序)
    asks: BTreeMap<Price, OrderQueue>,

    /// 最优价格缓存 (加速查询)
    best_bid: Option<Price>,
    best_ask: Option<Price>,

    /// 市场统计
    stats: MarketStats,
}

/// 同价格订单队列 (FIFO)
pub struct OrderQueue {
    /// 订单列表 (时间优先)
    orders: VecDeque<Order>,

    /// 总量缓存 (避免重复计算)
    total_size: u64,
}

/// 订单结构 (64 字节对齐,缓存友好)
#[repr(align(64))]
pub struct Order {
    order_id: u64,          // 8 bytes
    user_id: u64,           // 8 bytes
    price: u64,             // 8 bytes (fixed-point)
    size: u64,              // 8 bytes
    side: Side,             // 1 byte (Buy/Sell)
    order_type: OrderType,  // 1 byte
    timestamp: u64,         // 8 bytes
    flags: u8,              // 1 byte (PostOnly, ReduceOnly, IOC, FOK)
    _padding: [u8; 21],     // 填充到 64 字节
}
```

#### 2.3.2 撮合算法 (价格-时间优先)

```rust
impl MatchingEngine {
    /// 核心撮合逻辑 (目标: < 10μs)
    pub fn match_order(
        &mut self,
        incoming: Order,
    ) -> Result<MatchResult, MatchError> {
        // 1. 获取市场订单簿
        let mut book = self.markets
            .get_mut(&incoming.market_id)
            .ok_or(MatchError::MarketNotFound)?;

        // 2. 风控检查 (保证金充足性)
        self.risk_engine.check_order(&incoming)?;

        // 3. 执行撮合
        let mut fills = Vec::new();
        let mut remaining = incoming.size;

        // 获取对手盘 (买单看卖盘,卖单看买盘)
        let opposite_side = match incoming.side {
            Side::Buy => &mut book.asks,
            Side::Sell => &mut book.bids,
        };

        // 遍历可匹配价格档位
        while remaining > 0 {
            // 获取最优价格档位
            let best_price_entry = match incoming.side {
                Side::Buy => opposite_side.first_entry(),
                Side::Sell => opposite_side.last_entry(),
            };

            let Some(mut price_level) = best_price_entry else {
                break; // 无对手盘
            };

            // 检查价格是否匹配
            if !self.can_match(incoming.price, *price_level.key(), incoming.side) {
                break;
            }

            let queue = price_level.get_mut();

            // 匹配队首订单 (时间优先)
            while remaining > 0 && !queue.orders.is_empty() {
                let resting_order = queue.orders.front_mut().unwrap();
                let fill_size = remaining.min(resting_order.size);

                // 记录成交
                fills.push(Fill {
                    maker_order_id: resting_order.order_id,
                    taker_order_id: incoming.order_id,
                    price: resting_order.price,
                    size: fill_size,
                    timestamp: now(),
                });

                // 更新剩余量
                remaining -= fill_size;
                resting_order.size -= fill_size;

                // 完全成交则移除
                if resting_order.size == 0 {
                    queue.orders.pop_front();
                }
            }

            // 价格档位空了则移除
            if queue.orders.is_empty() {
                price_level.remove();
            }
        }

        // 4. 更新账户余额和持仓
        self.settle_fills(&fills)?;

        // 5. 剩余未成交部分处理
        if remaining > 0 && incoming.order_type != OrderType::IOC {
            self.add_to_book(incoming, remaining)?;
        }

        Ok(MatchResult { fills, remaining })
    }

    /// 价格匹配判断
    fn can_match(&self, taker_price: Price, maker_price: Price, side: Side) -> bool {
        match side {
            Side::Buy => taker_price >= maker_price,  // 买单价格 ≥ 卖单价格
            Side::Sell => taker_price <= maker_price, // 卖单价格 ≤ 买单价格
        }
    }

    /// 结算成交 (更新余额和持仓)
    fn settle_fills(&mut self, fills: &[Fill]) -> Result<(), MatchError> {
        for fill in fills {
            let quote_amount = fill.size * fill.price;

            // Maker 方结算
            let mut maker_balance = self.balances.get_mut(&fill.maker_user_id)?;
            maker_balance.settle_maker(fill)?;

            // Taker 方结算
            let mut taker_balance = self.balances.get_mut(&fill.taker_user_id)?;
            taker_balance.settle_taker(fill)?;

            // 计算手续费
            let maker_fee = self.calculate_fee(quote_amount, FeeType::Maker);
            let taker_fee = self.calculate_fee(quote_amount, FeeType::Taker);

            // 扣除手续费
            maker_balance.deduct_fee(maker_fee);
            taker_balance.deduct_fee(taker_fee);
        }

        Ok(())
    }
}
```

#### 2.3.3 性能优化

| 优化维度 | 技术手段 | 预期效果 |
|---------|---------|---------|
| **并发** | DashMap 分片锁 | 市场间 + 账户间并行 |
| **内存布局** | 64 字节对齐 | 缓存友好,减少 false sharing |
| **数据结构** | BTreeMap (价格) + VecDeque (队列) | O(log P) 插入, O(1) 队首操作 |
| **缓存** | best_bid/ask 缓存 | 避免重复遍历 |
| **SIMD** | AVX2 向量化价格比较 | 4-8x 加速 |
| **对象池** | 预分配 Order 对象 | 减少 GC 开销 |

---

### 2.4 Storage Layer (存储层)

#### 2.4.1 四层存储架构

```
┌─────────────────────────────────────────────────────────────┐
│  Layer 1: Memory Cache (DashMap)                            │
│  ├─ 读写延迟: < 1μs                                         │
│  ├─ 容量: 活跃数据 (如最近 1 小时订单)                       │
│  └─ 特性: 无锁并发访问                                      │
└─────────────────────────────────────────────────────────────┘
                         ↓ 异步刷盘
┌─────────────────────────────────────────────────────────────┐
│  Layer 2: WAL (Write-Ahead Log)                             │
│  ├─ 写入延迟: < 10ms (Group Commit)                         │
│  ├─ 格式: 顺序写入二进制日志                                 │
│  ├─ 用途: 故障恢复 (RPO = 0)                                │
│  └─ 清理: Snapshot 后可删除旧 WAL                           │
└─────────────────────────────────────────────────────────────┘
                         ↓ 定期快照
┌─────────────────────────────────────────────────────────────┐
│  Layer 3: Snapshot (LZ4 压缩)                               │
│  ├─ 创建频率: 每小时或每 100 万交易                          │
│  ├─ 恢复时间: < 5 分钟 (RTO)                                │
│  ├─ 压缩比: ~10:1                                           │
│  └─ 用途: 快速重启和状态同步                                 │
└─────────────────────────────────────────────────────────────┘
                         ↓ 最终持久化
┌─────────────────────────────────────────────────────────────┐
│  Layer 4: typed-store (RocksDB)                             │
│  ├─ 写入延迟: ~10ms                                         │
│  ├─ 表结构:                                                 │
│  │   ├─ DexOrderbookStore (市场订单簿状态)                  │
│  │   ├─ DexBalanceStore (账户余额)                         │
│  │   ├─ DexPositionStore (永续合约持仓)                     │
│  │   ├─ DexFundingRateStore (资金费率历史)                 │
│  │   └─ DexTradeHistoryStore (成交历史)                    │
│  └─ 特性: LSM-Tree, 批量写入优化                            │
└─────────────────────────────────────────────────────────────┘
```

#### 2.4.2 WAL 设计

**记录格式**:
```rust
pub struct WALRecord {
    /// 全局序列号 (用于回放)
    sequence: u64,

    /// 批次 ID
    batch_id: u64,

    /// 交易列表
    transactions: Vec<Transaction>,

    /// 执行后状态哈希 (用于验证)
    state_hash: Hash,

    /// 时间戳
    timestamp: u64,

    /// CRC32 校验和
    checksum: u32,
}
```

**Group Commit 策略**:
- **触发条件** (满足任一):
  1. 累积 100 条记录
  2. 距上次 commit ≥ 10ms
  3. 内存缓冲区 ≥ 4MB
- **fsync 保证**: 每次 commit 调用 `fsync()` 确保持久化 (RPO = 0)

**恢复流程**:
```rust
pub fn recover_from_wal(wal_path: &Path) -> Result<State> {
    let mut state = State::new();
    let records = read_all_wal_records(wal_path)?;

    for record in records {
        // 1. 验证校验和
        if !record.verify_checksum() {
            return Err(WalError::CorruptedRecord);
        }

        // 2. 重放交易
        for tx in record.transactions {
            state.apply_transaction(tx)?;
        }

        // 3. 验证状态哈希
        if state.hash() != record.state_hash {
            return Err(WalError::StateMismatch);
        }
    }

    Ok(state)
}
```

#### 2.4.3 Snapshot 设计

**创建策略**:
- **定时**: 每小时创建一次
- **阈值**: 累积交易数 ≥ 100 万笔
- **异步**: 后台线程执行,不阻塞主流程

**快照内容**:
```rust
pub struct Snapshot {
    /// 快照序列号 (对应 WAL sequence)
    sequence: u64,

    /// 压缩后的完整状态
    compressed_state: Vec<u8>,

    /// 元数据
    metadata: SnapshotMetadata,
}

pub struct SnapshotMetadata {
    /// 创建时间
    created_at: u64,

    /// 原始大小
    original_size: u64,

    /// 压缩后大小
    compressed_size: u64,

    /// 压缩算法
    compression: Compression, // LZ4

    /// 状态哈希
    state_hash: Hash,
}
```

**恢复流程**:
```rust
pub fn recover_from_snapshot(snapshot: Snapshot) -> Result<State> {
    // 1. 解压快照
    let decompressed = lz4::decompress(&snapshot.compressed_state)?;

    // 2. 反序列化状态
    let state = bcs::from_bytes(&decompressed)?;

    // 3. 重放快照后的 WAL 记录
    let wal_records = read_wal_since(snapshot.sequence)?;
    for record in wal_records {
        state.apply_batch(record)?;
    }

    Ok(state)
}
```

---

## 3. 数据流设计

### 3.1 Fast Path (纯 DEX 交易, < 50ms)

```
┌─────────────────────────────────────────────────────────────────┐
│  Client                                                         │
│  └─ 提交订单: PlaceOrder(BTC-USD, Buy, 0.1 BTC @ $50,000)       │
└─────────────────────────────────────────────────────────────────┘
                         ↓ JSON-RPC (< 5ms)
┌─────────────────────────────────────────────────────────────────┐
│  API Gateway                                                    │
│  ├─ 验证签名 (shared-crypto)                                    │
│  ├─ 速率限制 (10K req/s per IP)                                 │
│  └─ 路由到 Sequencer                                            │
└─────────────────────────────────────────────────────────────────┘
                         ↓ 转发 (< 2ms)
┌─────────────────────────────────────────────────────────────────┐
│  Sequencer (Leader)                                             │
│  ├─ 分配序列号: [Epoch:123][Counter:456789]                     │
│  ├─ 添加到批次缓冲 (5ms 或 1000 tx 触发)                         │
│  └─ 批次执行:                                                   │
│      ├─ Native Matching Engine (< 10μs)                        │
│      ├─ 更新 Memory Cache                                       │
│      └─ 生成软确认: SoftConfirmation {                          │
│            sequence: 123_456789,                                │
│            fills: [(Price: $50,000, Size: 0.1)],               │
│            remaining: 0,                                        │
│          }                                                      │
└─────────────────────────────────────────────────────────────────┘
                         ↓ 返回 (< 20ms)
┌─────────────────────────────────────────────────────────────────┐
│  Client                                                         │
│  └─ 收到软确认 (总延迟: < 50ms)                                  │
└─────────────────────────────────────────────────────────────────┘

                   --- 异步持久化流程 ---

┌─────────────────────────────────────────────────────────────────┐
│  Sequencer                                                      │
│  └─ 广播批次到所有验证者 (Tonic Network)                         │
└─────────────────────────────────────────────────────────────────┘
                         ↓ P2P 广播 (< 10ms)
┌─────────────────────────────────────────────────────────────────┐
│  Validators (2f+1)                                              │
│  ├─ 验证批次签名                                                │
│  ├─ 验证状态哈希                                                │
│  ├─ 写入本地 WAL (fsync)                                        │
│  └─ 返回确认签名                                                │
└─────────────────────────────────────────────────────────────────┘
                         ↓ 收集 2f+1 签名 (< 50ms)
┌─────────────────────────────────────────────────────────────────┐
│  Sequencer                                                      │
│  └─ 生成硬确认: HardConfirmation {                              │
│        sequence: 123_456789,                                    │
│        validator_sigs: [sig1, sig2, ..., sigN],                │
│        finalized: true,                                         │
│      }                                                          │
│  └─ 广播硬确认给客户端 (通过 WebSocket)                          │
└─────────────────────────────────────────────────────────────────┘
                         ↓ WebSocket Push (< 80ms)
┌─────────────────────────────────────────────────────────────────┐
│  Client                                                         │
│  └─ 收到硬确认 (总延迟: < 100ms, RPO=0)                          │
└─────────────────────────────────────────────────────────────────┘
```

### 3.2 Hybrid Path (存取款, < 100ms)

#### 3.2.1 存款流程 (Deposit)

```
┌─────────────────────────────────────────────────────────────────┐
│  Client                                                         │
│  └─ 调用 Move 函数: deposit_usdc(coin: Coin<USDC>, 1000 USDC)   │
└─────────────────────────────────────────────────────────────────┘
                         ↓ 提交交易
┌─────────────────────────────────────────────────────────────────┐
│  Transaction Router                                             │
│  └─ 检测到涉及 Coin 对象 + DEX 包调用 → Hybrid Path              │
└─────────────────────────────────────────────────────────────────┘
                         ↓ 路由到两阶段执行器
┌─────────────────────────────────────────────────────────────────┐
│  Phase 1: Signing (Move VM 执行)                                │
│  ├─ Move VM 执行 deposit_usdc():                                │
│  │   ├─ 转移 Coin 所有权到托管账户 (0xCUSTODY)                   │
│  │   ├─ 生成存款事件: DepositEvent {                            │
│  │   │     user: 0xALICE,                                       │
│  │   │     asset: USDC,                                         │
│  │   │     amount: 1000,                                        │
│  │   │   }                                                      │
│  │   └─ ⚠️ 不修改 DEX 内部余额 (仅准备)                          │
│  ├─ 验证者签名 (达成 2f+1 共识)                                  │
│  └─ 生成证书 (Certificate)                                      │
└─────────────────────────────────────────────────────────────────┘
                         ↓ 传递证书 (< 50ms)
┌─────────────────────────────────────────────────────────────────┐
│  Phase 2: Certificate Execution (Native Engine)                 │
│  ├─ Precompile 拦截器接收证书                                   │
│  ├─ 验证 2f+1 签名                                              │
│  ├─ 原子更新 DEX 内部余额:                                       │
│  │   └─ balances[0xALICE][USDC] += 1000                        │
│  ├─ 写入 WAL (fsync, RPO=0)                                     │
│  └─ 返回成功确认                                                │
└─────────────────────────────────────────────────────────────────┘
                         ↓ 总延迟 < 100ms
┌─────────────────────────────────────────────────────────────────┐
│  Client                                                         │
│  └─ 存款完成,可在 DEX 内交易                                     │
└─────────────────────────────────────────────────────────────────┘
```

**关键不变量**:
- **Phase 1 完成前**: Coin 对象已转移到托管账户,但 DEX 余额未增加
- **Phase 2 完成后**: Coin 对象在托管账户 + DEX 余额增加
- **原子性保证**: 要么两阶段都成功,要么都失败 (通过锁机制)

#### 3.2.2 取款流程 (Withdrawal)

```
┌─────────────────────────────────────────────────────────────────┐
│  Client                                                         │
│  └─ 调用 Move 函数: withdraw_usdc(amount: 500 USDC)             │
└─────────────────────────────────────────────────────────────────┘
                         ↓ 提交交易
┌─────────────────────────────────────────────────────────────────┐
│  Phase 1: Signing (Move VM 预留)                                │
│  ├─ Move VM 执行 withdraw_usdc():                               │
│  │   ├─ 创建取款锁: WithdrawalLock {                            │
│  │   │     user: 0xALICE,                                       │
│  │   │     asset: USDC,                                         │
│  │   │     amount: 500,                                         │
│  │   │     ttl: now() + 30s,                                    │
│  │   │   }                                                      │
│  │   └─ ⚠️ 仍未铸造 Coin (仅锁定)                                │
│  └─ 生成证书                                                    │
└─────────────────────────────────────────────────────────────────┘
                         ↓ 传递证书
┌─────────────────────────────────────────────────────────────────┐
│  Phase 2: Certificate Execution (Native Engine)                 │
│  ├─ 验证证书                                                    │
│  ├─ 检查余额充足性: balances[0xALICE][USDC] >= 500              │
│  ├─ 原子扣减余额:                                               │
│  │   └─ balances[0xALICE][USDC] -= 500                         │
│  ├─ 同步回调 Move VM:                                           │
│  │   └─ callback_mint_coin(user: 0xALICE, amount: 500)         │
│  │       ├─ 从托管账户铸造 Coin 对象                             │
│  │       └─ 转移给 0xALICE                                      │
│  ├─ 释放取款锁                                                  │
│  └─ 写入 WAL (fsync)                                            │
└─────────────────────────────────────────────────────────────────┘
                         ↓ 总延迟 < 100ms
┌─────────────────────────────────────────────────────────────────┐
│  Client                                                         │
│  └─ 取款完成,Coin 对象已到账                                     │
└─────────────────────────────────────────────────────────────────┘
```

**锁机制设计**:
```rust
pub struct WithdrawalLock {
    user: Address,
    asset: AssetId,
    amount: u64,
    created_at: u64,
    ttl: Duration, // 30s
}

impl WithdrawalLock {
    /// 检查是否过期
    pub fn is_expired(&self) -> bool {
        now() > self.created_at + self.ttl.as_millis()
    }

    /// 自动清理过期锁 (防止死锁)
    pub fn cleanup_expired_locks() {
        // 后台任务每 10s 扫描一次
    }
}
```

### 3.3 Standard Path (标准 Sui 交易, ~600ms)

**保持原有 Sui 流程不变**:

```
Client
  ↓ 提交交易
API Gateway
  ↓ 路由到 Mysticeti
Mysticeti Consensus (~400ms)
  ↓ 达成共识顺序
Move VM Execution (~200ms)
  ↓ 执行智能合约
Checkpoint & Storage
  ↓ 持久化
Client 收到确认 (~600ms)
```

---

## 4. 存储架构

### 4.1 RocksDB 表结构

**复用 `typed-store` 框架**,新增 DEX 专用表:

```rust
// /crates/sui-core/src/authority/authority_store_tables.rs

pub struct DexTables {
    /// 订单簿快照 (市场 ID → 订单簿状态)
    pub orderbook_snapshots: DBMap<MarketId, OrderbookSnapshot>,

    /// 账户余额 (账户 ID + 资产 ID → 余额)
    pub balances: DBMap<(AccountId, AssetId), Balance>,

    /// 永续合约持仓 (账户 ID + 合约 ID → 持仓)
    pub perpetual_positions: DBMap<(AccountId, ContractId), Position>,

    /// 资金费率历史 (合约 ID + 时间戳 → 费率)
    pub funding_rates: DBMap<(ContractId, Timestamp), FundingRate>,

    /// 成交历史 (交易 ID → 成交详情)
    pub trade_history: DBMap<TradeId, Trade>,

    /// 订单历史 (订单 ID → 订单状态)
    pub order_history: DBMap<OrderId, OrderStatus>,

    /// 清算事件 (清算 ID → 清算详情)
    pub liquidations: DBMap<LiquidationId, Liquidation>,

    /// Vault 股份 (用户 ID → 股份数)
    pub vault_shares: DBMap<UserId, VaultShare>,
}

/// 集成到 AuthorityStore
pub struct AuthorityStore {
    // ... 原有 Sui 表 ...

    /// DEX 专用表
    pub dex_tables: DexTables,
}
```

### 4.2 数据持久化策略

| 数据类型 | 持久化时机 | 存储位置 | RPO |
|---------|-----------|---------|-----|
| **活跃订单簿** | 每批次 | Memory Cache + WAL | 0 |
| **账户余额** | 每批次 | Memory Cache + WAL | 0 |
| **成交历史** | 每批次 | WAL → RocksDB | 0 |
| **订单簿快照** | 每小时 | Snapshot → RocksDB | ~1h |
| **资金费率** | 每小时计算 | 直接写 RocksDB | ~1h |

---

## 5. 网络拓扑

### 5.1 节点角色

```
┌───────────────────────────────────────────────────────────────┐
│  Sequencer Leader (当前轮换 Epoch)                            │
│  ├─ 角色: 接收订单、排序、执行、广播                           │
│  ├─ 硬件: 高频 CPU (5GHz+), 64GB RAM, NVMe SSD               │
│  └─ 网络: 专线连接到验证者集群                                 │
└───────────────────────────────────────────────────────────────┘
                         ↓ Tonic Network (P2P)
┌───────────────────────────────────────────────────────────────┐
│  Validator Cluster (2f+1 节点)                                │
│  ├─ 角色: 验证批次、持久化、返回签名                           │
│  ├─ 硬件: 标准 Sui 验证者配置                                 │
│  └─ 网络: 全网状连接 (anemo)                                  │
└───────────────────────────────────────────────────────────────┘
                         ↓ 可选
┌───────────────────────────────────────────────────────────────┐
│  Standby Sequencers (备用节点)                                │
│  ├─ 角色: 同步状态、健康检测、故障切换                         │
│  ├─ 数量: 2-3 个备用节点                                      │
│  └─ 切换时间: < 100ms                                         │
└───────────────────────────────────────────────────────────────┘
```

### 5.2 Tonic Network 配置

**复用 Sui 的 P2P 网络配置**:

```rust
// /consensus/core/src/network/tonic_network.rs

pub struct DexNetworkConfig {
    /// 连接窗口 (HTTP/2 流控)
    connection_window: u32, // 64 MiB

    /// 流窗口
    stream_window: u32, // 32 MiB

    /// 启用压缩 (Zstd)
    compression: bool, // true

    /// TCP 优化
    tcp_nodelay: bool, // true
    tcp_keepalive: Duration, // 10s

    /// 超时配置
    request_timeout: Duration, // 50ms (Sequencer 广播)
    response_timeout: Duration, // 100ms (验证者确认)
}
```

**网络优化**:
- **压缩**: Zstandard 压缩节省 70% 带宽
- **多路复用**: HTTP/2 单连接复用,减少握手开销
- **自适应超时**: 基于 RTT 估计动态调整超时

---

## 6. 安全与可靠性

### 6.1 故障恢复

#### 6.1.1 Sequencer 故障切换

**触发条件**:
- 心跳超时 (50ms 无响应)
- 恶意行为检测 (双重签名、序列号间隙)

**切换流程**:
```
1. 检测 Leader 故障 (50ms 心跳超时)
   ↓
2. 触发新 Leader 选举 (基于 DexSequencerSchedule)
   ↓
3. 新 Leader 从最新 Snapshot 加载状态
   ↓
4. 重放 Snapshot 后的 WAL 记录
   ↓
5. 验证状态哈希一致性
   ↓
6. 恢复服务,广播新 Leader 信息
   ↓
7. 总耗时: < 100ms (不触发 Snapshot 恢复)
          < 5min (触发 Snapshot 恢复)
```

#### 6.1.2 数据恢复

**RTO/RPO 目标**:
- **RPO (恢复点目标)**: 0 (无数据丢失,通过 WAL fsync 保证)
- **RTO (恢复时间目标)**: < 5min (通过 Snapshot + WAL 快速恢复)

**恢复流程**:
```rust
pub fn recover() -> Result<State> {
    // 1. 加载最新快照
    let snapshot = load_latest_snapshot()?;
    let mut state = decompress_snapshot(snapshot)?;

    // 2. 重放快照后的 WAL
    let wal_records = load_wal_since(snapshot.sequence)?;
    for record in wal_records {
        // 验证校验和
        if !record.verify_checksum() {
            return Err(RecoveryError::CorruptedWAL);
        }

        // 重放交易
        state.apply_batch(record.transactions)?;

        // 验证状态哈希
        if state.hash() != record.state_hash {
            return Err(RecoveryError::StateMismatch);
        }
    }

    // 3. 验证最终状态
    state.verify_invariants()?;

    Ok(state)
}
```

### 6.2 安全防护

#### 6.2.1 Slashing 机制

**惩罚场景**:

| 违规行为 | 检测方法 | 惩罚力度 |
|---------|---------|---------|
| **双重签名** | 检测同一序列号有不同签名 | 100% 质押没收 |
| **恶意审查** | 超时未处理合法交易 | 10% 质押罚款 |
| **序列号间隙** | 序列号不连续 | 5% 质押罚款 |
| **状态哈希不一致** | 批次状态哈希与执行结果不匹配 | 50% 质押没收 |

#### 6.2.2 DoS 防护

**多层限流**:

```rust
pub struct RateLimiter {
    /// IP 层限流 (每秒请求数)
    ip_limiter: DashMap<IpAddr, TokenBucket>,

    /// 账户层限流 (每秒订单数)
    account_limiter: DashMap<AccountId, TokenBucket>,

    /// 端点层限流 (全局 TPS 上限)
    global_limiter: TokenBucket,
}

impl RateLimiter {
    pub fn check_rate_limit(&self, req: &Request) -> Result<(), RateLimitError> {
        // 1. 检查全局限流
        if !self.global_limiter.consume(1) {
            return Err(RateLimitError::GlobalLimit);
        }

        // 2. 检查 IP 限流 (10K req/s per IP)
        let ip_bucket = self.ip_limiter.entry(req.ip).or_insert_with(|| {
            TokenBucket::new(10_000, Duration::from_secs(1))
        });
        if !ip_bucket.consume(1) {
            return Err(RateLimitError::IpLimit);
        }

        // 3. 检查账户限流 (1K orders/s per account)
        let account_bucket = self.account_limiter.entry(req.account).or_insert_with(|| {
            TokenBucket::new(1_000, Duration::from_secs(1))
        });
        if !account_bucket.consume(1) {
            return Err(RateLimitError::AccountLimit);
        }

        Ok(())
    }
}
```

#### 6.2.3 余额证明

**Merkle 状态树**:

```rust
pub struct StateTree {
    /// 账户余额 Merkle 树
    balance_tree: MerkleTree,

    /// 订单簿 Merkle 树
    orderbook_tree: MerkleTree,
}

impl StateTree {
    /// 生成账户余额证明
    pub fn prove_balance(
        &self,
        account: AccountId,
        asset: AssetId,
    ) -> BalanceProof {
        let leaf = self.balance_tree.leaf(account, asset);
        let proof = self.balance_tree.merkle_proof(leaf);

        BalanceProof {
            account,
            asset,
            balance: leaf.balance,
            merkle_proof: proof,
            root: self.balance_tree.root(),
        }
    }

    /// 验证余额证明
    pub fn verify_balance(proof: &BalanceProof) -> bool {
        MerkleTree::verify_proof(
            proof.merkle_proof,
            proof.root,
            proof.leaf_hash(),
        )
    }
}
```

**用途**:
- 用户可独立验证账户余额
- 审计节点可验证系统总账一致性
- 防止 Sequencer 篡改余额

---

## 7. 扩展性设计

### 7.1 Phase 2 演进路径

**目标**: 从中心化 Sequencer 演进到去中心化排序

| 演进阶段 | Sequencer 模式 | 延迟 | 去中心化程度 |
|---------|---------------|------|------------|
| **Phase 1.0** | 单 Sequencer + 2f+1 验证 | < 50ms | ⭐ (中心化排序) |
| **Phase 1.5** | 轮转 Sequencer (1 分钟轮换) | < 50ms | ⭐⭐ (多节点轮换) |
| **Phase 2.0** | 共享 Sequencer (如 Espresso) | ~100ms | ⭐⭐⭐ (去中心化排序) |
| **Phase 3.0** | 完全上链 (Mysticeti 共识) | ~400ms | ⭐⭐⭐⭐⭐ (完全去中心化) |

### 7.2 横向扩展

#### 7.2.1 市场分片

**策略**: 将不同市场路由到不同 Sequencer 实例

```
Sequencer-1: BTC-USD, ETH-USD (主流币)
Sequencer-2: LINK-USD, AVAX-USD (中盘币)
Sequencer-3: 长尾币市场
```

**优势**:
- 市场间完全并行,无锁竞争
- 单市场故障隔离
- 弹性扩展 (根据交易量动态分配)

#### 7.2.2 账户分片

**策略**: 按账户 ID 哈希分片余额缓存

```rust
pub struct ShardedBalanceCache {
    shards: Vec<DashMap<AccountId, Balance>>,
    num_shards: usize,
}

impl ShardedBalanceCache {
    pub fn get_shard(&self, account: AccountId) -> &DashMap<AccountId, Balance> {
        let shard_id = account.hash() % self.num_shards;
        &self.shards[shard_id]
    }
}
```

**优势**:
- 减少锁竞争
- 支持更高并发

---

## 8. 技术栈与依赖

### 8.1 核心依赖

| 组件 | 来源 | 版本 | 用途 |
|-----|------|------|-----|
| **typed-store** | Sui | latest | RocksDB 封装,持久化存储 |
| **mysten-network** | Sui | latest | Tonic P2P 网络 |
| **shared-crypto** | Sui | latest | Ed25519/BLS 签名验证 |
| **mysten-metrics** | Sui | latest | Prometheus 指标收集 |
| **bcs** | Sui | latest | 二进制序列化 |
| **dashmap** | 第三方 | 6.1 | 无锁并发 HashMap |
| **tokio** | 第三方 | 1.41 | 异步运行时 |
| **lz4** | 第三方 | 1.31 | 快照压缩 |

### 8.2 新增 Crates

```
sui/
├── crates/
│   ├── dex-types/          # 公共类型定义 (Order, Market, etc.)
│   ├── dex-sequencer/      # Sequencer 实现
│   ├── dex-engine/         # 撮合引擎
│   ├── dex-storage/        # 存储抽象 (WAL, Snapshot)
│   ├── dex-risk/           # 风控引擎
│   ├── dex-perpetual/      # 永续合约引擎
│   └── dex-framework/      # Move 框架 (存取款合约)
```

---

## 9. 部署架构

### 9.1 生产环境拓扑

```
┌─────────────────────────────────────────────────────────────┐
│  Load Balancer (Nginx/HAProxy)                              │
│  ├─ 健康检查 (每 1s)                                        │
│  └─ 流量分发 (轮询)                                         │
└─────────────────────────────────────────────────────────────┘
                         ↓
┌─────────────────────────────────────────────────────────────┐
│  API Gateway Cluster (3 节点)                               │
│  ├─ JSON-RPC Server (端口 9000)                            │
│  ├─ WebSocket Server (端口 9001)                           │
│  └─ Rate Limiter + Signature Verifier                      │
└─────────────────────────────────────────────────────────────┘
                         ↓ 内网专线
┌─────────────────────────────────────────────────────────────┐
│  Sequencer Cluster                                          │
│  ├─ Leader Sequencer (主节点)                               │
│  └─ Standby Sequencers (2 备用节点)                         │
└─────────────────────────────────────────────────────────────┘
                         ↓ Tonic Network
┌─────────────────────────────────────────────────────────────┐
│  Validator Cluster (2f+1 = 7 节点)                          │
│  ├─ 地理分布: US (2), EU (2), Asia (3)                      │
│  └─ 硬件: 32 核 CPU, 128GB RAM, 2TB NVMe                    │
└─────────────────────────────────────────────────────────────┘
```

### 9.2 监控与告警

**指标收集** (复用 `mysten-metrics`):

| 指标 | 类型 | 阈值告警 |
|-----|------|---------|
| `dex_order_latency_p99` | Histogram | > 50ms |
| `dex_matching_duration` | Histogram | > 10μs |
| `dex_sequencer_health` | Gauge | 0 (故障) |
| `dex_wal_fsync_duration` | Histogram | > 20ms |
| `dex_validator_confirmations` | Counter | < 2f+1 |
| `dex_memory_cache_hit_rate` | Gauge | < 95% |

**告警规则** (Prometheus):
```yaml
groups:
  - name: dex_critical
    rules:
      - alert: SequencerDown
        expr: dex_sequencer_health == 0
        for: 30s
        annotations:
          summary: "Sequencer 故障超过 30s"

      - alert: HighLatency
        expr: dex_order_latency_p99 > 0.05
        for: 1m
        annotations:
          summary: "P99 延迟 > 50ms 持续 1 分钟"
```

---

## 10. 演进路线

### 10.1 Phase 1 (当前) - 中心化 Sequencer

**时间线**: 2026 Q1-Q2

**功能**:
- ✅ 单 Sequencer + 2f+1 验证
- ✅ 现货交易 (Limit, Market, IOC, FOK)
- ✅ 存取款 (两阶段执行)
- ✅ 基础风控 (余额检查)

**性能**:
- 端到端延迟: < 50ms
- 吞吐量: 200,000 TPS
- 可用性: 99.9%

### 10.2 Phase 2 (未来) - 永续合约与去中心化

**时间线**: 2026 Q3-Q4

**新增功能**:
- 🔲 永续合约交易
- 🔲 资金费率机制
- 🔲 清算与 ADL
- 🔲 Vault 做市机制
- 🔲 共享 Sequencer (如 Espresso)

**性能优化**:
- 端到端延迟: < 100ms (去中心化排序)
- 吞吐量: 500,000 TPS (市场分片)
- 可用性: 99.99%

### 10.3 Phase 3 (长期) - 完全链上

**时间线**: 2027+

**目标**:
- 🔲 完全使用 Mysticeti 共识
- 🔲 原生并行执行优化
- 🔲 Layer 2 扩展方案

---

## 附录 A: 关键公式汇总

### A.1 风控公式

| 公式 | 说明 |
|-----|------|
| `NC = 资产价值 + 仓位净值` | 净抵押品 |
| `IMR = 名义价值 × 调整后保证金率` | 初始保证金 |
| `MMR = IMR × 维持比例` | 维持保证金 |
| `清算触发: NC < MMR` | 清算条件 |

### A.2 性能公式

| 公式 | 说明 |
|-----|------|
| `端到端延迟 = 网络延迟 + 排序延迟 + 执行延迟 + 返回延迟` | < 50ms 目标分解 |
| `吞吐量 = 批次大小 / 批次间隔` | 1000 tx / 5ms = 200K TPS |

---

## 附录 B: 设计决策记录 (ADR)

### ADR-001: 选择 Sui Fork 而非独立链

**上下文**: 需要快速构建高性能 DEX 基础设施

**决策**: 采用 Sui Fork 模式,直接修改 Sui 内核

**理由**:
- 复用 80% 成熟组件 (Tonic Network, typed-store)
- 深度集成,性能最优
- 开发周期短 (4-6 个月 vs 12+ 个月)

**代价**: 需跟进 Sui 升级,维护成本中等

---

### ADR-002: 原生 Rust 引擎绕过 Move VM

**上下文**: Move VM 执行开销 ~200ms,无法达成 < 10μs 撮合延迟

**决策**: DEX 交易使用原生 Rust 引擎,通过 Precompile 机制拦截

**理由**:
- 撮合算法需要极致性能 (< 10μs)
- Move VM 适合资产安全,但不适合高频交易

**代价**: 需维护两套执行路径 (Native + Move)

---

### ADR-003: 两阶段执行保证存取款原子性

**上下文**: 存取款涉及链上资产与 DEX 余额,需保证一致性

**决策**: 采用 Signing + Certificate 两阶段模型

**理由**:
- Phase 1 (Signing): Move VM 锁定资产,不修改 DEX 余额
- Phase 2 (Certificate): 验证 2f+1 后原子提交

**代价**: 存取款延迟增加到 < 100ms (仍可接受)

---

**文档版本**: v1.0
**作者**: DEX 架构团队
**最后更新**: 2026-01-07

**下一步行动**:
1. 评审本架构文档
2. 创建各模块详细设计文档
3. 启动 Phase 1 核心基础设施开发
