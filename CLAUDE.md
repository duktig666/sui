# CLAUDE.md

本文件为 Claude Code (claude.ai/code) 在处理本仓库代码时提供指导。

## 语言与文档规则 / Language and Documentation Rules

**代码规则**:
- ✅ **代码使用英文**: 变量名、函数名、类型名、注释等所有代码元素必须使用英文
- ✅ **文档使用中文**: 所有分析报告、设计文档、README、CLAUDE.md 等文档使用中文撰写
- ✅ **注释使用中文**: 业务逻辑注释、复杂算法解释等使用中文,便于团队理解
- ✅ **回答使用中文**: Claude Code 的回答、解释、分析等使用中文

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
- **参考对标**: Hyperliquid

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

## Crate-specific CLAUDE.md files
Always consult CLAUDE.md files in sub-crates. Instructions in local CLAUDE.md files override instructions
in this file when they are in conflict.

## Individual Preferences
Individual preferences supersede and extend project preferences. If a `CLAUDE.local.md` file exists in the repository root, follow those instructions in addition to this file.

## Documentation and Research

### Notes Directory

The `notes/` and `mynotes/` directory contains technical analysis and research documentation about the Sui codebase:

- **Purpose**: Deep-dive analysis of original code implementation, architecture decisions, and performance characteristics
- **Content**: Markdown documents analyzing specific subsystems, comparing design alternatives, and documenting research findings
- **Audience**: Developers and researchers seeking to understand Sui's internal mechanisms
- **Style**: Follows the objectivity principles outlined in this file - distinguishing facts from analysis, technical limitations from design choices, and including code evidence

Examples of notes documentation:
- `SUI_TRANSACTION_VERIFICATION_MECHANISM.md` - Analysis of transaction verification architecture
- `SUI_CERTIFICATE_SEPARATION_ANALYSIS.md` - Design analysis of certificate separation
- `LLM_OBJECTIVITY_ANALYSIS_AND_SOLUTIONS.md` - Guidelines for objective technical analysis

When creating new analysis documents in `notes/` and `mynotes/` , follow the "Technical Analysis and Documentation Objectivity" guidelines below.

## High-Level Architecture

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

### Mandatory Verification Checklist

Before completing any technical analysis, verify:

- [ ] Did I assume "current design = optimal design"?
- [ ] Did I consider alternative approaches?
- [ ] Did I distinguish "technical limitations" from "design choices"?
- [ ] Did I downplay any inconvenient evidence?
- [ ] Does each conclusion have code line number citations?
- [ ] Am I describing tradeoffs or claiming optimality?
- [ ] Did I question the assumptions in the original question?

### Prohibited Expressions

❌ **Never use** (they hide lack of evidence or make unjustified claims):
- "Obviously", "clearly", "naturally", "of course"
- "Optimal", "best", "superior"
- "Must", "necessary" (unless truly a technical limitation)
- "Only way", "impossible" (unless all alternatives exhausted)

### Encouraged Expressions

✅ **Use these** to maintain objectivity:
- "The code shows..." (fact statement)
- "Theoretically possible to... but Sui chose..." (distinguish possibility from choice)
- "This is a technical limitation because..." (explicit categorization)
- "This is a design choice; the tradeoff is..." (acknowledge alternatives)
- "According to code at X:L123,..." (evidence citation)
- "Inference: ..." or "Likely..." (mark speculation)

### Analysis Template for Technical Documentation

When writing technical analysis or documentation, use this structure:

```markdown
## 1. Facts (Pure Description)
- Current implementation: [describe without explaining]
- Code location: file.rs:lines
- Observable behavior: [what happens]

## 2. Possibility Analysis
- Alternative approach 1: [describe]
  - Feasibility: [can it work? why/why not?]
- Alternative approach 2: [describe]
  - Feasibility: [can it work? why/why not?]

## 3. Constraint Classification
**Technical Limitations** (cannot be changed):
- [constraint] | Reason | Code evidence

**Design Choices** (could be changed but chosen not to):
- [choice] | Alternative | Why current chosen | Code evidence

## 4. Tradeoff Analysis
**Current approach:**
- Cost: [specific costs]
- Benefit: [specific benefits]

**Alternative approach:**
- Cost: [specific costs]
- Benefit: [specific benefits]

(No conclusion about which is "better")

## 5. Evidence Index
| Claim | Supporting Evidence | Counter-evidence (if any) |
|-------|-------------------|--------------------------|
| ...   | file:line         | file:line                |
```

### Example: Biased vs Objective Analysis

**❌ Biased Analysis:**
```
Sui's two-phase design is necessary for safety. The first phase ensures
transaction validity and the second ensures correct execution. This is
optimal for Byzantine fault tolerance.
```
Problems:
- Uses "necessary" without distinguishing technical vs design necessity
- Uses "optimal" (value judgment)
- No code evidence
- No alternative approaches considered

**✅ Objective Analysis:**
```
## Facts
Sui implements two-phase finalization (authority.rs:1150-1217):
1. CertifiedTransaction: 2f+1 signatures on transaction
2. CertifiedEffects: 2f+1 signatures on execution results

## Possibility Analysis
For Owned Objects, one-phase alternative is theoretically possible:
- Validators execute immediately and return (signature, effects)
- Client collects 2f+1 identical effects
- One round trip instead of two

## Constraint Classification
**Design Choice** (for Owned Objects):
- Current: Two phases
- Alternative: One phase (feasible)
- Reason for current: Code shows ForkedExecution is non-retriable
  (effects_certifier.rs:555), no majority-override mechanism exists
- Design philosophy (inferred): Observable failure > silent corruption

**Technical Limitation** (for Shared Objects):
- Must await consensus ordering before execution
- Different orders yield different results (determinism issue)
- Cannot merge phases

## Tradeoff
Current two-phase:
- Cost: Two network round trips
- Benefit: State inconsistency immediately detectable

Alternative one-phase (for Owned Objects):
- Cost: Fork hidden by automatic majority override
- Benefit: One network round trip

Sui chose the former.
```

---

Reference: For detailed analysis of LLM objectivity challenges and solutions, see notes/LLM_OBJECTIVITY_ANALYSIS_AND_SOLUTIONS.md
