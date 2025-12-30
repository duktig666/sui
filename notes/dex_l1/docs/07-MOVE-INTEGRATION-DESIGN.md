# DEX L1 Move 集成设计 / Move Integration Design

> **版本**: v1.0
> **状态**: Draft
> **目标读者**: 技术评审 / 架构师

---

## 1. 概述 / Overview

### 1.1 设计目标 / Design Goals

1. **钱包兼容**: 标准 Sui 钱包无缝使用
2. **RPC 兼容**: 保持 Sui JSON-RPC API 兼容
3. **原子存取款**: 链上资产与 DEX 余额原子转换
4. **最小侵入**: 不修改 Move VM 核心

---

## 2. Precompile 架构 / Precompile Architecture

### 2.1 整体架构 / Overall Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Transaction Flow                          │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌─────────────────────────────────────────────────────────┐│
│  │  User Transaction                                       ││
│  │  { package: 0xDEX, function: "place_order", ... }      ││
│  └─────────────────────────────────────────────────────────┘│
│                           │                                  │
│                           ▼                                  │
│  ┌─────────────────────────────────────────────────────────┐│
│  │  Transaction Router (sui-core-ext)                      ││
│  │  ┌─────────────────────────────────────────────────┐   ││
│  │  │ Is DEX Precompile? (package == 0xDEX)           │   ││
│  │  └────────────────────────┬────────────────────────┘   ││
│  │                           │                            ││
│  │           ┌───────────────┴───────────────┐            ││
│  │           │ Yes                           │ No         ││
│  │           ▼                               ▼            ││
│  │  ┌─────────────────┐            ┌─────────────────┐   ││
│  │  │  DEX Native     │            │  Move VM        │   ││
│  │  │  Execution      │            │  Execution      │   ││
│  │  └─────────────────┘            └─────────────────┘   ││
│  └─────────────────────────────────────────────────────────┘│
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

### 2.2 Precompile 识别 / Precompile Identification

```rust
/// DEX Precompile 包地址
pub const DEX_PACKAGE_ID: ObjectID = ObjectID::from_hex_literal("0xDEX...").unwrap();

/// DEX 函数列表
pub const DEX_FUNCTIONS: &[&str] = &[
    "place_order",
    "cancel_order",
    "cancel_all_orders",
    "deposit",
    "withdraw",
    "get_balance",
    "get_orderbook",
];

/// 交易分类器
pub fn classify_transaction(tx: &Transaction) -> TransactionType {
    if let Some(call) = tx.as_programmable() {
        for cmd in &call.commands {
            if let Command::MoveCall(mc) = cmd {
                if mc.package == DEX_PACKAGE_ID {
                    return TransactionType::DexPrecompile;
                }
            }
        }
    }
    TransactionType::Standard
}
```

---

## 3. dex-framework Move 包 / dex-framework Move Package

### 3.1 模块结构 / Module Structure

```
dex-framework/
├── Move.toml
└── sources/
    ├── dex.move           # 主入口模块
    ├── order.move         # 订单类型
    ├── market.move        # 市场管理
    ├── account.move       # 账户管理
    └── events.move        # 事件定义
```

### 3.2 核心合约 / Core Contract

```move
/// dex-framework/sources/dex.move
module dex::dex {
    use sui::coin::{Self, Coin};
    use sui::transfer;
    use sui::tx_context::{Self, TxContext};

    /// 下单 (Precompile 拦截)
    public entry fun place_order<B, Q>(
        market_id: vector<u8>,
        side: u8,            // 0: Buy, 1: Sell
        order_type: u8,      // 0: Limit, 1: Market, ...
        price: u64,
        quantity: u64,
        ctx: &mut TxContext,
    ) {
        // 此函数被 Precompile 拦截
        // 实际执行由 DEX Native Engine 处理
        abort 0
    }

    /// 撤单 (Precompile 拦截)
    public entry fun cancel_order(
        order_id: vector<u8>,
        ctx: &mut TxContext,
    ) {
        abort 0
    }

    /// 存款 (Hybrid: Move + DEX)
    public entry fun deposit<T>(
        coin: Coin<T>,
        ctx: &mut TxContext,
    ) {
        // 1. 锁定代币到 DEX 托管账户
        let amount = coin::value(&coin);
        transfer::public_transfer(coin, @dex_custody);

        // 2. DEX Engine 通过事件监听更新余额
        // (通过 Precompile 回调)
    }

    /// 取款 (Hybrid: DEX + Move)
    public entry fun withdraw<T>(
        amount: u64,
        ctx: &mut TxContext,
    ) {
        // Precompile 拦截:
        // 1. DEX Engine 检查余额
        // 2. 扣减 DEX 余额
        // 3. 调用 Move 释放代币
        abort 0
    }
}
```

