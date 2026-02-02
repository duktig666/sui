# 多 Claude Code 并行协作最佳实践

## 问题场景

多个 Claude Code 实例同时编写同一个 git 仓库时会遇到：

| 问题类型 | 症状 | 原因 |
|---------|------|------|
| Git 冲突 | `CONFLICT (content): Merge conflict in...` | 多实例编辑同一文件 |
| 编译问题 | `Blocking waiting for file lock on package cache` | 共享 target/ 目录和 Cargo.lock |
| 工作目录污染 | 未提交的修改相互干扰 | 共享工作目录 |
| 测试干扰 | 测试结果不稳定 | 共享测试数据库/临时文件 |

## 解决方案：Git Worktree + 任务隔离

### 核心原则

```
┌─────────────────────────────────────────────────────────────────┐
│                       主仓库 (main)                              │
│  ├── .worktrees/           # 隔离的工作目录                      │
│  │   ├── feature-a/        # Claude #1 的独立空间                │
│  │   ├── feature-b/        # Claude #2 的独立空间                │
│  │   └── feature-c/        # Claude #3 的独立空间                │
│  └── .gitignore            # 必须包含 .worktrees/               │
└─────────────────────────────────────────────────────────────────┘
```

**每个 Claude Code 实例 = 独立的 worktree + 独立的分支 + 独立的任务域**

### 方案一：使用 Superpowers Skill（推荐）

Superpowers 提供了 `using-git-worktrees` skill，自动化创建和管理隔离工作空间。

#### 1. 创建 Worktree

在 Claude Code 中调用：
```
/superpowers:using-git-worktrees
```

Skill 会自动：
1. 检查是否存在 `.worktrees/` 或 `worktrees/` 目录（优先使用 `.worktrees/`）
2. 验证该目录已被 `.gitignore` 忽略（未忽略则自动添加并提交）
3. **自动创建新分支和 worktree**（使用 `git worktree add "$path" -b "$BRANCH_NAME"`）
4. 自动检测项目类型并运行依赖安装：
   - Node.js: `npm install`
   - Rust: `cargo build`
   - Python: `pip install -r requirements.txt` 或 `poetry install`
   - Go: `go mod download`
5. 执行基线测试验证，确保 worktree 起点干净

#### Superpowers 分支创建机制

**是的，Superpowers 会自动创建独立分支。** 内部执行：

```bash
# 检测项目名称
project=$(basename "$(git rev-parse --show-toplevel)")

# 根据目录位置确定路径
# 项目内: .worktrees/$BRANCH_NAME
# 全局:   ~/.config/superpowers/worktrees/$project/$BRANCH_NAME

# 创建 worktree 时同时创建新分支（-b 参数）
git worktree add "$path" -b "$BRANCH_NAME"
```

这意味着每次调用 skill 都会：
- 创建一个新的独立分支
- 将该分支检出到指定的 worktree 目录
- 每个 Claude Code 实例自然拥有独立的分支，避免冲突

#### 2. 分配独立任务

使用 `dispatching-parallel-agents` skill 分配任务：
```
/superpowers:dispatching-parallel-agents
```

关键：**确保任务域相互独立**

| 好的任务划分 | 坏的任务划分 |
|-------------|-------------|
| Agent A: 修改 `auth/` 模块 | Agent A: 修改 `user.rs` |
| Agent B: 修改 `api/` 模块 | Agent B: 也修改 `user.rs` |
| Agent C: 修改 `db/` 模块 | ❌ 必然冲突 |

### 方案二：手动 Git Worktree

#### 步骤 1：设置 .gitignore

```bash
# 确保 worktree 目录被忽略
echo ".worktrees/" >> .gitignore
git add .gitignore && git commit -m "Add .worktrees to gitignore"
```

#### 步骤 2：为每个 Claude 实例创建 Worktree

```bash
# Claude #1 的工作空间
git worktree add .worktrees/feature-auth -b feature/auth

# Claude #2 的工作空间
git worktree add .worktrees/feature-api -b feature/api

# Claude #3 的工作空间
git worktree add .worktrees/feature-db -b feature/db
```

