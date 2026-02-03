# dYdX 双通道索引机制在 Sui DEX 上的实现分析

## 1. 执行摘要

### 问题
Sui DEX 采用 Checkpoint 作为最终确认标准，延迟约 700ms+。是否有更好的方案来优化 OnChainUpdates 的用户体验？

### 答案
提供两种可选方案：

| 方案 | 描述 | 适用场景 |
|------|------|----------|
| **方案 A: 纯 Checkpoint** | 所有 OnChain 事件在 Checkpoint 时发送 | 追求简单性和数据一致性 |
| **方案 B: 双层设计** | Optimistic (~400ms) + Finalized (~700ms+) | 追求更好的用户体验 |

### 核心结论
1. **OffChainUpdates** 仅包含订单状态（OrderPlace/Update/Remove），**不包含成交记录和持仓变化**
2. **OnChainUpdates** 包含所有需要最终确认的数据（Fills、Positions、Balances、Transfers）
3. 双层设计可提供更快的用户反馈，但增加实现复杂度
4. 纯 Checkpoint 方案更简单可靠，Finalized 延迟与双层方案相同

---

## 2. dYdX 双通道机制分析

### 2.1 架构概述

dYdX v4 采用双通道设计，将事件按**确认要求**分为两类：

```
┌─────────────────────────────────────────────────────────────────────┐
│                        dYdX v4 双通道架构                            │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │  OffChainUpdates (CheckTx 阶段)                              │    │
│  │  延迟: 10-50ms                                               │    │
│  │  状态: 乐观 (可回滚)                                         │    │
│  │  Kafka: to-vulcan → Vulcan → Redis → WebSocket              │    │
│  └─────────────────────────────────────────────────────────────┘    │
│                                                                      │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │  OnChainUpdates (DeliverTx + EndBlocker 阶段)                │    │
│  │  延迟: 1000-2000ms (区块时间)                                │    │
│  │  状态: 最终确定                                              │    │
│  │  Kafka: to-ender → Ender → PostgreSQL → WebSocket           │    │
│  └─────────────────────────────────────────────────────────────┘    │
│                                                                      │
└─────────────────────────────────────────────────────────────────────┘
```

### 2.2 OffChainUpdates 触发时机

**触发阶段**: `CheckTx` (交易验证阶段)

| 时机 | 事件类型 | 说明 |
|------|---------|------|
| 订单进入订单簿 | OrderPlace | 短期订单验证通过后 |
| 乐观匹配中 Taker 成交 | OrderUpdate | Taker 部分/全部成交 |
| 乐观匹配中 Maker 被吃 | OrderUpdate | Maker 被匹配成交 |
| 订单取消 | OrderRemove | 取消订单请求 |
| 订单完全成交 | OrderRemove | 从订单簿移除 |
| Post-Only 失败 | OrderRemove | 会吃单时拒绝 |
| IOC/FOK 未满足 | OrderRemove | 立即取消未成交部分 |

**关键特点**:
- 延迟极低: 毫秒级 (交易到达节点后立即处理)
- 乐观状态: 可能因区块回滚而失效
- 仅短期订单: 长期订单走 OnChain 流程

> 参考: `dydx-indexer-analyst.md:1366-1416`

### 2.3 OnChainUpdates 触发时机

**触发阶段**: `DeliverTx` (交易执行) + `EndBlocker` (区块结束)

| 时机 | 事件类型 | 说明 |
|------|---------|------|
| 订单成交确认 | order_fill | MatchOrders 执行成功后 |
| 仓位/余额变化 | subaccount_update | UpdateSubaccounts 调用后 |
| 子账户转账 | transfer | MsgCreateTransfer 执行后 |
| 充值确认 | transfer | MsgDepositToSubaccount 执行后 |
| 提款确认 | transfer | MsgWithdrawFromSubaccount 执行后 |
| 长期订单放置 | stateful_order | MsgPlaceOrder (Long-Term) |
| 资金费率更新 | funding_values | 每个资金费率周期结束 |
| 清算事件 | deleveraging | MatchPerpetualDeleveraging |

**关键特点**:
- 最终确定: 事件代表链上已确认状态
- 延迟较高: 区块时间 (~1-2秒)
- 批量发送: EndBlocker 时一次性发送整个区块的所有事件

> 参考: `dydx-indexer-analyst.md:1420-1482`

### 2.4 关键差异总结

