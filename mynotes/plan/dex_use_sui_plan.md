# DEX 第一阶段实施计划:如何与 Sui 结合(完整版)

> **版本**: v2.0
> **日期**: 2026-01-07
> **目标**: 基于 Sui Fork 实现中心化 Sequencer 的高性能 DEX (Phase 1)
> **参考文档**: `notes/dex_l1/drafts/dex-plan.md`, `notes/dex_l1/DEX_L1_DESIGN_SUMMARY.md`, `mynotes/dex/prd/DEX完整业务需求.md`

---

## 执行摘要 (Executive Summary)

本计划详细说明了 DEX 第一阶段如何与 Sui 结合,采用**中心化 Sequencer + 异步验证者确认**的架构,在不使用 Sui Mysticeti 共识的前提下,实现 **< 50ms 端到端延迟**和 **≥ 200,000 TPS** 的性能目标。

**核心策略**:
1. **复用 Sui 基础设施** (80%): 验证者网络、存储层、执行调度、P2P 通信
2. **绕过 Move VM**: DEX 交易使用原生 Rust 引擎执行
3. **轮转 Sequencer**: 复用 Sui Leader 选举机制实现高可用
4. **两阶段执行**: 存取款采用混合路径保证原子性

---

## 1. 架构概览

### 1.1 整体架构图

```
┌─────────────────────────────────────────────────────────────────┐
│                      DEX L1 Architecture                        │
│                   (基于 Sui Fork - Phase 1)                      │
├─────────────────────────────────────────────────────────────────┤
│  Client Layer                                                   │
│  └── Sui SDK/Wallets ── JSON-RPC ── WebSocket                  │
├─────────────────────────────────────────────────────────────────┤
│  Transaction Router (NEW)                                       │
│  └── classify_transaction() → DEX / Standard / Hybrid          │
│      ├─ is_dex_transaction()     (检查包地址 0xDEX)             │
│      ├─ is_deposit_withdrawal()   (涉及链上资产)                 │
│      └─ route_to_path()           (分流到不同路径)               │
├─────────────────────────────────────────────────────────────────┤
│  Sequencer Layer (NEW) - 复用 Sui Leader Schedule               │
│  └── DexSequencerService                                        │
│      ├─ Leader Election (复用 leader_schedule.rs)               │
│      ├─ Sequence Assignment ([Epoch:16][Counter:48])           │
│      ├─ Batch Aggregation (5ms 或 1000 tx)                     │
│      └─ Broadcast via Tonic Network (复用 P2P)                  │
├─────────────────────────────────────────────────────────────────┤
│  Native DEX Engine (NEW)                                        │
│  └── Matching Engine (Rust Native)                             │
│      ├─ BTreeMap Orderbook (价格-时间优先)                       │
│      ├─ DashMap Balance Cache (无锁并发)                        │
│      ├─ Risk Engine (保证金/清算)                                │
│      └─ Perpetual Engine (资金费率)                             │
├─────────────────────────────────────────────────────────────────┤
│  Modified Sui Execution Layer                                  │
│  └── Precompile Interceptor (在 execution_engine.rs 中)        │
│      ├─ 检测 0xDEX 包调用                                       │
│      ├─ DEX Fast Path → Native Engine                         │
│      └─ 其他交易 → Move VM (正常流程)                           │
├─────────────────────────────────────────────────────────────────┤
│  Storage Layer (复用 Sui)                                       │
│  └── ExecutionCache (内存) → typed-store (RocksDB)             │
│      ├─ DexOrderbookStore (新增表)                             │
│      ├─ DexBalanceStore (新增表)                               │
│      ├─ DexPerpetualStore (新增表)                             │
│      └─ WAL (Group Commit, < 10ms)                            │
└─────────────────────────────────────────────────────────────────┘
```

### 1.2 三条交易路径

| 路径 | 交易类型 | 执行方式 | 延迟目标 |
|-----|---------|---------|---------|
| **Fast Path** | 纯 DEX 指令 (下单/撤单) | Sequencer → Native Engine | < 50ms |
| **Standard Path** | 标准 Sui 交易 (Shared Obj) | Mysticeti → Move VM | ~600ms |
| **Hybrid Path** | 存取款 (涉及链上资产) | 两阶段执行 (Signing + Cert) | < 100ms |