#### 步骤 3：在各自 Worktree 中启动 Claude Code

```bash
# 终端 1
cd .worktrees/feature-auth
claude

# 终端 2
cd .worktrees/feature-api
claude

# 终端 3
cd .worktrees/feature-db
claude
```

#### 步骤 4：完成后合并

```bash
# 回到主分支
cd /path/to/main/repo

# 合并各个功能分支
git merge feature/auth
git merge feature/api
git merge feature/db

# 清理 worktrees
git worktree remove .worktrees/feature-auth
git worktree remove .worktrees/feature-api
git worktree remove .worktrees/feature-db
```

### 方案三：全局 Worktree 目录

如果不想在项目内创建 `.worktrees/`，可以使用全局目录：

```bash
# 创建全局 worktree 目录
mkdir -p ~/.config/superpowers/worktrees/dex-sui/

# 创建 worktree
git worktree add ~/.config/superpowers/worktrees/dex-sui/feature-auth -b feature/auth
```

优点：完全在项目目录外，无需修改 `.gitignore`

## 任务隔离策略

### 按模块/目录划分

```
实例 1: crates/sui-core/       → 核心逻辑修改
实例 2: crates/sui-indexer/    → 索引器开发
实例 3: crates/sui-types/      → 类型定义修改
```

### 按功能划分

```
实例 1: 实现订单撮合引擎
实例 2: 实现事件发射系统
实例 3: 实现 API 端点
```

### 按层次划分

```
实例 1: 数据层 (models, migrations)
实例 2: 服务层 (business logic)
实例 3: 接口层 (handlers, routes)
```

## 避免冲突的检查清单

### 启动前检查

- [ ] 每个实例有独立的 worktree
- [ ] 每个实例在独立的分支
- [ ] 任务域相互独立（不编辑相同文件）
- [ ] `.worktrees/` 已添加到 `.gitignore`

### 运行中检查

- [ ] 不要在多个实例中编辑同一文件
- [ ] 定期在主仓库运行 `git fetch` 获取最新状态
- [ ] 如需共享代码，先合并到主分支再 pull

### 合并时检查

- [ ] 先运行测试确保各分支独立可用
- [ ] 按依赖顺序合并（被依赖的先合并）
- [ ] 合并后运行完整测试套件
- [ ] 处理任何合并冲突

## Rust 项目特殊考虑

### 编译缓存隔离

每个 worktree 有独立的 `target/` 目录，自动隔离。但可以配置共享：

```bash
# 不推荐共享，可能导致锁冲突
# 如果磁盘空间紧张，可以使用 sccache
cargo install sccache
export RUSTC_WRAPPER=sccache
```

### Cargo.lock 处理

- Worktree 共享同一个 `Cargo.lock`
- 如果多个实例同时修改依赖，可能冲突
- **建议**：依赖修改由单一实例负责

## 常见问题

### Q: Worktree 能否指向同一个分支？

**不能**。每个 worktree 必须是不同的分支。尝试会报错：
```
fatal: 'main' is already checked out at '/path/to/repo'
```

### Q: 如何查看当前所有 worktrees？

```bash
git worktree list
```

### Q: Worktree 中的 git log 显示什么？

显示完整的仓库历史。Worktree 只是检出目录不同，共享 `.git` 数据。

### Q: 删除 worktree 后分支还在吗？

在。`git worktree remove` 只删除工作目录，分支需要单独删除：
```bash
git branch -d feature/auth
```

## Superpowers 相关 Skills

| Skill | 用途 |
|-------|------|
| `using-git-worktrees` | 创建和管理隔离工作空间 |
| `dispatching-parallel-agents` | 并行调度多个独立任务 |
| `subagent-driven-development` | 子 agent 驱动的开发流程 |
| `finishing-a-development-branch` | 完成分支后的合并/清理 |

## 参考资源

- [Superpowers 仓库](https://github.com/obra/superpowers)
- [Git Worktree 官方文档](https://git-scm.com/docs/git-worktree)
- [Claude Code 文档](https://docs.anthropic.com/en/docs/claude-code)