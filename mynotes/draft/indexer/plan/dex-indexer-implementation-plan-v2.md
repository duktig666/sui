# DEX Indexer 实施计划 V2

> 基于 dex-indexer-tech-v4.md 技术方案的细粒度实施计划
> 创建日期: 2026-01-30
> 阶段总结目录：`sui/mynotes/dex/summary/`

---

## 1. 项目概览

### 1.1 目标
基于 V4 技术方案，在 dex-sui 中实现 DEX Indexer，复用 sui-indexer-alt-framework。

### 1.2 核心组件
1. **DEX 事件类型**（sui-types/src/dex_events.rs）
2. **DEX 事件发射**（sui-execution/src/dex.rs 修改）
3. **Indexer Handlers**（dex-indexer/src/handlers/）
4. **数据库 Schema**（PostgreSQL migrations）
5. **REST API**（dex-indexer/src/api/）

### 1.3 参考文件
- 技术方案：`sui/mynotes/dex/tech/dex-indexer-tech-v4.md`
- 现有测试：`dex-sui/crates/sui-e2e-tests/tests/dex_order_tests.rs`
- 现有测试：`dex-sui/crates/sui-e2e-tests/tests/dex_subaccount_tests.rs`
- Handler 模式：`dex-sui/crates/sui-indexer-alt/src/handlers/ev_emit_mod.rs`

---

## 2. 实施阶段

### Phase 0: 基础设施准备
**目标**: 搭建 dex-indexer crate 骨架和数据库基础

#### 0.1 创建 dex-indexer crate
- [ ] 在 `dex-sui/crates/` 下创建 `dex-indexer` 目录
- [ ] 创建 `Cargo.toml`，依赖 sui-indexer-alt-framework
- [ ] 创建基本目录结构：`src/{main.rs, lib.rs, handlers/, models/, api/, schema/}`
- [ ] 注册到 workspace `Cargo.toml`

**测试**: `cargo check -p dex-indexer`

#### 0.2 数据库 Schema 基础
- [ ] 创建 `migrations/` 目录
- [ ] 创建 `001_initial_schema.sql`：dex_watermarks 表
- [ ] 创建测试数据库连接模块

**测试**: 手动执行 migration，验证表创建

**验收标准**: crate 编译通过，数据库连接正常

---

### Phase 1: DEX 事件系统（最小可行版本）
**目标**: 实现 FillEvent 事件类型定义和发射机制，通过下单测试验证

#### 1.1 FillEvent 类型定义（最小版本）
- [ ] 创建 `sui-types/src/dex_events.rs`
- [ ] 定义 `DEX_EVENTS_PACKAGE` 常量（虚拟地址）
- [ ] **仅实现 `FillEvent`** 结构体和 `to_sui_event()` 方法
- [ ] 在 `sui-types/src/lib.rs` 导出 dex_events 模块

**测试**:
```bash
SUI_SKIP_SIMTESTS=1 cargo nextest run -p sui-types --lib -- dex_events
```

#### 1.2 FillEvent 发射集成
- [ ] 修改 `sui-execution/src/dex.rs` 中的 `DexExecutionResult`，添加 `events: Vec<Event>` 字段
- [ ] **仅在 `execute_place_order` 撮合时创建 `FillEvent`**
- [ ] 修改 `build_effects_and_events` 计算 `events_digest`
- [ ] 确保事件进入 `TransactionEffects` 和 `TransactionEvents`

#### 1.3 下单测试验证事件发射 ⭐ 关键验证点
- [ ] 修改 `dex_order_tests.rs` 的 `test_dex_order_matching` 测试
- [ ] 添加断言：验证 `response.events` 非空
- [ ] 添加断言：验证 `events_digest` 存在于 effects 中
- [ ] 打印事件内容，人工确认 FillEvent 结构正确

**测试**:
```bash
cargo simtest -p sui-e2e-tests -- test_dex_order_matching
# 观察日志：应打印 FillEvent 内容
```