---

## 2. 与 Sui 结合的核心模块

### 2.1 复用的 Sui 模块 ✅

| Sui 模块 | 路径 | 复用方式 | 用途 |
|---------|------|---------|------|
| **typed-store** | `crates/typed-store/` | 直接复用 | RocksDB 持久化存储 |
| **ExecutionCache** | `crates/sui-core/src/execution_cache.rs` | 直接复用 | 内存状态缓存 |
| **Leader Schedule** | `consensus/core/src/leader_schedule.rs` | **修改扩展** | Sequencer Leader 选举 |
| **Tonic Network** | `consensus/core/src/network/tonic_network.rs` | 直接复用 | 验证者间 P2P 通信 |
| **AuthorityState** | `crates/sui-core/src/authority.rs` | **修改扩展** | 交易路由和状态管理 |
| **mysten-metrics** | `crates/mysten-metrics/` | 直接复用 | Prometheus 指标收集 |
| **shared-crypto** | `crates/shared-crypto/` | 直接复用 | 签名验证 |
| **DashMap** | 第三方库 | 直接使用 | 无锁并发 Map |

### 2.2 需要修改的 Sui 文件 🔧

#### P0 - 核心路由和执行

**1. `/crates/sui-core/src/authority.rs` - 交易路由器**

```rust
// 新增 DEX 交易检测
fn is_dex_transaction(tx: &Transaction) -> bool {
    tx.data()
        .intent_message()
        .value
        .kind()
        .input_objects()
        .iter()
        .any(|obj| obj.object_id() == DEX_PACKAGE_ADDRESS)
}

// 修改 handle_transaction 添加路由逻辑
pub async fn handle_transaction(
    &self,
    transaction: Transaction,
) -> Result<HandleTransactionResponse, SuiError> {
    // 新增: 检测交易类型并路由
    if is_dex_transaction(&transaction) {
        // Fast Path: 直接发送到 DEX Sequencer
        return self.dex_sequencer
            .submit_transaction(transaction)
            .await;
    } else if is_deposit_withdrawal(&transaction) {
        // Hybrid Path: 两阶段执行
        return self.handle_dex_hybrid_transaction(transaction).await;
    }

    // Standard Path: 原有 Sui 流程 (Mysticeti + Move VM)
    self.handle_transaction_impl(transaction).await
}

// 新增: AuthorityState 字段
pub struct AuthorityState {
    // ... 原有字段 ...

    /// DEX Sequencer (新增)
    dex_sequencer: Arc<DexSequencerService>,

    /// DEX Engine (新增)
    dex_engine: Arc<RwLock<MatchingEngine>>,
}
```

**2. `/sui-execution/latest/sui-adapter/src/execution_engine.rs` - Precompile 拦截**

```rust
// 在 execute_transaction_to_effects() 开头添加
pub fn execute_transaction_to_effects(
    &self,
    transaction: &VerifiedExecutableTransaction,
    // ...
) -> Result<TransactionEffects, ExecutionError> {
    // 新增: DEX Precompile 拦截
    if self.is_dex_precompile_call(transaction) {
        return self.execute_dex_precompile(transaction);
    }

    // 原有 Move VM 执行流程...
}

fn is_dex_precompile_call(&self, tx: &VerifiedExecutableTransaction) -> bool {
    // 检查是否调用 0xDEX 包的函数
    matches!(
        tx.kind().package(),
        Some(pkg) if pkg == &DEX_PACKAGE_ID
    )
}

fn execute_dex_precompile(
    &self,
    tx: &VerifiedExecutableTransaction,
) -> Result<TransactionEffects, ExecutionError> {
    // 调用原生 DEX Engine
    let engine = self.dex_engine.read().unwrap();
    engine.execute_transaction(tx)
}
```

**3. `/crates/sui-core/src/consensus_adapter.rs` - Sequencer 集成**

