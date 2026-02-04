# DEX Indexer Phase 1: 模块分离总结

> 完成时间: 2026-02-04
> 涵盖范围: Phase 1.1 ~ 1.4 (dex-indexer → dex-api 模块分离)
> 参考文档: `dex-sui/docs/indexer/plan/dex-indexer-implementation-plan-latest.md`

---

## 概要

Phase 1 模块分离已全部完成。成功将 REST API 服务从 dex-indexer 中拆分为独立的 dex-api crate，实现了索引和查询服务的解耦。两个 crate 现在可以独立部署和扩展，dex-api 通过 dex-indexer 的 schema 模块共享数据库定义，避免了代码重复。

---

## 1. 完成任务清单

### Phase 1.1: 创建 dex-api crate ✅

| 任务 | 状态 | 输出文件 |
|------|------|----------|
| 创建目录结构 | ✅ | `dex-sui/crates/dex-api/` |
| 创建 Cargo.toml | ✅ | `dex-sui/crates/dex-api/Cargo.toml` |
| 迁移 types.rs | ✅ | `dex-sui/crates/dex-api/src/types.rs` |
| 迁移 server.rs | ✅ | `dex-sui/crates/dex-api/src/server.rs` |
| 迁移 handlers.rs | ✅ | `dex-sui/crates/dex-api/src/handlers.rs` |
| 迁移 main.rs | ✅ | `dex-sui/crates/dex-api/src/main.rs` |
| 创建 lib.rs | ✅ | `dex-sui/crates/dex-api/src/lib.rs` |
| 创建 cache/mod.rs | ✅ | `dex-sui/crates/dex-api/src/cache/mod.rs` (Phase 3 占位) |

### Phase 1.2: 重构 dex-indexer ✅

| 任务 | 状态 | 说明 |
|------|------|------|
| 删除 api/ 目录 | ✅ | API 相关代码已迁移至 dex-api |
| 删除 api_main.rs | ✅ | 入口已迁移至 dex-api/src/main.rs |
| 更新 Cargo.toml | ✅ | 移除 dex-api binary 定义 |
| 更新 lib.rs | ✅ | 移除 api 模块导出，保留 schema/handlers |

### Phase 1.3: 更新 Workspace ✅

| 任务 | 状态 | 说明 |
|------|------|------|
| 注册 workspace member | ✅ | `"crates/dex-api"` 添加到 members |
| 注册 workspace dependency | ✅ | `dex-api = { path = "crates/dex-api" }` |

### Phase 1.4: 迁移测试 ✅

| 任务 | 状态 | 输出文件 |
|------|------|----------|
| 创建测试目录 | ✅ | `dex-sui/crates/dex-api/tests/` |
| 迁移 API 集成测试 | ✅ | `dex-sui/crates/dex-api/tests/api_integration.rs` |

---

## 2. 模块架构设计

### 2.1 分离前后对比

**分离前 (Phase 0-8)**:
```
dex-indexer/
├── src/
│   ├── main.rs          # indexer 入口
│   ├── api_main.rs      # api 入口
│   ├── api/             # REST API 实现
│   │   ├── types.rs
│   │   ├── server.rs
│   │   └── handlers.rs
│   ├── handlers/        # Checkpoint 处理器
│   └── schema/          # 数据库 schema
└── tests/
    ├── handler_integration.rs
    └── api_integration.rs
```

**分离后 (Phase 1)**:
```
dex-indexer/                    dex-api/
├── src/                        ├── src/
│   ├── main.rs                 │   ├── main.rs
│   ├── handlers/               │   ├── lib.rs
│   │   ├── fills.rs            │   ├── types.rs
│   │   ├── balances.rs         │   ├── server.rs
│   │   ├── positions.rs        │   ├── handlers.rs
│   │   ├── transfers.rs        │   └── cache/
│   │   └── perpetuals.rs       │       └── mod.rs
│   └── schema/                 └── tests/
│       └── mod.rs                  └── api_integration.rs
└── migrations/
```

### 2.2 依赖关系

```
┌─────────────────┐
│     dex-api     │
│  (REST 服务)     │
└────────┬────────┘
         │ 依赖 schema
         ▼
┌─────────────────┐
│   dex-indexer   │
│ (Checkpoint 处理)│
└────────┬────────┘
         │ 依赖框架
         ▼
┌─────────────────┐
│sui-indexer-alt  │
│   -framework    │
└─────────────────┘
```

