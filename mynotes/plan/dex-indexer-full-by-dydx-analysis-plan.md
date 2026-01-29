# dYdX 双通道索引机制在 Sui 上的实现分析

## 目标
分析 dYdX 的 OffChainUpdates/OnChainUpdates 双通道机制如何适配 Sui，特别是 **无 Move 合约** 的纯原生 Rust DEX 引擎场景。
输出文档: `sui/mynotes/dex/analyst/dex-indexer-full-by-dydx-analysis.md`

## 背景

### dYdX 双通道架构
- **OffChainUpdates**: 订单簿更新后立即发出，低延迟（撮合后 <10ms）
- **OnChainUpdates**: 区块共识后发出，最终一致性（区块确认后）

### 场景约束
- **无 Move 合约**: DEX 逻辑完全是原生 Rust
- **无 Move Event**: 不能使用 Sui 的事件订阅 API
- **与 Sui 交互**: 仅限于资产存取（Deposit/Withdraw）

## 研究发现

### 1. 当前架构确认

- **DEX 架构**: 与 Sui 验证器集成（如 precompile 方式）
- **确认标准**: Checkpoint 确认
- **核心问题**: Checkpoint 延迟较高（~700ms+），是否有更好的方案？

### 2. Sui 确认机制层级

| 层级 | 延迟 | 触发条件 | 可靠性 | 适合场景 |
|------|------|----------|--------|----------|
| **交易执行** | ~400ms | Effects 生成 | 高 | 乐观确认 |
| **共识确认** | ~500ms | 2f+1 签名 | 很高 | 乐观确认 |
| **Checkpoint** | ~700ms+ | 进入 Checkpoint | 最高 | 最终确认 |

### 3. 为什么 Checkpoint 延迟高？

```
交易执行 ──► 共识确认 ──► Checkpoint 生成 ──► Checkpoint 签名
   ~400ms      ~100ms       ~100-200ms         ~100ms
            │                               │
            └───────── ~200-400ms ──────────┘

总延迟: ~700ms+ (取决于 min_checkpoint_interval_ms: 200)
```

### 4. OnChainUpdates 方案对比

| 方案 | 延迟 | 可靠性 | 实现难度 | 说明 |
|------|------|--------|----------|------|
| A. 纯 Checkpoint | ~700ms+ | 最高 | 低 | 当前方案，延迟最高但最可靠 |
| B. 交易执行确认 | ~400ms | 高 | 中 | 监听 TransactionEffects |
| C. 共识确认 | ~500ms | 很高 | 中高 | 监听 2f+1 签名 |
| D. 双层设计 | 乐观~400ms, 最终~700ms | 最高 | 中 | **推荐** |

### 5. 推荐方案：双层 OnChainUpdates

```
┌─────────────────────────────────────────────────────────────────┐
│                     双层 OnChainUpdates                         │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  Layer 1: Optimistic OnChainUpdates (~400ms)                    │
│  ─────────────────────────────────────────────                  │
│  - 触发点: 交易执行完成（TransactionEffects 生成）               │
│  - 获取方式: suix_subscribeTransaction 或内部执行回调            │
│  - 用途: 实时通知、UI 更新、交易确认提示                         │
│  - 可回滚: 极少数情况下可能因共识失败回滚                        │
│                                                                  │
│  Layer 2: Finalized OnChainUpdates (~700ms+)                    │
│  ─────────────────────────────────────────────                  │
│  - 触发点: 交易进入 Checkpoint                                   │
│  - 获取方式: gRPC Checkpoint 订阅                                │
│  - 用途: 历史记录、审计、对账、资金结算                          │
│  - 不可回滚: 全局最终一致性                                      │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

### 6. 与 dYdX 对比

| 特性 | dYdX v4 | Sui DEX (双层方案) |
|------|---------|-------------------|
| Optimistic 触发 | 撮合完成 | 交易执行完成 |
| Optimistic 延迟 | <10ms | ~400ms |
| Finalized 触发 | 区块确认 | Checkpoint |
| Finalized 延迟 | ~1-2s | ~700ms+ |

**注意**: Sui 的 Optimistic 延迟（~400ms）比 dYdX（<10ms）高，因为 Sui DEX 集成于验证器，交易需要经过执行流程。

### 7. 实现要点

**Layer 1 (Optimistic) 获取方式**:
```rust
// 方式 A: 通过 RPC 订阅（适合外部客户端）
let stream = sui_client.subscribe_transaction(filter).await?;