```rust
pub struct ConsensusAdapter {
    // ... 原有字段 ...

    /// DEX Sequencer Client (新增)
    dex_sequencer: Arc<DexSequencerClient>,
}

// 新增方法
impl ConsensusAdapter {
    pub async fn submit_to_dex_sequencer(
        &self,
        transaction: &ConsensusTransaction,
    ) -> SuiResult<()> {
        // 发送到 Sequencer 而非 Mysticeti
        self.dex_sequencer.submit(transaction).await
    }
}
```

#### P1 - 配置和存储

**4. `/crates/sui-core/src/authority/authority_store_tables.rs` - 新增 DEX 表**

```rust
// 新增 DEX 专用表
pub struct DexTables {
    /// 订单簿状态
    pub orderbook: DBMap<MarketId, OrderbookSnapshot>,

    /// 账户余额
    pub balances: DBMap<(UserId, AssetId), Balance>,

    /// 永续合约持仓
    pub perpetual_positions: DBMap<(UserId, ContractId), Position>,

    /// 资金费率历史
    pub funding_rates: DBMap<(ContractId, Timestamp), FundingRate>,
}

// 集成到 AuthorityStore
pub struct AuthorityStore {
    // ... 原有字段 ...

    /// DEX 表 (新增)
    pub dex_tables: DexTables,
}
```

**5. `/consensus/core/src/leader_schedule.rs` - Sequencer Leader 选举**

```rust
// 扩展原有 LeaderSchedule 支持 DEX Sequencer
pub struct DexSequencerSchedule {
    /// 复用 Mysticeti 的 leader 调度
    inner: LeaderSchedule,

    /// Sequencer epoch (比共识 epoch 更短,如 1 分钟)
    sequencer_epoch_duration: Duration,
}

impl DexSequencerSchedule {
    /// 基于时间戳的确定性轮转
    pub fn current_sequencer_leader(&self, timestamp: u64) -> AuthorityIndex {
        let epoch = timestamp / self.sequencer_epoch_duration.as_millis() as u64;
        let committee = self.inner.committee();
        committee.leader_by_epoch(epoch)
    }

    /// 故障切换:跳到下一个 leader
    pub fn next_leader(&self, failed_leader: AuthorityIndex) -> AuthorityIndex {
        self.inner.elect_leader_excluding(failed_leader)
    }
}
```

**6. `/crates/sui-node/src/lib.rs` - 节点初始化**

```rust
pub async fn build_authority_server(
    config: &NodeConfig,
    // ...
) -> SuiNode {
    // ... 原有初始化 ...

    // 新增: 初始化 DEX Sequencer
    let dex_sequencer = if config.dex_mode_enabled {
        Some(Arc::new(
            DexSequencerService::new(
                config.dex_config.clone(),
                network.clone(), // 复用 Tonic Network
                metrics_registry.clone(),
            )
            .await?
        ))
    } else {
        None
    };

    // 新增: 初始化 DEX Engine
    let dex_engine = if config.dex_mode_enabled {
        Some(Arc::new(RwLock::new(
            MatchingEngine::new(
                dex_store.clone(),
                config.dex_config.engine_config.clone(),
            )
        )))
    } else {
        None
    };

    // ... 剩余初始化 ...
}
```

---

## 3. 使用的 Sui 特性和内容

### 3.1 核心特性矩阵

| Sui 特性 | 使用方式 | DEX 用途 | 重要性 |
|---------|---------|---------|--------|
| **对象模型** | 部分使用 | 存取款时操作 Coin 对象 | ⭐⭐⭐ |
| **typed-store** | 完全复用 | DEX 状态持久化 | ⭐⭐⭐⭐⭐ |
| **ExecutionCache** | 完全复用 | 内存状态缓存 | ⭐⭐⭐⭐ |
| **Leader Schedule** | 修改扩展 | Sequencer Leader 选举 | ⭐⭐⭐⭐⭐ |
| **Tonic Network** | 完全复用 | 验证者 P2P 通信 | ⭐⭐⭐⭐⭐ |
| **Mysticeti Consensus** | **不使用** | Phase 1 绕过共识 | N/A |
| **Move VM** | 部分使用 | 仅用于存取款 | ⭐⭐⭐ |
| **签名验证** | 完全复用 | 交易签名验证 | ⭐⭐⭐⭐⭐ |
| **Metrics** | 完全复用 | 性能监控 | ⭐⭐⭐⭐ |
| **CheckpointService** | 完全复用 | 状态快照和恢复 | ⭐⭐⭐⭐ |