| 维度 | OffChainUpdates | OnChainUpdates |
|------|----------------|----------------|
| **触发阶段** | CheckTx (交易验证) | DeliverTx + EndBlocker |
| **延迟** | 10-50ms | 1000-2000ms |
| **状态性质** | 乐观 (可回滚) | 最终确定 |
| **存储** | Redis (热数据) | PostgreSQL (持久化) |
| **事件内容** | 订单状态变化 | 成交、仓位、转账等 |

> 参考: `dydx-indexer-analyst.md:1539-1551`

---

## 3. 各通道事件分类

### 3.1 OffChainUpdates 事件

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
```

**为什么 OffChainUpdates 不包含成交记录？**

1. **乐观状态可回滚**: 如果区块回滚，成交可能失效
2. **数据一致性**: 持仓和余额变化必须基于确认的成交
3. **审计需求**: 成交记录需要最终确认后才能持久化

### 3.2 OnChainUpdates 事件

```
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

---

## 4. Sui 确认机制层级

### 4.1 确认层级对比

Sui 与 dYdX 使用不同的共识机制，确认层级也不同：

| 层级 | Sui | dYdX v4 (Cosmos) |
|------|-----|------------------|
| **乐观确认** | 交易执行完成 (~400ms) | CheckTx (~10-50ms) |
| **共识确认** | 2f+1 签名 (~500ms) | Tendermint 共识 |
| **最终确认** | Checkpoint (~700ms+) | 区块确认 (~1-2s) |

### 4.2 Sui 确认机制详解

```
交易提交
    │
    ▼
┌───────────────────────────────────────────────────────────────┐
│  交易执行 (~400ms)                                             │
│  - TransactionEffects 生成                                     │
│  - 状态: 高可靠性，极少数情况可能回滚                           │
│  代码: sui-core/src/subscription_handler.rs:119-124           │
└───────────────────────────────────────────────────────────────┘
    │
    ▼
┌───────────────────────────────────────────────────────────────┐
│  共识确认 (~500ms)                                             │
│  - 2f+1 验证者签名                                             │
│  - 状态: 很高可靠性                                            │
└───────────────────────────────────────────────────────────────┘
    │
    ▼
┌───────────────────────────────────────────────────────────────┐
│  Checkpoint (~700ms+)                                          │
│  - 交易进入 Checkpoint                                         │
│  - 状态: 最终确认，不可回滚                                     │
│  配置: min_checkpoint_interval_ms: 200                         │
│  代码: sui-protocol-config/src/lib.rs:1712                    │
└───────────────────────────────────────────────────────────────┘
```

### 4.3 为什么 Checkpoint 延迟较高？

```
交易执行 ──► 共识确认 ──► Checkpoint 生成 ──► Checkpoint 签名
   ~400ms      ~100ms       ~100-200ms         ~100ms
            │                               │
            └───────── ~200-400ms ──────────┘

总延迟: ~700ms+ (取决于 min_checkpoint_interval_ms: 200)
```

Checkpoint 需要等待多个交易批量打包，并经过验证者签名，因此延迟高于单笔交易执行。

### 4.4 与 dYdX 对比

| 对比项 | dYdX v4 | Sui DEX |
|--------|---------|---------|
| **乐观触发** | 撮合完成 (<10ms) | 交易执行 (~400ms) |
| **最终确认** | 区块确认 (~1-2s) | Checkpoint (~700ms+) |
| **延迟差距** | 乐观→最终: ~1-2s | 乐观→最终: ~300ms |

**关键差异**:
- Sui 的乐观确认延迟（~400ms）比 dYdX（<10ms）高，因为 DEX 集成于验证器，交易需要经过执行流程
- 但 Sui 的最终确认比 dYdX 快（~700ms vs ~1-2s）

---

## 5. OnChainUpdates 方案对比

### 5.1 方案 A: 纯 Checkpoint

```
┌─────────────────────────────────────────────────────────────────────┐
│  方案 A: 纯 Checkpoint OnChainUpdates                               │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  触发: 交易进入 Checkpoint (~700ms+)                                 │
│  获取: gRPC Checkpoint 订阅                                          │
│                                                                      │
│  事件:                                                               │
│  • Fill             - 成交记录                                       │
│  • Position         - 持仓变化                                       │
│  • Balance          - 余额变化                                       │
│  • Transfer         - 存取款                                         │
│  • Liquidation      - 清算事件                                       │
│  • FundingRate      - 资金费率                                       │
│                                                                      │
│  特点: 所有事件都是最终确认状态                                      │
└─────────────────────────────────────────────────────────────────────┘
```

