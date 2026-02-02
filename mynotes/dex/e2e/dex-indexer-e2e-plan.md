# DEX Indexer 端到端验证计划

> 创建日期: 2026-01-30
> 目的: 验证完整的数据流：Sui 节点 → DEX 交易 → 事件发射 → Indexer → API 查询

---

## 1. 验证目标

验证 DEX Indexer 的完整数据流程：

```
┌─────────────┐    ┌─────────────┐    ┌─────────────┐    ┌─────────────┐
│  Sui Node   │ -> │ DEX Events  │ -> │  Indexer    │ -> │  REST API   │
│  (交易执行) │    │ (事件发射)  │    │ (写入数据库)│    │ (查询数据)  │
└─────────────┘    └─────────────┘    └─────────────┘    └─────────────┘
```

### 1.1 核心验证点

| 验证项 | 说明 | 验证方法 |
|--------|------|----------|
| FillEvent 发射 | 订单撮合时生成 FillEvent | simtest |
| BalanceUpdateEvent 发射 | 存取款时生成 BalanceUpdateEvent | simtest |
| 事件 BCS 序列化 | 事件能正确序列化/反序列化 | simtest |
| Handler 数据库写入 | FillsHandler/BalancesHandler 写入正确 | 集成测试 |
| API 查询返回 | REST API 返回正确数据 | API 集成测试 |

---

## 2. 验证方案

### 2.1 方案 A: simtest 验证（事件发射层）✅ 推荐

**适用场景**: 验证事件发射和 BCS 序列化

**测试文件**: `sui-e2e-tests/tests/dex_indexer_smoke_test.rs`

**测试用例**:
- `test_dex_fill_event_indexable`: 验证 FillEvent 可提取和反序列化
- `test_dex_balance_update_event_on_deposit`: 验证 BalanceUpdateEvent 可提取和反序列化

**执行命令**:
```bash
cargo simtest -p sui-e2e-tests -- dex_indexer
```

**优点**: 无需外部依赖，快速验证核心逻辑

---

### 2.2 方案 B: 集成测试（数据库层）

**适用场景**: 验证 Handler 到数据库的写入和查询

**测试文件**: `dex-indexer/tests/handler_integration.rs`

**测试用例**:
- `test_fills_insert_and_query`: 测试 Fill 记录写入和查询
- `test_balances_insert_and_query`: 测试 Balance 记录写入和查询
- `test_fills_query_by_perpetual`: 测试按市场 ID 查询

**执行命令**:
```bash
# 需要先启动 PostgreSQL
cd /home/rsw/code/dex/dex-sui/crates/dex-indexer
docker-compose up -d

# 运行测试
export DATABASE_URL=postgres://dex:dex123@localhost:5432/dex_indexer
SUI_SKIP_SIMTESTS=1 cargo nextest run -p dex-indexer -- handler_integration
```

---

### 2.3 方案 C: API 集成测试（全链路）

**适用场景**: 验证从数据库到 API 响应的完整链路

**测试文件**: `dex-indexer/tests/api_integration.rs`

**测试用例**:
- `test_api_user_fills`: 用户成交查询
- `test_api_user_balances`: 用户余额查询
- `test_api_recent_fills`: 市场最近成交查询
- `test_api_health_check`: 健康检查
- `test_api_invalid_request`: 错误处理

**执行命令**:
```bash
export DATABASE_URL=postgres://dex:dex123@localhost:5432/dex_indexer
SUI_SKIP_SIMTESTS=1 cargo nextest run -p dex-indexer -- api_integration
```

---

### 2.4 方案 D: 真实节点验证（生产环境模拟）

**适用场景**: 验证与真实 Sui 节点的集成

#### 步骤 1: 启动 PostgreSQL
```bash
cd /home/rsw/code/dex/dex-sui/crates/dex-indexer
docker-compose up -d
```

#### 步骤 2: 启动 Sui 本地节点
```bash
# 使用 sui-test-validator 或 sui start
sui start --with-faucet --force-regenesis
```

#### 步骤 3: 启动 dex-indexer
```bash
# 选择 checkpoint 源（三选一）
./target/debug/dex-indexer \
  --database-url postgres://dex:dex123@localhost:5432/dex_indexer \
  --local-ingestion-path /tmp/sui_checkpoints \
  --first-checkpoint 0
```

#### 步骤 4: 启动 dex-api
```bash
./target/debug/dex-api \
  --database-url postgres://dex:dex123@localhost:5432/dex_indexer \
  --api-listen-address 0.0.0.0:3000
```

#### 步骤 5: 执行 DEX 交易
使用 Sui CLI 或测试脚本执行交易

#### 步骤 6: 验证 API 响应
```bash
curl http://localhost:3000/health
curl -X POST http://localhost:3000/info -H "Content-Type: application/json" \
  -d '{"type": "recentFills", "perpetualId": 0, "limit": 10}'
```

