# CLAUDE.md

本文件为 Claude Code (claude.ai/code) 在处理本仓库代码时提供指导。

## 语言与文档规则 / Language and Documentation Rules

**代码规则**:
- ✅ **代码使用英文**: 变量名、函数名、类型名、注释等所有代码元素必须使用英文
- ✅ **文档使用中文**: 所有分析报告、设计文档、README、CLAUDE.md 等文档使用中文撰写
- ✅ **注释使用中文**: 业务逻辑注释、复杂算法解释等使用中文,便于团队理解
- ✅ **回答使用中文**: Claude Code 的回答、解释、分析等使用中文

**示例**:
```rust
// 订单撮合引擎 - 价格时间优先算法
pub struct MatchingEngine {
    // 买单队列 - 按价格从高到低排序
    bid_orders: BTreeMap<Price, OrderQueue>,
    // 卖单队列 - 按价格从低到高排序
    ask_orders: BTreeMap<Price, OrderQueue>,
}
```

## 项目定位与研究导向 / Project Positioning

### 核心定位
本项目是 **Sui 与 DEX 结合的研究性项目**,主要目标:

1. **深入研究 Sui 架构**: 理解 Sui 的共识机制、交易执行、对象模型等核心设计
2. **DEX 设计调研**: 研究去中心化交易所的架构模式,特别关注高性能订单簿设计
3. **技术方案探索**: 探索如何借助 Sui 实现媲美 Hyperliquid 的 DEX 系统
4. **原型验证**: 通过代码原型验证技术方案的可行性

### 技术栈
- **主要语言**: Rust
- **区块链平台**: Sui (Move VM)
- **目标系统**: 高性能去中心化交易所 (DEX)
- **参考对标**: Hyperliquid、dYdX v4

### 文档结构
```
sui/
├── notes/              # 团队优质调研文档 (权威参考)
│   ├── dex_l1/         # DEX L1 设计文档
│   ├── research/       # 共识、DeepBook 等研究
│   └── docs/           # 架构演进文档
├── mynotes/            # 个人思考和梳理
│   ├── dex/            # DEX 相关分析
│   │   ├── prd/        # 产品需求文档
│   │   ├── data_structure/  # 数据结构设计
│   │   └── business/   # 业务逻辑分析
│   ├── analysis/       # analyst 角色分析结果
│   ├── design/         # architect 角色设计输出
│   └── plan/           # 项目计划与思考
└── protocol/           # 实际代码实现
```

## Crate-specific CLAUDE.md files
Always consult CLAUDE.md files in sub-crates. Instructions in local CLAUDE.md files override instructions
in this file when they are in conflict.

## Individual Preferences
Individual preferences supersede and extend project preferences. If a `CLAUDE.local.md` file exists in the repository root, follow those instructions in addition to this file.

## 研究工作流程 / Research Workflow

### 分析与调研流程
当进行技术调研和分析时,推荐使用以下三个 AI 角色 (定义在 `.claude/agents/`):

1. **analyst** - 业务分析师
   - 专注: DEX 业务分析、高频交易系统、区块链和 Sui 调研
   - 输出: 需求分析、业务流程、性能指标分析
   - 使用时机: 需要理解业务逻辑、分析系统行为、性能评估时

2. **architect** - 系统架构师
   - 专注: 系统架构设计、模块边界、可扩展性方案
   - 输出: 架构设计文档、技术方案选型、重构计划
   - 使用时机: 需要设计系统架构、评估技术方案、规划重构时

3. **engineer** - 开发工程师
   - 专注: Rust/Move 代码实现、DEX 功能开发
   - 输出: 代码实现、Bug 修复、功能实现
   - 使用时机: 需要编写代码、修复问题、实现功能时

### 推荐工作流
```
需求 → analyst 分析 → architect 设计 → engineer 实现 → 测试验证
         ↓               ↓               ↓
    mynotes/analysis  mynotes/design   protocol/
```

## DEX 领域知识 / DEX Domain Knowledge

