# DEX Indexer 实施计划 V3

> 基于 dex-indexer-tech-v5.md 技术方案的模块分离实施计划
> 创建日期: 2026-02-03
> 前置版本：V2（基础功能已完成）
> 阶段总结目录：`sui/mynotes/dex/summary/`

---

## 1. 项目概览

### 1.1 目标

基于 V5 技术方案，将 dex-indexer 拆分为四个独立模块：
- **dex-indexer**：Checkpoint 事件处理
- **dex-api**：REST API 服务
- **dex-realtime**：RPC 实时监听（Phase 2）
- **dex-ws**：WebSocket 推送（Phase 2）

### 1.2 当前状态

V2 已完成的功能（保留在 dex-indexer 中）：
- ✅ DEX 事件类型定义（sui-types/src/dex_events.rs）
- ✅ Checkpoint 事件处理（handlers/）
- ✅ 数据库 Schema（schema/, migrations/）
- ✅ REST API（api/）- 待迁移到 dex-api

### 1.3 参考文件

- 技术方案：`sui/mynotes/dex/tech/dex-indexer-tech-v5.md`
- 模块分离计划：`.claude/plans/federated-giggling-honey.md`
- 现有代码：`dex-sui/crates/dex-indexer/`

---

## 2. Phase 1: 模块分离（dex-indexer → dex-api）

### Phase 1.1: 创建 dex-api crate

**目标**：创建独立的 REST API 服务

#### 1.1.1 创建目录结构
```bash
dex-sui/crates/dex-api/
├── src/
│   ├── lib.rs
│   ├── main.rs      # 原 api_main.rs
│   ├── types.rs     # 原 api/types.rs
│   ├── server.rs    # 原 api/server.rs
│   ├── handlers.rs  # 原 api/handlers.rs
│   └── cache/       # Phase 3 占位
│       └── mod.rs
└── Cargo.toml
```

#### 1.1.2 迁移步骤
- [ ] 创建 `dex-sui/crates/dex-api/` 目录
- [ ] 创建 `Cargo.toml`，依赖 dex-indexer (schema)
- [ ] 复制 `dex-indexer/src/api/types.rs` → `dex-api/src/types.rs`
- [ ] 复制 `dex-indexer/src/api/server.rs` → `dex-api/src/server.rs`
- [ ] 复制 `dex-indexer/src/api/handlers.rs` → `dex-api/src/handlers.rs`
- [ ] 复制 `dex-indexer/src/api_main.rs` → `dex-api/src/main.rs`
- [ ] 创建 `dex-api/src/lib.rs`，导出模块
- [ ] 创建 `dex-api/src/cache/mod.rs` 占位文件

#### 1.1.3 Cargo.toml 配置
```toml
[package]
name = "dex-api"
version.workspace = true
edition = "2024"

[[bin]]
name = "dex-api"
path = "src/main.rs"

[dependencies]
# Schema 依赖
dex-indexer.workspace = true

# Web 框架
axum.workspace = true
sui-pg-db.workspace = true

# 序列化
serde = { workspace = true, features = ["derive"] }
serde_json.workspace = true

# 工具库
anyhow.workspace = true
clap.workspace = true
diesel = { workspace = true, features = ["chrono"] }
diesel-async = { workspace = true, features = ["bb8", "postgres"] }
hex.workspace = true
tokio.workspace = true
tracing.workspace = true
telemetry-subscribers.workspace = true
url.workspace = true
```

**验证**：
```bash
cargo build -p dex-api
./target/debug/dex-api --help
```

---

### Phase 1.2: 重构 dex-indexer

**目标**：从 dex-indexer 移除 API 相关代码

#### 1.2.1 删除文件
- [ ] 删除 `dex-indexer/src/api/` 目录
- [ ] 删除 `dex-indexer/src/api_main.rs`

#### 1.2.2 更新 Cargo.toml
- [ ] 移除 `[[bin]] dex-api` 定义
- [ ] 移除 axum 依赖（如仅用于 API）
- [ ] 保留其他依赖

#### 1.2.3 更新 lib.rs
- [ ] 移除 `pub mod api;` 导出
- [ ] 确保 `pub mod schema;` 和 `pub mod handlers;` 仍可导出

**验证**：
```bash
cargo build -p dex-indexer
cargo test -p dex-indexer
```

---

### Phase 1.3: 更新 Workspace

**目标**：注册新 crate 到 workspace

#### 1.3.1 更新根 Cargo.toml
- [ ] 在 `[workspace.members]` 添加 `"crates/dex-api"`
- [ ] 在 `[workspace.dependencies]` 添加 `dex-api`

#### 1.3.2 验证完整构建
```bash
cargo build -p dex-indexer
cargo build -p dex-api
cargo test -p dex-indexer
cargo test -p dex-api
```

---

### Phase 1.4: 迁移测试

**目标**：确保 API 相关测试仍能通过

#### 1.4.1 迁移 API 集成测试
- [ ] 检查 `dex-indexer/tests/api_integration.rs` 是否需要迁移
- [ ] 如需迁移，创建 `dex-api/tests/` 目录
- [ ] 更新测试导入路径

#### 1.4.2 运行完整测试
```bash
cargo test -p dex-indexer
cargo test -p dex-api
```

**验收标准**：
1. `dex-indexer` 编译通过，仅包含 handlers、schema
2. `dex-api` 编译通过，包含完整 REST API
3. 所有现有测试通过

---

## 3. Phase 2: 实时通道（dex-realtime + dex-ws）

### Phase 2.1: 创建 dex-realtime crate

**目标**：实现 Sui RPC 事件监听，发布到 Redis