### 3.3 事件定义 / Event Definitions

```move
/// dex-framework/sources/events.move
module dex::events {
    use sui::event;

    /// 订单创建事件
    struct OrderPlaced has copy, drop {
        order_id: vector<u8>,
        market_id: vector<u8>,
        account: address,
        side: u8,
        price: u64,
        quantity: u64,
    }

    /// 订单成交事件
    struct OrderFilled has copy, drop {
        order_id: vector<u8>,
        trade_id: vector<u8>,
        price: u64,
        quantity: u64,
        is_maker: bool,
    }

    /// 订单取消事件
    struct OrderCancelled has copy, drop {
        order_id: vector<u8>,
        remaining: u64,
    }

    /// 存款事件
    struct Deposited has copy, drop {
        account: address,
        asset: vector<u8>,
        amount: u64,
    }

    /// 取款事件
    struct Withdrawn has copy, drop {
        account: address,
        asset: vector<u8>,
        amount: u64,
    }
}
```

---

## 4. 执行路径 / Execution Paths

### 4.1 读操作 / Read Operations

```
┌─────────────────────────────────────────────────────────────┐
│                    Read Path (查询余额/订单簿)               │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  Client ──► RPC Server ──► DEX Engine ──► Response          │
│                                                              │
│  不经过 Move VM，直接从 DEX 内存状态读取                     │
│  延迟: < 1ms                                                 │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

### 4.2 写操作 (纯 DEX) / Write Operations (DEX Only)

```
┌─────────────────────────────────────────────────────────────┐
│                    Write Path (下单/撤单)                    │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  Client ──► RPC ──► Authority ──► Router ──► DEX Engine     │
│                                     │                        │
│                                     │ (Precompile)           │
│                                     ▼                        │
│                              ┌─────────────┐                │
│                              │ 1. 签名验证  │                │
│                              │ 2. 序列号    │                │
│                              │ 3. 撮合执行  │                │
│                              │ 4. 状态更新  │                │
│                              └─────────────┘                │
│                                     │                        │
│                                     ▼                        │
│                              Effects + Events               │
│                                                              │
│  不经过 Move VM                                              │
│  延迟: < 50ms                                                │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

### 4.3 混合操作 / Hybrid Operations (存取款)

```
┌─────────────────────────────────────────────────────────────┐
│                    Hybrid Path (存款)                        │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  Phase 1: Move VM 执行                                       │
│  ┌─────────────────────────────────────────────────────────┐│
│  │ 1. 验证用户签名                                         ││
│  │ 2. 执行 Move 合约 (deposit)                             ││
│  │ 3. 转移代币到托管账户                                   ││
│  │ 4. 发出 Deposited 事件                                  ││
│  └─────────────────────────────────────────────────────────┘│
│                           │                                  │
│                           ▼                                  │
│  Phase 2: DEX Engine 处理                                    │
│  ┌─────────────────────────────────────────────────────────┐│
│  │ 1. 监听 Deposited 事件                                  ││
│  │ 2. 验证链上转账成功                                     ││
│  │ 3. 更新 DEX 余额                                        ││
│  └─────────────────────────────────────────────────────────┘│
│                                                              │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│                    Hybrid Path (取款)                        │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  Phase 1: DEX Engine 处理                                    │
│  ┌─────────────────────────────────────────────────────────┐│
│  │ 1. 验证 DEX 余额充足                                    ││
│  │ 2. 锁定 DEX 余额                                        ││
│  │ 3. 生成取款凭证                                         ││
│  └─────────────────────────────────────────────────────────┘│
│                           │                                  │
│                           ▼                                  │
│  Phase 2: Move VM 执行                                       │
│  ┌─────────────────────────────────────────────────────────┐│
│  │ 1. 验证取款凭证                                         ││
│  │ 2. 从托管账户释放代币                                   ││
│  │ 3. 转移代币到用户                                       ││
│  │ 4. 扣减 DEX 余额 (回调)                                 ││
│  └─────────────────────────────────────────────────────────┘│
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

---

## 5. Authority 集成点 / Authority Integration

### 5.1 交易路由注入 / Transaction Router Injection

```rust
/// sui-core-ext/src/router.rs

