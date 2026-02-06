# dex-realtime 多节点数据一致性方案

## 问题描述

三个关键问题：
1. **多用户并发下单** - 大量用户同时下单，保证实时数据推送的正确性
2. **多 realtime 实例部署** - 高可用部署，避免单点故障
3. **Sui 节点连接** - 连接单个节点是否能获取全量事件？

需要保证：
- 事件不丢失
- 事件不重复
- 事件顺序正确

---

## Sui 事件订阅机制分析

### 核心结论

| 问题 | 答案 | 保证级别 |
|------|------|---------|
| 单 fullnode 能获取全网事件？ | ✅ 可以，延迟 ~200-400ms | 最终一致 |
| 不同 fullnode 事件一致？ | ✅ 完全一致 | 强一致（来自 Checkpoint） |
| **Checkpoint 前能看到事件？** | ⚠️ **部分可以** | 见下文 |
| subscribe_event 保证不丢失？ | ❌ 否 | Best-effort |
| 断线重连会丢失事件？ | ❌ 是的 | 无重放机制 |

### 快速通道事件推送

事件在 Checkpoint 之前就可以推送。

**代码证据**（`sui-core/src/authority.rs:3324-3326`）：
```rust
// execute_certificate 完成后立即调用
self.subscription_handler
    .process_tx(certificate.data().transaction_data(), &effects, &events)
```

**事件推送时序**：

```
用户提交交易
    │
    ↓ ~100-400ms (Mysticeti 共识)
共识排序完成
    │
    ↓ ~50-200ms (验证器本地执行)
execute_certificate 完成
    │
    ↓ <1ms (同步调用 subscription_handler.process_tx)
事件推送给 WebSocket 订阅者 ← 此时 Checkpoint 尚未形成！
    │
    ↓ ~10-50ms (网络传输)
dex-realtime 收到事件
```

### 验证器 vs 全节点

| 连接目标 | 事件延迟 | 说明 |
|----------|----------|------|
| **验证器节点** | ~200-600ms | execute_certificate 后立即推送 |
| **全节点（公共 RPC）** | ~200-400ms（额外） | 需等 Checkpoint + 本地执行 |

**重要限制**：
- ⚠️ **验证器不对外提供 WebSocket RPC**
- 公共 RPC 节点都是**全节点**
- 全节点需要等 Checkpoint 同步后才能推送事件

**代码证据**（`sui-core/src/authority.rs:5140`）：
```rust
// 订阅功能对验证器是禁用的
if self.indexes.is_none() || self.is_validator(epoch_store) {
    return Ok(());
}
```

### 延迟估算

| 阶段 | 延迟 | 说明 |
|------|------|------|
| 共识排序 | 100-400ms | Mysticeti BFT |
| 本地执行 | 50-200ms | Move VM 执行 |
| 事件分发 | <1ms | 内存 channel |
| 全节点同步 | 50-150ms | Checkpoint 同步 |
| **总计** | **200-600ms** | 正常情况 |

### 关键限制

**1. 必须连接全节点，非验证器**

验证器禁用订阅功能，必须连接全节点（公共 RPC 或自建）。

**2. 缓冲区满则断开**

代码位置：`sui/crates/sui-core/src/streamer.rs`

```rust
const EVENT_DISPATCH_BUFFER_SIZE: usize = 1000;

// 缓冲满时不是等待，而是断开连接
match subscriber.try_send(data.into()) {
    Ok(_) => success_counter.inc(),
    Err(e) => {
        to_remove.push(id.clone());  // 订阅者被移除！
        failure_counter.inc();
    }
}
```

**3. 断线无重放机制**

```
时间线：
  T0: 连接建立，开始接收事件
  T1: 网络抖动，连接断开
      ↓
  T1-T2: 期间事件丢失（无法恢复）
      ↓
  T2: 重连成功，从新事件开始
```

### 对 dex-realtime 的影响

