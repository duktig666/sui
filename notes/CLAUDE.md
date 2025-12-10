# CLAUDE.md - Notes Directory

本文件为 Claude Code 提供关于 `notes/` 目录的开发规范和指南。此目录专门用于研究和实验 Sui 共识层代码。

## 目录用途

`notes/` 目录用于：
1. **研究 Sui 共识层实现** - 基于 Mysticeti 协议的深入分析
2. **共识层原型开发** - 将 Sui 共识层作为共识框架进行二次开发
3. **AppChain 实验** - 使用 Sui 共识构建自定义应用链
4. **学习笔记和文档** - 记录研究过程和发现

## 项目结构

```
notes/
├── research/              # 研究笔记和分析文档
│   ├── consensus/         # 共识层深入研究
│   ├── architecture/      # 架构分析
│   └── performance/       # 性能分析
├── experiments/           # 实验性代码和原型
│   ├── consensus-poc/     # 共识层 PoC 实现
│   ├── appchain/          # AppChain 原型
│   └── benchmarks/        # 性能基准测试
└── docs/                  # 研究文档和教程
```

## Sui 共识层核心组件

### 关键 Crate

```
consensus/
├── consensus-config       # 共识配置管理
├── consensus-core         # Mysticeti 核心实现
├── consensus-types        # 共识相关类型定义
└── simtests              # 模拟测试框架
```

### 核心概念

1. **Mysticeti 协议**: Sui 使用的高性能 DAG-based 共识协议
   - 参考论文: https://arxiv.org/pdf/2310.14821
   - 实现位置: `consensus/core/src/`

2. **Authority**: 验证者节点，参与共识过程
   - 每个 Authority 维护本地状态
   - 通过消息传递达成共识

3. **Block & DAG**:
   - Blocks 组成有向无环图 (DAG)
   - 通过 causal ordering 确定交易顺序

4. **Commit Rule**:
   - 确定哪些 blocks 已达成共识
   - 基于投票和引用关系

## 开发规范

### 研究代码编写规范

1. **模块化设计**
   ```rust
   // 将共识层抽象为可复用的组件
   pub trait ConsensusProtocol {
       fn submit_transaction(&mut self, tx: Transaction) -> Result<()>;
       fn get_committed_blocks(&self) -> Vec<Block>;
   }
   ```

2. **依赖 Sui 共识 Crate**
   ```toml
   [dependencies]
   consensus-core = { path = "../../consensus/core" }
   consensus-config = { path = "../../consensus/config" }
   consensus-types = { path = "../../consensus/types" }
   sui-types = { path = "../../crates/sui-types" }
   ```

3. **实验性代码标注**
   ```rust
   // EXPERIMENTAL: 此代码用于研究共识层特性
   // TODO: 评估生产环境可行性
   ```

### 测试规范

```bash
# 运行共识层单元测试（跳过仿真测试）
SUI_SKIP_SIMTESTS=1 cargo nextest run -p consensus-core

# 运行共识层仿真测试
cargo simtest -p consensus-simtests

# 运行 notes 目录中的实验测试（快速迭代）
cargo nextest run --lib -p your-experiment-crate
```

### 性能分析

```bash
# 使用 criterion 进行性能基准测试
cargo bench -p your-benchmark-crate

# 使用 flamegraph 分析性能瓶颈
cargo flamegraph --bench your_benchmark
```

## 共识层二次开发指南

### 将 Sui 共识作为共识框架使用

1. **提取核心共识逻辑**
   - 识别可复用的共识组件
   - 解耦 Sui 特定的逻辑（对象模型、交易格式）

2. **定义自定义交易类型**
   ```rust
   // 定义 AppChain 特定的交易类型
   pub struct AppChainTransaction {
       // 自定义字段
   }

   // 实现到共识层的适配器
   impl Into<ConsensusTransaction> for AppChainTransaction {
       fn into(self) -> ConsensusTransaction {
           // 转换逻辑
       }
   }
   ```

3. **配置共识参数**
   ```rust
   let config = ConsensusConfig {
       committee_size: 4,
       wave_length: 3,
       leader_timeout: Duration::from_secs(2),
       // AppChain 特定配置
   };
   ```

4. **集成自定义执行层**
   ```rust
   pub trait ExecutionEngine {
       fn execute_block(&mut self, block: &CommittedBlock) -> Result<Effects>;
   }

   // 实现 AppChain 特定的执行逻辑
   impl ExecutionEngine for AppChainExecutor {
       // ...
   }
   ```

### 关键抽象层

```rust
// 共识层接口
pub struct ConsensusFramework<E: ExecutionEngine> {
    core: MysticetiCore,
    executor: E,
    config: ConsensusConfig,
}

impl<E: ExecutionEngine> ConsensusFramework<E> {
    pub fn new(config: ConsensusConfig, executor: E) -> Self {
        // 初始化共识核心
    }

    pub async fn submit(&mut self, tx: Vec<u8>) -> Result<TxHash> {
        // 提交交易到共识层
    }

    pub async fn get_committed(&self) -> Vec<CommittedBlock> {
        // 获取已提交的区块
    }
}
```

## 常见研究任务

### 分析共识性能

```bash
# 1. 运行共识仿真测试收集指标
cargo simtest -p consensus-simtests -- --nocapture

# 2. 使用自定义基准测试
cd notes/experiments/benchmarks
cargo run --release -- --scenario high-load

# 3. 分析延迟和吞吐量
cargo run --bin consensus-analyzer -- --data ./test-results
```