### 3.2 Tonic Network 的作用与 DEX 集成

#### 3.2.1 Tonic Network 解决的核心问题

根据 `notes/SUI_NETWORK_PROPAGATION_ANALYSIS.md` 的分析,Tonic Network 是 Sui 共识层的高性能 P2P 通信框架,主要解决:

1. **验证者间高效通信**
   - 基于 gRPC/Tonic (HTTP/2)
   - 支持 Zstandard 压缩 (70% 带宽节省)
   - 内置连接池和多路复用

2. **共识消息传播**
   - 推送路径 (Broadcaster): < 10ms 低延迟
   - 拉取路径 (Subscriber): 高可靠性
   - 自适应超时机制 (基于 RTT 估计)

3. **网络优化**
   - 连接窗口: 64 MiB
   - 流窗口: 32 MiB
   - TCP_NODELAY / SO_REUSEADDR

#### 3.2.2 DEX 第一阶段如何利用 Tonic Network

**场景 1: Sequencer 批次广播**

```rust
// 主 Sequencer 向所有验证者广播批次
pub struct DexSequencerService {
    network: Network, // 复用 Tonic Network
}

impl DexSequencerService {
    pub async fn broadcast_batch(&self, batch: SequencedBatch) {
        // 并行广播到所有验证者
        let futures = self.committee.authorities()
            .filter(|idx| *idx != self.own_index)
            .map(|authority_idx| {
                let mut client = DexSequencerClient::new(
                    self.network.peer(authority_idx).unwrap()
                );
                async move {
                    client.apply_batch(Request::new(batch.clone()))
                        .await
                }
            })
            .collect::<Vec<_>>();

        // 等待 2f+1 确认
        let confirmations = futures::future::join_all(futures).await;
        self.verify_quorum(confirmations);
    }
}
```

**场景 2: 从节点接收订单并转发**

```rust
// 从节点接收客户订单后转发给主 Sequencer
pub async fn handle_client_order(&self, order: Order) -> Result<SoftConfirmation> {
    // 1. 本地验证签名 (复用 shared-crypto)
    self.verify_order_signature(&order)?;

    // 2. 转发给当前 Leader Sequencer
    let leader = self.sequencer_schedule.current_sequencer_leader(now());
    let mut client = DexSequencerClient::new(
        self.network.peer(leader).unwrap()
    );

    // 3. 通过 Tonic Network 发送
    let response = client.submit_order(Request::new(order))
        .timeout(Duration::from_millis(50)) // 50ms 超时
        .await?;

    Ok(response.into_inner())
}
```

**场景 3: Sequencer 故障切换心跳检测**

```rust
pub struct SequencerFailover {
    network: Network,
    heartbeat_timeout: Duration, // 50ms
}

impl SequencerFailover {
    pub async fn monitor_leader_health(&self) {
        loop {
            tokio::time::sleep(Duration::from_millis(50)).await;

            let leader = self.sequencer_schedule.current_leader();
            let mut client = DexSequencerClient::new(
                self.network.peer(leader).unwrap()
            );

            // 发送心跳
            match client.heartbeat(Request::new(()))
                .timeout(self.heartbeat_timeout)
                .await
            {
                Ok(_) => continue, // Leader 健康
                Err(_) => {
                    // Leader 超时,触发故障切换
                    self.trigger_failover(leader).await;
                }
            }
        }
    }
}
```

#### 3.2.3 Tonic Network 为 DEX 带来的优势

| 优势维度 | 具体收益 |
|---------|---------|
| **延迟** | HTTP/2 多路复用减少连接建立开销 (~30% 延迟降低) |
| **带宽** | Zstd 压缩节省 70% 带宽 |
| **可靠性** | 连接池自动重连,故障透明切换 |
| **开发成本** | 免去从零实现 P2P 网络层 |
| **成熟度** | Sui 主网验证的生产级代码 |

---

## 4. DEX 完整业务需求与 Sui 集成分析