// 方式 B: 内部执行回调（适合验证器集成）
// 在 execution_engine 执行完成后直接触发
fn on_transaction_executed(&self, effects: &TransactionEffects) {
    self.emit_optimistic_update(effects);
}
```

**Layer 2 (Finalized) 获取方式**:
```rust
// gRPC Checkpoint 订阅
let mut client = SubscriptionServiceClient::connect(endpoint).await?;
let stream = client.subscribe_checkpoints(request).await?;
```

## 推荐架构

### 双通道事件分类（参考 dYdX）

根据 dYdX 的设计，事件按**确认要求**分为两类：

| 通道 | 事件类型 | 确认要求 | 延迟要求 |
|------|----------|----------|----------|
| **OffChainUpdates** | 订单状态变化 | 乐观（可回滚） | 极低（<10ms） |
| **OnChainUpdates** | 成交、仓位、资金 | 最终确认 | 可接受延迟 |

**关键区分**:
- ✅ OffChainUpdates: 仅**订单状态**（OrderPlace/Update/Remove）
- ❌ OffChainUpdates: **不包含**成交记录、持仓变化、余额变化
- ✅ OnChainUpdates: **所有**需要最终确认的数据

### 各通道更新内容

```
┌─────────────────────────────────────────────────────────────────────┐
│                     OffChainUpdates (<10ms)                         │
│                     (乐观状态，可能回滚)                             │
├─────────────────────────────────────────────────────────────────────┤
│  • OrderPlace      - 订单进入订单簿                                  │
│  • OrderUpdate     - 订单部分成交 (乐观)                             │
│  • OrderRemove     - 订单取消/完全成交 (乐观)                        │
│  • OrderBookL2     - 订单簿深度快照                                  │
│                                                                      │
│  ❌ 不包含: Fills, Positions, Balances, Transfers                   │
└─────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────┐
│                     OnChainUpdates (最终确认)                        │
│                     (不可回滚，持久化存储)                           │
├─────────────────────────────────────────────────────────────────────┤
│  • Fills           - 成交记录确认                                    │
│  • Positions       - 持仓变化确认                                    │
│  • Balances        - 余额变化确认                                    │
│  • Transfers       - 存取款确认                                      │
│  • Liquidations    - 清算事件                                        │
│  • FundingRates    - 资金费率结算                                    │
│                                                                      │
│  📝 这些数据必须等待最终确认才能更新                                 │
└─────────────────────────────────────────────────────────────────────┘
```

### 架构图

```
┌─────────────────────────────────────────────────────────────────────┐
│                  Sui 验证器集成 DEX 引擎                            │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  ┌───────────┐      ┌──────────────────────────────────────────┐   │
│  │  撮合引擎  │ ───► │  OffChainUpdates (gRPC/WebSocket)        │   │
│  │ (Matching) │      │  - OrderPlace/Update/Remove              │   │
│  └─────┬─────┘      │  - OrderBook 深度                         │   │
│        │            │  延迟: <10ms (乐观状态)                    │   │
│        │            └───────────────────────────┬────────────────┘   │
│        │                                        │                    │
│        ▼                                        ▼                    │
│  ┌───────────────────────┐              ┌──────────────┐           │
│  │   交易执行流程        │              │  WebSocket   │           │
│  │ (Transaction Execution)│              │  Clients     │           │
│  └─────────┬─────────────┘              └──────────────┘           │
│            │                                                         │
│            │                                                         │
│            ▼                                                         │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │             OnChainUpdates - 方案选择                         │  │
│  │                                                                │  │
│  │  方案 A (纯 Checkpoint):                                       │  │
│  │    └── 所有 OnChain 事件在 Checkpoint 时发送 (~700ms+)        │  │
│  │                                                                │  │
│  │  方案 B (双层):                                                │  │
│  │    ├── Layer 1 (Optimistic): 执行完成时 (~400ms)              │  │
│  │    └── Layer 2 (Finalized):  Checkpoint 时 (~700ms+)          │  │
│  │                                                                │  │
│  └──────────────────────────────────────────────────────────────┘  │
│                                                                      │
└──────────────────────────────────────────────────────────────────────┘
```

### 双层 vs 单层 Checkpoint 对比

| 维度 | 方案 A: 纯 Checkpoint | 方案 B: 双层 OnChainUpdates |
|------|----------------------|---------------------------|
| **延迟** | ~700ms+ | Optimistic ~400ms, Finalized ~700ms+ |
| **实现复杂度** | 低 | 中 |
| **数据一致性** | 最高（单一来源） | 需要处理 Optimistic→Finalized 状态转换 |
| **用户体验** | 成交确认慢 | 成交确认较快（但标注为 Optimistic） |
| **回滚处理** | 不需要 | 需要处理极少数 Optimistic 失败情况 |
| **API 设计** | 简单（单一事件流） | 复杂（需区分 Optimistic/Finalized） |
| **与 dYdX 对比** | 类似 dYdX 的 OnChainUpdates | 类似 dYdX + Sui 独有的 Optimistic 层 |

### 方案 B 双层详解

```
┌─────────────────────────────────────────────────────────────────────┐
│  OnChainUpdates - Layer 1: Optimistic (~400ms)                      │
├─────────────────────────────────────────────────────────────────────┤
│  触发: 交易执行完成（TransactionEffects 生成）                       │
│  获取: 内部回调 或 suix_subscribeTransaction                         │
│                                                                      │
│  事件:                                                               │
│  • FillOptimistic        - 成交（乐观确认）                          │
│  • PositionOptimistic    - 持仓变化（乐观确认）                      │
│  • BalanceOptimistic     - 余额变化（乐观确认）                      │
│  • TransferOptimistic    - 存取款（乐观确认）                        │
│                                                                      │
│  用途: 快速 UI 更新，但需标注"确认中"                                │
│  风险: 极少数情况可能回滚                                            │
└─────────────────────────────────────────────────────────────────────┘

                              │
                              │ ~300ms later
                              ▼

