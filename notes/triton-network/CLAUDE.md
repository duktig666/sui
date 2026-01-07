# Triton Network 项目指导

> Triton — 海神之子，象征继承与变革

## 项目定位

基于 Sui 源码的二次开发研究项目，目标是创建一条针对特定场景优化的定制区块链网络。

**与 sui-devnet-local 的区别**：
- `sui-devnet-local`: 零源码修改，仅用于本地测试
- `triton-network`: 源码级修改研究，构建定制链

---

## 核心研究方向：Unified Path

### Sui 现有交易路径

| 路径 | 适用对象 | 机制 | 延迟 |
|------|---------|------|------|
| **Fast Path** | Owned Objects | 2f+1 签名，无共识 | ~400ms |
| **Consensus Path** | Shared Objects | Mysticeti 共识排序 | ~2-3s |

**现状问题**：
- Shared Objects（如 DEX 流动性池、订单簿）必须走 Consensus Path
- 共识排序引入额外延迟（~2-3s），不利于高频交易场景

### Triton 研究目标

```
Sui 现有:
┌─────────────────┐     ┌─────────────────┐
│  Owned Object   │     │  Shared Object  │
│   Fast Path     │     │ Consensus Path  │
│    ~400ms       │     │     ~2-3s       │
└─────────────────┘     └─────────────────┘
        ✗ 不可混合 ✗

Triton 扩展:
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│  Owned Object   │     │  Shared Object  │     │ FastShare Object│
│   Fast Path     │     │ Consensus Path  │     │  Unified Path   │
│    ~400ms       │     │     ~2-3s       │     │    ~400ms       │
└─────────────────┘     └─────────────────┘     └─────────────────┘
        │                                              │
        └──────────────── 可混合操作 ──────────────────┘
```

### 两个技术方案

#### 方案一：FastShare 对象 + Unified Path（推荐）
- **新增 FastShare 对象类型**：专为低延迟共享访问设计
- **Owned + FastShare 可混合**：走统一的 Unified Path
- **乐观并发 + 冲突重试**：无冲突时延迟 ~400ms
- **详细设计**：[FASTSHARE_UNIFIED_PATH.md](./FASTSHARE_UNIFIED_PATH.md)

#### 方案二：Deterministic Batch Ordering
- **针对现有 Shared Objects**：无需新对象类型
- **时间窗口批处理**：50-100ms 窗口收集交易
- **确定性排序**：所有验证者独立计算相同顺序
- **延迟 ~200-300ms**：比传统共识快 10 倍
- **详细设计**：[BATCH_ORDERING_PATH.md](./BATCH_ORDERING_PATH.md)

### 方案对比

| 维度 | 方案一：FastShare | 方案二：Batch Ordering |
|------|------------------|----------------------|
| 延迟 | ~400ms | ~200-300ms |
| 与 Owned 混合 | **支持** | 不支持 |
| 适用对象 | 新 FastShare 类型 | 现有 Shared Objects |
| 冲突处理 | 回滚重试 | 批内排序 |
| 实现复杂度 | 中 | 低 |

---

## 关键 Sui 模块参考

### 交易路径相关

| 模块 | 路径 | 说明 |
|------|------|------|
| 交易驱动 | `crates/sui-core/src/transaction_driver/` | 交易提交和执行入口 |
| 权威处理 | `crates/sui-core/src/authority.rs` | 验证者处理交易逻辑 |
| 效果签名 | `crates/sui-core/src/effects_certifier.rs` | CertifiedEffects 生成 |

### 共识相关

| 模块 | 路径 | 说明 |
|------|------|------|
| Mysticeti | `consensus/` | 共识算法实现 |
| 共识适配 | `crates/sui-core/src/consensus_adapter.rs` | 交易提交到共识 |
| 共识处理 | `crates/sui-core/src/consensus_handler.rs` | 共识输出处理 |

### 对象模型相关

| 模块 | 路径 | 说明 |
|------|------|------|
| 对象类型 | `crates/sui-types/src/object.rs` | Owned/Shared 对象定义 |
| 交易类型 | `crates/sui-types/src/transaction.rs` | 交易结构定义 |
| 输入对象 | `crates/sui-types/src/input_objects.rs` | 交易输入对象处理 |

---

## 文档规范

### 修改提案格式

每个源码修改研究应包含：

```markdown
## 修改提案: [标题]

### 1. 目标
- 要解决的问题
- 预期效果

### 2. 影响分析
- 涉及的模块
- 对现有功能的影响
- 兼容性考虑

### 3. 技术方案
- 修改点列表（文件:行号）
- 实现细节

### 4. 验证方案
- 测试用例
- 性能基准
```

### 研究笔记格式

遵循根目录 `CLAUDE.md` 中的 "Technical Analysis and Documentation Objectivity" 规范：
- 区分 "事实" 和 "分析"
- 区分 "技术限制" 和 "设计选择"
- 所有结论必须有代码证据

---

## 相关项目

- [`sui-devnet-local`](../sui-devnet-local/) - 本地开发测试网络（零源码修改）
- [`dex_l1`](../dex_l1/) - DEX L1 设计文档
