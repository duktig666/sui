# Sui 架构文档导航

> **版本**: 1.64.0 | **更新日期**: 2026-01-13

本文档集全面梳理 Sui 项目的 Rust 模块架构,包含 134 个 crates 的职责、依赖关系和调用流程。

---

## 📚 快速开始

### 我想要...

- **10分钟理解 Sui 整体设计** → [架构概览](00-ARCHITECTURE-OVERVIEW.md)
- **深入了解核心模块** → [关键模块详解](02-KEY-MODULES.md)
- **查看特定模块信息** → [模块完整索引](04-MODULE-INDEX.md)
- **理解交易处理流程** → [交易流程分析](03-TRANSACTION-FLOWS.md)
- **实现 DEX 应用** → [DEX 实现专项](05-DEX-IMPLEMENTATION.md)

---

## 📖 文档列表

| 文档 | 内容概要 | 预计阅读时间 | 适合人群 |
|-----|---------|------------|---------|
| [架构概览](00-ARCHITECTURE-OVERVIEW.md) | Sui 核心设计理念、4层架构、性能指标、与其他链对比 | 10分钟 | 架构师、新人、决策者 |
| [层级架构](01-LAYER-ARCHITECTURE.md) | 基础设施层、协议层、服务层、应用层的详细职责与交互 | 30分钟 | 系统设计者、架构师 |
| [关键模块](02-KEY-MODULES.md) | 20-30个核心模块的深度解析,包含代码路径和依赖关系 | 1-2小时 | 开发者、代码贡献者 |
| [交易流程](03-TRANSACTION-FLOWS.md) | 拥有对象交易、共享对象交易、共识、查询等关键流程 | 30分钟 | 开发者、性能优化 |
| [模块索引](04-MODULE-INDEX.md) | 全部 134 个 crates 的表格索引,按层级和字母排序 | 按需查询 | 开发者、调试者 |
| [DEX实现](05-DEX-IMPLEMENTATION.md) | DEX 所需模块、最小节点配置、抽离方案对比 | 45分钟 | DEX开发者、运维 |

---

## 🎨 图表速查

| 图表 | 类型 | 内容 |
|-----|------|------|
| [整体架构图](diagrams/00-overall-architecture.mmd) | 流程图 | 4层架构和数据流向 |
| [核心模块依赖图](diagrams/02-core-modules.mmd) | 依赖图 | 20-30个核心模块的依赖关系 |
| [拥有对象交易流程](diagrams/03-tx-flow-owned.mmd) | 时序图 | FastPath 交易的完整调用链 |
| [共享对象交易流程](diagrams/04-tx-flow-shared.mmd) | 时序图 | 共识路径交易的处理流程 |
| [Mysticeti 共识流程](diagrams/05-consensus-flow.mmd) | 流程图 | DAG 共识的决策过程 |
| [数据查询流程](diagrams/06-query-flow.mmd) | 时序图 | RPC 查询的缓存和存储访问 |
| [DEX 依赖关系](diagrams/07-dex-dependencies.mmd) | 依赖图 | DEX 实现的完整模块依赖 |

> **提示**: 可将 `.mmd` 文件内容复制到 [Mermaid Live Editor](https://mermaid.live/) 进行交互式查看

---

## 🗺️ 学习路径推荐

### 路径 1: 新人快速上手 (1小时)

```
1. 架构概览 (10分钟)
   └─> 理解 Sui 的核心设计理念

2. 整体架构图 (5分钟)
   └─> 可视化 4 层架构

3. 层级架构 - 核心协议层 (15分钟)
   └─> 深入 consensus, sui-core, sui-execution

4. 拥有对象交易流程图 (10分钟)
   └─> 理解 FastPath 机制

5. 关键模块 - 浏览 5-10 个核心模块 (20分钟)
   └─> 了解模块职责和代码位置
```

### 路径 2: 开发者深度学习 (3-4小时)

```
1. 架构概览 → 2. 层级架构(全文) → 3. 关键模块(全文)
   → 4. 交易流程(全文) → 5. 模块索引(查询需要的模块)
```

### 路径 3: DEX 开发专项 (2小时)

```
1. 架构概览 (10分钟)
   └─> 理解 Sui 基础

2. DEX 实现专项 (45分钟)
   └─> 了解 DEX 所需模块和节点配置

3. 关键模块 - 执行层和存储层 (30分钟)
   └─> sui-execution, sui-storage, consensus-core

4. 共享对象交易流程 (15分钟)
   └─> 理解订单簿(共享对象)的处理

5. DEX 依赖关系图 (5分钟)
   └─> 可视化完整依赖

6. 模块索引 - 查询 DeepBook 相关模块 (15分钟)
```

---

## 🎯 核心概念速览

### Sui 的 4 层架构

```
┌─────────────────────────────────────┐
│  应用层 (Application Layer)         │  sui-sdk, Move合约, 前端应用
├─────────────────────────────────────┤
│  服务层 (Service Layer)             │  sui-node, JSON-RPC, GraphQL, 索引器
├─────────────────────────────────────┤
│  核心协议层 (Protocol Layer)        │  共识, 执行, 存储, sui-core
├─────────────────────────────────────┤
│  基础设施层 (Infrastructure Layer)  │  types, network, storage, crypto
└─────────────────────────────────────┘
```

### 模块统计

- **总计**: 134 个 Rust crates
- **核心模块**: 20-30 个 (经常使用)
- **DEX 最小集**: 33-40 个 crates
- **可删减**: 约 70% (针对特定场景)

### 关键设计特点

- ✅ **对象中心模型**: 支持对象级并行执行
- ✅ **FastPath 机制**: 拥有对象交易跳过共识 (~200ms)
- ✅ **Mysticeti 共识**: DAG-based BFT,3轮消息 (~400ms)
- ✅ **版本化执行层**: 防止协议升级分叉
- ✅ **分片缓存**: 64 个 LRU 分片减少锁竞争

---

## 📝 文档维护

### 如何贡献

1. 发现模块信息过时 → 更新对应文档
2. 新增重要模块 → 添加到 `02-KEY-MODULES.md` 和 `04-MODULE-INDEX.md`
3. 架构发生变化 → 更新图表和概览文档

### 版本记录

| 版本 | 日期 | 主要变更 |
|-----|------|---------|
| 1.0 | 2026-01-13 | 初始版本,覆盖 Sui v1.64.0 的 134 个 crates |

---

## 🔗 相关资源

### 官方文档

- [Sui 官方文档](https://docs.sui.io/)
- [Move 语言书](https://move-book.com/)
- [Sui GitHub](https://github.com/MystenLabs/sui)

### 项目内文档

- `CLAUDE.md` - 项目规范和技术分析指南
- `notes/` - 团队优质调研文档
- `mynotes/dex/` - DEX 相关分析文档

---

## ❓ 常见问题

**Q: 文档中的代码路径是绝对路径吗?**
A: 是的,所有路径形如 `/Users/renshiwei/code/company/DEX/sui/crates/...`,便于直接定位。

**Q: Mermaid 图表无法渲染怎么办?**
A: 复制 `.mmd` 文件内容到 [Mermaid Live Editor](https://mermaid.live/) 查看。

**Q: 如何快速查找某个 crate 的职责?**
A: 使用 [模块索引](04-MODULE-INDEX.md) 的表格搜索功能(Ctrl+F)。

**Q: DEX 开发应该从哪里开始?**
A: 直接阅读 [DEX 实现专项](05-DEX-IMPLEMENTATION.md),包含完整的模块清单和实施路径。

---

**开始探索**: [架构概览 →](00-ARCHITECTURE-OVERVIEW.md)
