# DEX 执行层文档

> **目录**: `mynotes/dex/exec_layer/`  
> **目的**: 单节点 DEX 执行层的调研、架构设计和方案设计文档

---

## 文档概述

本目录包含三份核心文档，聚焦**一期单节点执行层**的实现，并包含**二期演进路径**（Sui DAG 或 ZK 共识）。

### 文档结构

| 文档 | 文件名 | 内容 | 状态 |
|-----|--------|------|------|
| **技术调研报告** | `01_research.md` | 对标 HyperLiquid，分析 Sui/Reth 可借鉴点，**重点分析 Object 模型和 FastPath 在第一阶段的使用性** | 待创建 |
| **架构设计文档** | `02_architecture.md` | 基于 PRD 的模块划分、技术栈、数据流设计，**包含 Object 模型集成设计** | 待创建 |
| **方案设计文档** | `03_solution.md` | 一期实现方案和二期演进路径，**包含 Object 模型实现方案和兼容性设计** | 待创建 |

---

## 核心问题

本系列文档特别关注以下关键问题：

1. **Object 模型和 FastPath 在第一阶段是否可以使用？**
2. **哪些模块可以使用 Object 模型？**
3. **如何使用可以兼容第二阶段的 ZK 或共识？**
4. **Object 模型与 Sui 共识的绑定关系分析**

---

## 文档依赖关系

```
01_research.md (技术调研)
    ↓
02_architecture.md (架构设计)
    ↓
03_solution.md (方案设计)
```

**说明**：
- 调研报告为架构设计提供技术选型依据
- 架构设计为方案设计提供设计框架
- 方案设计基于架构设计提供具体实现路径

---

## 参考文档

### 业务需求
- `prd/DEX完整业务需求.md` - 12 个模块的完整业务需求

### 现有分析
- `research/01-DEX执行层技术调研.md` - 现有技术调研
- `arch/sui_dex_arch.md` - Sui DEX 架构设计（作为二期参考）
- `tech/sui_dex_tech.md` - Sui DEX 技术方案（作为二期参考）

### Sui 机制分析
- `../sui/sui_object.md` - Sui Object 模型详解
- `../sui/sui_arch.md` - Sui FastPath 机制说明
- `result/use_sui_result.md` - Sui 使用结论

### 对标分析
- `analyst/liquidations/hyperliquid_comparison.md` - HyperLiquid 对比分析

---

## 关键设计原则

1. **一期聚焦单节点**：不涉及共识层，专注于执行层性能
2. **二期兼容设计**：所有设计考虑 Phase 1 到 Phase 2 的平滑演进
3. **Object 模型优先**：优先考虑使用 Object 模型，明确可用性边界
4. **复用 Sui 组件**：尽可能复用 Sui 基础设施（typed-store、事件系统等）
5. **性能目标**：延迟 < 50ms、吞吐量 200K TPS、撮合 < 10μs

---

## 使用指南

### 阅读顺序

1. **首次阅读**：按 `01_research.md` → `02_architecture.md` → `03_solution.md` 顺序阅读
2. **快速了解**：直接阅读 `03_solution.md` 的执行摘要
3. **深入技术**：重点阅读 `01_research.md` 的 Object 模型和 FastPath 分析章节

### 关键章节

**01_research.md**：
- 第 4 章：Object 模型和 FastPath 使用性分析（**重点**）

**02_architecture.md**：
- 第 5 章：Object 模型集成设计（**重点**）
- 第 6 章：存储层设计（Object 模型集成）

**03_solution.md**：
- 第 4 章：Object 模型和 FastPath 实现方案（**重点**）
- 第 5 章：二期演进路径

---

**最后更新**: 2026-01-08

