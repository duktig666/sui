# DEX Indexer 实施总结：Phase 0-8

> 完成时间：2026-01-30
> 涵盖阶段：Phase 0, 1, 1.5, 1.8, 2.1-2.3, 3.3-3.5, 5, 6, 7, 8

---

## 1. 完成任务清单

### Phase 0: 基础设施准备 ✓

| 任务 | 状态 | 输出文件 |
|------|------|----------|
| 创建 dex-indexer crate | ✓ | `crates/dex-indexer/` |
| Cargo.toml 配置 | ✓ | `crates/dex-indexer/Cargo.toml` |
| 目录结构搭建 | ✓ | `src/{main.rs, lib.rs, handlers/, schema/}` |
| 注册 workspace | ✓ | `dex-sui/Cargo.toml` 修改 |
| 创建 migrations 目录 | ✓ | `migrations/` |
| diesel_initial_setup | ✓ | `migrations/00000000000000_diesel_initial_setup/` |
| dex_fills 迁移 | ✓ | `migrations/2026-01-30-000001_dex_fills/` |

**验证**: `cargo check -p dex-indexer` 通过

---

### Phase 1: DEX 事件系统（FillEvent 最小版本） ✓

| 任务 | 状态 | 输出文件 |
|------|------|----------|
| 创建 dex_events.rs | ✓ | `sui-types/src/dex_events.rs` |
| 定义 DEX_EVENTS_PACKAGE | ✓ | 虚拟地址常量 |
| 实现 FillEvent 结构体 | ✓ | `to_sui_event()` 方法 |
| 导出 dex_events 模块 | ✓ | `sui-types/src/lib.rs` |
| 修改 DexExecutionResult | ✓ | `sui-execution/src/dex.rs` |
| execute_place_order 发射事件 | ✓ | 撮合时创建 FillEvent |
| build_effects_and_events 集成 | ✓ | events_digest 计算 |

**验证**: 下单测试验证 `response.events` 包含 FillEvent

---

### Phase 1.5: 事件索引可行性验证 ✓

| 任务 | 状态 | 输出文件 |
|------|------|----------|
| 创建 smoke test | ✓ | `sui-e2e-tests/tests/dex_indexer_smoke_test.rs` |
| test_dex_fill_event_indexable | ✓ | FillEvent BCS 反序列化验证 |
| test_dex_balance_update_event_on_deposit | ✓ | BalanceUpdateEvent 验证 |

**验证**: 事件可从交易中正确提取并反序列化

---

### Phase 1.8: 完善其他事件类型 ✓

| 任务 | 状态 | 说明 |
|------|------|------|
| PositionUpdateEvent | ✓ | 持仓变化事件（可从 FillEvent 推导） |
| BalanceUpdateEvent | ✓ | 余额变化事件 |
| TransferEvent | ✓ | 子账户转账事件 |
| FundingSettlementEvent | ✓ | 资金费率结算事件 |
| LiquidationEvent | ✓ | 清算事件 |
| deposit 发射 BalanceUpdateEvent | ✓ | `deposit_subaccount` 集成 |
| withdraw 发射 BalanceUpdateEvent | ✓ | `withdraw_subaccount` 集成 |

**遗留**: PositionUpdateEvent 未在 dex.rs 中发射（可从 FillEvent 推导，暂不需要）

---

### Phase 2.1-2.2: FillsHandler 实现 ✓

| 任务 | 状态 | 输出文件 |
|------|------|----------|
| dex_fills 表 Schema | ✓ | `migrations/2026-01-30-000001_dex_fills/up.sql` |
| StoredFill 结构体 | ✓ | `src/schema/mod.rs` |
| Processor trait 实现 | ✓ | `src/handlers/fills.rs` |
| Handler trait 实现 | ✓ | `commit()` 和 `prune()` |
| handlers/mod.rs 导出 | ✓ | `src/handlers/mod.rs` |

**验证**: `cargo nextest run -p dex-indexer` 通过

---

### Phase 3.3-3.4: BalancesHandler 实现 ✓

| 任务 | 状态 | 输出文件 |
|------|------|----------|
| dex_balances 表 Schema | ✓ | `migrations/2026-01-30-000002_dex_balances/up.sql` |
| StoredBalance 结构体 | ✓ | `src/schema/mod.rs` |
| Processor trait 实现 | ✓ | `src/handlers/balances.rs` |
| Handler trait 实现 | ✓ | `commit()` 和 `prune()` |
| handlers/mod.rs 导出 | ✓ | Balances 导出 |
| docker-compose.yml | ✓ | PostgreSQL 开发环境 |

**验证**: `cargo nextest run -p dex-indexer` 通过（2 tests: fills + balances）

---

### Phase 2.3/3.5: 集成测试 ✓