**验收标准**:
1. 下单撮合后，TransactionEffects 包含非空的 events_digest
2. TransactionEvents.data 包含 FillEvent
3. 现有 dex_order_tests 全部通过

---

### Phase 1.5: 事件索引可行性验证 ⭐ 新增验证阶段
**目标**: 使用简单测试验证 sui-indexer-alt-framework 能正确解析 DEX 事件

#### 1.5.1 创建最小 Handler 测试
- [ ] 创建 `dex-sui/crates/sui-e2e-tests/tests/dex_indexer_smoke_test.rs`
- [ ] 在测试中：
  1. 启动 TestCluster
  2. 执行下单撮合交易
  3. 获取 Checkpoint（从 fullnode）
  4. 手动遍历 checkpoint.transactions 提取 FillEvent
  5. 验证 BCS 反序列化成功

**测试代码结构**:
```rust
#[sim_test]
async fn test_dex_event_indexable() {
    // 1. 启动 TestCluster，执行撮合交易
    // 2. 等待 checkpoint 生成
    // 3. 获取包含该交易的 checkpoint
    // 4. 遍历 events，过滤 DEX_EVENTS_PACKAGE
    // 5. BCS 反序列化为 FillEvent
    // 6. 断言字段值正确
}
```

**测试**:
```bash
cargo simtest -p sui-e2e-tests -- test_dex_event_indexable
```

**验收标准**:
- FillEvent 能从 Checkpoint 正确提取并反序列化
- 字段值（perpetual_id, price, quantity 等）与交易参数匹配

---

### Phase 1.8: 完善其他事件类型
**目标**: 在验证 FillEvent 可索引后，完善其他事件类型

#### 1.8.1 补充事件类型
- [ ] 实现 `PositionUpdateEvent` 结构体
- [ ] 实现 `BalanceUpdateEvent` 结构体
- [ ] 实现 `TransferEvent` 结构体
- [ ] 实现 `FundingSettlementEvent` 结构体
- [ ] 实现 `LiquidationEvent` 结构体

#### 1.8.2 补充事件发射点
- [ ] 在 `deposit_subaccount` 时发射 `BalanceUpdateEvent`
- [ ] 在 `withdraw_subaccount` 时发射 `BalanceUpdateEvent`
- [ ] 在持仓变化时发射 `PositionUpdateEvent`

#### 1.8.3 创建完整事件测试
- [ ] 创建 `dex-sui/crates/sui-e2e-tests/tests/dex_event_tests.rs`
- [ ] 测试各类事件的发射和可索引性

**测试**:
```bash
cargo simtest -p sui-e2e-tests -- dex_event
```

**验收标准**: 所有事件类型都能正确发射和索引

---

### Phase 2: 核心 Handlers（成交数据）
**目标**: 实现最基础的 FillsHandler

#### 2.1 fills 表 Schema
- [ ] 创建 `migrations/002_create_fills.sql`
- [ ] 定义 fills 表结构（参照 V4 spec 5.1）
- [ ] 创建索引：perpetual_time, taker, maker, checkpoint

**测试**: 执行 migration，验证表和索引创建

#### 2.2 FillsHandler 实现
- [ ] 创建 `dex-indexer/src/handlers/fills.rs`
- [ ] 实现 `StoredFill` 结构体
- [ ] 实现 `Processor` trait（从 Checkpoint 提取 FillEvent）
- [ ] 实现 `ConcurrentHandler` trait（写入数据库）
- [ ] 创建 `dex-indexer/src/handlers/mod.rs` 导出

**测试**:
```bash
SUI_SKIP_SIMTESTS=1 cargo nextest run -p dex-indexer --lib -- fills
```

#### 2.3 FillsHandler 集成测试
- [ ] 创建 `dex-indexer/tests/fills_integration.rs`
- [ ] 使用 TestCheckpointBuilder 构造含 FillEvent 的 Checkpoint
- [ ] 验证 Handler 正确解析并写入数据库
- [ ] 验证重复处理的幂等性