┌─────────────────────────────────────────────────────────────────────┐
│  OnChainUpdates - Layer 2: Finalized (~700ms+)                      │
├─────────────────────────────────────────────────────────────────────┤
│  触发: 交易进入 Checkpoint                                           │
│  获取: gRPC Checkpoint 订阅                                          │
│                                                                      │
│  事件:                                                               │
│  • FillFinalized         - 成交（最终确认）                          │
│  • PositionFinalized     - 持仓（最终确认）                          │
│  • BalanceFinalized      - 余额（最终确认）                          │
│  • TransferFinalized     - 存取款（最终确认）                        │
│                                                                      │
│  用途: 持久化存储、审计、对账、历史查询                              │
│  特点: 不可回滚，最终一致性                                          │
└─────────────────────────────────────────────────────────────────────┘
```

### 与 dYdX 完整对比

| 通道 | dYdX v4 | Sui DEX (方案 A) | Sui DEX (方案 B) |
|------|---------|------------------|------------------|
| **OffChainUpdates** | CheckTx ~10-50ms | 撮合 <10ms | 撮合 <10ms |
| **OnChainUpdates-Optimistic** | 无 | 无 | 执行 ~400ms |
| **OnChainUpdates-Finalized** | EndBlocker ~1-2s | Checkpoint ~700ms+ | Checkpoint ~700ms+ |

**Sui 方案 B 优势**:
- 比 dYdX 多一层 Optimistic 确认
- Finalized 比 dYdX 更快（~700ms vs ~1-2s）
- 用户可更快看到成交确认（虽然是 Optimistic）

**Sui 方案 A 优势**:
- 实现简单，维护成本低
- 数据一致性最高
- Finalized 延迟与方案 B 相同

## 文档结构

```
1. 执行摘要
   - 问题：Checkpoint 延迟高，是否有更好方案？
   - 答案：方案 A (纯 Checkpoint) vs 方案 B (双层)

2. dYdX 双通道机制分析
   - OffChainUpdates: 仅订单状态（OrderPlace/Update/Remove）
   - OnChainUpdates: 成交、仓位、余额、转账
   - 关键：成交记录在 OnChain，不在 OffChain

3. 各通道事件分类
   3.1 OffChainUpdates 事件
       - OrderPlace/Update/Remove
       - OrderBook 深度
   3.2 OnChainUpdates 事件
       - Fills, Positions, Balances, Transfers

4. Sui 确认机制层级
   - 交易执行 (~400ms)
   - Checkpoint (~700ms+)
   - 与 dYdX 区块确认 (~1-2s) 对比

5. OnChainUpdates 方案对比
   5.1 方案 A: 纯 Checkpoint
       - 优点: 简单、一致性高
       - 缺点: 延迟高
   5.2 方案 B: 双层设计
       - Optimistic Layer (~400ms)
       - Finalized Layer (~700ms+)
       - 优缺点分析

6. 利弊分析总结
   - 方案 A vs 方案 B 详细对比
   - 何时选择哪个方案

7. 实现要点
   - 方案 A 实现
   - 方案 B 实现
   - 代码示例

8. 结论
   - 根据业务需求选择方案
```

## 关键代码引用

| 组件 | 文件 | 说明 |
|------|------|------|
| dYdX 分析 | `mynotes/dex/analyst/dydx-indexer-analyst.md:1366-1551` | OffChain/OnChain 事件详解 |
| 交易订阅 | `sui-core/src/subscription_handler.rs:119-124` | `subscribe_transactions` |
| Checkpoint 间隔 | `sui-protocol-config/src/lib.rs:1712` | 200ms 最小间隔 |
| Checkpoint 流 | `sui-indexer-alt-framework/src/ingestion/streaming_client.rs:28-30` | gRPC 订阅 |

## 验证方式
- 验证事件分类与 dYdX 分析文档一致
- 验证延迟数据有代码依据
- 确保方案对比清晰明了