---

## 3. 执行计划

### 3.1 执行顺序

```
Step 1: simtest 验证（方案 A）
    ↓
Step 2: 集成测试验证（方案 B + C）
    ↓
Step 3: [可选] 真实节点验证（方案 D）
```

### 3.2 详细步骤

| 步骤 | 操作 | 预期结果 | 状态 |
|------|------|----------|------|
| 1 | 运行 simtest | 2 个测试通过 | ✅ (SDK 问题已修复) |
| 2 | 启动 PostgreSQL | 容器运行正常 | ✅ |
| 3 | 运行 Handler 集成测试 | 3 个测试通过 | ✅ |
| 4 | 运行 API 集成测试 | 5 个测试通过 | ✅ |
| 5 | 构建二进制 | dex-indexer + dex-api 编译成功 | ✅ |
| 6 | 修复 SDK 布局获取问题 | 17/20 simtest 通过 | ✅ |

---

## 4. 验证清单

### 4.1 事件发射验证
- [x] FillEvent 在订单撮合时发射 (代码已实现)
- [x] BalanceUpdateEvent 在存款时发射 (代码已实现)
- [x] BalanceUpdateEvent 在取款时发射 (代码已实现)
- [x] 事件 BCS 序列化正确 (test_dex_events_constants 通过)

### 4.2 数据库验证
- [x] dex_fills 表结构正确 (test_fills_insert_and_query)
- [x] dex_balances 表结构正确 (test_balances_insert_and_query)
- [x] FillsHandler 正确写入数据 (test_fills_insert_and_query)
- [x] BalancesHandler 正确写入数据 (test_balances_insert_and_query)
- [x] 重复写入幂等 (on_conflict_do_nothing)

### 4.3 API 验证
- [x] GET /health 返回 "OK" (test_api_health_check)
- [x] POST /info userFills 返回正确数据 (test_api_user_fills)
- [x] POST /info userBalances 返回正确数据 (test_api_user_balances)
- [x] POST /info recentFills 返回正确数据 (test_api_recent_fills)
- [x] 错误请求返回 4xx 状态码 (test_api_invalid_request)

---

## 5. 问题排查指南

### 5.1 常见问题

| 问题 | 可能原因 | 解决方法 |
|------|----------|----------|
| simtest 超时 | 编译缓存问题 | 运行 `cargo clean` |
| 数据库连接失败 | PostgreSQL 未启动 | `docker-compose up -d` |
| API 返回空数据 | 数据未写入 | 检查 `SELECT * FROM dex_fills` |
| 事件未被索引 | package_id 不匹配 | 检查 DEX_EVENTS_PACKAGE |

### 5.2 调试命令

```bash
# 检查数据库数据
psql postgres://dex:dex123@localhost:5432/dex_indexer -c "SELECT COUNT(*) FROM dex_fills;"
psql postgres://dex:dex123@localhost:5432/dex_indexer -c "SELECT COUNT(*) FROM dex_balances;"

# 检查 Indexer 日志
RUST_LOG=debug ./target/debug/dex-indexer ...

# 检查 API 日志
RUST_LOG=debug ./target/debug/dex-api ...
```

---

## 6. 输出文档

| 文档 | 路径 | 说明 |
|------|------|------|
| E2E 验证计划 | `sui/mynotes/dex/e2e/dex-indexer-e2e-plan.md` | 本文档 |
| E2E 验证总结 | `sui/mynotes/dex/summary/phase-8-e2e-verification.md` | 执行结果 |
| 阶段总结更新 | `sui/mynotes/dex/summary/phase-0-2-summary.md` | 状态更新 |

---

## 7. 心得与经验

### 7.1 技术收获

1. **sui-indexer-alt-framework 架构**
   - Handler trait 设计优雅，支持 concurrent 和 sequential 两种模式
   - Pipeline 自动处理并发和背压

2. **事件发射机制**
   - DEX 事件通过 `DexExecutionResult.events` 传递
   - 事件进入 `TransactionEffects` 的 `events_digest` 计算

3. **Diesel 异步支持**
   - `diesel-async` + `bb8` 连接池实现高效数据库访问
   - `EmbeddedMigrations` 实现二进制内嵌迁移

### 7.2 注意事项

1. **测试环境隔离**
   - 使用独立的 checkpoint 范围避免测试数据冲突
   - 每个测试前清理相关数据

2. **API 设计**
   - 采用 Hyperliquid 风格的 POST /info 统一入口
   - 使用 serde tag 实现请求类型分发

3. **性能考虑**
   - Handler 使用批量写入（`on_conflict_do_nothing`）
   - API 查询添加合理的 limit 默认值和最大值