**实现方式**:
```rust
// gRPC Checkpoint 订阅
// 代码参考: sui-indexer-alt-framework/src/ingestion/streaming_client.rs:28-30
let mut client = SubscriptionServiceClient::connect(endpoint).await?;
let stream = client.subscribe_checkpoints(request).await?;

// 处理 Checkpoint 中的 DEX 事件
for checkpoint in stream {
    for tx in checkpoint.transactions {
        let dex_events = extract_dex_events(&tx);
        publish_onchain_updates(dex_events).await;
    }
}
```

**优点**:
- 实现简单，维护成本低
- 数据一致性最高（单一来源）
- 不需要处理回滚情况
- API 设计简单（单一事件流）

**缺点**:
- 延迟较高 (~700ms+)
- 用户成交确认较慢

### 5.2 方案 B: 双层 OnChainUpdates

```
┌─────────────────────────────────────────────────────────────────────┐
│  Layer 1: Optimistic OnChainUpdates (~400ms)                        │
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
│  Layer 2: Finalized OnChainUpdates (~700ms+)                        │
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

**Layer 1 实现方式**:
```rust
// 方式 A: 通过 RPC 订阅（适合外部客户端）
// 代码参考: sui-core/src/subscription_handler.rs:119-124
let stream = sui_client.subscribe_transaction(filter).await?;

// 方式 B: 内部执行回调（适合验证器集成）
fn on_transaction_executed(&self, effects: &TransactionEffects) {
    let dex_events = extract_dex_events(effects);
    self.emit_optimistic_update(dex_events);
}
```

**Layer 2 实现方式**:
```rust
// 与方案 A 相同
let stream = client.subscribe_checkpoints(request).await?;
```

**优点**:
- 用户可更快看到成交确认 (~400ms vs ~700ms+)
- 比 dYdX 多一层 Optimistic 确认
- 改善交易体验

**缺点**:
- 实现复杂度增加
- 需要处理 Optimistic→Finalized 状态转换
- 需要处理极少数 Optimistic 失败情况
- API 设计复杂（需区分 Optimistic/Finalized）

---

## 6. 利弊分析总结

### 6.1 详细对比表

| 维度 | 方案 A: 纯 Checkpoint | 方案 B: 双层 OnChainUpdates |
|------|----------------------|---------------------------|
| **延迟** | ~700ms+ | Optimistic ~400ms, Finalized ~700ms+ |
| **实现复杂度** | 低 | 中 |
| **代码量** | ~500 行 | ~1500 行 |
| **数据一致性** | 最高（单一来源） | 需要处理状态转换 |
| **用户体验** | 成交确认慢 | 成交确认较快（标注为"确认中"） |
| **回滚处理** | 不需要 | 需要处理极少数 Optimistic 失败 |
| **API 设计** | 简单（单一事件流） | 复杂（需区分 Optimistic/Finalized） |
| **维护成本** | 低 | 中 |
| **测试复杂度** | 低 | 中高 |

### 6.2 与 dYdX 完整对比

| 通道 | dYdX v4 | Sui DEX (方案 A) | Sui DEX (方案 B) |
|------|---------|------------------|------------------|
| **OffChainUpdates** | CheckTx ~10-50ms | 撮合 <10ms | 撮合 <10ms |
| **OnChainUpdates-Optimistic** | 无 | 无 | 执行 ~400ms |
| **OnChainUpdates-Finalized** | EndBlocker ~1-2s | Checkpoint ~700ms+ | Checkpoint ~700ms+ |

### 6.3 何时选择哪个方案

**选择方案 A (纯 Checkpoint) 当**:
- 团队人力有限，需要快速上线
- 系统简单性是首要考虑
- 700ms+ 延迟对业务可接受
- 不想处理回滚和状态转换逻辑

**选择方案 B (双层) 当**:
- 用户体验是首要考虑
- 需要与中心化交易所竞争
- 团队有能力处理额外复杂度
- 愿意投入更多测试资源

---

## 7. 实现要点

### 7.1 方案 A 实现

```rust
pub struct CheckpointIndexer {
    checkpoint_client: SubscriptionServiceClient,
    event_publisher: EventPublisher,
}

impl CheckpointIndexer {
    pub async fn run(&mut self) -> Result<()> {
        let stream = self.checkpoint_client.subscribe_checkpoints().await?;

        while let Some(checkpoint) = stream.next().await {
            for tx in checkpoint.transactions {
                // 提取 DEX 相关事件
                let events = self.extract_dex_events(&tx)?;

                // 发布 OnChainUpdates
                for event in events {
                    self.event_publisher.publish_onchain(event).await?;
                }
            }
        }
        Ok(())
    }