| 场景 | 影响 | 解决方案 |
|------|------|----------|
| 连接全节点 | 比验证器多 50-150ms | 可接受，自建全节点可优化 |
| 高频事件 | 缓冲满导致断连 | 提高处理速度，减少阻塞操作 |
| 网络抖动 | 丢失中间事件 | 使用 Checkpoint API 补齐 |
| 节点宕机 | 丢失事件 | 双通道架构（realtime + indexer） |

---

## 延迟对比分析

### 与竞品对比

| 指标 | Sui DEX (dex-realtime) | Hyperliquid | 中心化交易所 |
|-----|------------------------|-------------|-------------|
| 订单簿更新 | **~200-600ms** | ~50ms | ~10ms |
| 最近成交 | **~200-600ms** | ~50ms | ~10ms |
| 跨节点可见 | 全节点同步后 | 实时 | 实时 |
| 差距倍数 | 基准 | 4-12x 快 | 20-60x 快 |

### 实际用户体验

```
场景：用户 A 在 UI 上下单

T0:       用户 A 点击下单
T0+50ms:  交易提交到节点
T0+200ms: 共识排序完成
T0+400ms: 交易执行，订单进入订单簿
T0+450ms: 事件推送给全节点订阅者

用户等待 ~450ms 看到自己的订单（可接受）
```

### 仍需优化的场景

| 场景 | 当前延迟 | 目标 | 方案 |
|------|----------|------|------|
| 用户下单确认 | ~450ms | <100ms | 乐观 UI |
| 订单簿实时更新 | ~450ms | <100ms | DEX Engine 乐观事件 |
| 高频交易 | ~450ms | <50ms | 需要 Sequencer |

### Sui 架构的限制

| 限制 | 原因 | 实际影响 |
|------|------|----------|
| 共识延迟 ~200ms | Mysticeti BFT | 不可避免的最低延迟 |
| 执行延迟 ~50-200ms | Move VM | 可通过优化减少 |
| 全节点同步 ~50-150ms | Checkpoint 同步 | 自建全节点可优化 |
| 验证器不开放 RPC | 安全设计 | 必须使用全节点 |

---

## dYdX v4 解决方案分析

dYdX v4 通过**两层事件系统**解决延迟问题：

```
┌─────────────────────────────────────────────────────────────┐
│                     dYdX v4 事件流                          │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  用户下单                                                    │
│      ↓                                                      │
│  CheckTx（验证交易）                                         │
│      ↓                                                      │
│  ┌─────────────────┐                                        │
│  │ 乐观匹配 MemClob │ ──→ 乐观事件（~1ms）──→ UI 立即更新    │
│  └─────────────────┘                                        │
│      ↓                                                      │
│  共识排序（CometBFT）                                        │
│      ↓                                                      │
│  FinalizeBlock                                              │
│      ↓                                                      │
│  ┌─────────────────┐                                        │
│  │ 确定性成交/清算  │ ──→ 确定事件（~600ms）──→ 持久化存储    │
│  └─────────────────┘                                        │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

**两层事件的作用**：

| 层级 | 触发时机 | 延迟 | 可靠性 | 用途 |
|------|----------|------|--------|------|
| **乐观事件** | CheckTx（交易验证阶段） | ~1ms | 可能被回滚 | UI 实时反馈、订单簿显示 |
| **确定事件** | FinalizeBlock（区块确认后） | ~600ms | 不可逆 | 数据持久化、账户结算 |

**关键代码机制**（`full_node_streaming_manager.go`）：
```go
// 乐观阶段：CheckTx 时直接缓存并发送
if !lib.IsDeliverTxMode(ctx) {
    streamUpdates := getStreamUpdatesFromOffchainUpdates(...)
    sm.AddOrderUpdatesToCache(streamUpdates, clobPairIds)
    return  // 不等共识，立即返回
}