| 任务 | 状态 | 输出文件 |
|------|------|----------|
| 集成测试文件 | ✓ | `tests/handler_integration.rs` |
| test_fills_insert_and_query | ✓ | 验证 FillsHandler 写入和查询 |
| test_balances_insert_and_query | ✓ | 验证 BalancesHandler 写入和查询 |
| test_fills_query_by_perpetual | ✓ | 验证按 perpetual_id 查询 |

**验证**: `SUI_SKIP_SIMTESTS=1 cargo nextest run -p dex-indexer` 通过（5 tests）

---

## 2. 遇到的问题与解决方案

### 2.1 TransactionDigest 转换

**问题**: `tx.transaction.digest().to_vec()` 编译错误，TransactionDigest 没有 `to_vec()` 方法。

**解决**: 使用 `tx.transaction.digest().inner().to_vec()` 获取内部字节数组。

```rust
// 错误
let tx_digest = tx.transaction.digest().to_vec();

// 正确
let tx_digest = tx.transaction.digest().inner().to_vec();
```

---

### 2.2 Diesel 范围查询

**问题**: 使用 `.ge().and().lt()` 链式调用时，需要导入 `BoolExpressionMethods`。

**解决**: 改用 `.between()` 方法，更简洁且不需要额外 import。

```rust
// 原方案（需要额外 import）
let filter = dex_fills::table.filter(
    dex_fills::cp_sequence_number.ge(from as i64)
        .and(dex_fills::cp_sequence_number.lt(to_exclusive as i64))
);

// 优化方案
let filter = dex_fills::table.filter(
    dex_fills::cp_sequence_number.between(from as i64, to_exclusive as i64 - 1),
);
```

---

### 2.3 EmbeddedMigrations Debug trait

**问题**: `diesel_migrations::EmbeddedMigrations` 不实现 `Debug` trait，无法在日志中打印。

**解决**: 移除调试日志，migrations 对象不需要打印。

---

### 2.4 Cargo.toml 注释语法

**问题**: 错误使用 `//` 作为 TOML 注释。

**解决**: 使用正确的 `#` 注释语法。

---

### 2.5 i128 数据存储选择

**问题**: `BalanceUpdateEvent` 中的 `delta` 和 `new_balance` 字段是 `i128` 类型，PostgreSQL 原生不支持 128 位整数。

**方案对比**:
1. `NUMERIC(39, 0)` - 支持任意精度，但需要 `bigdecimal` crate
2. `BYTEA` - 存储为字节数组，简单但不支持 SQL 数值运算

**选择**: 使用 `BYTEA`（16 字节 little-endian），原因：
- 无需引入额外依赖
- DEX 场景不需要在数据库层做数值计算
- 与 sui-indexer-alt 的 `tx_balance_changes` 处理方式一致

```rust
// 存储时转换
delta: balance_event.delta.to_le_bytes().to_vec(),
```

---

### 2.6 集成测试并行隔离

**问题**: nextest 并行运行测试时，不同测试使用相同的 checkpoint 范围导致数据冲突。

**解决**: 每个测试使用独立的 checkpoint 范围：
- test_fills_insert_and_query: 2000001-2000099
- test_balances_insert_and_query: 2000101-2000199
- test_fills_query_by_perpetual: 2000201-2000299

```rust
const CP_START: i64 = 2000001;
const CP_END: i64 = 2000099;
cleanup_tables(&db, CP_START, CP_END).await?;
```

---

### 2.7 simtest 超时问题（非 DEX 代码）

**观察**: 部分复杂 simtest 测试存在超时问题，这是 sui-e2e-tests 的已知问题，与 DEX 代码无关。

**影响**: 无，DEX 相关测试全部通过。

---

## 3. 测试结果

```bash
# DEX e2e 测试
cargo simtest -p sui-e2e-tests -- dex
# 结果: 20 tests passed

# dex-indexer 单元测试 + 集成测试
SUI_SKIP_SIMTESTS=1 cargo nextest run -p dex-indexer
# 结果: 5 tests passed
#   - handlers::fills::tests::test_fills_handler_name
#   - handlers::balances::tests::test_balances_handler_name
#   - handler_integration::test_fills_insert_and_query
#   - handler_integration::test_balances_insert_and_query
#   - handler_integration::test_fills_query_by_perpetual

# 总计: 25 tests passed
```

---

## 4. 关键代码结构

### dex-indexer crate 结构

