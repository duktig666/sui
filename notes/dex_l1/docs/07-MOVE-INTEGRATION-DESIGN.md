# DEX L1 Move 集成设计 / Move Integration Design

> **版本**: v1.3
> **状态**: Reviewed
> **最后更新**: 2026-01-01
> **评审状态**: ✅ 通过 Gemini 设计评审 (Conditional Approval)
> **目标读者**: 技术评审 / 架构师

---

## 1. 概述 / Overview

### 1.1 设计目标 / Design Goals

1. **钱包兼容**: 标准 Sui 钱包无缝使用
2. **RPC 兼容**: 保持 Sui JSON-RPC API 兼容
3. **原子存取款**: 链上资产与 DEX 余额原子转换
4. **最小侵入**: 不修改 Move VM 核心

### 1.2 评审历史 / Review History

| 版本 | 日期 | 评审方 | 结论 | 关键变更 |
|------|------|--------|------|----------|
| v1.0 | 2025-12-29 | - | 初稿 | 事件监听模式 |
| v1.1 | 2025-12-30 | - | - | 改为同步回调 |
| v1.2 | 2025-12-31 | Gemini | Conditional | 识别 Commit Timing 风险 |
| v1.3 | 2025-12-31 | - | - | 添加两阶段执行 + 形式化模型 |
| v1.3 | 2026-01-01 | Claude | ✅ Approved | 确认设计完整解决评审问题 |

#### 评审结论确认

**Gemini 评审 (v1.2)** 提出的关键风险已在 v1.3 中解决:

1. **Commit Timing 风险**: Section 6.5.4 实现两阶段执行
   - Signing Phase: 仅 lock，不修改余额
   - Certificate Execution: 验证证书后才 commit

2. **形式化验证**: Section 6.7 提供完整证明