// 确定阶段：FinalizeBlock 后暂存
stagedEvent := clobtypes.StagedFinalizeBlockEvent{...}
sm.finalizeBlockStager.StageFinalizeBlockEvent(ctx, &stagedEvent)
```

**dYdX vs Sui 延迟对比**：

| 指标 | dYdX v4 | Sui | 差距 |
|------|---------|-----|------|
| 乐观事件 | ~1ms | ❌ 无 | N/A |
| 确定事件 | ~600ms | ~450ms | 0.75x |
| 用户感知 | ~1ms | ~450ms | **450x** |

### 为什么 dYdX 能做到？

| 特性 | dYdX v4 | Sui | 影响 |
|------|---------|-----|------|
| **订单簿位置** | 链外（MemClob）| 链上共享对象 | dYdX 可乐观匹配 |
| **事件存储** | 内存 transient store | Checkpoint 持久化 | dYdX 无需等签名 |
| **节点角色** | 验证器 = Full Node | 验证器 ≠ Full Node | dYdX 直接提供流 |
| **共识模型** | CometBFT 单链 | DAG 并行共识 | Sui 需等 Checkpoint |

---

## 推荐方案

### 方案评估

| 方案 | 延迟 | 必要性 | 推荐 |
|------|------|--------|------|
| 原生 Sui（快速通道） | ~450ms | 基准 | ✅ 可接受 |
| DEX Engine 乐观事件 | ~10ms | 锦上添花 | ⚠️ 可选 |
| 纯客户端乐观 UI | ~0ms（感知） | 简单提升 | ✅ 推荐 |

### 首选：原生 Sui + 乐观 UI

```
┌────────────────────────────────────────────────────────────┐
│                    推荐实施方案                            │
├────────────────────────────────────────────────────────────┤
│                                                            │
│  用户          Sui 链           dex-realtime     客户端    │
│   │              │                 │               │       │
│   ├─ 下单 ──────→│                 │               │       │
│   │              │                 │    ┌──────────┤       │
│   │              │                 │    │ 乐观 UI  │ (~0ms)│
│   │              │                 │    │ "确认中" │       │
│   │              │                 │    └──────────┤       │
│   │              ├─ 共识（~200ms） │               │       │
│   │              ├─ 执行（~200ms） │               │       │
│   │              ├─ 事件推送 ─────→│               │       │
│   │              │                 ├──────────────→│(~450ms)│
│   │              │                 │    ┌──────────┤       │
│   │              │                 │    │ 更新为   │       │
│   │              │                 │    │ "已确认" │       │
│   │              │                 │    └──────────┤       │
└────────────────────────────────────────────────────────────┘
```

**实施步骤**：

1. **Phase 1**：优化 dex-realtime 连接
   - 连接自建全节点（减少公共 RPC 延迟）
   - 确保处理速度跟上事件产生速度

2. **Phase 2**：客户端乐观 UI
   - 下单后立即显示"确认中"
   - 收到事件后更新为"已确认"
   - 用户感知延迟：~0ms

3. **Phase 3（可选）**：DEX Engine 乐观事件
   - 如果 450ms 仍不满足需求
   - 借鉴 dYdX 架构实现双层事件

**延迟效果**：

| 阶段 | 原生 Sui | +乐观 UI | +DEX Engine 乐观 |
|------|----------|----------|-----------------|
| 用户感知 | ~450ms | ~0ms | ~0ms |
| 其他用户看到 | ~450ms | ~450ms | ~10ms |
| 最终确认 | ~450ms | ~450ms | ~450ms |

### DEX Engine 乐观事件层（可选）

如果 450ms 对高频交易仍然太慢：

```
用户        DEX Engine       Redis Stream      其他用户
  │              │                │               │
  ├─ 下单 ──────→│                │               │
  │              ├─ 匹配/风控     │               │
  │              ├─ 乐观事件 ────→│ optimistic:* │
  │              │                ├──────────────→│ (~10ms)
  │              ├─ 提交 Sui      │               │
  │              │   ...          │               │
  │              ├─ 确定事件 ────→│ finalized:*  │
  │              │                ├──────────────→│ (~450ms)
```

**代码示例**：
```rust
// DEX Engine 乐观事件
pub struct OptimisticFillEvent {
    pub event_id: String,       // 本地生成的 ID
    pub is_optimistic: bool,    // true
    pub fill: FillData,
    pub timestamp_ms: u64,
}