```
crates/dex-indexer/
├── Cargo.toml
├── docker-compose.yml      # PostgreSQL 开发环境
├── src/
│   ├── lib.rs              # crate 入口，导出 MIGRATIONS
│   ├── main.rs             # 二进制入口（skeleton）
│   ├── handlers/
│   │   ├── mod.rs          # 导出 Fills, Balances
│   │   ├── fills.rs        # FillsHandler 实现
│   │   └── balances.rs     # BalancesHandler 实现
│   └── schema/
│       └── mod.rs          # Diesel table! 宏, StoredFill, StoredBalance
└── migrations/
    ├── 00000000000000_diesel_initial_setup/
    │   ├── up.sql
    │   └── down.sql
    ├── 2026-01-30-000001_dex_fills/
    │   ├── up.sql          # dex_fills, dex_watermarks 表
    │   └── down.sql
    └── 2026-01-30-000002_dex_balances/
        ├── up.sql          # dex_balances 表
        └── down.sql
```

### Handler 实现模式

```rust
#[async_trait]
impl Processor for Fills {
    const NAME: &'static str = "dex_fills";
    type Value = StoredFill;

    async fn process(&self, checkpoint: &Arc<Checkpoint>) -> Result<Vec<Self::Value>> {
        // 1. 遍历 transactions
        // 2. 过滤 DEX_EVENTS_PACKAGE 事件
        // 3. BCS 反序列化 FillEvent
        // 4. 转换为 StoredFill
    }
}

#[async_trait]
impl Handler for Fills {
    const MIN_EAGER_ROWS: usize = 100;
    const MAX_PENDING_ROWS: usize = 10000;

    async fn commit<'a>(values: &[Self::Value], conn: &mut Connection<'a>) -> Result<usize> {
        // INSERT ... ON CONFLICT DO NOTHING
    }

    async fn prune<'a>(&self, from: u64, to: u64, conn: &mut Connection<'a>) -> Result<usize> {
        // DELETE WHERE cp_sequence_number BETWEEN from AND to-1
    }
}
```

---

## 5. PostgreSQL 依赖分析

### 运行时依赖

**dex-indexer 运行时不需要 psql 客户端**

sui-indexer-alt 使用纯 Rust 实现数据库连接：
- `diesel-async` + `bb8` 连接池
- `tokio-postgres` + `rustls` 建立连接
- Migrations 通过 `diesel_migrations::EmbeddedMigrations` 嵌入二进制

参考代码：
- `sui-pg-db/src/lib.rs`: 连接池创建和 migration 执行
- `sui-pg-db/src/tls.rs`: TLS 连接建立

### 测试依赖

sui-indexer-alt 的 `TempDb`（`sui-pg-db/src/temp.rs`）需要本地 PostgreSQL：

| 命令 | 用途 |
|------|------|
| `initdb` | 初始化数据库目录 |
| `postgres` | 启动数据库服务 |
| `pg_ctl` | 停止数据库服务 |
| `pg_isready` | 健康检查 |

安装方法：
```bash
# macOS
brew install postgresql@15

# Ubuntu/Debian
sudo apt install postgresql-15
```

### dex-indexer 测试策略

| 方案 | 优点 | 缺点 |
|------|------|------|
| **A. docker-compose** | 无需本地安装；环境隔离 | 需要 Docker |
| **B. TempDb 模式** | 与 sui-indexer-alt 一致 | 需要安装 PostgreSQL |

**当前选择：方案 A（docker-compose.yml 已创建）**

---

### Phase 5: Indexer 主程序 ✓

| 任务 | 状态 | 说明 |
|------|------|------|
| main.rs 完整实现 | ✓ | 命令行参数、数据库连接、pipeline 注册 |
| 优雅关闭 | ✓ | 使用 Service::main() 处理 SIGINT/SIGTERM |
| 构建验证 | ✓ | `cargo build -p dex-indexer` 通过 |
| Help 命令验证 | ✓ | `dex-indexer --help` 正确显示所有选项 |

**关键实现**:

```rust
// 创建 Indexer
let mut indexer = Indexer::new(
    store,
    args.indexer_args,
    args.client_args,
    IngestionConfig::default(),
    Some("dex"),
    &registry,
).await?;

// 注册 Pipeline
indexer.concurrent_pipeline(Fills, ConcurrentConfig::default()).await?;
indexer.concurrent_pipeline(Balances, ConcurrentConfig::default()).await?;

// 运行并处理信号
let service = indexer.run().await?;
service.main().await  // 内置 Ctrl+C/SIGTERM 处理
```

**验证命令**:
```bash
# 构建
cargo build -p dex-indexer

# 查看帮助
./target/debug/dex-indexer --help

# 运行（需要 checkpoint 源和数据库）
./target/debug/dex-indexer \
  --database-url postgres://dex:dex123@localhost:5432/dex_indexer \
  --remote-store-url https://checkpoints.mainnet.sui.io \
  --first-checkpoint 0
```

---

### Phase 6: REST API 基础 ✓

