# Sui Mysticeti 共识研究与 AppChain 开发

## 📁 目录索引

| 路径 | 说明 |
|------|------|
| **项目计划与总结** | |
| [`ONE_WEEK_PLAN.md`](./ONE_WEEK_PLAN.md) | 一周研究计划 |
| [`DAY1_SUMMARY.md`](./DAY1_SUMMARY.md) | Day 1: 核心组件理解 |
| [`DAY3_SUMMARY.md`](./DAY3_SUMMARY.md) | Day 3: 共识框架抽象 |
| [`DAY45_SUMMARY.md`](./DAY45_SUMMARY.md) | Day 4-5: Token Chain 开发 |
| [`DAY6_SUMMARY.md`](./DAY6_SUMMARY.md) | Day 6: 测试与验证 |
| [`DAY7_SUMMARY.md`](./DAY7_SUMMARY.md) | Day 7: 文档编写 |
| [`PROJECT_STATUS.md`](./PROJECT_STATUS.md) | 项目状态总览 |
| [`NEXT_STEPS.md`](./NEXT_STEPS.md) | 下一步计划 |
| [`PERFORMANCE_BASELINE.md`](./PERFORMANCE_BASELINE.md) | 性能基准测试结果 |
| **开发规范** | |
| [`CLAUDE.md`](./CLAUDE.md) | Claude Code 开发指南 |
| **研究文档** | |
| [`research/consensus/`](./research/consensus/) | 共识层深度研究 |
| ├─ [`core-components-analysis.md`](./research/consensus/core-components-analysis.md) | 核心组件分析 |
| ├─ [`transaction-execution-flow-analysis.md`](./research/consensus/transaction-execution-flow-analysis.md) | 交易执行流程 |
| └─ [`transaction-timing-diagrams.md`](./research/consensus/transaction-timing-diagrams.md) | 交易时序详解 |
| **实验代码** | |
| [`experiments/consensus-framework/`](./experiments/consensus-framework/) | 共识框架抽象层 |
| ├─ [`src/traits.rs`](./experiments/consensus-framework/src/traits.rs) | 核心 Trait 定义 |
| ├─ [`src/mysticeti_adapter.rs`](./experiments/consensus-framework/src/mysticeti_adapter.rs) | Mysticeti 适配器 |
| └─ [`tests/`](./experiments/consensus-framework/tests/) | 集成测试 |
| [`experiments/simple-token-chain/`](./experiments/simple-token-chain/) | Token Chain 区块链 |
| ├─ [`src/types.rs`](./experiments/simple-token-chain/src/types.rs) | 类型定义 |
| ├─ [`src/executor.rs`](./experiments/simple-token-chain/src/executor.rs) | 执行引擎 |
| ├─ [`src/node.rs`](./experiments/simple-token-chain/src/node.rs) | 节点实现 |
| ├─ [`src/rpc.rs`](./experiments/simple-token-chain/src/rpc.rs) | JSON-RPC 服务 |
| ├─ [`tests/integration_tests.rs`](./experiments/simple-token-chain/tests/integration_tests.rs) | 集成测试 (9) |
| ├─ [`benches/throughput.rs`](./experiments/simple-token-chain/benches/throughput.rs) | 性能基准 (6) |
| └─ [`examples/client.rs`](./experiments/simple-token-chain/examples/client.rs) | 客户端示例 |
| [`experiments/dag-visualizer/`](./experiments/dag-visualizer/) | DAG 可视化工具 |
| [`experiments/consensus-poc/`](./experiments/consensus-poc/) | 共识层 PoC 研究 |
| **文档中心** | |
| [`docs/getting-started.md`](./docs/getting-started.md) | 快速开始指南 |
| [`docs/api-reference.md`](./docs/api-reference.md) | API 参考文档 |
| [`docs/architecture.md`](./docs/architecture.md) | 系统架构设计 |
| [`docs/research-summary.md`](./docs/research-summary.md) | 研究总结报告 |

## 🚀 快速开始

```bash
# 查看项目规划
cat ONE_WEEK_PLAN.md

# 运行 Token Chain
cd experiments/simple-token-chain
cargo run --release

# 运行测试
cargo test --release
cargo bench

# 查看研究文档
cat research/consensus/transaction-timing-diagrams.md
```

## 📊 项目统计

| 指标 | 数值 |
|------|------|
| **代码量** | ~2,400 行生产代码 |
| **测试** | 21 单元测试 + 9 集成测试 |
| **基准测试** | 6 个性能 benchmark |
| **文档** | ~5,500 行 Markdown |
| **Crates** | consensus-framework, simple-token-chain |
| **性能** | 1.1M transfers/sec (内存), 4.2M tx/sec (提交) |

## 🎯 核心成果

- ✅ Mysticeti 共识机制深度理解
- ✅ 通用共识框架抽象（3 核心 Trait）
- ✅ 完整 Token Chain 实现
- ✅ 全面测试覆盖（100%）
- ✅ 性能基准建立
- ✅ 完整文档体系

## 📖 推荐阅读顺序

1. [`ONE_WEEK_PLAN.md`](./ONE_WEEK_PLAN.md) - 了解项目背景
2. [`research/consensus/transaction-timing-diagrams.md`](./research/consensus/transaction-timing-diagrams.md) - 理解 Sui 交易流程
3. [`docs/getting-started.md`](./docs/getting-started.md) - 运行 Token Chain
4. [`docs/architecture.md`](./docs/architecture.md) - 深入系统设计
5. [`NEXT_STEPS.md`](./NEXT_STEPS.md) - 后续开发计划