根据 `mynotes/dex/prd/DEX完整业务需求.md`,完整需求包含 12 个模块。下面分析哪些可以与 Sui 结合,哪些需要自行 Rust 开发。

### 4.1 需求模块分类

| 模块 | 与 Sui 集成度 | 实现方式 | 说明 |
|-----|-------------|---------|------|
| **1. 账户模块** | 🟡 部分集成 | 混合实现 | - **Sui 提供**: 链上账户系统 (ObjectID)<br>- **DEX 自建**: 子账户管理、保证金模式 |
| **2. 资产模块** | 🟢 高度集成 | 复用 Sui | - **Sui 提供**: Coin 对象、余额存储<br>- **DEX 扩展**: Quantums 换算、内部余额映射 |
| **3. 风险控制** | 🔴 完全自建 | 原生 Rust | - **DEX 实现**: NC/IMR/MMR 计算、OIMF 动态保证金 |
| **4. 上币与市场** | 🟡 部分集成 | 混合实现 | - **Sui 提供**: 管理员权限 (OwnerCap)<br>- **DEX 自建**: 市场状态管理、精度参数 |
| **5. 价格预言机** | 🟢 高度集成 | 复用 Sui | - **可选项 1**: 复用 Sui 的 oracle 框架<br>- **可选项 2**: 集成 Pyth/Chainlink (通过 Sui Move) |
| **6. 合约模块** | 🔴 完全自建 | 原生 Rust | - **DEX 实现**: 永续合约参数、资金费率索引、流动性层级 |
| **7. 撮合结算** | 🔴 完全自建 | 原生 Rust | - **DEX 实现**: 订单簿、撮合引擎、结算逻辑 |
| **8. 资金费率** | 🔴 完全自建 | 原生 Rust | - **DEX 实现**: 溢价采样、费率计算、索引更新 |
| **9. 清算模块** | 🔴 完全自建 | 原生 Rust | - **DEX 实现**: 清算触发、破产价计算、保险基金 |
| **10. 手续费层** | 🟡 部分集成 | 混合实现 | - **Sui 提供**: 费用收取框架<br>- **DEX 自建**: VIP 层级、推荐人返佣 |
| **11. 交易奖励** | 🟢 高度集成 | 复用 Sui | - **Sui 提供**: 代币铸造、分发机制<br>- **DEX 扩展**: 奖励规则计算 |
| **12. Vault 机制** | 🟡 部分集成 | 混合实现 | - **Sui 提供**: 股份 NFT、资产托管<br>- **DEX 自建**: 做市策略、收益计算 |

**图例**:
- 🟢 高度集成 (>70% 复用 Sui)
- 🟡 部分集成 (30%-70% 复用 Sui)
- 🔴 完全自建 (<30% 复用 Sui)

### 4.2 具体实现建议

#### 模块 1: 账户模块

**Sui 提供部分**:
```move
// 使用 Sui 对象作为账户标识
module dex::account {
    struct Account has key {
        id: UID,
        owner: address,
        // Sui 提供的基础字段
    }
}
```

**DEX 自建部分** (Rust):
```rust
// 子账户和保证金模式在链外管理
pub struct SubAccount {
    wallet_address: SuiAddress,
    sub_account_id: u32, // 0-127999
    margin_mode: MarginMode, // Cross / Isolated
    asset_holdings: HashMap<AssetId, Balance>,
    perpetual_positions: HashMap<ContractId, Position>,
}
```

#### 模块 2: 资产模块

**完全复用 Sui Coin**:
```move
module dex::assets {
    use sui::coin::{Self, Coin};
    use sui::balance::{Self, Balance};

    // 存款: 将 Sui Coin 转换为 DEX 内部余额
    public entry fun deposit_usdc(
        coin: Coin<USDC>,
        account: &mut Account,
        ctx: &mut TxContext,
    ) {
        let amount = coin::value(&coin);
        let quantums = amount * 1_000_000; // 换算为 quantums
        account.balance = account.balance + quantums;
        transfer::public_transfer(coin, DEX_CUSTODY_ADDRESS);
    }
}
```

