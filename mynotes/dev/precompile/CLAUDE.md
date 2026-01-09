# CLAUDE.md

本文件为 Claude Code 提供项目指导。

## 语言规则
- 代码/变量名用英文,文档/复杂注释用中文,Claude 回答用中文

## 项目定位
**Sui + DEX 研究项目**: Rust/Sui/Move VM, 参考 Hyperliquid

## 目录结构
```
notes/       # 团队文档 (权威)
mynotes/     # 个人分析/设计/计划
protocol/    # 代码实现
```

## 工作流程
```
需求 → /analyze → /design → /implement → 测试
```

## 关键文档
- DEX L1: `notes/dex_l1/DEX_L1_DESIGN_SUMMARY.md`
- PRD: `mynotes/dex/prd/README.md`
- Sui: `notes/SUI_ARCHITECTURE_REPORT.md`

## 核心命令
```bash
# 构建
cargo check

# 测试
cargo simtest -p sui-e2e-tests
SUI_SKIP_SIMTESTS=1 cargo nextest run -p <crate>

# 质量检查
cargo fmt --all && cargo xclippy
```

## 架构要点
- `crates/sui-core/` - 核心
- `sui-execution/` - 执行层 (必须通过此访问)
- Authority 代码需通过 `sui-execution`
- 禁用 `#[allow(dead_code)]`

## 注释规则
**需要**: 非显而易见逻辑、阈值原因、复杂计算
**不需要**: 简单赋值、标准库调用
