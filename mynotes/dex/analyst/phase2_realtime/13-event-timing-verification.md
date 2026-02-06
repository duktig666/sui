# Sui 事件订阅时序验证报告

## 概述

本文档记录了对 `sui_subscribeEvent` 事件推送时序的验证测试，目的是确认 DEX Indexer 实时架构的核心假设：**事件订阅可以在 Checkpoint 生成前获得事件通知**。

## 验证目标

| 假设 | 预期结果 |
|------|----------|
| 事件推送先于 Checkpoint | Event 延迟 < Checkpoint 延迟 |
| 实现 400-650ms 低延迟 | 事件在交易执行后立即推送 |

## 验证方法

### 方案：源码插桩 + 日志分析

在 Sui 源码中添加时序插桩，记录关键节点的时间戳：

#### 插桩点 1：事件推送时刻

**文件**: `sui-core/src/authority.rs:3326`

```rust
// [TIMING INSTRUMENTATION] Record event emission time for timing verification
let event_emit_time_ms = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map(|d| d.as_millis() as u64)
    .unwrap_or(0);
info!(
    ?tx_digest,
    event_emit_time_ms,
    event_count = events.data.len(),
    "[TIMING] Event emitted to subscribers"
);
self.subscription_handler
    .process_tx(certificate.data().transaction_data(), &effects, &events)
```

#### 插桩点 2：Checkpoint 包含交易时刻

**文件**: `sui-core/src/checkpoints/mod.rs:1461`

```rust
// [TIMING INSTRUMENTATION] Collect tx digests before write_checkpoints
let checkpoint_tx_info: Vec<(_, u64)> = new_checkpoints
    .iter()
    .flat_map(|(ckpt, contents)| {
        let seq = *ckpt.sequence_number();
        contents.iter().map(move |digests| (digests.transaction, seq))
    })
    .collect();

self.write_checkpoints(last_details.checkpoint_height, new_checkpoints)
    .await?;

// [TIMING INSTRUMENTATION] Record checkpoint inclusion time
let checkpoint_time_ms = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map(|d| d.as_millis() as u64)
    .unwrap_or(0);
for (tx_digest, checkpoint_seq) in checkpoint_tx_info {
    info!(
        ?tx_digest,
        checkpoint_seq,
        checkpoint_time_ms,
        "[TIMING] Transaction included in checkpoint"
    );
}
```

### 测试环境

```bash
# 启动带插桩的本地节点（单 Validator 模式）
sudo RUST_LOG="info" \
  ./target/debug/sui start --committee-size 1 \
    --network.config "$SUI_CHAIN_DIR" \
    --with-faucet=0.0.0.0:9123 \
    --fullnode-rpc-port 9001 2>&1 | tee timing_test.log
```

### 分析工具

Python 分析脚本 `dex-sui/scripts/analyze_timing.py`，用于：
- 解析 `[TIMING]` 标记的日志
- 匹配同一交易的 Event 和 Checkpoint 时间戳
- 计算延迟差异统计

## 测试结果

### 测试 1：默认多节点模式

```
Total transactions analyzed: 2857
Events before checkpoint: 0/2857 (0.0%)
Events after checkpoint:  2857/2857 (100.0%)

Checkpoint - Event Difference:
  Average: -409.8ms
  Min:     -1327ms
  Max:     -198ms
  P50:     -390ms
  P90:     -300ms
```

### 测试 2：单 Validator 模式 (`--committee-size 1`)

```
Total transactions analyzed: 8030
Events before checkpoint: 0/8030 (0.0%)
Events after checkpoint:  8030/8030 (100.0%)

Checkpoint - Event Difference:
  Average: -395.6ms
  Min:     -1230ms
  Max:     -168ms
  P50:     -383ms
  P90:     -295ms
```

### 关键发现：Sui 节点架构

即使使用 `--committee-size 1`，`sui start` 仍会启动**两个独立节点**：

| 节点 | 角色 | Event 日志 | Checkpoint 日志 | RPC 服务 |
|------|------|------------|-----------------|----------|
| Fullnode | 执行用户交易 | ✅ 10,248 | ❌ 0 | ✅ 9001 |
| Validator | 共识排序 | ❌ 0 | ✅ 10,255 | ❌ 无 |

**关键证据**：`execution_driver` 调用分布

