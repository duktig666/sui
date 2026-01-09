# 方案一：FastShare 对象 + Unified Path

> 通过引入新的对象类型 FastShare，实现 Shared Object 级别的低延迟访问

---

## 1. 概述

### 1.1 目标
- 新增 `FastShare` 对象类型
- Owned + FastShare 混合交易走 Unified Path
- 目标延迟：~400ms（与 Fast Path 持平）

### 1.2 核心思想

```
Sui 现有：
┌─────────────────┐     ┌─────────────────┐
│  Owned Object   │     │  Shared Object  │
│   Fast Path     │     │ Consensus Path  │
│    ~400ms       │     │     ~2-3s       │
└─────────────────┘     └─────────────────┘
        ✗ 不可混合 ✗

Triton 扩展：
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│  Owned Object   │     │  Shared Object  │     │ FastShare Object│
│   Fast Path     │     │ Consensus Path  │     │  Unified Path   │
│    ~400ms       │     │     ~2-3s       │     │    ~400ms       │
└─────────────────┘     └─────────────────┘     └─────────────────┘
        │                                              │
        └──────────────── 可混合操作 ──────────────────┘
                         Unified Path
```

---

## 2. FastShare 对象设计

### 2.1 对象特性

| 特性 | Owned | Shared | FastShare |
|------|-------|--------|-----------|
| 所有权 | 单一 | 无 | 无 |
| 并发访问 | 不允许 | 允许（共识排序） | 允许（乐观并发） |
| 延迟 | ~400ms | ~2-3s | ~400ms |
| 冲突处理 | N/A | 共识排序 | 回滚重试 |
| 与 Owned 混合 | - | 不可 | **可** |

### 2.2 Move 定义

```move
module triton::transfer {
    /// 创建 FastShare 对象
    /// 对象将支持乐观并发访问，可与 Owned Object 混合操作
    public fun fast_share_object<T: key>(obj: T) {
        // 内部实现：设置对象为 FastShare 类型
        // 初始化版本号为 0
        native_fast_share_object(obj)
    }
}

// 使用示例
module dex::pool {
    use triton::transfer;

    struct Pool has key {
        id: UID,
        reserve_a: u64,
        reserve_b: u64,
    }

    public fun create_pool(ctx: &mut TxContext) {
        let pool = Pool {
            id: object::new(ctx),
            reserve_a: 0,
            reserve_b: 0,
        };
        // 使用 FastShare 而非 Shared
        transfer::fast_share_object(pool);
    }
}
```

### 2.3 对象存储结构

```rust
// sui-types/src/object.rs

/// FastShare 对象的元数据
pub struct FastShareMetadata {
    /// 乐观并发版本号，每次写操作递增
    pub version: u64,

    /// 最后一次成功写入的交易摘要
    pub last_writer: TransactionDigest,

    /// 最后写入时间戳（用于冲突分析）
    pub last_write_timestamp: u64,
}

/// 扩展 ObjectOwner 枚举
pub enum ObjectOwner {
    AddressOwner(SuiAddress),
    ObjectOwner(ObjectID),
    Shared { initial_shared_version: SequenceNumber },
    Immutable,
    /// 新增：FastShare 类型
    FastShare {
        initial_version: SequenceNumber,
        metadata: FastShareMetadata,
    },
}
```

---

## 3. Unified Path 协议

### 3.1 交易流程

```
Unified Path (Owned + FastShare):

┌────────┐    ┌────────────┐    ┌────────────┐    ┌──────────┐    ┌────────┐
│ Client │───→│ Validators │───→│  乐观执行   │───→│ 版本检查  │───→│ 确认   │
└────────┘    └────────────┘    └────────────┘    └──────────┘    └────────┘
                   │                   │                │              │
                   ↓                   ↓                ↓              ↓
              2f+1 签名          并行执行          无冲突？        写入状态
              (与 Fast Path      (假设版本         ├─ Yes → 确认
               相同)              有效)            └─ No  → 回滚
                   │
                   ↓
              总延迟 ~400ms
```

### 3.2 详细步骤

**Step 1: 交易签名**
```rust
// 客户端构造交易，包含 Owned 和 FastShare 对象
let tx = Transaction::new(
    sender,
    vec![
        InputObject::Owned(coin_object_id),           // Owned Object
        InputObject::FastShare(pool_object_id, expected_version),  // FastShare + 期望版本
    ],
    move_call,
);

// 发送给验证者收集签名
let certificate = client.sign_transaction(tx).await?;
```