#### 模块 3-9: 交易核心逻辑 (完全 Rust 实现)

这些模块是 DEX 的核心竞争力,需要极致性能,**不使用 Sui/Move**,而是:

```rust
// crates/dex-engine/src/matching.rs
pub struct MatchingEngine {
    orderbooks: DashMap<MarketId, Orderbook>,
    risk_engine: Arc<RiskEngine>,
    funding_rate_engine: Arc<FundingRateEngine>,
    liquidation_engine: Arc<LiquidationEngine>,
}

impl MatchingEngine {
    pub fn match_order(&mut self, order: Order) -> MatchResult {
        // < 10μs 撮合逻辑
        let book = self.orderbooks.get_mut(&order.market_id)?;
        let matches = book.match_taker(order);
        self.settle_matches(matches)
    }
}
```

#### 模块 10-12: 激励与 Vault (部分集成)

**Sui 提供股份代币**:
```move
module dex::vault {
    struct VaultShare has key, store {
        id: UID,
        shares: u64,
    }

    // 存入时铸造股份 NFT
    public fun deposit(
        vault: &mut Vault,
        coin: Coin<USDC>,
        ctx: &mut TxContext,
    ): VaultShare {
        let amount = coin::value(&coin);
        let shares = calculate_shares(vault, amount);

        VaultShare {
            id: object::new(ctx),
            shares,
        }
    }
}
```

**DEX Rust 计算收益**:
```rust
// 链外计算 Vault 做市收益
pub fn calculate_vault_pnl(vault_id: VaultId) -> i64 {
    let positions = self.get_vault_positions(vault_id);
    positions.iter().map(|p| p.unrealized_pnl()).sum()
}
```

---

## 5. 集成方式选择:Sui Fork vs SDK 引入

### 5.1 两种方案对比

| 维度 | Sui Fork (在 Sui 仓库开发) | SDK 引入 (独立项目) |
|-----|---------------------------|-------------------|
| **开发模式** | 直接修改 Sui 代码 | 依赖 Sui crates 作为库 |
| **代码位置** | `sui/crates/dex-*` | `dex-l1/crates/dex-*` |
| **编译方式** | 单一 workspace | 多 workspace (Sui + DEX) |
| **升级策略** | 随 Sui 升级 | 独立控制依赖版本 |
| **集成深度** | 深度集成 (修改内核) | 有限集成 (仅 API 调用) |

### 5.2 关键决策因素分析

#### 因素 1: 需要修改的 Sui 核心文件

根据第 2.2 节,我们**必须修改**以下文件:
- `authority.rs` - 交易路由
- `execution_engine.rs` - Precompile 拦截
- `consensus_adapter.rs` - Sequencer 集成
- `leader_schedule.rs` - Leader 选举扩展

这些文件在 Sui 内核中,**无法通过外部 SDK 方式修改**。

#### 因素 2: Tonic Network 的深度依赖

Tonic Network 是 `consensus/core/src/network/tonic_network.rs` 的内部实现,不是公开 API。要复用需要:

```rust
// 如果是 SDK 方式,无法访问这些内部结构
use consensus::network::tonic_network::{TonicManager, Network};
// ❌ 这些模块可能不在 public API 中
```

#### 因素 3: typed-store 和 ExecutionCache 的集成

这些组件与 `AuthorityState` 紧密耦合:

```rust
// AuthorityState 持有 store 引用
pub struct AuthorityState {
    pub(crate) database: Arc<AuthorityStore>,
    pub(crate) execution_cache: Arc<ExecutionCache>,
}

// DEX 需要新增表,必须修改 AuthorityStore 定义
pub struct AuthorityStore {
    // 原有表...
    pub dex_tables: DexTables, // ❌ 无法在外部添加
}
```

### 5.3 推荐方案:**Sui Fork (在 Sui 仓库开发)**

**理由**:
1. ✅ **必须修改内核**: `authority.rs`, `execution_engine.rs` 等核心文件
2. ✅ **深度集成 Tonic Network**: 需要访问内部 P2P 实现
3. ✅ **共享 AuthorityState**: DEX 和 Sui 共用同一个节点进程
4. ✅ **简化部署**: 单一二进制文件,无需多进程协调
5. ✅ **性能最优**: 零开销集成,无跨进程通信