| 节点 | `execution_driver` 次数 | 说明 |
|------|------------------------|------|
| Fullnode | 12,387 | 执行所有用户交易 |
| Validator | 132 | 仅执行系统交易（epoch 变更等） |

## 架构分析

### Sui 交易执行流程

```
用户提交交易
     │
     v
┌─────────────┐     ┌─────────────────────────────────┐
│  Fullnode   │────>│         Validator               │
│  (RPC 9001) │     │  - 共识排序                     │
│             │     │  - Checkpoint 构建 (T=0)        │
│             │<────│                                 │
│  - 同步共识结果    │     └─────────────────────────────────┘
│  - 执行交易 (T+300ms)
│  - 推送 Event (T+400ms)
│             │
└─────────────┘
     │
     v
WebSocket 订阅者收到事件
```

### 为什么 Event 总是晚于 Checkpoint？

1. **Validator 不执行用户交易** - 只负责共识排序和 Checkpoint 构建
2. **Fullnode 执行交易** - 需要先从 Validator 同步共识结果
3. **同步延迟** - Fullnode 同步 + 执行需要约 300-400ms

### 源码证据

**Event 推送条件** (`authority.rs:3281`)：
```rust
fn post_process_one_tx(...) -> SuiResult {
    if self.indexes.is_none() {
        return Ok(());  // Validator 可能没有启用 indexes
    }
    // ... Event 推送逻辑
}
```

**Validator 不提供 RPC** (`sui-node/src/lib.rs:2496`)：
```rust
// Validators do not expose these APIs
if config.consensus_config().is_some() {
    return Ok((HttpServers::default(), None));
}
```

## 验证结论

### ❌ 原假设不成立

| 验证点 | 结论 | 说明 |
|--------|------|------|
| Event 先于 Checkpoint | ❌ 否 | Event 晚于 Checkpoint 约 400ms |
| 可绕过 Checkpoint | ❌ 否 | Event 本质上依赖 Checkpoint 同步 |

### ✅ 实际行为

| 特性 | 说明 |
|------|------|
| Event 已被确认 | 收到的 Event 已经过 Validator 共识 |
| 无需额外验证 | 不需要再次检查 Checkpoint |
| 延迟约 400-600ms | 端到端延迟包含同步时间 |

## 对 DEX Indexer 的影响

### 架构调整

原设计假设：
```
sui_subscribeEvent → 立即获取未确认事件 → 需要 Checkpoint 验证
```

实际情况：
```
sui_subscribeEvent → 获取已确认事件 → 无需额外验证
```

### 延迟预期

| 阶段 | 延迟 |
|------|------|
| 交易提交 → Validator 共识 | ~200ms |
| Checkpoint 构建 | ~50ms |
| Fullnode 同步 | ~300ms |
| Event 推送 | ~50ms |
| **总计** | **~600ms** |

### 实现建议

```rust
// 直接使用 Event，无需 Checkpoint 验证
let event_stream = sui_client.event_api()
    .subscribe_event(filter)
    .await?;

while let Some(event) = event_stream.next().await {
    // Event 已经被共识确认，可以直接处理
    process_confirmed_event(event).await;
}
```

### Checkpoint 的作用

| 用途 | 说明 |
|------|------|
| 历史数据同步 | 启动时回补历史事件 |
| 故障恢复 | 重新同步丢失的事件 |
| 审计追溯 | 验证历史状态 |

## 相关文件

| 文件 | 说明 |
|------|------|
| `sui-core/src/authority.rs:3326` | Event 推送入口 |
| `sui-core/src/checkpoints/mod.rs:1461` | Checkpoint 构建 |
| `sui-node/src/lib.rs:2496` | Validator RPC 禁用检查 |
| `dex-sui/scripts/analyze_timing.py` | 时序分析脚本 |

## 总结

**核心结论**：`sui_subscribeEvent` 推送的事件**已经过共识确认**。

- Event 在 Checkpoint 构建后约 400ms 到达
- 不需要额外的 Checkpoint 验证步骤
- DEX Indexer 可以直接使用 Event 数据
- 端到端延迟约 400-600ms，符合设计目标

**架构启示**：Sui 的 Validator/Fullnode 分离架构确保了：
1. **数据可靠性** - Event 已被共识确认
2. **职责分离** - Validator 专注共识，Fullnode 服务用户
3. **简化 Indexer** - 无需实现复杂的确认逻辑