### 2.3 数据流向

```
Sui Fullnode                     PostgreSQL                      Client
     │                               │                              │
     │ Checkpoints                   │                              │
     ▼                               │                              │
┌─────────────┐                      │                              │
│dex-indexer  │──────写入事件────────▶│                              │
│ (5 handlers)│                      │                              │
└─────────────┘                      │                              │
                                     │                              │
                                     │                              │
                               ┌─────┴─────┐                        │
                               │           │                        │
                               │ dex_fills │◀───────────────────────┤
                               │dex_balances                        │
                               │dex_positions                       │
                               │dex_transfers    ┌─────────────┐    │
                               │dex_perpetuals   │  dex-api    │    │
                               │           │────▶│ (REST 服务) │◀───┤
                               └───────────┘     └─────────────┘    │
                                                      │             │
                                                      │ JSON        │
                                                      ▼             │
                                                 HTTP Response ─────┘
```

---

## 3. 核心实现详情

### 3.1 dex-api Crate

#### 目录结构

```
dex-sui/crates/dex-api/
├── Cargo.toml
├── src/
│   ├── lib.rs           # crate 入口，导出公共 API
│   ├── main.rs          # 二进制入口
│   ├── types.rs         # 请求/响应类型定义
│   ├── server.rs        # Axum 服务器配置
│   ├── handlers.rs      # 查询处理逻辑
│   └── cache/
│       └── mod.rs       # Phase 3 缓存占位
└── tests/
    └── api_integration.rs
```

#### API 端点实现

| 端点 | 方法 | 请求类型 | 说明 |
|------|------|----------|------|
| `/health` | GET | - | 健康检查 |
| `/info` | POST | `userFills` | 查询用户成交记录 |
| `/info` | POST | `userBalances` | 查询用户余额变动 |
| `/info` | POST | `userTransfers` | 查询用户转账记录 |
| `/info` | POST | `recentFills` | 查询市场最近成交 |
| `/info` | POST | `clearinghouseState` | 查询用户持仓和保证金 |
| `/info` | POST | `meta` | 查询永续合约市场元数据 |

#### 请求分发模式

```rust
// server.rs:61-148
async fn info_handler(
    State(state): State<AppState>,
    Json(request): Json<InfoRequest>,
) -> impl IntoResponse {
    match request {
        InfoRequest::UserFills(req) => handlers::query_user_fills(&state.db, req).await,
        InfoRequest::UserBalances(req) => handlers::query_user_balances(&state.db, req).await,
        InfoRequest::UserTransfers(req) => handlers::query_user_transfers(&state.db, req).await,
        InfoRequest::RecentFills(req) => handlers::query_recent_fills(&state.db, req).await,
        InfoRequest::ClearinghouseState(req) => handlers::query_clearinghouse_state(&state.db, req).await,
        InfoRequest::Meta(_) => handlers::query_meta(&state.db).await,
    }
}
```

#### Cargo.toml 关键配置

```toml
[package]
name = "dex-api"
version.workspace = true
edition = "2024"

[[bin]]
name = "dex-api"
path = "src/main.rs"

[dependencies]
# Schema 依赖 - 共享数据库定义
dex-indexer.workspace = true

# Web 框架
axum.workspace = true
sui-pg-db.workspace = true

# 序列化与异步
serde.workspace = true
tokio.workspace = true
diesel.workspace = true
diesel-async.workspace = true
```

### 3.2 dex-indexer Crate

#### 重构后结构

```
dex-sui/crates/dex-indexer/
├── Cargo.toml
├── src/
│   ├── lib.rs           # 导出 MIGRATIONS, schema, handlers
│   ├── main.rs          # Indexer 二进制入口
│   ├── handlers/
│   │   ├── mod.rs       # 导出所有 Handler
│   │   ├── fills.rs     # FillEvent 处理
│   │   ├── balances.rs  # BalanceUpdateEvent 处理
│   │   ├── positions.rs # PositionUpdateEvent 处理
│   │   ├── transfers.rs # TransferEvent 处理
│   │   └── perpetuals.rs# PerpetualCreateEvent 处理
│   └── schema/
│       └── mod.rs       # Diesel table! 宏和 Stored* 类型
└── migrations/
    ├── 00000000000000_diesel_initial_setup/
    ├── 2026-01-30-000001_dex_fills/
    ├── 2026-01-30-000002_dex_balances/
    ├── 2026-02-03-000001_dex_positions/
    ├── 2026-02-03-000002_dex_transfers/
    ├── 2026-02-03-000003_dex_perpetuals/
    └── 2026-02-05-000001_subaccount_split/
```