**测试**:
```bash
SUI_SKIP_SIMTESTS=1 cargo nextest run -p dex-indexer -- fills_integration
```

**验收标准**: FillEvent 能正确索引到 PostgreSQL fills 表

---

### Phase 3: 核心 Handlers（账户状态）
**目标**: 实现 PositionsHandler 和 BalancesHandler

#### 3.1 positions 表 Schema
- [ ] 创建 `migrations/003_create_positions.sql`
- [ ] 定义 positions 表（当前状态快照）
- [ ] 定义 position_history 表（历史变化）

#### 3.2 PositionsHandler 实现
- [ ] 创建 `dex-indexer/src/handlers/positions.rs`
- [ ] 实现 `StoredPosition` 结构体
- [ ] 实现 `Processor` trait（提取 PositionUpdateEvent）
- [ ] 实现 `SequentialHandler` trait（UPSERT 逻辑）

**测试**:
```bash
SUI_SKIP_SIMTESTS=1 cargo nextest run -p dex-indexer -- positions
```

#### 3.3 balances 表 Schema
- [ ] 创建 `migrations/004_create_balances.sql`
- [ ] 定义 balances 表

#### 3.4 BalancesHandler 实现
- [ ] 创建 `dex-indexer/src/handlers/balances.rs`
- [ ] 实现 `StoredBalance` 结构体
- [ ] 实现 Handler traits

**测试**:
```bash
SUI_SKIP_SIMTESTS=1 cargo nextest run -p dex-indexer -- balances
```

#### 3.5 账户状态集成测试
- [ ] 创建集成测试验证持仓更新
- [ ] 测试余额变化的正确记录
- [ ] 测试 UPSERT 逻辑的正确性

**验收标准**: 持仓和余额状态能正确追踪

---

### Phase 4: 高级 Handlers
**目标**: 实现剩余 Handlers

#### 4.1 CandlesHandler（K线聚合）
- [ ] 创建 `migrations/005_create_candles.sql`
- [ ] 创建 `dex-indexer/src/handlers/candles.rs`
- [ ] 实现 `CandleAggregator` 状态管理
- [ ] 支持多时间粒度：1m, 5m, 15m, 1h, 4h, 1d

**测试**: 验证 K 线聚合逻辑

#### 4.2 FundingHandler（资金费率）
- [ ] 创建 `migrations/006_create_funding_rates.sql`
- [ ] 实现 `FundingSettlementEvent` 处理

#### 4.3 LiquidationsHandler
- [ ] 创建 `migrations/007_create_liquidations.sql`
- [ ] 实现 `LiquidationEvent` 处理

#### 4.4 TransfersHandler
- [ ] 创建 `migrations/008_create_transfers.sql`
- [ ] 实现 `TransferEvent` 处理

**测试**:
```bash
SUI_SKIP_SIMTESTS=1 cargo nextest run -p dex-indexer -- handlers
```

**验收标准**: 所有事件类型都能正确索引

---

### Phase 5: Indexer 主程序
**目标**: 整合所有 Handlers，创建可运行的 Indexer

#### 5.1 主程序入口
- [ ] 完善 `dex-indexer/src/main.rs`
- [ ] 实现配置加载（IndexerConfig）
- [ ] 注册所有 Handlers 到 Pipeline
- [ ] 实现优雅退出

#### 5.2 Indexer 启动测试
- [ ] 创建 Indexer 启动脚本
- [ ] 验证与本地 Sui 节点的连接
- [ ] 验证 Checkpoint 订阅正常

**测试**: 手动启动 Indexer，观察日志

**验收标准**: Indexer 能启动并处理 Checkpoint

---

### Phase 6: REST API 基础
**目标**: 实现核心查询 API