### 修改共识参数实验

```rust
// notes/experiments/consensus-poc/src/main.rs
use consensus_config::Parameters;

fn experiment_with_wave_length() {
    for wave_length in [2, 3, 4, 5] {
        let mut params = Parameters::default();
        params.wave_length = wave_length;

        let result = run_consensus_simulation(params);
        println!("Wave length {}: latency={:?}", wave_length, result.avg_latency);
    }
}
```

### 实现自定义 AppChain

```rust
// notes/experiments/appchain/src/lib.rs

// 1. 定义应用特定的状态机
pub struct MyAppState {
    // 状态字段
}

impl MyAppState {
    pub fn apply_transaction(&mut self, tx: &MyTransaction) -> Result<()> {
        // 状态转换逻辑
    }
}

// 2. 集成 Sui 共识
pub struct MyAppChain {
    consensus: ConsensusFramework<MyAppExecutor>,
    state: MyAppState,
}

// 3. 实现 AppChain 特定逻辑
impl MyAppChain {
    pub async fn submit_tx(&mut self, tx: MyTransaction) -> Result<()> {
        let consensus_tx = tx.encode();
        self.consensus.submit(consensus_tx).await?;
        Ok(())
    }
}
```

## 重要注意事项

### 代码质量要求

1. **实验代码也要保持高质量**
   - 即使是研究代码，也要通过 `cargo xclippy` 检查
   - 避免使用 `#[allow(dead_code)]` 等 linting 抑制
   - 编写测试验证实验假设

2. **文档化研究发现**
   - 在代码中添加详细注释解释实验目的
   - 记录性能数据和观察结果
   - 维护 `research/` 目录下的研究笔记

3. **版本控制**
   - 提交有意义的实验结果和发现
   - 使用描述性的 commit message
   - 不提交大型二进制文件或测试数据

### 依赖管理

```toml
# 使用本地路径依赖引用 Sui 共识层
[dependencies]
consensus-core = { path = "../../consensus/core" }

# 固定关键依赖版本避免破坏性更改
tokio = { version = "1.35", features = ["full"] }
```

### 性能考虑

- 使用 `--release` 模式进行性能测试
- 共识层测试建议超时设置为至少 10 分钟
- 使用 `-p` 标志减少编译时间，加快迭代

## 参考资源

### Sui 共识相关文档
- [Mysticeti 论文](https://arxiv.org/pdf/2310.14821)
- [Sui 共识 README](../consensus/README.md)
- [Sui Architecture 文档](../docs)

### 推荐阅读
- `consensus/core/src/core.rs` - Mysticeti 核心实现
- `consensus/core/src/dag_state.rs` - DAG 状态管理
- `consensus/core/src/commit.rs` - 提交规则实现
- `consensus/types/src/lib.rs` - 共识类型定义

### 相关 Crate
- `sui-types` - Sui 核心类型
- `sui-core` - Sui 核心逻辑
- `sui-node` - 验证者节点实现

## 开发流程示例

### 研究 Mysticeti 性能特性

```bash
# 1. 创建研究项目
cd notes/experiments
cargo new --lib consensus-performance-study

# 2. 添加依赖
cd consensus-performance-study
# 编辑 Cargo.toml 添加共识层依赖

# 3. 编写测试代码
# 编辑 src/lib.rs

# 4. 快速迭代测试
cargo nextest run --lib

# 5. 运行完整测试
SUI_SKIP_SIMTESTS=1 cargo nextest run

# 6. 性能分析
cargo bench

# 7. 代码检查
cargo xclippy
cargo fmt

# 8. 记录发现
# 更新 notes/research/consensus/findings.md
```

### 开发自定义 AppChain

```bash
# 1. 创建 AppChain 项目
cd notes/experiments
cargo new --bin my-appchain

# 2. 实现核心逻辑
# - 定义交易类型
# - 实现状态机
# - 集成共识层

# 3. 编写集成测试
mkdir tests
# 创建测试文件

# 4. 测试 AppChain
cargo test --release

# 5. 运行 AppChain 节点
cargo run --release -- --config config.yaml

# 6. 性能基准测试
cargo bench --bench appchain_throughput
```

## 故障排查

### 常见问题

1. **编译超时**: 使用 `-p` 选择特定包，设置更长的超时时间
2. **Simtest 失败**: 必须使用 `cargo simtest`，不能用 `cargo nextest`
3. **依赖冲突**: 确保使用与 Sui 主 repo 一致的依赖版本

### 调试技巧

```bash
# 启用详细日志
RUST_LOG=debug cargo test

# 使用 lldb/gdb 调试
rust-lldb target/debug/your-binary

# 分析 panic
RUST_BACKTRACE=full cargo test
```

## 最后检查清单

在提交研究代码前，确保：

- [ ] 所有测试通过（包括单元测试和集成测试）
- [ ] 运行 `cargo xclippy` 无警告
- [ ] 运行 `cargo fmt` 格式化代码
- [ ] 添加必要的文档和注释（解释非显而易见的逻辑）
- [ ] 更新研究笔记文档
- [ ] 清理临时文件和测试数据
- [ ] 验证依赖版本正确

## 贡献指南

虽然 `notes/` 目录主要用于个人研究，但有价值的发现可以：
1. 整理成文档贡献到 `docs/`
2. 提取通用组件贡献到主代码库
3. 在团队内分享研究成果

---

**记住**: 这是研究和实验环境，鼓励大胆尝试新想法，但始终保持代码质量和良好的工程实践。