// Redis Stream 双通道
// dex:stream:optimistic:* - 乐观事件
// dex:stream:finalized:*  - 确定事件
```

### 长期关注

- Sui 协议升级：共识延迟可能进一步优化
- 自建验证器：如果能成为验证器，可获得更快的事件推送
- 应用链方案：如果需要 <50ms 延迟，考虑独立应用链

---

## 场景 1：多用户并发下单

### 结论：已有保证，无需额外处理

```
用户A下单 ──┐
用户B下单 ──┼─→ Sui 链（共识排序）──→ 全局顺序的事件流
用户C下单 ──┘                              │
                                           ↓
                                    dex-realtime 订阅
                                           │
                                           ↓
                                    所有客户端收到相同顺序
```

**保证机制**：
| 层级 | 机制 | 说明 |
|------|------|------|
| Sui 共识层 | Narwhal/Bullshark | 全局事件顺序，高吞吐（~100k TPS） |
| WebSocket 订阅 | 单连接顺序 | Sui RPC 保证事件按顺序推送 |
| Redis Stream | FIFO | 消息按写入顺序存储 |

**无需改动**：Sui 链天然保证了多用户并发下单的顺序一致性。

### 关于节点连接

**问题**：需要连接所有节点吗？

**答案**：**不需要**。连接任意一个健康的 fullnode 即可获取全量事件。

**原因**：
- 所有事件包含在全网一致的 Checkpoint 中
- Checkpoint 由 2f+1 验证者签名，不可伪造
- 每个 fullnode 同步相同的 Checkpoint
- 事件来自区块链状态，不是本地生成

**时间差**：
- 不同 fullnode 同步速度可能有 100-500ms 差异
- 但最终都会看到相同的事件（最终一致性）

---

## 场景 2：多 realtime 实例部署

### 问题：事件重复发布

```
                         ┌─ dex-realtime-1 ──┐
Sui RPC ────WebSocket───┼                    ├─→ Redis Stream ─→ 消息重复！
                         └─ dex-realtime-2 ──┘
```

当前两个实例会订阅相同事件，导致 Redis 中每个事件被写入两次。

### 当前架构状态

| 层级 | 保证机制 | 状态 |
|------|----------|------|
| Sui 链 | 全局事件顺序（consensus） | ✅ |
| Redis Stream | 消息持久化 + 顺序保证 | ✅ |
| dex-ws 消费 | 消费者组负载均衡 | ✅ |

### 缺失保证

| 问题 | 影响 | 优先级 |
|------|------|--------|
| 多 realtime 实例重复发布 | 消息重复 | 高 |
| 事件 ID 未用于去重 | 重启后可能重发 | 中 |
| 无 leader election | 无法高可用部署 | 中 |

### 方案 1：事件幂等性（推荐）

使用 Sui 事件的唯一标识作为去重 key，实现幂等发布。

```rust
// publisher.rs 核心修改
async fn publish_event(&mut self, event: &SuiEvent) -> Result<()> {
    let stream_key = Self::get_stream_key(&event);
    let event_json = serde_json::to_string(&event)?;

    // Sui 事件唯一标识：交易摘要 + 事件序号
    let sui_event_id = format!(
        "{}-{}",
        event.id.tx_digest.base58(),  // Base58 编码
        event.id.event_seq
    );

    // 使用 Redis SET NX + XADD 组合实现幂等
    let dedup_key = format!("dex:event:seen:{}", sui_event_id);

    // SETNX - 只在不存在时设置（原子操作）
    let is_new: bool = redis::cmd("SET")
        .arg(&dedup_key)
        .arg("1")
        .arg("NX")           // 不存在才设置
        .arg("EX")
        .arg(3600)           // 1 小时过期
        .query_async(&mut self.conn)
        .await?;

    if is_new {
        // 首次见到此事件，发布到 Stream
        redis::cmd("XADD")
            .arg(stream_key)
            .arg("*")
            .arg("event_id")
            .arg(&sui_event_id)
            .arg("data")
            .arg(&event_json)
            .query_async(&mut self.conn)
            .await?;
    } else {
        debug!("Duplicate event skipped: {}", sui_event_id);
    }
    Ok(())
}
```

**优点**：
- 实现简单，改动小
- 无论多少实例发布，消息只有一份
- 不需要分布式协调

**TTL 设计**：
- 1 小时足够覆盖实例重启场景
- 比 checkpoint 延迟（3-5s）长很多，安全边际足够

### 方案 2：Leader Election（可选增强）

使用 Redis 分布式锁实现主节点选举。

```rust
// leader.rs
pub struct LeaderElection {
    redis: MultiplexedConnection,
    instance_id: String,
    lock_key: &'static str,
    lock_ttl_secs: u64,
}