#### 6.1 API Server 框架
- [ ] 创建 `dex-indexer/src/api/server.rs`（Axum server）
- [ ] 创建 `dex-indexer/src/api/types.rs`（请求/响应类型）
- [ ] 实现 POST /info 路由分发

#### 6.2 Info API 实现（Phase 1 核心）
- [ ] `type: "userFills"` - 查询成交记录
- [ ] `type: "clearinghouseState"` - 查询持仓状态
- [ ] `type: "candleSnapshot"` - 查询 K 线
- [ ] `type: "fundingHistory"` - 查询资金费率

**测试**:
```bash
# API 单元测试
SUI_SKIP_SIMTESTS=1 cargo nextest run -p dex-indexer -- api

# 手动 curl 测试
curl -X POST http://localhost:3000/info -d '{"type":"userFills","user":"0x..."}'
```

#### 6.3 API 集成测试
- [ ] 创建 API 集成测试
- [ ] 测试各 endpoint 的正确响应
- [ ] 测试错误处理

**验收标准**: REST API 能正确返回索引数据

---

### Phase 7: 端到端集成
**目标**: 完整的端到端流程验证

#### 7.1 端到端测试
- [ ] 创建 `dex-indexer/tests/e2e_test.rs`
- [ ] 启动 TestCluster + Indexer
- [ ] 执行 DEX 交易（下单、撮合）
- [ ] 验证 API 返回正确数据

#### 7.2 性能基准测试
- [ ] 测试 Handler 处理吞吐量
- [ ] 测试 API 响应延迟
- [ ] 识别性能瓶颈

**验收标准**: 完整流程：DEX 交易 → 事件 → Indexer → API

---

## 3. 验证清单

### 编译验证
```bash
# 全量编译
cargo build -p dex-indexer

# Clippy 检查
cargo xclippy
```

### 测试验证
```bash
# Handler 单元测试
SUI_SKIP_SIMTESTS=1 cargo nextest run -p dex-indexer --lib

# E2E 事件测试
cargo simtest -p sui-e2e-tests -- dex_event

# 集成测试
SUI_SKIP_SIMTESTS=1 cargo nextest run -p dex-indexer -- integration
```

---

## 4. 关键文件清单

### 新增文件
| 文件 | 用途 |
|------|------|
| `sui-types/src/dex_events.rs` | DEX 事件类型定义 |
| `dex-indexer/Cargo.toml` | Indexer crate 配置 |
| `dex-indexer/src/main.rs` | Indexer 入口 |
| `dex-indexer/src/handlers/*.rs` | 各 Handler 实现 |
| `dex-indexer/src/api/*.rs` | REST API 实现 |
| `dex-indexer/migrations/*.sql` | 数据库迁移 |

### 修改文件
| 文件 | 修改内容 |
|------|----------|
| `sui-execution/src/dex.rs` | 添加事件发射逻辑 |
| `sui-types/src/lib.rs` | 导出 dex_events 模块 |
| `dex-sui/Cargo.toml` | 注册 dex-indexer 到 workspace |

---

## 5. 阶段总结输出

每个阶段完成后，在 `sui/mynotes/dex/summary/` 创建总结文档：

- `phase-0-infrastructure-summary.md`
- `phase-1-event-system-summary.md`
- `phase-2-fills-handler-summary.md`
- `phase-3-account-handlers-summary.md`
- `phase-4-advanced-handlers-summary.md`
- `phase-5-indexer-main-summary.md`
- `phase-6-rest-api-summary.md`
- `phase-7-e2e-integration-summary.md`

总结内容包括：
1. 完成的任务清单
2. 遇到的问题和解决方案
3. 测试结果
4. 下一阶段准备事项

---

## 6. 风险与缓解

| 风险 | 缓解措施 |
|------|----------|
| sui-indexer-alt-framework API 变化 | 固定依赖版本，跟踪上游变化 |
| 事件发射破坏共识 | 先在测试环境验证，确保 events_digest 一致 |
| 性能瓶颈 | 使用批量写入，合理设置 Pipeline 并发度 |
| 数据库锁竞争 | Sequential Handler 仅用于有状态依赖的场景 |