pub struct TransactionRouter {
    dex_engine: Arc<DexEngine>,
    move_executor: Arc<MoveExecutor>,
    classifier: TransactionClassifier,
}

impl TransactionRouter {
    pub async fn execute(&self, tx: Transaction) -> Result<Effects> {
        match self.classifier.classify(&tx) {
            TransactionType::DexPrecompile => {
                // DEX 原生执行
                self.dex_engine.execute(tx).await
            }
            TransactionType::DexHybrid => {
                // 混合执行 (存取款)
                self.execute_hybrid(tx).await
            }
            TransactionType::Standard => {
                // 标准 Move 执行
                self.move_executor.execute(tx).await
            }
        }
    }

    async fn execute_hybrid(&self, tx: Transaction) -> Result<Effects> {
        // 1. 执行 Move 部分
        let move_effects = self.move_executor.execute(tx.clone()).await?;

        // 2. 触发 DEX 回调
        let dex_effects = self.dex_engine.on_move_effects(&move_effects).await?;

        // 3. 合并 Effects
        Ok(Effects::merge(move_effects, dex_effects))
    }
}
```

### 5.2 集成入口 / Integration Entry Point

```rust
/// 在 Authority 中注入 Router
impl Authority {
    pub fn with_dex_router(mut self, router: TransactionRouter) -> Self {
        self.tx_router = Some(Arc::new(router));
        self
    }

    pub async fn handle_transaction(&self, tx: Transaction) -> Result<Effects> {
        if let Some(router) = &self.tx_router {
            router.execute(tx).await
        } else {
            // 默认 Move 执行
            self.execute_move(tx).await
        }
    }
}
```

---

## 6. 存取款原子性保证 / Deposit/Withdraw Atomicity

### 6.1 存款原子性 / Deposit Atomicity

```
┌─────────────────────────────────────────────────────────────┐
│                    Deposit Atomicity                         │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  保证: 链上代币转移 ↔ DEX 余额增加 原子性                   │
│                                                              │
│  机制:                                                       │
│  1. Move 交易成功 → 代币已转移                              │
│  2. 事件监听 → DEX 更新余额                                 │
│  3. 两者在同一个交易中完成                                   │
│                                                              │
│  失败处理:                                                   │
│  • Move 执行失败 → 整个交易回滚                             │
│  • DEX 更新失败 → 回滚代币转移 (通过 Effects)               │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

### 6.2 取款原子性 / Withdraw Atomicity

```
┌─────────────────────────────────────────────────────────────┐
│                    Withdraw Atomicity                        │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  两阶段提交 (2PC):                                          │
│                                                              │
│  Phase 1: Prepare                                            │
│  ┌─────────────────────────────────────────────────────────┐│
│  │ DEX: 检查余额 → 锁定金额 → 生成凭证                     ││
│  └─────────────────────────────────────────────────────────┘│
│                           │                                  │
│                           ▼                                  │
│  Phase 2: Commit / Rollback                                  │
│  ┌─────────────────────────────────────────────────────────┐│
│  │ Move: 执行取款                                          ││
│  │   成功 → DEX 扣减余额 (Commit)                          ││
│  │   失败 → DEX 解锁金额 (Rollback)                        ││
│  └─────────────────────────────────────────────────────────┘│
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

---

## 7. 钱包/RPC 兼容性 / Wallet/RPC Compatibility

### 7.1 RPC 扩展 / RPC Extensions

```rust
/// DEX RPC 扩展
pub trait DexRpcApi {
    /// 获取 DEX 余额
    async fn dex_get_balance(
        &self,
        account: SuiAddress,
        asset: Option<String>,
    ) -> RpcResult<Vec<DexBalance>>;