impl LeaderElection {
    const LOCK_KEY: &'static str = "dex:realtime:leader";

    pub async fn try_acquire(&mut self) -> Result<bool> {
        let acquired: bool = redis::cmd("SET")
            .arg(Self::LOCK_KEY)
            .arg(&self.instance_id)
            .arg("NX")
            .arg("EX")
            .arg(self.lock_ttl_secs)  // 10s
            .query_async(&mut self.redis)
            .await?;
        Ok(acquired)
    }

    pub async fn renew(&mut self) -> Result<bool> {
        // 只有自己持有锁时才续期
        let script = r#"
            if redis.call('GET', KEYS[1]) == ARGV[1] then
                return redis.call('EXPIRE', KEYS[1], ARGV[2])
            else
                return 0
            end
        "#;
        let renewed: i32 = redis::Script::new(script)
            .key(Self::LOCK_KEY)
            .arg(&self.instance_id)
            .arg(self.lock_ttl_secs)
            .invoke_async(&mut self.redis)
            .await?;
        Ok(renewed == 1)
    }
}
```

**优点**：
- 只有一个实例工作，完全避免重复
- 支持自动故障转移

**缺点**：
- 增加复杂性
- 故障切换有延迟（锁过期时间）

---

## 场景 3：事件丢失恢复

### 问题：WebSocket 订阅可能丢失事件

由于 Sui RPC 的限制，单纯依赖 `subscribe_event` **无法保证不丢失**。

### 推荐架构：双通道互补

```
┌───────────────────────────────────────────────────────┐
│                   Sui Full Node                       │
│                                                       │
│   subscribe_event (WS)          Checkpoint API (HTTP) │
│   延迟: <500ms                  延迟: 2-3s            │
│   保证: Best-effort            保证: 不丢失           │
└───────────────────────────────────────────────────────┘
        │                                │
        ▼                                ▼
   dex-realtime                    dex-indexer
   (实时推送)                      (持久化存储)
        │                                │
        │    ┌───────────────────┐       │
        └───→│   Redis Stream    │←──────┘
             │   (可重放缓冲)     │
             └───────────────────┘
                      │
                      ▼
                   dex-ws
              (WebSocket 推送)
```

### 双通道职责

| 通道 | 职责 | 延迟 | 可靠性 |
|------|------|------|--------|
| **dex-realtime** | 实时推送，维护内存订单簿 | <500ms | Best-effort |
| **dex-indexer** | 持久化，补齐丢失的事件 | 2-3s | 100% |

### 事件恢复机制

**方案 A：基于 Checkpoint 的补齐**

```rust
// dex-realtime 断线重连时
async fn recover_missed_events(&mut self) -> Result<()> {
    // 1. 获取最后处理的 checkpoint 序号
    let last_checkpoint = self.get_last_processed_checkpoint().await?;

    // 2. 从 Checkpoint API 获取遗漏的事件
    let missed_events = self.fetch_events_since(last_checkpoint).await?;

    // 3. 去重后发布到 Redis
    for event in missed_events {
        self.publish_if_not_exists(event).await?;
    }

    Ok(())
}
```

**方案 B：依赖 dex-indexer 补齐（更简单，推荐）**

```
断线恢复流程：

dex-realtime 断线
       ↓
重连后从最新事件开始
       ↓
中间事件由 dex-indexer 通过 Checkpoint 补齐
       ↓