**可能的担忧与解答**:

| 担忧 | 解答 |
|-----|------|
| 难以跟进 Sui 升级? | - 使用 Git submodule 或定期 rebase<br>- DEX 代码隔离在 `crates/dex-*` 下 |
| 维护成本高? | - 修改点集中在 6 个核心文件<br>- 通过 feature flag 控制 DEX 模式 |
| 无法独立发布? | - 可以 fork Sui 为 `dex-sui` 独立仓库<br>- 保持上游同步策略 |

### 5.4 具体实施方案

#### 方案 A: 直接 Fork Sui 仓库 (推荐)

```bash
# 1. Fork Sui
git clone https://github.com/your-org/sui.git dex-sui
cd dex-sui

# 2. 添加 DEX crates
mkdir -p crates/dex-{types,sequencer,engine,storage}

# 3. 修改 Cargo.toml
[workspace]
members = [
    # ... 原有 Sui crates ...
    "crates/dex-types",
    "crates/dex-sequencer",
    "crates/dex-engine",
    "crates/dex-storage",
]

# 4. 修改核心文件 (带 feature flag)
# crates/sui-core/src/authority.rs
#[cfg(feature = "dex-mode")]
use dex_sequencer::DexSequencerService;
```

#### 方案 B: 保持 Sui 原仓库,通过 patch (备选)

如果未来想合并回 Sui 主线:

```toml
# Cargo.toml
[patch.crates-io]
sui-core = { path = "../sui/crates/sui-core" }  # 本地修改版本
```

但这种方式**不推荐**,因为修改太深入。

---

## 6. 实施思路总结

### 6.1 如何与 Sui 结合

**1. 在 Sui 节点上扩展 DEX 功能**
- 在 `AuthorityState` 中添加 DEX 组件 (`dex_sequencer`, `dex_engine`)
- 在节点启动时初始化 DEX 组件 (`sui-node/src/lib.rs`)

**2. 交易路由分流**
- 在 `authority.rs` 的 `handle_transaction()` 中分类交易
- DEX 交易走 Fast Path (Sequencer → Native Engine)
- 标准 Sui 交易保持原有流程 (Mysticeti → Move VM)

**3. Precompile 机制绕过 Move VM**
- 在 `execution_engine.rs` 拦截 0xDEX 包调用
- 转向原生 Rust 撮合引擎,避免 Move VM 开销

**4. 复用验证者网络实现 Sequencer 高可用**
- 扩展 `leader_schedule.rs` 实现轮转 Sequencer
- 使用 Tonic Network 进行 P2P 通信和批次广播
- 50ms 心跳检测,< 100ms 故障切换

**5. 复用 Sui 存储层**
- 在 `authority_store_tables.rs` 新增 DEX 专用表
- 使用 typed-store (RocksDB) 持久化
- 使用 ExecutionCache 提供内存缓存

**6. 两阶段执行保证原子性**
- Phase 1 (Signing): Move VM 计算效果,创建锁,禁止修改余额
- Phase 2 (Certificate): 验证 2f+1 证书,DEX Engine 执行,Commit

### 6.2 核心决策

| 维度 | 决策 | 理由 |
|-----|------|------|
| **共识** | 不使用 Mysticeti,采用中心化 Sequencer | 达成 < 50ms 延迟目标 |
| **执行** | 原生 Rust 引擎,绕过 Move VM | 达成 < 10μs 撮合延迟 |
| **存储** | 复用 typed-store (RocksDB) | 成熟稳定,免去重复开发 |
| **网络** | 复用 Tonic P2P | 高性能,已在 Sui 验证 |
| **高可用** | 复用 Leader Schedule 机制 | 权益加权,自动故障检测 |
| **安全** | 两阶段执行 + 不变量保护 | 保证原子性和一致性 |
| **集成方式** | **Sui Fork (直接在 Sui 仓库开发)** | 必须修改内核,深度集成 |

### 6.3 复用 vs 新建