    /// 获取订单簿
    async fn dex_get_orderbook(
        &self,
        market_id: String,
        depth: Option<u32>,
    ) -> RpcResult<OrderBookDepth>;

    /// 获取用户订单
    async fn dex_get_orders(
        &self,
        account: SuiAddress,
        market_id: Option<String>,
    ) -> RpcResult<Vec<DexOrder>>;

    /// 获取成交历史
    async fn dex_get_trades(
        &self,
        market_id: String,
        limit: Option<u32>,
    ) -> RpcResult<Vec<DexTrade>>;
}
```

### 7.2 钱包兼容 / Wallet Compatibility

```
┌─────────────────────────────────────────────────────────────┐
│                    Wallet Integration                        │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  标准 Sui 钱包 (Sui Wallet, Suiet, etc.)                    │
│                                                              │
│  支持的操作:                                                 │
│  ✅ 连接钱包                                                 │
│  ✅ 签名交易 (标准 Sui 签名)                                │
│  ✅ 发送交易 (通过 Sui RPC)                                 │
│  ✅ 查看余额 (通过 DEX RPC 扩展)                            │
│                                                              │
│  实现方式:                                                   │
│  • DEX 交易构建为标准 Sui PTB                               │
│  • 使用标准签名流程                                          │
│  • 通过标准 RPC 提交                                         │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

---

## 8. Effects 生成 / Effects Generation

### 8.1 DEX Effects 结构 / DEX Effects Structure

```rust
/// DEX 执行效果
pub struct DexEffects {
    /// 交易摘要
    pub tx_digest: TxDigest,
    /// 序列号
    pub seq_number: SeqNumber,
    /// 状态变更
    pub state_changes: Vec<StateChange>,
    /// 事件
    pub events: Vec<DexEvent>,
    /// Gas 消耗
    pub gas_used: u64,
}

/// 转换为 Sui Effects
impl From<DexEffects> for TransactionEffects {
    fn from(dex: DexEffects) -> Self {
        TransactionEffects {
            status: ExecutionStatus::Success,
            gas_used: GasCostSummary::new(dex.gas_used, 0, 0),
            modified_at_versions: vec![],
            shared_objects: vec![],
            transaction_digest: dex.tx_digest,
            // ... 其他字段
        }
    }
}
```

---

## 9. 关键集成代码 / Key Integration Code

```rust
/// dex-integration/src/lib.rs

pub struct DexIntegration {
    engine: Arc<MatchingEngine>,
    sequencer: Arc<Sequencer>,
    storage: Arc<DexStorage>,
    move_bridge: Arc<MoveBridge>,
}

impl DexIntegration {
    /// 初始化集成
    pub fn new(
        config: DexConfig,
        authority: &Authority,
    ) -> Result<Self> {
        let storage = Arc::new(DexStorage::new(&config.storage)?);
        let engine = Arc::new(MatchingEngine::new(&config.engine, storage.clone())?);
        let sequencer = Arc::new(Sequencer::new(&config.sequencer)?);
        let move_bridge = Arc::new(MoveBridge::new(authority.move_executor())?);

        Ok(Self {
            engine,
            sequencer,
            storage,
            move_bridge,
        })
    }

    /// 创建交易路由器
    pub fn create_router(&self) -> TransactionRouter {
        TransactionRouter::new(
            self.engine.clone(),
            self.sequencer.clone(),
            self.move_bridge.clone(),
        )
    }
}
```

---

*文档版本: v1.0 | 最后更新: 2025-01-01*