**Step 2: 乐观执行**
```rust
// authority.rs - 验证者处理 Unified Path 交易

async fn execute_unified_path_transaction(
    &self,
    certificate: CertifiedTransaction,
) -> Result<TransactionEffects, UnifiedPathError> {
    // 1. 获取 FastShare 对象当前版本
    let fastshare_objects = certificate.fastshare_input_objects();

    // 2. 乐观执行（假设版本有效）
    let effects = self.execute_transaction_optimistically(&certificate)?;

    // 3. 版本检查
    for (obj_id, expected_version) in fastshare_objects {
        let current_version = self.get_fastshare_version(obj_id)?;
        if current_version != expected_version {
            return Err(UnifiedPathError::VersionConflict {
                object_id: obj_id,
                expected: expected_version,
                actual: current_version,
            });
        }
    }

    // 4. 提交状态变更
    self.commit_effects(effects).await
}
```

**Step 3: 冲突处理**
```rust
// 客户端处理冲突
match client.execute_unified_path(tx).await {
    Ok(effects) => {
        // 成功
        println!("Transaction confirmed: {:?}", effects.digest());
    }
    Err(UnifiedPathError::VersionConflict { object_id, expected, actual }) => {
        // 版本冲突，需要重试
        println!("Conflict on {:?}: expected v{}, got v{}", object_id, expected, actual);

        // 指数退避重试
        let new_version = actual;
        let retry_tx = rebuild_transaction_with_version(tx, object_id, new_version);
        // 重试...
    }
}
```

### 3.3 冲突处理策略

```
冲突场景：

时间 ──────────────────────────────────────→

TX-A: ────[签名]────[执行]────[确认]────
              ↑
         读取 v1

TX-B: ──────────[签名]────[执行]────[冲突!]
                    ↑              ↑
               读取 v1        发现 v2
                              (TX-A 已写入)

TX-B 重试: ─────────────────────[签名]────[执行]────[确认]
                                     ↑
                                读取 v2
```

**重试策略**：
```rust
pub struct RetryPolicy {
    /// 最大重试次数
    pub max_retries: u32,  // 默认: 3

    /// 初始退避时间
    pub initial_backoff: Duration,  // 默认: 50ms

    /// 退避因子
    pub backoff_factor: f64,  // 默认: 2.0

    /// 最大退避时间
    pub max_backoff: Duration,  // 默认: 1s
}

// 重试间隔: 50ms, 100ms, 200ms, ...
```

---

## 4. 实现路径

### 4.1 Sui 源码修改点

| 模块 | 文件 | 修改内容 |
|------|------|---------|
| **类型定义** | `sui-types/src/object.rs` | 新增 `ObjectOwner::FastShare` |
| **交易类型** | `sui-types/src/transaction.rs` | 支持 FastShare 输入对象 |
| **路径判断** | `sui-types/src/transaction.rs` | `is_unified_path_tx()` 方法 |
| **执行逻辑** | `sui-core/src/authority.rs` | Unified Path 执行流程 |
| **冲突检测** | `sui-core/src/transaction_driver/` | 版本冲突检测与回滚 |
| **存储层** | `sui-core/src/authority_store.rs` | FastShare 元数据存储 |

### 4.2 Move Framework 扩展

```move
// sui-framework/packages/sui-framework/sources/transfer.move

/// 将对象转换为 FastShare 类型
/// FastShare 对象支持乐观并发访问，延迟与 Owned Object 相当
public native fun fast_share_object<T: key>(obj: T);

/// 获取 FastShare 对象的当前版本（用于客户端构造交易）
public native fun fastshare_version<T: key>(obj: &T): u64;
```

### 4.3 渐进式实现

```
Phase 1: 基础支持
├── FastShare 对象类型定义
├── 存储层支持
└── 单 FastShare 对象交易

Phase 2: 混合操作
├── Owned + FastShare 混合交易
├── Unified Path 执行流程
└── 冲突检测与回滚

Phase 3: 客户端支持
├── SDK 支持
├── 重试策略
└── 版本查询 API

Phase 4: 优化
├── 批量版本查询
├── 预测性版本获取
└── 冲突统计与监控
```

---

## 5. 开发者指南

### 5.1 何时使用 FastShare

**适合场景**：
- DEX 流动性池（低冲突）
- 配置对象（读多写少）
- 计数器、累加器
- 需要与 Owned Object 原子操作

**不适合场景**：
- 高频写入的热点对象
- 严格顺序依赖的场景
- 需要全局公平排序

### 5.2 合约设计模式