### 核心概念
- **订单簿 (Order Book)**: 中央限价订单簿 (CLOB),价格-时间优先匹配
- **永续合约 (Perpetuals)**: 无到期日的合约交易,通过资金费率锚定现货价格
- **资金费率 (Funding Rate)**: 多空之间定期支付的费用,用于平衡合约价格与现货价格
- **保证金 (Margin)**: 初始保证金 (IMR)、维持保证金 (MMR)、跨仓/逐仓模式
- **清算 (Liquidation)**: 保证金不足时强制平仓机制
- **MEV 保护**: 防止矿工可提取价值攻击的机制
- **自动做市 (AMM vs Vault)**: Megavault 为订单簿提供流动性

### 关键文档参考
- **DEX L1 设计**: `notes/dex_l1/DEX_L1_DESIGN_SUMMARY.md` - 完整的 DEX L1 架构设计
- **PRD 文档**: `mynotes/dex/prd/README.md` - 7 个核心模块的产品需求
- **数据结构**: `mynotes/dex/data_structure/README.md` - 完整的数据结构文档
- **Sui 架构**: `notes/SUI_ARCHITECTURE_REPORT.md` - Sui 架构分析报告

### 性能目标
根据 `notes/dex_l1/DEX_L1_DESIGN_SUMMARY.md`,目标性能指标:
- 端到端延迟 (P99): < 50ms
- 撮合吞吐量 (TPS): ≥ 200,000
- 单次撮合耗时: < 10μs
- 软确认延迟: < 50ms
- 硬确认延迟: < 100ms

## Essential Development Commands

### Building and Installation

```bash
# Build a specific crate (generally don't need release build for development)
cargo build -p sui-core

# Check code without building (faster, preferred for iteration)
cargo check

# Build with specific profile
cargo build --profile simulator  # for simulation tests
```

### Testing

```bash
# Run simulation tests (MUST use cargo simtest to avoid false negatives)
cargo simtest -p sui-e2e-tests

# Run Rust unit tests (skip simulation tests to avoid false negatives with nextest)
SUI_SKIP_SIMTESTS=1 cargo nextest run

# Run tests for specific packages (faster iteration)
SUI_SKIP_SIMTESTS=1 cargo nextest run -p sui-types -p sui-core

# Run only library tests (skip integration tests for faster feedback)
SUI_SKIP_SIMTESTS=1 cargo nextest run --lib

# Run a single test by name
SUI_SKIP_SIMTESTS=1 cargo nextest run test_name
```

**Critical Testing Notes:**
- **Simulation tests** must use `cargo simtest`, NOT `cargo test` or `cargo nextest` - they will produce false negatives otherwise
- Set timeout limits to **at least 10 minutes** when compiling or running tests due to large codebase size
- Use `-p` flag to select specific packages for faster iteration
- Consult crate-specific CLAUDE.md files for which tests to run when changing files in those crates
- When changing Move framework code, see "Framework Changes" section below

### Move Framework Changes

When modifying Move code in `crates/sui-framework/packages/`:

```bash
# Update framework snapshots after Move code changes (from repository root)
./scripts/update_all_snapshots.sh

# Rebuild framework and update documentation (from crates/sui-framework/)
UPDATE=1 cargo nextest run build_system_packages
```

**Important**: Framework snapshot updates require `cargo-insta`. Install with: `cargo install cargo-insta`

### Linting and Formatting

```bash
# Format and lint all Rust & Move code (run before commit)
./scripts/lint.sh

# Individual linting commands:
cargo fmt --all                    # Format Rust code
cargo xclippy                      # Run clippy with project lints
cargo xclippy -D warnings          # Treat warnings as errors
cargo xlint                        # Run additional project-specific lints
```

**Known Issues:**
- `cargo xclippy` does not recognize the `-p` option for package selection
- `cargo xlint` runs custom lints defined in the `crates/x` package

## High-Level Architecture

### Core Components Structure

```
sui/
├── crates/                   # Main Rust crates
│   ├── sui-core/             # Core blockchain logic
│   ├── sui-node/             # Validator node implementation
│   ├── sui-framework/        # Move system packages & stdlib
│   ├── sui-types/            # Core type definitions
│   ├── sui-json-rpc/         # JSON-RPC API server
│   ├── sui-graphql-rpc/      # GraphQL API server
│   └── sui-indexer-alt/      # Blockchain data indexer
├── consensus/                # Consensus mechanism (Mysticeti)
├── sui-execution/            # Move execution layer with versions (v0, v1, v2 and latest)
├── apps/                     # Frontend applications
└── external-crates/          # Move compiler and VM
```

