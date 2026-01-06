# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this directory.

## Crate-specific CLAUDE.md files
Always consult CLAUDE.md files in sub-crates. Instructions in local CLAUDE.md files override instructions
in this file when they are in conflict.

# Individual Preferences
Individual preferences supercede and extend project preferences:
- @CLAUDE.local.md

## Documentation and Research

### Notes Directory

The `notes/` directory contains technical analysis and research documentation about the Sui codebase:

- **Purpose**: Deep-dive analysis of original code implementation, architecture decisions, and performance characteristics
- **Content**: Markdown documents analyzing specific subsystems, comparing design alternatives, and documenting research findings
- **Audience**: Developers and researchers seeking to understand Sui's internal mechanisms
- **Style**: Follows the objectivity principles outlined in this file - distinguishing facts from analysis, technical limitations from design choices, and including code evidence

Examples of notes documentation:
- `SUI_TRANSACTION_VERIFICATION_MECHANISM.md` - Analysis of transaction verification architecture
- `SUI_CERTIFICATE_SEPARATION_ANALYSIS.md` - Design analysis of certificate separation
- `LLM_OBJECTIVITY_ANALYSIS_AND_SOLUTIONS.md` - Guidelines for objective technical analysis

When creating new analysis documents in `notes/`, follow the "Technical Analysis and Documentation Objectivity" guidelines below.

## Essential Development Commands

### Building and Installation

```bash
# Build a specific crate. Generally don't need to do release build.
cargo build -p sui-core

# Check code without building (preferred)
cargo check
```

### Testing

```bash
# Run e2e tests. simtests must be run with `cargo simtest` to avoid false negatives
cargo simtest -p sui-e2e-tests

# Run Rust unittests. skip simulation tests as they may cause false negatives with `cargo nextest`
SUI_SKIP_SIMTESTS=1 cargo nextest run
```

**Important Notes for Testing:**
- When compiling or running tests in this repository, set timeout limits to at least 10 minutes due to the large codebase size
- For faster iteration, use -p to select only the most relevant packages for testing. Use multiple `-p` flags if necessary, e.g. `cargo nextest run -p sui-types -p sui-core`
- Use `cargo nextest --lib` to run only library tests and skip integration tests for faster feedback
- Consult crate-specific CLAUDE.md files for instructions on which tests to run, when changing files in those crates

### Linting and Formatting

```bash
# Formats & lints all Rust & Move, run before commit:
./scripts/lint.sh

# Alternatively, run individual lints:
cargo fmt --all -- --check
cargo xclippy
```

`cargo xclippy does not recognize -p option` - This is a known issue with some clippy command variations

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

3. **Transaction Flow**:
   - Client → Transaction Driver → Authority Client → Validator
   - Transactions affecting only owned objects can start execution before consensus
   - Shared object transactions require consensus ordering before execution

4. **Storage Layer**: 
   - Uses RocksDB for persistent storage
   - Separate stores for objects, transactions, and effects
   - Checkpointing system for state synchronization

5. **Execution Pipeline**:
   - Transaction validation → Certificate creation → Execution → Effects commitment
   - Move VM executes smart contracts with gas metering
   - Parallel execution for non-conflicting transactions

### Critical Development Notes
1. **Testing Requirements**:
   - Always run tests before submitting changes
   - Framework changes require snapshot updates
2. **CRITICAL - Final Development Steps**:
   - **ALWAYS run `cargo xclippy` after finishing development** to ensure code passes all linting checks
   - **NEVER disable or ignore tests** - all tests must pass and be enabled
   - **NEVER use `#[allow(dead_code)]`, `#[allow(unused)]`, or any other linting suppressions** - fix the underlying issues instead
   - **All unit tests must work properly** - use `#[tokio::test]` for async tests, not `#[test]`

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

---

## Technical Analysis and Documentation Objectivity

When analyzing code, writing technical documentation, or explaining design decisions, follow these rules to ensure objectivity and avoid common biases.

### Core Principles

#### 1. Distinguish "What Is" from "Why"
- ✅ First state facts (what the code does), then analyze reasons (why it does that)
- ❌ Never mix explanation into fact statements
- Example:
  - ✅ "The code performs two network round trips (authority.rs:1150). This is a design choice to enable fork detection."
  - ❌ "The code performs two network round trips to ensure safety."

#### 2. Distinguish "Technical Limitations" from "Design Choices"
- **Technical Limitation**: Physically or logically impossible to implement differently
- **Design Choice**: Could be implemented differently but deliberately chosen not to
- ✅ Always explicitly label which category each conclusion falls into
- ❌ Never describe a "design choice" as a "technical limitation"
- Example:
  - ✅ "Shared objects require consensus ordering before execution (technical limitation: different orders yield different results)"
  - ✅ "Owned objects use two-phase protocol (design choice: could merge phases but Sui prioritizes fork detection)"
  - ❌ "Two-phase protocol is necessary for safety" (without specifying which type of necessity)

#### 3. Actively Seek Alternative Approaches
- ✅ For any design, always ask: "Could this be implemented differently?"
- ✅ List at least one alternative approach, even if not currently used
- ✅ Explain why alternatives were not chosen (with code evidence)
- ❌ Never assume "current implementation = only possible implementation"
- Example:
  - ✅ "Current: two phases. Alternative: validators could execute and return (signature, effects) in one round trip. Sui chose two phases to avoid silent state corruption (effects_certifier.rs:555 shows ForkedExecution is non-retriable)."
  - ❌ "The system must use two phases."

#### 4. Acknowledge Tradeoffs, Not "Optimality"
- ✅ State: "Approach A has cost X and benefit Y; Approach B has cost M and benefit N"
- ❌ Never say: "Approach A is better/optimal/superior"
- ✅ Present tradeoffs neutrally, let readers decide
- Example:
  - ✅ "Two-phase design: Cost = two network round trips, Benefit = detectable state inconsistency. One-phase alternative: Cost = potential silent corruption, Benefit = one round trip."
  - ❌ "Two-phase design is better because it's safer."

#### 5. All Conclusions Must Have Code Evidence
- ✅ Cite specific file names and line numbers: `file.rs:123-145`
- ✅ If inferring, explicitly label as "inference" or "likely"
- ❌ Never use "obviously", "necessarily", "clearly" to hide lack of evidence
- Example:
  - ✅ "ForkedExecution is non-retriable (transaction_driver/error.rs:93-97)"
  - ❌ "The system obviously can't handle forks automatically"

#### 6. Question Implicit Assumptions
- ✅ If asked "Why X?", first verify whether "X is true"
- ✅ If the question contains implicit assumptions, state them explicitly
- ❌ Never directly accept question premises without verification
- Example:
  - User: "Why can't CertifiedTransaction and CertifiedEffects be merged?"
  - ✅ "First, let's verify: Can they be merged? For Owned Objects: yes, theoretically. For Shared Objects: no, due to ordering requirements."
  - ❌ "They can't be merged because..." (accepts the premise)

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

**Reference**: For detailed analysis of LLM objectivity challenges and solutions, see `notes/LLM_OBJECTIVITY_ANALYSIS_AND_SOLUTIONS.md`