---

## 7. 当前状态

**更新日期**: 2026-02-03

- [x] Phase 0: 基础设施准备 - **已完成** ✅
  - dex-indexer crate 已创建
  - 数据库 migrations 已建立 (dex_fills, dex_balances)
  - 测试数据库连接正常
- [x] Phase 1.1: FillEvent 类型定义 - **已完成** ✅
  - 创建 `sui-types/src/dex_events.rs`
  - 定义 `DEX_EVENTS_PACKAGE` 虚拟地址
  - 实现 `FillEvent` 结构体和 `to_sui_event()` 方法
  - 4 个单元测试全部通过
- [x] Phase 1.2: FillEvent 发射集成 - **已完成** ✅
  - 修改 `sui-execution/src/dex.rs`
  - 在订单撮合时创建 `FillEvent`
  - 计算 `events_digest` 并包含在 `TransactionEffects`
  - 将事件放入 `InnerTemporaryStore.events`
- [ ] Phase 1.3: 下单测试验证 - **已阻塞** ⚠️
  - simtest 有预先存在的版本问题: "version must be monotonically increasing (0x2 < 0x2)"
  - 此问题与事件发射无关，是 DEX 执行层的预先存在问题
  - 单元测试（dex_events）已验证事件序列化正确
- [ ] Phase 1.5: 事件索引可行性验证 ⭐ **被同一 simtest 问题阻塞**
- [x] Phase 1.8: 完善其他事件类型 - **大部分完成**
  - [x] `BalanceUpdateEvent` - 已实现并验证通过 ✅
  - [x] 在 `deposit_subaccount` 时发射 `BalanceUpdateEvent` ✅
  - [x] 在 `withdraw_subaccount` 时发射 `BalanceUpdateEvent` ✅
  - [x] `PositionUpdateEvent` - 已实现 ✅ (2026-02-03，订单撮合时发射)
  - [ ] `FundingSettlementEvent` - 待实现
  - [ ] `LiquidationEvent` - 待实现
- [ ] Phase 2: 核心 Handlers（成交数据） - **部分完成**
  - [x] fills 表 Schema ✅
  - [x] FillsHandler 实现 ✅ (代码完成，但 FillEvent 阻塞无法验证)
- [x] Phase 3: 核心 Handlers（账户状态） - **已完成** ✅
  - [x] balances 表 Schema ✅
  - [x] BalancesHandler 实现 ✅
  - [x] BalanceUpdateEvent 全流程验证通过 ✅ (2026-02-02)
  - [x] positions 表 Schema ✅ (2026-02-03，migration 2026-02-03-000001_dex_positions)
  - [x] PositionsHandler 实现 ✅ (2026-02-03，双表设计：dex_positions + dex_position_updates)
- [ ] Phase 4: 高级 Handlers - **待开始**
- [x] Phase 5: Indexer 主程序 - **已完成** ✅
  - [x] main.rs 入口 ✅
  - [x] api_main.rs 入口 ✅
  - [x] 配置加载 ✅
- [x] Phase 6: REST API 基础 - **大部分完成**
  - [x] Axum server 框架 ✅
  - [x] POST /info 路由分发 ✅
  - [x] `type: "userBalances"` ✅
  - [x] `type: "userFills"` ✅ (代码完成，格式需改为 Hyperliquid 兼容)
  - [x] `type: "clearinghouseState"` ✅ (2026-02-03，⚠️ 计算值为占位符，需完善)
  - [ ] `type: "candleSnapshot"` - 待实现
- [ ] Phase 7: 端到端集成 - **部分完成**
  - [x] BalanceUpdateEvent 端到端验证 ✅
  - [ ] FillEvent 端到端验证 - 阻塞

### 待解决问题