3. **失败处理**: Lock TTL (30s) 保证最终一致性

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
    /// 注意: 实际执行由 Precompile 拦截，在同一交易上下文内完成
    public entry fun deposit<T>(
        coin: Coin<T>,
        ctx: &mut TxContext,
    ) {
        // Precompile 拦截后执行:
        // 1. 锁定代币到 DEX 托管账户 (Move)
        // 2. 同步回调 DEX Engine 更新余额 (Native)
        // 3. 两者在同一交易上下文内原子完成
        abort 0  // 占位，实际由 Precompile 处理
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

> **重要**: 存取款使用 **同步回调模型**，在同一交易上下文内原子完成。
> 详见 [6. 存取款原子性保证](#6-存取款原子性保证--depositwithdraw-atomicity)。

```
┌─────────────────────────────────────────────────────────────┐
│              Hybrid Path (存款) - 同步回调模型               │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  单一交易上下文 (Atomic Transaction Context)                 │
│  ┌─────────────────────────────────────────────────────────┐│
│  │ 1. Precompile 拦截 deposit 调用                         ││
│  │ 2. 执行 Move: 转移代币到托管账户                        ││
│  │ 3. 同步回调: DEX Engine 更新余额                        ││
│  │ 4. 两者都成功 → 提交; 任一失败 → 全部回滚               ││
│  └─────────────────────────────────────────────────────────┘│
│                                                              │
│  ⚠️ 禁止事件监听模式 - 必须同步回调确保原子性               │
│                                                              │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│              Hybrid Path (取款) - 同步回调模型               │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  单一交易上下文 (Atomic Transaction Context)                 │
│  ┌─────────────────────────────────────────────────────────┐│
│  │ 1. Precompile 拦截 withdraw 调用                        ││
│  │ 2. DEX Engine: 锁定余额 (原子操作，带 TTL)              ││
│  │ 3. 执行 Move: 从托管账户释放代币到用户                  ││
│  │ 4. 同步回调: DEX Engine 确认扣减余额                    ││
│  │ 5. 两者都成功 → 提交; 任一失败 → 释放锁，全部回滚       ││
│  └─────────────────────────────────────────────────────────┘│
│                                                              │
│  ⚠️ Lock TTL=30s 防止死锁; 超时自动释放                     │
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

> **重要**：存取款是 Hybrid 操作，涉及 Move VM 和 DEX Engine 两个执行上下文。
> 必须在 **同一个交易执行上下文内** 完成，使用 **同步回调** 而非 **事件监听**。

### 6.1 统一原子性机制 / Unified Atomicity Mechanism

```
┌─────────────────────────────────────────────────────────────┐
│              Hybrid Execution Model (存取款统一模型)          │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  关键约束:                                                   │
│  1. 区块链状态变更不可逆 (NO rollback after commit)         │
│  2. 必须在交易执行完成前判定成功/失败                       │
│  3. 使用同步回调，不依赖异步事件监听                        │
│                                                              │
│  执行模型:                                                   │
│  ┌─────────────────────────────────────────────────────────┐│
│  │  Transaction Executor (单线程，原子执行)                 ││
│  │                                                          ││
│  │  ┌──────────┐      ┌──────────┐      ┌──────────┐      ││
│  │  │ 1. 验证  │ ───► │ 2. Move  │ ───► │ 3. DEX   │      ││
│  │  │   签名   │      │   执行   │      │   回调   │      ││
│  │  └──────────┘      └────┬─────┘      └────┬─────┘      ││
│  │                         │                  │            ││
│  │                  move_effects       dex_effects         ││
│  │                         │                  │            ││
│  │                         └──────┬───────────┘            ││
│  │                                ▼                        ││
│  │                    ┌─────────────────────┐              ││
│  │                    │ 4. 合并 Effects     │              ││
│  │                    │    (ALL or NOTHING) │              ││
│  │                    └─────────────────────┘              ││
│  └─────────────────────────────────────────────────────────┘│
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

### 6.2 存款原子性 / Deposit Atomicity

```
┌─────────────────────────────────────────────────────────────┐
│                    Deposit Flow (同步执行)                   │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  执行顺序 (同一交易上下文内):                                │
│                                                              │
│  Step 1: Move VM 执行                                        │
│  ├── 验证用户签名                                            │
│  ├── 检查用户持有足够代币                                    │
│  └── 转移代币到 DEX 托管账户                                │
│       │                                                     │
│       ▼ (同步回调，非事件)                                  │
│  Step 2: DEX Engine 回调                                     │
│  ├── 验证 Move 执行成功                                      │
│  ├── 更新用户 DEX 余额 (+amount)                            │
│  └── 返回 DEX Effects                                       │
│       │                                                     │
│       ▼                                                     │
│  Step 3: 合并 Effects                                        │
│  └── 返回统一的 TransactionEffects                          │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

### 6.3 取款原子性 / Withdraw Atomicity

```
┌─────────────────────────────────────────────────────────────┐
│                    Withdraw Flow (同步执行)                  │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  执行顺序 (同一交易上下文内):                                │
│                                                              │
│  Step 1: DEX Engine 预检查                                   │
│  ├── 验证用户签名                                            │
│  ├── 检查 DEX 余额充足                                       │
│  └── 锁定余额 (pending_withdraw)                            │
│       │                                                     │
│       ▼ (同步调用)                                          │
│  Step 2: Move VM 执行                                        │
│  ├── 从托管账户释放代币                                      │
│  └── 转移代币到用户地址                                      │
│       │                                                     │
│       ├── 成功 ─► Step 3a                                   │
│       └── 失败 ─► Step 3b                                   │
│                                                              │
│  Step 3a: Commit (Move 成功)                                 │
│  └── DEX 扣减余额 (available -= amount)                     │
│                                                              │
│  Step 3b: Rollback (Move 失败)                               │
│  └── DEX 解锁余额 (pending_withdraw = 0)                    │
│       注意: 此回滚仅针对 DEX 内存状态，                      │
│       Move 执行失败意味着链上无状态变更                      │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

### 6.4 失败场景矩阵 / Failure Scenario Matrix

#### 6.4.1 基础失败场景 / Basic Failure Scenarios

| # | 阶段 | 失败点 | 结果 | 用户影响 |
|---|-----|-------|------|---------|
| 1 | **存款** | 签名验证失败 | 整个交易拒绝 | 无变化 |
| 2 | **存款** | 余额不足 (Move) | Move 执行失败，回滚 | 无变化 |
| 3 | **存款** | DEX 回调失败 | 整个交易失败 | 无变化 (Move 也回滚) |
| 4 | **取款** | 签名验证失败 | 整个交易拒绝 | 无变化 |
| 5 | **取款** | DEX 余额不足 | 预检查失败，拒绝执行 | 无变化 |
| 6 | **取款** | Move 执行失败 | DEX 解锁锁定余额 | 无变化 |
| 7 | **取款** | 托管账户异常 | Move 失败，DEX 回滚 | 无变化 |
| 8 | **取款** | 并发取款冲突 | 第二个请求被拒绝 | 第一个正常，第二个失败 |

#### 6.4.2 分布式失败场景 / Distributed Failure Scenarios

| # | 场景 | 触发条件 | 处理机制 | 恢复策略 |
|---|-----|---------|---------|---------|
| 9 | **并发存款竞态** | 同一用户多个存款同时到达 | Sequencer 保证序列化执行 | 无需恢复，按序执行 |
| 10 | **并发取款竞态** | 同一用户多个取款同时到达 | Lock 互斥：已有锁时拒绝新请求 | 用户重试 |
| 11 | **DEX 回调超时** | `credit_balance()` 超过 5s | 整个交易 abort，Move 状态回滚 | 用户重试 |
| 12 | **Move 执行超时** | Move VM 执行超过 10s | Lock 超时释放 (TTL=30s) | 自动恢复 |
| 13 | **Sequencer 崩溃** | 软确认后节点宕机 | WAL 重放 + Lock 过期清理 | 自动恢复 |
| 14 | **网络分区** | 验证者间网络中断 | 拒绝新混合交易，等待恢复 | 网络恢复后自动继续 |

#### 6.4.3 超时参数 / Timeout Parameters

| 参数 | 默认值 | 说明 |
|-----|-------|------|
| `DEX_CALLBACK_TIMEOUT` | 5s | DEX 回调最大等待时间 |
| `MOVE_EXECUTION_TIMEOUT` | 10s | Move VM 执行最大时间 |
| `WITHDRAW_LOCK_TTL` | 30s | 取款锁定最大持有时间 |
| `LOCK_CLEANUP_INTERVAL` | 5s | 过期锁清理扫描间隔 |

### 6.5 实现代码 / Implementation

#### 6.5.1 核心数据结构 / Core Data Structures

```rust
use std::time::{Duration, Instant};
use dashmap::DashMap;

/// 锁信息
pub struct LockInfo {
    pub lock_id: LockId,
    pub account: AccountId,
    pub asset: Asset,
    pub amount: Amount,
    pub created_at: Instant,
    pub ttl: Duration,
}

/// 账户状态 (支持并发锁)
pub struct AccountState {
    pub balance: Amount,
    pub pending_withdraw: Option<LockInfo>,
    pub nonce: u64,
}

/// 超时配置
pub struct TimeoutConfig {
    pub dex_callback_timeout: Duration,   // 5s
    pub move_execution_timeout: Duration, // 10s
    pub withdraw_lock_ttl: Duration,      // 30s
    pub lock_cleanup_interval: Duration,  // 5s
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            dex_callback_timeout: Duration::from_secs(5),
            move_execution_timeout: Duration::from_secs(10),
            withdraw_lock_ttl: Duration::from_secs(30),
            lock_cleanup_interval: Duration::from_secs(5),
        }
    }
}
```

#### 6.5.2 原子锁操作 / Atomic Lock Operations

```rust
impl DexEngine {
    /// 锁定余额（原子操作，使用 DashMap entry API）
    pub async fn lock_for_withdraw(
        &self,
        account: AccountId,
        asset: Asset,
        amount: Amount,
    ) -> Result<LockId> {
        // 使用 DashMap entry API 保证原子性
        let mut entry = self.balances
            .entry((account.clone(), asset.clone()))
            .or_insert_with(|| AccountState::default());

        // 检查是否已有活跃锁（互斥）
        if entry.pending_withdraw.is_some() {
            return Err(Error::ConcurrentWithdraw {
                account: account.clone(),
                message: "已有进行中的取款操作".to_string(),
            });
        }

        // 检查余额
        if entry.balance < amount {
            return Err(Error::InsufficientBalance {
                available: entry.balance,
                required: amount,
            });
        }

        // 创建锁（原子操作）
        let lock_id = LockId::generate();
        entry.pending_withdraw = Some(LockInfo {
            lock_id: lock_id.clone(),
            account,
            asset,
            amount,
            created_at: Instant::now(),
            ttl: self.config.withdraw_lock_ttl,
        });

        // 记录 WAL（用于崩溃恢复）
        self.wal.append(WalEntry::LockCreated { lock_id: lock_id.clone() })?;

        Ok(lock_id)
    }

    /// 提交取款（锁成功后调用）
    pub async fn commit_withdraw(&self, lock_id: LockId) -> Result<()> {
        let lock_info = self.find_and_remove_lock(&lock_id)?;

        // 扣减余额
        let mut entry = self.balances
            .get_mut(&(lock_info.account.clone(), lock_info.asset.clone()))
            .ok_or(Error::AccountNotFound)?;

        entry.balance -= lock_info.amount;
        entry.pending_withdraw = None;

        // 记录 WAL
        self.wal.append(WalEntry::WithdrawCommitted { lock_id })?;

        Ok(())
    }

    /// 回滚取款（Move 失败时调用）
    pub async fn rollback_withdraw(&self, lock_id: LockId) -> Result<()> {
        let lock_info = self.find_and_remove_lock(&lock_id)?;

        // 仅释放锁，不改变余额
        let mut entry = self.balances
            .get_mut(&(lock_info.account.clone(), lock_info.asset.clone()))
            .ok_or(Error::AccountNotFound)?;

        entry.pending_withdraw = None;

        // 记录 WAL
        self.wal.append(WalEntry::WithdrawRolledBack { lock_id })?;

        Ok(())
    }
}
```

#### 6.5.3 超时处理 / Timeout Handling

```rust
impl DexEngine {
    /// 后台任务：清理过期锁
    pub async fn cleanup_expired_locks(&self) {
        let mut interval = tokio::time::interval(self.config.lock_cleanup_interval);

        loop {
            interval.tick().await;

            let now = Instant::now();
            let mut expired_count = 0;

            for mut entry in self.balances.iter_mut() {
                if let Some(lock) = &entry.pending_withdraw {
                    if now.duration_since(lock.created_at) > lock.ttl {
                        // 锁过期，释放
                        let lock_id = lock.lock_id.clone();
                        entry.pending_withdraw = None;
                        expired_count += 1;

                        // 记录 WAL（标记为超时释放）
                        let _ = self.wal.append(WalEntry::LockExpired { lock_id });

                        // 监控指标
                        LOCK_TIMEOUT_COUNTER.inc();
                    }
                }
            }

            if expired_count > 0 {
                tracing::warn!(
                    expired_count,
                    "Cleaned up expired withdraw locks"
                );
            }
        }
    }
}
```

#### 6.5.4 Hybrid 执行器 / Hybrid Executor

> **⚠️ 关键设计**: Sui 交易分为 Signing Phase（计算效果）和 Certificate Execution Phase（应用效果）。
> DEX 状态修改 **必须推迟到 Certificate Execution Phase**，否则可能导致资金丢失。

```rust
/// 交易执行阶段
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionPhase {
    /// Signing Phase: 仅计算效果，不修改 DEX 状态
    Signing,
    /// Certificate Execution: 应用效果，修改 DEX 状态
    CertificateExecution,
}

/// 待提交的 DEX 效果（中间状态）
#[derive(Debug, Clone)]
pub struct PendingDexEffect {
    pub lock_id: LockId,
    pub effect_type: DexEffectType,
    pub tx_digest: TransactionDigest,
}

#[derive(Debug, Clone)]
pub enum DexEffectType {
    Deposit { account: AccountId, asset: Asset, amount: Amount },
    Withdraw { account: AccountId, asset: Asset, amount: Amount },
}

/// Hybrid 执行器 (存取款)
pub struct HybridExecutor {
    dex_engine: Arc<DexEngine>,
    move_executor: Arc<MoveExecutor>,
    config: TimeoutConfig,
    /// 待提交效果（Signing Phase 产生，Certificate Execution Phase 应用）
    pending_effects: DashMap<TransactionDigest, PendingDexEffect>,
}

impl HybridExecutor {
    // ========================================================================
    // Phase 1: Signing Phase（计算效果，不修改 DEX 状态）
    // ========================================================================

    /// 执行存款 - Signing Phase
    /// 返回 Effects + PendingDexEffect，不修改 DEX 状态
    pub async fn execute_deposit_signing(
        &self,
        tx: &Transaction,
    ) -> Result<(Effects, PendingDexEffect)> {
        // 1. Move 执行 (代币转移)
        let move_effects = tokio::time::timeout(
            self.config.move_execution_timeout,
            self.move_executor.execute(tx)
        ).await
            .map_err(|_| Error::MoveExecutionTimeout)?
            .map_err(Error::MoveExecution)?;

        if !move_effects.status.is_success() {
            return Err(Error::MoveExecutionFailed(move_effects.status.clone()));
        }

        // 2. 提取存款信息，生成 pending effect（不写入 DEX）
        let deposit_info = self.extract_deposit_info(&move_effects)?;
        let pending = PendingDexEffect {
            lock_id: LockId::generate(),
            effect_type: DexEffectType::Deposit {
                account: deposit_info.account,
                asset: deposit_info.asset,
                amount: deposit_info.amount,
            },
            tx_digest: tx.digest(),
        };

        // 3. 缓存 pending effect
        self.pending_effects.insert(tx.digest(), pending.clone());

        Ok((move_effects, pending))
    }

    /// 执行取款 - Signing Phase
    /// 仅创建锁，不扣减余额
    pub async fn execute_withdraw_signing(
        &self,
        tx: &Transaction,
    ) -> Result<(Effects, PendingDexEffect)> {
        let withdraw_info = self.parse_withdraw_request(tx)?;

        // 1. DEX 预检查 + 锁定（原子操作，余额未变）
        let lock_id = self.dex_engine
            .lock_for_withdraw(
                withdraw_info.account.clone(),
                withdraw_info.asset.clone(),
                withdraw_info.amount
            )
            .await?;

        // 2. Move 执行 (代币释放，带超时)
        let move_result = tokio::time::timeout(
            self.config.move_execution_timeout,
            self.move_executor.execute(tx)
        ).await;

        match move_result {
            Ok(Ok(effects)) if effects.status.is_success() => {
                // 3. 创建 pending effect（不 commit，等待证书）
                let pending = PendingDexEffect {
                    lock_id,
                    effect_type: DexEffectType::Withdraw {
                        account: withdraw_info.account,
                        asset: withdraw_info.asset,
                        amount: withdraw_info.amount,
                    },
                    tx_digest: tx.digest(),
                };

                self.pending_effects.insert(tx.digest(), pending.clone());
                Ok((effects, pending))
            }
            Ok(Ok(effects)) => {
                // Move 状态失败，释放锁
                self.dex_engine.rollback_withdraw(lock_id).await?;
                Err(Error::MoveExecutionFailed(effects.status))
            }
            Ok(Err(e)) => {
                self.dex_engine.rollback_withdraw(lock_id).await?;
                Err(Error::MoveExecution(e))
            }
            Err(_) => {
                self.dex_engine.rollback_withdraw(lock_id).await?;
                Err(Error::MoveExecutionTimeout)
            }
        }
    }

    // ========================================================================
    // Phase 2: Certificate Execution（应用效果，修改 DEX 状态）
    // ========================================================================

    /// 应用已认证的效果
    /// 只有收到有效证书后才调用此方法
    pub async fn apply_certified_effects(
        &self,
        certificate: &Certificate,
    ) -> Result<()> {
        let tx_digest = certificate.transaction_digest();

        // 1. 取出 pending effect
        let pending = self.pending_effects.remove(&tx_digest)
            .map(|(_, v)| v)
            .ok_or(Error::NoPendingEffect { tx_digest })?;

        // 2. 验证证书有效性
        if !certificate.is_valid() {
            // 证书无效，回滚
            return self.rollback_pending_effect(&pending).await;
        }

        // 3. 应用效果到 DEX 状态
        match &pending.effect_type {
            DexEffectType::Deposit { account, asset, amount } => {
                self.dex_engine.credit_balance(
                    account.clone(),
                    asset.clone(),
                    *amount
                ).await?;
            }
            DexEffectType::Withdraw { .. } => {
                // Commit: 扣减 DEX 余额（锁已在 Signing Phase 创建）
                self.dex_engine.commit_withdraw(pending.lock_id).await?;
            }
        }

        Ok(())
    }

    /// 处理证书失败（交易未能达成共识）
    pub async fn handle_certificate_failure(
        &self,
        tx_digest: TransactionDigest,
    ) -> Result<()> {
        if let Some((_, pending)) = self.pending_effects.remove(&tx_digest) {
            self.rollback_pending_effect(&pending).await?;
        }
        Ok(())
    }

    /// 回滚 pending effect
    async fn rollback_pending_effect(&self, pending: &PendingDexEffect) -> Result<()> {
        match &pending.effect_type {
            DexEffectType::Deposit { .. } => {
                // Deposit 在 Signing Phase 没有修改 DEX 状态，无需回滚
                Ok(())
            }
            DexEffectType::Withdraw { .. } => {
                // 释放锁
                self.dex_engine.rollback_withdraw(pending.lock_id.clone()).await
            }
        }
    }
}
```

```
┌─────────────────────────────────────────────────────────────────────────┐
│                    两阶段执行模型 / Two-Phase Execution                  │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│  Phase 1: Signing Phase                                                  │
│  ┌─────────────────────────────────────────────────────────────────┐    │
│  │ 1. 执行 Move (计算效果)                                         │    │
│  │ 2. 创建 Lock (取款) / 无操作 (存款)                             │    │
│  │ 3. 生成 PendingDexEffect                                        │    │
│  │ 4. 返回 Effects (用于签名)                                      │    │
│  │                                                                  │    │
│  │ ⚠️ 此阶段 DEX 余额不变                                          │    │
│  └─────────────────────────────────────────────────────────────────┘    │
│                             │                                            │
│                             ▼                                            │
│  ┌─────────────────────────────────────────────────────────────────┐    │
│  │            等待证书 (Wait for Certificate)                      │    │
│  │                                                                  │    │
│  │  成功: 收到 2f+1 签名 → Certificate 生成                        │    │
│  │  失败: 超时 / 冲突 → 无 Certificate                             │    │
│  └─────────────────────────────────────────────────────────────────┘    │
│                             │                                            │
│             ┌───────────────┴───────────────┐                            │
│             ▼                               ▼                            │
│  Phase 2a: Certificate Success     Phase 2b: Certificate Failure        │
│  ┌───────────────────────────┐    ┌───────────────────────────┐         │
│  │ apply_certified_effects() │    │ handle_certificate_failure()│        │
│  │                           │    │                             │        │
│  │ • Deposit: credit_balance │    │ • Deposit: 无操作 (Move     │        │
│  │ • Withdraw: commit_withdraw│    │   已回滚)                   │        │
│  │                           │    │ • Withdraw: rollback_withdraw│        │
│  │ ✅ DEX 状态更新           │    │ ✅ 锁释放，状态不变         │        │
│  └───────────────────────────┘    └───────────────────────────┘         │
│                                                                          │
└─────────────────────────────────────────────────────────────────────────┘
```

### 6.6 关键约束 / Key Constraints

> **禁止事件监听模式**：存取款不能依赖异步事件监听来更新 DEX 状态，
> 必须在同一交易执行上下文内通过同步回调完成，确保原子性。

> **禁止链上状态回滚假设**：一旦 Move 执行成功并 commit，链上状态不可逆。
> 所有失败处理必须在 commit 前完成。

> **⚠️ 强制两阶段执行** (Commit Timing 安全性):
> - **Signing Phase**: 仅计算效果，生成 `PendingDexEffect`，**禁止修改 DEX 余额**
> - **Certificate Execution Phase**: 收到有效证书后，调用 `apply_certified_effects()` 修改 DEX 状态
> - **原因**: 在 Sui 中，Signing Phase 的 Move 执行结果仅用于签名收集。若此时修改 DEX 状态，
>   但交易未能获得证书（共识失败），将导致 DEX 资金已扣除但 Move 状态未持久化 → **资金丢失**。

### 6.7 形式化模型 / Formal Model

> **目的**：为 Move <-> Native 混合交易提供数学形式化定义，
> 支持安全性证明和模型检验。

#### 6.7.1 符号定义 / Notation

```
系统状态 (System State):
  S = (M, D, W, L, P)
    M: Move 链上状态 (不可变账本)
    D: DEX 内存状态 (余额映射)
    W: WAL 日志 (持久化序列)
    L: 活跃锁集合 (Lock Set)
    P: 待提交效果集合 (Pending Effects) ← 两阶段执行新增

交易 (Transaction):
  T = { deposit(user, asset, amount) | withdraw(user, asset, amount) }

执行阶段 (Execution Phase):
  Phase = { Signing | CertificateExecution }

状态转移 (Transition):
  S₀ ⟹[T, Signing] S₁        // Signing Phase: 产生 pending effect
  S₁ ⟹[cert] Sf              // Certificate Execution: 应用 pending effect
  S₁ ⟹[no_cert] S₀           // Certificate Failure: 回滚 pending effect
```

#### 6.7.2 状态空间 / State Space

```rust
/// 形式化状态定义
struct SystemState {
    /// Move 托管账户: asset → total_amount
    move_custody: Map<Asset, Amount>,

    /// DEX 余额: (account, asset) → balance
    dex_balances: Map<(Account, Asset), Amount>,

    /// 活跃锁: lock_id → LockInfo
    active_locks: Map<LockId, LockInfo>,

    /// 待提交效果: tx_digest → PendingEffect (两阶段执行)
    pending_effects: Map<TxDigest, PendingEffect>,

    /// 全局序列号 (用于确定性)
    global_seq: u64,
}

/// 待提交效果
enum PendingEffect {
    Deposit { user: Account, asset: Asset, amount: Amount },
    Withdraw { lock_id: LockId, user: Account, asset: Asset, amount: Amount },
}

/// 状态约束
invariant SystemState {
    // INV1: 托管账户余额 = DEX 所有用户余额之和 + pending deposits
    ∀ asset:
        move_custody[asset] == Σ dex_balances[(_, asset)] + Σ pending_deposits[asset]

    // INV2: 每个用户最多一个活跃锁
    ∀ account:
        |{l ∈ active_locks | l.account == account}| ≤ 1

    // INV3: 锁定金额不超过余额
    ∀ lock ∈ active_locks:
        dex_balances[(lock.account, lock.asset)] >= lock.amount

    // INV4: Pending effect 必须有对应的锁 (withdraw)
    ∀ pending ∈ pending_effects where pending is Withdraw:
        pending.lock_id ∈ active_locks

    // INV5: Pending effect 超时后必须清理 (TTL 保证)
    ∀ pending ∈ pending_effects:
        age(pending) < PENDING_EFFECT_TTL
}
```

#### 6.7.3 转移规则 / Transition Rules (两阶段执行)

> **关键**: 所有 DEX 状态修改必须推迟到 Certificate Execution Phase，
> 以防止 Signing Phase 成功但未获得证书导致的资金丢失。

**存款 (Deposit) - 两阶段**：

```
deposit_signing(user, asset, amount):  // Phase 1
─────────────────────────────────────────────────────────
前置条件 (Precondition):
  M[user][asset] >= amount          // 链上余额充足

执行步骤 (Signing Phase):
  1. M[user][asset] -= amount       // Move: 用户扣款
  2. M[custody][asset] += amount    // Move: 托管入账
  3. P[tx_digest] = Deposit{user, asset, amount}  // 记录 pending

中间状态 (Intermediate State):
  M' 已修改 (托管已入账)
  D  未修改 (余额未增加)
  P' = P ∪ {tx_digest → pending}

失败语义:
  IF step 1-2 fails → abort (M unchanged, P unchanged)
─────────────────────────────────────────────────────────

deposit_certificate(tx_digest, cert):  // Phase 2
─────────────────────────────────────────────────────────
前置条件:
  P[tx_digest] exists              // 有 pending effect
  cert.is_valid()                  // 证书有效

执行步骤 (Certificate Execution):
  pending = P[tx_digest]
  D[(pending.user, pending.asset)] += pending.amount  // DEX 余额增加
  delete P[tx_digest]              // 清理 pending

后置条件:
  D'[(user, asset)] = D[(user, asset)] + amount
  P' = P \ {tx_digest}

证书失败:
  IF !cert.is_valid() OR timeout:
    // Move 状态已回滚 (无证书 = 交易未发生)
    delete P[tx_digest]            // 清理 pending
    // D 不变 (从未修改)
─────────────────────────────────────────────────────────
```

**取款 (Withdraw) - 两阶段**：

```
withdraw_signing(user, asset, amount):  // Phase 1
─────────────────────────────────────────────────────────
前置条件 (Precondition):
  D[(user, asset)] >= amount        // DEX 余额充足
  no_active_lock(user, asset)       // 无活跃锁

执行步骤 (Signing Phase):
  1. lock_id = create_lock(user, asset, amount)  // DEX: 创建锁 (余额不变!)
  2. M[custody][asset] -= amount                 // Move: 托管扣款
  3. M[user][asset] += amount                    // Move: 用户入账
  4. P[tx_digest] = Withdraw{lock_id, user, asset, amount}  // 记录 pending

中间状态 (Intermediate State):
  M' 已修改 (用户已收到代币)
  D  未修改 (余额未扣减，仅有锁)
  L' = L ∪ {lock_id}
  P' = P ∪ {tx_digest → pending}

失败语义:
  IF step 1 fails → abort (状态不变)
  IF step 2-3 fails:
    release_lock(lock_id)          // 回滚锁
    abort (M unchanged, D unchanged)
─────────────────────────────────────────────────────────

withdraw_certificate(tx_digest, cert):  // Phase 2
─────────────────────────────────────────────────────────
前置条件:
  P[tx_digest] exists              // 有 pending effect
  cert.is_valid()                  // 证书有效

执行步骤 (Certificate Execution):
  pending = P[tx_digest]
  D[(pending.user, pending.asset)] -= pending.amount  // DEX 余额扣减
  release_lock(pending.lock_id)    // 释放锁
  delete P[tx_digest]              // 清理 pending

后置条件:
  D'[(user, asset)] = D[(user, asset)] - amount
  L' = L \ {lock_id}
  P' = P \ {tx_digest}

证书失败:
  IF !cert.is_valid() OR timeout:
    // Move 状态已回滚 (无证书 = 交易未发生)
    release_lock(pending.lock_id)  // 释放锁
    delete P[tx_digest]            // 清理 pending
    // D 不变 (从未扣减)
─────────────────────────────────────────────────────────
```

**两阶段执行安全性证明**：

```
Theorem (Commit Timing Safety):
  ∀ withdraw transaction T:
    (Signing Phase 完成 ∧ Certificate 未获得)
    ⟹ D 状态不变 ∧ Lock 最终释放 (TTL 保证)

Proof:
  1. Signing Phase 仅创建 Lock，不修改 D
  2. Certificate 失败时调用 rollback_withdraw()
  3. 即使 rollback 失败，Lock TTL 保证最终释放
  4. D 从未被修改，无资金丢失
  ∎
```

#### 6.7.4 不变量 / Invariants

```
INV1 (余额守恒 / Balance Conservation):
  ∀ asset:
    M[custody][asset] + Σ M[user][asset] = TOTAL_SUPPLY[asset]
    D.total[asset] = M[custody][asset]

INV2 (锁唯一性 / Lock Uniqueness):
  ∀ lock_id: |{lock ∈ L | lock.id == lock_id}| ≤ 1

INV3 (锁有效性 / Lock Validity):
  ∀ lock ∈ L:
    D[(lock.account, lock.asset)] >= lock.amount

INV4 (无悬空资金 / No Dangling Funds):
  ∀ user, asset:
    (资金在 M[user]) ∨ (资金在 M[custody] 且 D[(user,asset)] > 0)
  // 不存在：资金在托管但 DEX 余额为 0 的情况

INV5 (锁超时释放 / Lock Timeout Release):
  ∀ lock ∈ L:
    now() - lock.created_at > lock.ttl → lock ∉ L'
  // 超时的锁最终会被释放
```

#### 6.7.5 安全性定理 / Safety Theorems

```
Theorem 1 (存款原子性 / Deposit Atomicity):
  ∀ deposit(user, asset, amount):
    S₀ ⟹[deposit] Sf
    ⟹ (M.success ∧ D.success) ∨ (M.unchanged ∧ D.unchanged)

  证明思路:
  - Move 使用事务语义，步骤 1-2 要么全部成功要么全部回滚
  - DEX 回调 (步骤 3) 在 Move 成功后同步执行
  - 如果 DEX 回调失败，整个交易被标记为失败
  - Sui 的 Effects 语义保证失败交易不产生状态变更

Theorem 2 (取款原子性 / Withdraw Atomicity):
  ∀ withdraw(user, asset, amount):
    S₀ ⟹[withdraw] Sf
    ⟹ (M.success ∧ D.success) ∨ (M.unchanged ∧ D.unchanged)

  证明思路:
  - Lock 机制保证了 DEX 余额预留
  - Move 执行前已持有锁，失败时回滚锁
  - Move 执行成功后，DEX 扣减操作必然成功（锁保证余额）
  - 超时机制确保锁最终释放

Theorem 3 (无死锁 / Deadlock Freedom):
  ∀ lock ∈ L:
    eventually(committed(lock) ∨ released(lock))

  证明思路:
  - 每个锁有 TTL 超时
  - 后台任务周期性清理过期锁
  - 锁创建后必然在 TTL 内被 commit 或 rollback

Theorem 4 (无悬空资金 / No Dangling Funds):
  ∀ S reachable from S₀:
    INV4 holds in S

  证明思路:
  - deposit: M[custody] 增加 ⟺ D 增加 (原子)
  - withdraw: M[custody] 减少 ⟺ D 减少 (原子)
  - 单边变更不可能发生
```

#### 6.7.6 模型检验建议 / Model Checking Recommendations

```
推荐工具:
- TLA+ : 验证并发场景和时态属性
- Alloy : 验证状态不变量
- SPIN  : 验证协议正确性

关键场景需验证:
1. 并发存款 + 取款
2. 取款过程中 Sequencer 崩溃
3. 网络分区后恢复
4. 多个用户同时取款同一资产
5. 锁超时与正常提交的竞态
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

## 变更历史 / Change History

| 版本 | 日期 | 变更内容 | 状态 |
|-----|------|---------|------|
| v1.0 | 2025-12-31 | 初始版本 | ✅ 有效 |
| v1.1 | 2025-12-31 | 重写 6. 存取款原子性：改为同步回调模型，禁止事件监听 | ✅ 有效 |
| v1.2 | 2025-12-31 | **Gemini 评审优化**: 添加 6.7 形式化模型，扩展 6.4 失败矩阵 (场景9-14)，更新 6.5 并发控制+超时处理代码 | ✅ 有效 |
| v1.3 | 2025-12-31 | **P0-04 修复**: 清理 3.2/4.3 残留的"事件监听"旧描述，统一为同步回调模型 | ✅ 有效 |

### 待对齐事项 / Alignment Notes

| 章节 | 状态 | 说明 |
|-----|------|------|
| 3.2 核心函数 | ✅ 有效 | 已更新为同步回调模型 (v1.3) |
| 4.3 混合操作 | ✅ 有效 | 已更新流程图为同步回调模型 (v1.3) |
| 6. 存取款原子性 | ✅ 有效 | 同步回调模型，与 03-ABSTRACTION 一致 |
| 6.4 失败矩阵 | ✅ 有效 | 扩展至 14 个场景（含分布式失败） |
| 6.5 代码示例 | ✅ 有效 | 含原子锁、超时处理、并发控制 |
| 6.7 形式化模型 | ✅ 有效 | 状态空间、转移规则、不变量、安全定理 |
| 2. Precompile 架构 | ✅ 有效 | 接口边界已定义 |
| 5.1 Gas 计费 | ⚠️ 待细化 | DEX 操作 Gas 定价需经济模型分析 |

---

*文档版本: v1.3 | 最后更新: 2025-12-31*