Redis Stream 最终包含完整事件序列
```

**推荐方案 B**：
- 职责分离更清晰
- dex-realtime 保持简单，专注实时推送
- dex-indexer 负责持久化和补齐

### 对客户端的保证

| 数据源 | 延迟 | 完整性 | 使用场景 |
|--------|------|--------|----------|
| dex-ws (WebSocket) | <500ms | 可能丢失 | UI 实时更新 |
| dex-api (REST) | 2-5s | 100% | 交易确认、持仓查询 |

**客户端策略**：
- 用 WebSocket 做实时展示（接受偶尔丢失）
- 用 REST API 做关键操作确认（保证准确）

---

## 实施路径

### 阶段 1：幂等发布（优先）

| 修改内容 | 文件 | 工作量 |
|----------|------|--------|
| 添加实例 ID 配置 | `config.rs` | 0.5 天 |
| 幂等发布逻辑 | `publisher.rs` | 1 天 |
| 处理自定义消息 ID | `dex-ws/subscriber.rs` | 0.5 天 |

### 阶段 2：Leader Election（可选）

| 修改内容 | 文件 | 工作量 |
|----------|------|--------|
| 新增 leader 模块 | `leader.rs` | 1 天 |
| 集成到 main | `main.rs` | 0.5 天 |
| 续期任务 | `leader.rs` | 0.5 天 |

### 阶段 3：监控告警

| 指标 | 说明 | 告警阈值 |
|------|------|----------|
| `events_deduplicated` | 被去重的事件数 | 持续增长说明多实例同时工作 |
| `publish_latency_p99` | 发布延迟 | > 100ms |
| `leader_status` | 是否为 leader | 无 leader 超过 30s |

---

## 验证方法

### 测试 1：多实例不重复

```bash
# 终端 1 - 启动实例 A
INSTANCE_ID=realtime-a cargo run --bin dex-realtime

# 终端 2 - 启动实例 B
INSTANCE_ID=realtime-b cargo run --bin dex-realtime

# 终端 3 - 触发链上事件后检查
redis-cli XLEN dex:stream:fills          # 应该等于链上事件数
redis-cli XRANGE dex:stream:fills - + COUNT 10  # 检查内容

# 检查去重 key
redis-cli KEYS "dex:event:seen:*" | wc -l  # 应该等于事件数
```

### 测试 2：故障切换

```bash
# 启动实例 A（成为 leader）
INSTANCE_ID=realtime-a cargo run --bin dex-realtime

# 启动实例 B（等待）
INSTANCE_ID=realtime-b cargo run --bin dex-realtime

# 杀掉实例 A
kill <pid-a>

# 观察实例 B 日志 - 应该在 10s 内成为 leader
# 触发事件验证 - 不应丢失
```

### 测试 3：并发下单顺序

```bash
# 使用 dex-node-test 并发下单
cargo run --example concurrent_orders -- --count 100 --concurrency 10

# 检查 WebSocket 客户端收到的事件顺序
# 所有客户端应收到相同顺序
```

---

## 待确认问题

1. **DEX Engine 是否需要乐观事件层？**
   - 当前 450ms 延迟是否可接受？
   - 高频交易场景的延迟要求？

2. **自建全节点 vs 公共 RPC？**
   - 自建可减少 ~50-150ms 延迟
   - 需要运维成本

3. **事件补齐由谁负责？**
   - dex-realtime 自己补齐（复杂）
   - dex-indexer 负责补齐（推荐）

4. **Leader Election 是否必要？**
   - 幂等发布已能解决重复问题
   - Leader Election 是更严格的保证

---

## 总结

| 问题 | 解决方案 | 优先级 |
|------|----------|--------|
| 多用户并发下单 | Sui 链天然保证 | ✅ 无需处理 |
| 多 realtime 实例重复 | 幂等发布（SET NX + XADD） | 高 |
| 事件丢失 | 双通道架构（realtime + indexer） | 高 |
| 高可用部署 | Leader Election（可选） | 中 |
| 延迟优化 | 乐观 UI + DEX Engine 乐观事件（可选） | 低 |