#### Handler 注册

```rust
// main.rs:91-122
// 注册 5 个 Handler
indexer.concurrent_pipeline(Fills, concurrent_config.clone()).await?;
indexer.concurrent_pipeline(Balances, concurrent_config.clone()).await?;
indexer.concurrent_pipeline(Positions, concurrent_config.clone()).await?;
indexer.concurrent_pipeline(Transfers, concurrent_config.clone()).await?;
indexer.concurrent_pipeline(Perpetuals, concurrent_config).await?;
```

#### 共享组件导出

```rust
// lib.rs
pub mod handlers;
pub mod schema;

pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

// Subaccount 解析工具函数
pub fn parse_subaccount(subaccount: &[u8]) -> Option<ParsedSubaccount>;
pub fn build_subaccount(account_address: &str, subaccount_number: u32) -> Option<Vec<u8>>;
```

---

## 4. 数据库设计

### 4.1 表结构概览

| 表名 | 主键 | 说明 |
|------|------|------|
| `dex_fills` | (cp_seq, tx_seq, event_idx) | 成交记录 |
| `dex_balances` | (cp_seq, tx_seq, event_idx) | 余额变动 |
| `dex_positions` | (account_addr, subaccount_num, perp_id) | 当前持仓 |
| `dex_position_updates` | (cp_seq, tx_seq, event_idx) | 持仓变动历史 |
| `dex_perpetuals` | perpetual_id | 永续合约元数据 |
| `dex_transfers` | (cp_seq, tx_seq, event_idx) | 子账户转账 |
| `dex_watermarks` | pipeline | 索引进度追踪 |

### 4.2 迁移版本记录

| 迁移文件 | 日期 | 说明 |
|----------|------|------|
| `00000000000000_diesel_initial_setup` | - | Diesel 初始化 |
| `2026-01-30-000001_dex_fills` | 01-30 | dex_fills 表 |
| `2026-01-30-000002_dex_balances` | 01-30 | dex_balances 表 |
| `2026-02-03-000001_dex_positions` | 02-03 | dex_positions 和 dex_position_updates 表 |
| `2026-02-03-000002_dex_transfers` | 02-03 | dex_transfers 表 |
| `2026-02-03-000003_dex_perpetuals` | 02-03 | dex_perpetuals 表 |
| `2026-02-05-000001_subaccount_split` | 02-04 | Subaccount 字段分离 |

### 4.3 Subaccount 处理

采用 dYdX 风格的 SubaccountId 设计：32 字节地址 + 4 字节子账户编号

```rust
// lib.rs:51-65
pub fn parse_subaccount(subaccount: &[u8]) -> Option<ParsedSubaccount> {
    if subaccount.len() != 36 { return None; }
    let number_bytes: [u8; 4] = subaccount[32..36].try_into().ok()?;
    let number = u32::from_le_bytes(number_bytes);
    Some(ParsedSubaccount {
        account_address: format!("0x{}", hex::encode(&subaccount[0..32])),
        subaccount_number: number as i32,
    })
}
```

**数据库存储**：分离为 `account_address (Text)` 和 `subaccount_number (Int4)` 两个字段，支持：
- 按地址聚合查询所有子账户
- 按特定子账户精确查询
- 高效的复合索引

---

## 5. 遇到的问题与解决方案

### 5.1 依赖循环风险

**问题**: dex-api 需要访问数据库 schema，可能导致与 dex-indexer 的循环依赖。

**解决方案**: dex-api 单向依赖 dex-indexer，只使用其 schema 模块：
```toml
# dex-api/Cargo.toml
[dependencies]
dex-indexer.workspace = true  # 仅使用 schema 定义
```