#### 2.1.1 创建目录结构
```bash
dex-sui/crates/dex-realtime/
├── src/
│   ├── lib.rs
│   ├── main.rs
│   ├── listener.rs    # Sui RPC 订阅
│   └── publisher.rs   # Redis Stream 发布
└── Cargo.toml
```

#### 2.1.2 核心实现
- [ ] 实现 `listener.rs`：Sui WebSocket 订阅
- [ ] 实现 `publisher.rs`：Redis Stream 写入
- [ ] 实现 `main.rs`：命令行参数和启动逻辑

#### 2.1.3 Cargo.toml
```toml
[package]
name = "dex-realtime"
version.workspace = true

[[bin]]
name = "dex-realtime"
path = "src/main.rs"

[dependencies]
sui-types.workspace = true
sui-sdk.workspace = true
redis = { version = "0.24", features = ["tokio-comp"] }
tokio.workspace = true
tracing.workspace = true
clap.workspace = true
anyhow.workspace = true
```

---

### Phase 2.2: 创建 dex-ws crate

**目标**：实现 WebSocket 推送服务

#### 2.2.1 创建目录结构
```bash
dex-sui/crates/dex-ws/
├── src/
│   ├── lib.rs
│   ├── main.rs
│   ├── types.rs       # 订阅/消息类型
│   ├── server.rs      # WebSocket 服务器
│   ├── channels.rs    # 订阅频道管理
│   └── subscriber.rs  # Redis 订阅消费
└── Cargo.toml
```

#### 2.2.2 核心实现
- [ ] 实现 `types.rs`：订阅请求、推送消息类型
- [ ] 实现 `channels.rs`：频道管理（l2Book, trades, orderUpdates）
- [ ] 实现 `subscriber.rs`：Redis Stream 消费
- [ ] 实现 `server.rs`：WebSocket 服务器

#### 2.2.3 Cargo.toml
```toml
[package]
name = "dex-ws"
version.workspace = true

[[bin]]
name = "dex-ws"
path = "src/main.rs"

[dependencies]
tokio-tungstenite = "0.21"
redis = { version = "0.24", features = ["tokio-comp"] }
serde = { workspace = true, features = ["derive"] }
serde_json.workspace = true
tokio.workspace = true
tracing.workspace = true
clap.workspace = true
anyhow.workspace = true
futures-util.workspace = true
```

---

## 4. Phase 3: 缓存优化

### Phase 3.1: 实现 dex-api 缓存层

**目标**：dex-api 支持 Redis 缓存查询

#### 3.1.1 实现 cache 模块
- [ ] 实现 `dex-api/src/cache/mod.rs`
- [ ] 实现 Redis 客户端封装
- [ ] 实现缓存读取逻辑

#### 3.1.2 更新查询处理
- [ ] 修改 `handlers.rs`，优先查询 Redis
- [ ] 实现缓存未命中回退到 PostgreSQL

---

## 5. 验证计划

### 5.1 Phase 1 验证

```bash
# 编译验证
cargo build -p dex-indexer
cargo build -p dex-api

# 测试验证
cargo test -p dex-indexer
cargo test -p dex-api

# 功能验证
./target/debug/dex-indexer --help
./target/debug/dex-api --help

# API 端到端测试
./target/debug/dex-api --database-url "postgres://..." &
curl -X POST http://localhost:3000/info \
  -H "Content-Type: application/json" \
  -d '{"type": "recentFills", "perpetualId": 0}'
```

### 5.2 Phase 2 验证

```bash
# 编译验证
cargo build -p dex-realtime
cargo build -p dex-ws

# 功能验证
./target/debug/dex-realtime --help
./target/debug/dex-ws --help

# WebSocket 测试
wscat -c ws://localhost:8080
# 发送: {"type": "subscribe", "channel": "trades:0"}
```

---

## 6. 关键文件清单

### 6.1 Phase 1 迁移文件

| 源文件 | 目标位置 | 操作 |
|--------|----------|------|
| `dex-indexer/src/api/types.rs` | `dex-api/src/types.rs` | 复制 |
| `dex-indexer/src/api/server.rs` | `dex-api/src/server.rs` | 复制 |
| `dex-indexer/src/api/handlers.rs` | `dex-api/src/handlers.rs` | 复制 |
| `dex-indexer/src/api_main.rs` | `dex-api/src/main.rs` | 复制 |
| `dex-indexer/src/api/mod.rs` | - | 删除 |
| `dex-indexer/src/api/` | - | 删除目录 |

### 6.2 保留文件

| 文件 | 位置 | 说明 |
|------|------|------|
| schema/ | dex-indexer | 数据库表定义 |
| handlers/ | dex-indexer | Checkpoint 处理 |
| migrations/ | dex-indexer | 数据库迁移 |

---

## 7. 时间线

| 阶段 | 目标 | 依赖 |
|------|------|------|
| Phase 1.1 | 创建 dex-api crate | - |
| Phase 1.2 | 重构 dex-indexer | Phase 1.1 |
| Phase 1.3 | 更新 workspace | Phase 1.2 |
| Phase 1.4 | 迁移测试 | Phase 1.3 |
| Phase 2.1 | 创建 dex-realtime | Phase 1 完成 |
| Phase 2.2 | 创建 dex-ws | Phase 2.1 |
| Phase 3.1 | dex-api 缓存层 | Phase 2 完成 |

---

## 8. 风险与缓解

| 风险 | 影响 | 缓解措施 |
|------|------|----------|
| 依赖循环 | 编译失败 | dex-api 仅依赖 dex-indexer 的 schema |
| 测试失败 | 回归 | 分步验证，每步确保测试通过 |
| workspace 配置 | 构建失败 | 参考现有 crate 配置 |