**复用 Sui** (80%):
- ✅ typed-store (RocksDB)
- ✅ ExecutionCache
- ✅ Leader Schedule (扩展)
- ✅ Tonic Network
- ✅ shared-crypto
- ✅ mysten-metrics
- ✅ CheckpointService

**新建 DEX** (20%):
- 🆕 Matching Engine (原生 Rust)
- 🆕 DexSequencer
- 🆕 WriteAheadLog
- 🆕 Risk Engine
- 🆕 DEX Move Framework

---

## 7. 性能预期

| 指标 | 目标 | 当前 Sui | 提升倍数 |
|-----|------|---------|---------|
| 端到端延迟 (P99) | **< 50ms** | ~700ms | **14x** |
| 撮合吞吐量 (TPS) | **≥ 200,000** | ~2,000 | **100x** |
| 单次撮合延迟 | **< 10μs** | N/A | - |
| 软确认延迟 | **< 50ms** | ~2s | **40x** |
| 硬确认延迟 | **< 100ms** | ~2s | **20x** |

---

## 8. 实施路线图

### Phase 1.1: 核心基础设施 (4-6 周)
- 创建 DEX crates (`dex-types`, `dex-engine`, `dex-sequencer`, `dex-storage`)
- 实现 MatchingEngine 核心撮合逻辑
- 修改 `authority.rs` 和 `execution_engine.rs`
- **目标**: 10,000 TPS, < 50ms 延迟

### Phase 1.2: Sequencer 高可用 (2-3 周)
- 扩展 `leader_schedule.rs` 实现 DexSequencerSchedule
- 实现心跳检测和故障切换
- 2f+1 确认机制
- **目标**: < 100ms 切换,无数据丢失

### Phase 1.3: Move 集成和存取款 (3-4 周)
- 创建 DEX Move framework (0xDEX 包)
- 实现两阶段执行模型
- 实现取款锁机制
- **目标**: 原子性保证,< 100ms 存取款延迟

### Phase 1.4: 性能优化和测试 (3-4 周)
- SIMD 加速 (AVX2)
- 内存布局优化 (64 字节对齐)
- 压力测试和故障注入
- **目标**: 200,000 TPS, 99.99% 可用性

---

## 9. 关键文件清单

### 需修改的 Sui 文件 (6 个)
1. `/crates/sui-core/src/authority.rs` - 交易路由
2. `/sui-execution/latest/sui-adapter/src/execution_engine.rs` - Precompile 拦截
3. `/crates/sui-core/src/authority/authority_store_tables.rs` - 新增 DEX 表
4. `/consensus/core/src/leader_schedule.rs` - Sequencer 选举
5. `/crates/sui-node/src/lib.rs` - 节点初始化
6. `/crates/sui-config/src/node.rs` - 配置管理

### 新增的 DEX Crates (4 个)
1. `crates/dex-types/` - 类型定义
2. `crates/dex-sequencer/` - 交易排序
3. `crates/dex-engine/` - 撮合引擎
4. `crates/dex-storage/` - 存储抽象

---

## 10. 风险与缓解

| 风险 | 影响 | 缓解措施 |
|-----|------|---------|
| Sui 升级导致冲突 | 高 | - 使用 feature flag 隔离 DEX 代码<br>- 定期 rebase 上游变更<br>- 自动化回归测试 |
| Sequencer 单点故障 | 高 | - 轮转 Leader 机制<br>- 50ms 心跳检测<br>- < 100ms 故障切换 |
| 存取款原子性破坏 | 严重 | - 形式化验证不变量<br>- 两阶段执行模型<br>- 锁机制 + TTL |
| 性能无法达标 | 中 | - SIMD 优化<br>- 内存对齐<br>- 压力测试验证 |

---

**文档版本**: v2.0 (完整版)
**作者**: DEX 团队
**最后更新**: 2026-01-07

**关键结论**:
1. ✅ **必须采用 Sui Fork 方式**,在 Sui 仓库直接开发
2. ✅ **80% 复用 Sui 基础设施**,20% 新建 DEX 核心
3. ✅ **Tonic Network 是关键**,支撑 Sequencer 高可用和验证者通信
4. ✅ **12 个业务模块**中,5 个可集成 Sui,7 个需原生 Rust 实现