1. **Simtest 版本问题**: DEX 执行层在 checkpoint 执行时出现版本不递增错误
   - 错误位置: `sui-core/src/execution_cache/cache_types.rs:54`
   - 错误信息: "version must be monotonically increasing (0x2 < 0x2)"
   - 影响范围: 所有 DEX simtest（`test_dex_place_limit_order`, `test_dex_order_matching` 等）
   - 需要单独调查并修复

2. **FillEvent 端到端验证阻塞**: 依赖 simtest 问题修复

### 已验证可工作的功能

1. **BalanceUpdateEvent 全流程** (2026-02-02)
   - Deposit/Withdraw → 事件发射 → Checkpoint → Indexer → PostgreSQL → API
   - 通过本地节点 + place_order 测试验证

---

## 8. 执行顺序建议

**关键路径**：先验证事件机制可行，再构建完整 Indexer

```
Phase 0 (crate 骨架)
    │
    ▼
Phase 1 (FillEvent 最小版本)
    │
    ├──────────────────────┐
    ▼                      │
Phase 1.5 (索引验证) ⭐    │ ← 如果验证失败，在此调整
    │                      │
    ▼                      │
[验证通过]                 │
    │                      │
    ▼                      │
Phase 1.8 (完善事件)       │
    │                      │
    ▼                      │
Phase 2-7 (完整 Indexer)   │
```

**如果 Phase 1.5 验证失败**：
- 检查 events_digest 计算是否正确
- 检查 TransactionEvents 是否正确包含事件
- 检查 BCS 序列化/反序列化是否一致

---

## 9. Phase 8: API 完善与 Hyperliquid 兼容

> 基于 2026-02-03 设计审查结论添加

### 9.1 审查结论

| 维度 | 评分 | 主要问题 |
|------|------|---------|
| 事件设计 | 7.3/10 | 缺少 asset_id、total_filled、funding_index |
| 数据库 Schema | 7.5/10 | 缺少资产持仓快照表、PnL 字段 |
| API 设计 | 4.6/10 | 端点覆盖率仅 14%，响应格式不兼容 |

### 9.2 紧急修复（阻塞前端开发）

#### 9.2.1 API 计算值实现
- [ ] clearinghouseState 实现真实 margin_summary 计算（非占位符 "0"）
- [ ] clearinghouseState 实现 position_value、unrealized_pnl、liquidation_px 计算
- [ ] 添加 MarginSummary.total_margin_used 字段
- [ ] 添加 PositionInfo.max_leverage、cum_funding 字段

#### 9.2.2 高优先级端点实现
- [ ] `meta` / `metaAndAssetCtxs` - 市场元数据
- [ ] `l2Book` - 订单簿深度
- [ ] `openOrders` / `historicalOrders` - 订单查询
- [ ] `userFunding` / `fundingHistory` - 资金费率

#### 9.2.3 响应格式兼容
- [ ] 统一错误响应格式为 `{status: "ok"/"err", response: ...}`
- [ ] userFills 重构为 Hyperliquid 兼容格式（字段名、数据类型）

### 9.3 高优先级（核心功能）

#### 9.3.1 事件字段补全
- [ ] TransferEvent 添加 `asset_id: u32`
- [ ] FillEvent 添加 `total_filled_taker: u64, total_filled_maker: u64`
- [ ] FundingSettlementEvent 添加 `funding_index: i128`
- [ ] LiquidationEvent 添加 `is_final_settlement: bool`

#### 9.3.2 数据库 Schema 补全
- [ ] 创建 `dex_asset_positions` 表（当前资产持仓快照）
- [ ] dex_positions 添加 `realized_pnl`, `settled_funding` 字段
- [ ] 添加索引 `dex_fills_perpetual_timestamp_idx`

### 9.4 中优先级（完善功能）

- [ ] 实现 `candleSnapshot` K 线数据端点
- [ ] 实现 `spotClearinghouseState` 现货余额端点
- [ ] dex_fills 添加 builder_fee、affiliate_fee 字段