**模式一：乐观更新**
```move
module dex::swap {
    use triton::transfer;

    struct Pool has key {
        id: UID,
        reserve_a: u64,
        reserve_b: u64,
    }

    /// 创建 FastShare 池
    public fun create_pool(ctx: &mut TxContext) {
        let pool = Pool { id: object::new(ctx), reserve_a: 0, reserve_b: 0 };
        transfer::fast_share_object(pool);
    }

    /// Swap 操作 - 客户端需处理冲突重试
    public fun swap(
        pool: &mut Pool,
        coin_in: Coin<A>,
        ctx: &mut TxContext
    ): Coin<B> {
        let amount_in = coin::value(&coin_in);
        let amount_out = calculate_output(pool, amount_in);

        // 更新储备
        pool.reserve_a = pool.reserve_a + amount_in;
        pool.reserve_b = pool.reserve_b - amount_out;

        // 销毁输入，创建输出
        coin::destroy(coin_in);
        coin::mint(amount_out, ctx)
    }
}
```

**模式二：重试友好设计**
```move
/// 设计原则：操作应该是幂等的或可安全重试的
public fun safe_increment(counter: &mut Counter, amount: u64) {
    // 使用单调递增，避免重试导致的重复增加
    counter.value = counter.value + amount;
    counter.last_update = tx_context::epoch();
}
```

### 5.3 客户端最佳实践

```typescript
// TypeScript SDK 使用示例

import { SuiClient, Transaction } from '@mysten/sui.js';

async function swapWithRetry(
    client: SuiClient,
    pool: FastShareObject,
    coinIn: OwnedObject,
    maxRetries: number = 3
) {
    let retries = 0;
    let currentVersion = await client.getFastShareVersion(pool.id);

    while (retries < maxRetries) {
        try {
            const tx = new Transaction();
            tx.setFastShareVersion(pool.id, currentVersion);
            tx.moveCall({
                target: 'dex::swap::swap',
                arguments: [tx.object(pool.id), tx.object(coinIn.id)],
            });

            const result = await client.signAndExecuteTransaction(tx);
            return result;

        } catch (error) {
            if (error instanceof VersionConflictError) {
                currentVersion = error.actualVersion;
                retries++;
                await sleep(50 * Math.pow(2, retries));  // 指数退避
            } else {
                throw error;
            }
        }
    }

    throw new Error('Max retries exceeded');
}
```

---

## 6. 性能分析

### 6.1 延迟对比

| 场景 | Fast Path | Consensus Path | Unified Path |
|------|-----------|----------------|--------------|
| Owned only | **~400ms** | N/A | N/A |
| Shared only | N/A | ~2-3s | N/A |
| FastShare only | N/A | N/A | **~400ms** |
| Owned + Shared | N/A | ~2-3s | N/A |
| Owned + FastShare | N/A | N/A | **~400ms** |

### 6.2 冲突影响

```
冲突率 vs 平均延迟

冲突率    平均延迟     说明
0%       400ms       理想场景
5%       420ms       轻微冲突
10%      450ms       可接受
20%      540ms       需要优化
50%      800ms+      不适合使用 FastShare
```

### 6.3 吞吐量

- 无冲突场景：与 Fast Path 相当
- 高冲突场景：因重试导致吞吐量下降

---

## 7. 局限性与权衡

### 7.1 局限性

1. **高冲突场景性能下降**
   - 频繁冲突导致大量重试
   - 热点对象不适合使用 FastShare

2. **客户端复杂度增加**
   - 需要处理版本冲突
   - 实现重试逻辑

3. **版本管理开销**
   - 额外的版本号存储和更新
   - 版本查询 API 调用

### 7.2 设计权衡

| 权衡点 | FastShare 选择 | 替代方案 |
|--------|---------------|---------|
| 一致性 vs 延迟 | 最终一致（冲突重试） | 强一致（共识排序） |
| 简单性 vs 灵活性 | 客户端处理冲突 | 服务端自动排序 |
| 通用性 vs 性能 | 针对低冲突优化 | 通用但延迟高 |

---

## 8. 与方案二的对比

参见 [BATCH_ORDERING_PATH.md](./BATCH_ORDERING_PATH.md)

| 维度 | FastShare + Unified Path | Batch Ordering |
|------|-------------------------|----------------|
| 延迟 | ~400ms | ~200-300ms |
| 与 Owned 混合 | **支持** | 不支持 |
| 冲突处理 | 客户端重试 | 批内排序 |
| 适用场景 | 低冲突共享状态 | 批量撮合 |
| 实现复杂度 | 中 | 低 |