    fn extract_dex_events(&self, tx: &Transaction) -> Result<Vec<DexEvent>> {
        // 从交易中提取 Fill, Position, Balance 等事件
        // ...
    }
}
```

### 7.2 方案 B 实现

```rust
pub struct DualLayerIndexer {
    // Layer 1: Optimistic
    execution_listener: ExecutionListener,

    // Layer 2: Finalized
    checkpoint_client: SubscriptionServiceClient,

    event_publisher: EventPublisher,
    pending_optimistic: HashMap<TxDigest, Vec<DexEvent>>,
}

impl DualLayerIndexer {
    pub async fn run(&mut self) -> Result<()> {
        tokio::select! {
            // Layer 1: 监听交易执行
            result = self.run_optimistic_layer() => result?,

            // Layer 2: 监听 Checkpoint
            result = self.run_finalized_layer() => result?,
        }
        Ok(())
    }

    async fn run_optimistic_layer(&mut self) -> Result<()> {
        let stream = self.execution_listener.subscribe().await?;

        while let Some(effects) = stream.next().await {
            let events = self.extract_dex_events(&effects)?;

            // 发布 Optimistic 事件
            for event in &events {
                self.event_publisher.publish_optimistic(event.clone()).await?;
            }

            // 记录待确认事件
            self.pending_optimistic.insert(effects.digest, events);
        }
        Ok(())
    }

    async fn run_finalized_layer(&mut self) -> Result<()> {
        let stream = self.checkpoint_client.subscribe_checkpoints().await?;

        while let Some(checkpoint) = stream.next().await {
            for tx in checkpoint.transactions {
                let digest = tx.digest();

                // 移除已确认的 Optimistic 事件
                self.pending_optimistic.remove(&digest);

                let events = self.extract_dex_events(&tx)?;

                // 发布 Finalized 事件
                for event in events {
                    self.event_publisher.publish_finalized(event).await?;
                }
            }

            // 处理超时未确认的 Optimistic 事件（回滚）
            self.handle_rollbacks(&checkpoint).await?;
        }
        Ok(())
    }

    async fn handle_rollbacks(&mut self, checkpoint: &Checkpoint) -> Result<()> {
        let cutoff = checkpoint.timestamp - ROLLBACK_TIMEOUT;

        for (digest, events) in self.pending_optimistic.drain_filter(|_, e| e.timestamp < cutoff) {
            // 发布回滚通知
            for event in events {
                self.event_publisher.publish_rollback(event).await?;
            }
        }
        Ok(())
    }
}
```

### 7.3 API 设计对比

**方案 A API**:
```typescript
// 简单的单一事件流
interface OnChainEvent {
  type: 'fill' | 'position' | 'balance' | 'transfer';
  data: EventData;
  timestamp: number;
  checkpoint: number;
}
```

**方案 B API**:
```typescript
// 区分 Optimistic 和 Finalized
interface OnChainEvent {
  type: 'fill' | 'position' | 'balance' | 'transfer';
  data: EventData;
  timestamp: number;
  status: 'optimistic' | 'finalized' | 'rolled_back';
  checkpoint?: number;  // 仅 finalized 有
}
```

---

## 8. 结论

### 8.1 推荐策略

**阶段 1 (MVP)**: 采用方案 A (纯 Checkpoint)
- 快速上线，验证核心功能
- ~700ms 延迟对早期用户可接受

**阶段 2 (优化)**: 评估是否升级到方案 B
- 根据用户反馈决定
- 如果延迟成为痛点，再投入资源实现双层

### 8.2 关键决策点

| 决策因素 | 选方案 A | 选方案 B |
|----------|----------|----------|
| 团队规模 | 小团队 | 大团队 |
| 上线时间 | 紧迫 | 充裕 |
| 用户预期 | 理解区块链延迟 | 期望中心化体验 |
| 竞品压力 | 低 | 高 |

### 8.3 核心架构图

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

---

## 附录: 代码引用

| 组件 | 文件 | 说明 |
|------|------|------|
| dYdX OffChain/OnChain 分析 | `mynotes/dex/analyst/dydx-indexer-analyst.md:1366-1551` | 完整时序和事件详解 |
| Sui 交易订阅 | `sui-core/src/subscription_handler.rs:119-124` | `subscribe_transactions` 实现 |
| Checkpoint 最小间隔 | `sui-protocol-config/src/lib.rs:1712` | `min_checkpoint_interval_ms: 200` |
| Checkpoint gRPC 客户端 | `sui-indexer-alt-framework/src/ingestion/streaming_client.rs:28-30` | `CheckpointStreamingClient` trait |