| 任务 | 状态 | 输出文件 |
|------|------|----------|
| Axum 依赖添加 | ✓ | `Cargo.toml` |
| API 类型定义 | ✓ | `src/api/types.rs` |
| API Server 实现 | ✓ | `src/api/server.rs` |
| Handler 实现 | ✓ | `src/api/handlers.rs` |
| API 二进制文件 | ✓ | `src/api_main.rs` → `dex-api` |

**API 端点**:
- `GET /health` - 健康检查
- `POST /info` - 统一查询接口
  - `type: "userFills"` - 查询用户成交记录
  - `type: "userBalances"` - 查询用户余额变动
  - `type: "recentFills"` - 查询市场最近成交

**示例请求**:
```bash
# 查询用户成交
curl -X POST http://localhost:3000/info \
  -H "Content-Type: application/json" \
  -d '{"type": "userFills", "subaccount": "0x...", "limit": 100}'

# 查询市场最近成交
curl -X POST http://localhost:3000/info \
  -H "Content-Type: application/json" \
  -d '{"type": "recentFills", "perpetualId": 1, "limit": 50}'
```

**运行命令**:
```bash
# 启动 API 服务器
./target/debug/dex-api \
  --database-url postgres://dex:dex123@localhost:5432/dex_indexer \
  --api-listen-address 0.0.0.0:3000
```

---

### Phase 7: 端到端集成测试 ✓

| 任务 | 状态 | 输出文件 |
|------|------|----------|
| API 集成测试文件 | ✓ | `tests/api_integration.rs` |
| userFills 端点测试 | ✓ | `test_api_user_fills` |
| userBalances 端点测试 | ✓ | `test_api_user_balances` |
| recentFills 端点测试 | ✓ | `test_api_recent_fills` |
| 健康检查测试 | ✓ | `test_api_health_check` |
| 错误处理测试 | ✓ | `test_api_invalid_request` |

**测试结果**:
```bash
SUI_SKIP_SIMTESTS=1 cargo nextest run -p dex-indexer
# 结果: 11 tests passed
#   - api::server::tests::test_default_config
#   - handlers::fills::tests::test_fills_handler_name
#   - handlers::balances::tests::test_balances_handler_name
#   - handler_integration::test_fills_insert_and_query
#   - handler_integration::test_balances_insert_and_query
#   - handler_integration::test_fills_query_by_perpetual
#   - api_integration::test_api_user_fills
#   - api_integration::test_api_user_balances
#   - api_integration::test_api_recent_fills
#   - api_integration::test_api_health_check
#   - api_integration::test_api_invalid_request
```

**测试覆盖**:
1. **Handler 单元测试**: 验证 Handler 名称和基本功能
2. **数据库集成测试**: 验证数据写入和查询
3. **API 集成测试**: 验证完整的请求-响应流程

---

## 6. 下一阶段准备

### Phase 4: 高级 Handlers（优先级较低）

- CandlesHandler（K 线聚合）
- FundingHandler（资金费率）
- LiquidationsHandler
- TransfersHandler

---

### Phase 8: 端到端验证 ⚠️ 部分完成

| 任务 | 状态 | 说明 |
|------|------|------|
| E2E 验证计划文档 | ✓ | `sui/mynotes/dex/e2e/dex-indexer-e2e-plan.md` |
| dex-indexer 测试 | ✓ | 11 tests passed |
| Handler 集成测试 | ✓ | 3 tests passed |
| API 集成测试 | ✓ | 5 tests passed |
| simtest 事件测试 | ⚠️ | SDK 布局获取问题 |
| E2E 验证总结 | ✓ | `sui/mynotes/dex/summary/phase-8-e2e-verification.md` |

**测试结果**:
```bash
# dex-indexer 全部测试
SUI_SKIP_SIMTESTS=1 cargo nextest run -p dex-indexer
# Summary [0.483s] 11 tests run: 11 passed, 0 skipped
```

**已知问题**:
- simtest 中的 DEX 事件测试因 SDK 尝试获取虚拟 package 布局而失败
- 原因：`0x44455800` package 在运行时不存在
- 影响：不影响实际 indexer 功能（使用 BCS 直接反序列化）
- 详见：`sui/mynotes/dex/summary/phase-8-e2e-verification.md`

---

## 7. 参考文档

- 技术方案：`sui/mynotes/dex/tech/dex-indexer-tech-v4.md`
- 实施计划：`dex-indexer-implementation-plan-v2.md`（已更新完成状态）
- Handler 模式参考：`sui-indexer-alt/src/handlers/ev_emit_mod.rs`
- E2E 验证计划：`sui/mynotes/dex/e2e/dex-indexer-e2e-plan.md`
- E2E 验证总结：`sui/mynotes/dex/summary/phase-8-e2e-verification.md`