### 5.2 Workspace 成员注册

**问题**: 新 crate 需要正确注册到 workspace。

**解决方案**: 在 `dex-sui/Cargo.toml` 中添加：
```toml
[workspace]
members = [
  # ...
  "crates/dex-api",
]

[workspace.dependencies]
dex-api = { path = "crates/dex-api" }
```

### 5.3 测试迁移

**问题**: API 集成测试需要从 dex-indexer 迁移到 dex-api。

**解决方案**:
1. 创建 `dex-api/tests/` 目录
2. 迁移 `api_integration.rs`
3. 更新导入路径使用 `dex_api::` 而非 `dex_indexer::api::`

---

## 6. 测试验证

### 6.1 编译验证

```bash
# dex-indexer 编译
$ cargo build -p dex-indexer
   Compiling dex-indexer v0.1.0
    Finished `dev` profile target(s)

# dex-api 编译
$ cargo build -p dex-api
   Compiling dex-api v0.1.0
    Finished `dev` profile target(s)
```

### 6.2 二进制验证

```bash
$ ./target/debug/dex-indexer --help
DEX Event Indexer for Sui

Options:
  --database-url <DATABASE_URL>
  --remote-store-url <REMOTE_STORE_URL>
  --first-checkpoint <FIRST_CHECKPOINT>
  ...

$ ./target/debug/dex-api --help
DEX API Server

Options:
  --database-url <DATABASE_URL>
  --api-listen-address <API_LISTEN_ADDRESS>
  ...
```

### 6.3 测试套件

**dex-indexer 测试**:
- `handlers::fills::tests::test_fills_handler_name` ✅
- `handlers::balances::tests::test_balances_handler_name` ✅
- Handler 集成测试（3 个）✅

**dex-api 测试**:
- `api::server::tests::test_default_config` ✅
- `api_integration::test_api_health_check` ✅
- `api_integration::test_api_user_fills` ✅
- `api_integration::test_api_user_balances` ✅
- `api_integration::test_api_recent_fills` ✅
- `api_integration::test_api_invalid_request` ✅

---

## 7. 下一阶段准备

### Phase 2: dex-realtime + dex-ws

| 模块 | 说明 | 依赖 |
|------|------|------|
| dex-realtime | Sui RPC 实时事件监听 | sui-sdk |
| dex-ws | WebSocket 推送服务 | Redis Stream |

**架构预览**:
```
Sui RPC ──▶ dex-realtime ──▶ Redis Stream ──▶ dex-ws ──▶ WebSocket Clients
```

### Phase 3: 缓存优化

| 任务 | 说明 |
|------|------|
| 实现 cache 模块 | `dex-api/src/cache/mod.rs` |
| Redis 客户端集成 | 连接 dex-realtime 写入的缓存 |
| 查询优化 | 缓存优先，回退到 PostgreSQL |

---

## 8. 总结

Phase 1 模块分离取得完全成功：

1. ✅ **dex-api crate 创建完成** - 独立的 REST API 服务，6 个查询端点
2. ✅ **dex-indexer 重构完成** - 专注 Checkpoint 处理，5 个 Handler
3. ✅ **Workspace 配置正确** - 两个 crate 正确注册
4. ✅ **测试全部通过** - 编译验证和集成测试均通过
5. ✅ **架构清晰** - 索引和查询服务解耦，可独立部署

**关键成果**:
- 索引服务和 API 服务可独立扩展
- Schema 共享避免代码重复
- Phase 3 缓存层预留位置
- 为 Phase 2 实时通道奠定基础

---

## 9. 参考文档

| 文档 | 路径 |
|------|------|
| 实施计划 | `dex-sui/docs/indexer/plan/dex-indexer-implementation-plan-latest.md` |
| 技术方案 | `dex-sui/docs/indexer/tech/dex-indexer-tech-latest.md` |
| 架构设计 | `dex-sui/docs/indexer/arch/dex-indexer-structure-latest.md` |
| Phase 0-8 总结 | `sui/mynotes/dex/summary/phase-0-2-summary.md` |
| Phase 8 验证 | `sui/mynotes/dex/summary/phase-8-e2e-verification.md` |
| 测试指南 | `dex-sui/docs/indexer/test/index-test.md` |