### Key Architectural Patterns

1. **Authority System**: Sui uses a set of validators (authorities) that process transactions in parallel. Each authority maintains its own state and participates in Byzantine consensus.

2. **Object Model**: Unlike account-based blockchains, Sui uses an object-centric model where:
   - Each object has a unique ID and version
   - Objects can be owned, shared, or immutable
   - Object ownership enables parallel transaction execution

3. **Transaction Flow**:
   - Client → Transaction Driver → Authority Client → Validator
   - Transactions affecting only owned objects can start execution before consensus
   - Shared object transactions require consensus ordering before execution

4. **Consensus Layer** (`consensus/`):
   - Implements the Mysticeti consensus protocol
   - Byzantine fault tolerant with ~400ms latency
   - Subdirectories: `config/`, `core/`, `types/`, `simtests/`

5. **Execution Layer** (`sui-execution/`):
   - **Critical**: All authority code MUST access execution via `sui-execution` crate, NOT directly
   - Contains versioned execution environments: `v0/`, `v1/`, `v2/`, `latest/`
   - Versions are protocol-gated to prevent forks during state sync
   - Includes: Move VM, adapter (integrates Move into Sui), metered verifier
   - Move-specific crates in `external-crates/move/`

6. **Storage Layer**:
   - Uses RocksDB for persistent storage
   - Separate stores for objects, transactions, and effects
   - Checkpointing system for state synchronization
   - `sui-storage` crate handles persistent state

7. **Execution Pipeline**:
   - Transaction validation → Certificate creation → Execution → Effects commitment
   - Move VM executes smart contracts with gas metering
   - Parallel execution for non-conflicting transactions
   - Gas accounting happens during execution

### Important Crate Relationships

**Core Blockchain Crates:**
- `sui-core` - Core blockchain logic, transaction orchestration
- `sui-node` - Validator and fullnode implementation
- `sui-types` - Fundamental type definitions used across the codebase
- `sui-storage` - Persistent storage abstractions and implementations
- `sui-config` - Configuration management for nodes and validators

**API & RPC Crates:**
- `sui-json-rpc` - JSON-RPC API server (legacy)
- `sui-graphql-rpc` - GraphQL API server (preferred)
- `sui-rpc-api` - Common RPC API traits and types

**Indexer Crates:**
- `sui-indexer` - Legacy indexer
- `sui-indexer-alt-*` - New indexer architecture with modular design

**Framework & Move:**
- `sui-framework` - Move system packages and stdlib
- `sui-move-build` - Move package compilation
- `sui-adapter-transactional-tests` - Transactional test framework for Move

**Testing & Development:**
- `sui-test-validator` - Local test validator
- `sui-e2e-tests` - End-to-end integration tests
- `sui-simulator` - Deterministic simulation testing
- `test-cluster` - Test cluster management

### Critical Development Notes

**Testing Requirements:**
- Always run tests before submitting changes
- Framework changes require snapshot updates via `./scripts/update_all_snapshots.sh`
- Use simulation tests (`cargo simtest`) for concurrency-sensitive code
- For async tests, use `#[tokio::test]`, not `#[test]`

**CRITICAL - Final Development Steps:**
- **ALWAYS run `cargo xclippy` after finishing development** to ensure code passes all linting checks
- **NEVER disable or ignore tests** - all tests must pass and be enabled
- **NEVER use `#[allow(dead_code)]`, `#[allow(unused)]`, or similar linting suppressions** - fix the underlying issues instead
- Run `./scripts/lint.sh` before committing to format and lint all code

**Execution Layer Access:**
- Authority code (validators/fullnodes) MUST access execution via `sui-execution` crate
- Direct dependencies on execution version crates can cause forks
- CLI tools and other non-authority code can directly use `latest` version

### **Comment Writing Guidelines**

**Do NOT comment the obvious** - comments should not simply repeat what the code does.
**When to comment**:
- Non-obvious algorithms or business logic
- Temporary exclusions, timeouts, or thresholds and their reasoning  
- Complex calculations where the "why" isn't immediately clear
- Subtle race conditions or threading considerations
- Assumptions about external state or preconditions

**When NOT to comment**:
- Simple variable assignments
- Standard library usage
- Self-descriptive function calls
- Basic control flow (if/for/while)
