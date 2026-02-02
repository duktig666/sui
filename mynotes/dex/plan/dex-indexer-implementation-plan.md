# DEX Indexer 细粒度实施规划

> 基于 `dex-indexer-tech-v3.md` 技术方案，在 `dex-sui` 仓库下实施

## 背景

- **技术方案**: `sui/mynotes/dex/tech/dex-indexer-tech-v3.md`
- **实施位置**: `dex-sui/crates/dex-indexer-*`
- **现有依赖**: DEX 引擎已实现（参考 `dex_order_tests.rs`, `dex_subaccount_tests.rs`）
- **可复用**: `sui-indexer-alt-framework` 已存在于 dex-sui

---

## 测试环境策略

### PostgreSQL 依赖分析

| Phase | 是否需要 PostgreSQL | 说明 |
|-------|-------------------|------|
| 1.1 项目初始化 | ❌ | 仅编译检查 |
| 1.2 Proto 定义 | ❌ | 编译 + 序列化测试 |
| 1.3 Types 定义 | ❌ | 编译 + serde 测试 |
| 1.4 Schema 定义 | ✅ | migration + 插入/查询测试 |
| 2.1-2.6 Handlers | ✅ | 集成测试需要写入 DB |
| 3.x REST API | ✅ | API 需要查询 DB |
| 4.x 集成测试 | ✅ | 端到端测试 |

### 推荐方案：TestContainers 自动管理

使用 `testcontainers-rs` crate 在测试中自动启动/停止 PostgreSQL：

```rust
// dex-indexer-schema/tests/common/mod.rs
use testcontainers::{clients::Cli, images::postgres::Postgres};

pub async fn setup_test_db() -> (Container<Postgres>, PgPool) {
    let docker = Cli::default();
    let container = docker.run(Postgres::default());

    let connection_string = format!(
        "postgres://postgres:postgres@localhost:{}/postgres",
        container.get_host_port_ipv4(5432)
    );

    let pool = PgPool::connect(&connection_string).await.unwrap();

    // 运行 migrations
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();

    (container, pool)
}
```

### 测试命令分类

```bash
# 不需要 PostgreSQL 的测试（Phase 1.1-1.3）
cargo test -p dex-indexer-proto
cargo test -p dex-indexer-types

# 需要 PostgreSQL 的测试（Phase 1.4+）
# TestContainers 会自动启动 Docker 容器
cargo test -p dex-indexer-schema --features test-containers
cargo test -p dex-indexer-handlers --features test-containers
cargo test -p dex-indexer-api --features test-containers
```

### Cargo.toml 配置

```toml
# dex-indexer-schema/Cargo.toml
[dev-dependencies]
testcontainers = "0.15"
testcontainers-modules = { version = "0.3", features = ["postgres"] }

[features]
test-containers = []
```

### 本地开发选项

如果不想每次测试都启动容器，可以手动启动一个持久 PostgreSQL：

```bash
# 一次性启动（保持运行）
docker run -d --name dex-indexer-test-db \
  -p 5432:5432 \
  -e POSTGRES_PASSWORD=password \
  -e POSTGRES_DB=dex_indexer_test \
  postgres:15

# 设置环境变量后运行测试
DATABASE_URL=postgres://postgres:password@localhost/dex_indexer_test \
  cargo test -p dex-indexer-schema
```

---

## 阶段完成工作流

> **重要**: 每个阶段完成后必须执行以下流程，确认通过后才能进入下一阶段。

### 工作流程

```
开始阶段 → 实现任务 → 运行测试 → Code Review → 更新完成状态 → 进入下一阶段
    ↑                              ↓
    └──── 修复问题 ←───── 发现问题 ←┘
```

### 阶段完成检查步骤

1. **运行测试**
   ```bash
   # 运行该阶段所有相关测试
   cargo test -p <package-name>

   # 如果有集成测试
   cargo test -p <package-name> --features test-containers
   ```

2. **Code Review**
   - 检查代码是否符合项目规范
   - 验证是否有未处理的 TODO 或 FIXME
   - 确认 clippy 无警告: `cargo xclippy`
   - 确认格式正确: `cargo fmt --check`

3. **更新完成状态**
   - 在本文档的「任务完成状态追踪」表格中更新状态
   - 标记为 ✅ 已完成 或 ⏳ 进行中

4. **记录问题（如有）**
   - 在阶段对应的「问题记录」区域记录遇到的问题和解决方案

---

## 任务完成状态追踪

> 每个子任务完成后更新状态，使用以下标记：
> - ⬜ 未开始
> - ⏳ 进行中
> - ✅ 已完成
> - ❌ 阻塞

### Phase 1: 基础设施

| 任务 | 状态 | 完成日期 | 备注 |
|------|------|----------|------|
| 1.1 项目初始化 | ⬜ | | |
| 1.2 Proto 定义 | ⬜ | | |
| 1.3 Types 定义 | ⬜ | | |
| 1.4 Schema 定义 | ⬜ | | |

### Phase 2: 核心框架

| 任务 | 状态 | 完成日期 | 备注 |
|------|------|----------|------|
| 2.1 gRPC 客户端 | ⬜ | | |
| 2.2 Pipeline 封装 | ⬜ | | |
| 2.3 Fills Handler | ⬜ | | |
| 2.4 Positions & Balances | ⬜ | | |
| 2.5 Candles Handler | ⬜ | | |
| 2.6 Funding & Others | ⬜ | | |

### Phase 3: REST API

| 任务 | 状态 | 完成日期 | 备注 |
|------|------|----------|------|
| 3.1 API 基础设施 | ⬜ | | |
| 3.2 Info API - 市场数据 | ⬜ | | |
| 3.3 Info API - 用户数据 | ⬜ | | |
| 3.4 Info API - 市场历史 | ⬜ | | |
| 3.5 Exchange API | ⬜ | | |

### Phase 4: 集成与主程序

| 任务 | 状态 | 完成日期 | 备注 |
|------|------|----------|------|
| 4.1 主程序入口 | ⬜ | | |
| 4.2 端到端集成测试 | ⬜ | | |
| 4.3 与现有 DEX 测试集成 | ⬜ | | |

### Phase 5: DEX Engine 事件发送端

| 任务 | 状态 | 完成日期 | 备注 |
|------|------|----------|------|
| 5.1 gRPC 服务端骨架 | ⬜ | | |
| 5.2 下单事件发送（先验证）| ⬜ | | |
| 5.3 完善其他事件发送 | ⬜ | | |

### Phase 6: 部署与运维

| 任务 | 状态 | 完成日期 | 备注 |
|------|------|----------|------|
| 6.1 Docker Compose 配置 | ⬜ | | |
| 6.2 分区表自动管理 | ⬜ | | |
| 6.3 Prometheus + Grafana | ⬜ | | |

### Phase 7: API 文档与 SDK

| 任务 | 状态 | 完成日期 | 备注 |
|------|------|----------|------|
| 7.1 OpenAPI 规范生成 | ⬜ | | |
| 7.2 TypeScript SDK 示例 | ⬜ | | |

---

## Phase 1: 基础设施（第 1-2 周）

### 1.1 项目初始化（2天）

**目标**: 创建 crate 骨架，配置 workspace

**步骤**:
1. 在 `dex-sui/crates/` 下创建目录:
   ```
   dex-indexer-proto/
   dex-indexer-types/
   dex-indexer-schema/
   dex-indexer-framework/
   dex-indexer-handlers/
   dex-indexer-api/
   dex-indexer/
   ```
2. 配置各 crate 的 `Cargo.toml`
3. 更新 workspace `Cargo.toml` 添加新 crates
4. 创建基本的 `lib.rs` / `main.rs` 骨架

**测试验证**:
```bash
# 编译通过
cargo check -p dex-indexer-proto -p dex-indexer-types -p dex-indexer-schema \
            -p dex-indexer-framework -p dex-indexer-handlers -p dex-indexer-api \
            -p dex-indexer
```

**关键文件**:
- `dex-sui/Cargo.toml` (workspace 配置)
- `dex-sui/crates/dex-indexer-*/Cargo.toml`

---

### 1.2 Proto 定义（2天）

**目标**: 定义 gRPC 事件和服务接口

**步骤**:
1. 创建 `proto/dex/indexer/v1/events.proto`:
   - FillEvent, PositionUpdateEvent, BalanceUpdateEvent
   - FundingRateEvent, LiquidationEvent, TransferEvent
   - MarketUpdateEvent
2. 创建 `proto/dex/indexer/v1/service.proto`:
   - OnChainEventBatch, OffChainEventBatch
   - DexEventService (SubscribeOnChainEvents, GetLatestCheckpoint)
3. 配置 `build.rs` 使用 prost-build
4. 生成 Rust 代码并导出

**测试验证**:
```bash
# 编译并生成代码
cargo build -p dex-indexer-proto

# 单元测试 - 序列化/反序列化
cargo test -p dex-indexer-proto
```

**关键文件**:
- `dex-indexer-proto/proto/dex/indexer/v1/events.proto`
- `dex-indexer-proto/proto/dex/indexer/v1/service.proto`
- `dex-indexer-proto/build.rs`
- `dex-indexer-proto/src/lib.rs`

---

### 1.3 Types 定义（2天）

**目标**: 定义 Rust 类型，对标 Hyperliquid API

**步骤**:
1. 实现 `events.rs`:
   - 从 proto 转换的事件类型
   - `From<proto::FillEvent>` 等实现
2. 实现 `models/`:
   - `market.rs`: PerpAsset, SpotAsset, AssetCtx
   - `order.rs`: OpenOrder, FrontendOrder
   - `fill.rs`: UserFill
   - `position.rs`: AssetPosition, PositionInfo
   - `candle.rs`: Candle
   - `funding.rs`: FundingHistory
3. 实现 `api/`:
   - `info.rs`: InfoRequest 枚举（20+ types）
   - `exchange.rs`: ExchangeRequest 枚举
4. 实现 `enums.rs`: Side, LeverageType, OrderStatus, TimeInForce

**测试验证**:
```bash
# 编译
cargo check -p dex-indexer-types

# 单元测试 - serde 序列化
cargo test -p dex-indexer-types

# 验证 InfoRequest 枚举覆盖所有 type
# 在测试中检查 "meta", "l2Book" 等 type 可正确反序列化
```

**关键文件**:
- `dex-indexer-types/src/api/info.rs`
- `dex-indexer-types/src/models/*.rs`

---

### 1.4 Schema 定义（3天）

**目标**: 定义数据库表结构和 Diesel 模型

**步骤**:
1. 创建 migrations:
   ```sql
   -- 00000000000000_init/up.sql
   -- markets, orders, fills, perpetual_positions
   -- balances, candles, funding_rates, user_funding_records
   -- transfers, liquidations, dex_watermarks
   ```
2. 实现 Diesel schema (`schema.rs`)
3. 实现各表的 Row 模型:
   - `FillRow`, `PositionRow`, `BalanceRow`
   - `CandleRow`, `FundingRateRow`, `WatermarkRow`
4. 实现 queries 模块:
   - `fills.rs`: 按用户/市场/时间查询
   - `positions.rs`: 按用户查询
5. 设置分区表（fills 按天，candles 按月）

**测试验证**:
```bash
# 本地启动 PostgreSQL
docker run -d -p 5432:5432 -e POSTGRES_PASSWORD=password postgres:15

# 运行 migration
DATABASE_URL=postgres://postgres:password@localhost/dex_indexer \
  diesel migration run

# 验证表创建
psql -h localhost -U postgres -d dex_indexer -c "\dt"

# 单元测试 - 插入和查询
cargo test -p dex-indexer-schema
```

**关键文件**:
- `dex-indexer-schema/migrations/*/up.sql`
- `dex-indexer-schema/src/schema.rs`
- `dex-indexer-schema/src/models/*.rs`

---

## Phase 2: 核心框架（第 2-3 周）

### 2.1 Framework - gRPC 客户端（3天）

**目标**: 实现 DEX Engine gRPC 事件订阅客户端

**步骤**:
1. 定义 `DexEventSource` trait:
   ```rust
   pub trait DexEventSource {
       async fn subscribe(&self, from_checkpoint: u64)
           -> impl Stream<Item = OnChainEventBatch>;
   }
   ```
2. 实现 `GrpcEventClient`:
   - 连接管理（自动重连）
   - 心跳检测
   - 背压控制
3. 实现 Mock 客户端（用于测试）:
   - `MockEventSource` 生成测试数据
4. 配置模块 `config.rs`

**测试验证**:
```bash
# 单元测试 - Mock 客户端
cargo test -p dex-indexer-framework --lib

# 集成测试 - 连接 Mock gRPC Server（需要先实现 Mock Server）
cargo test -p dex-indexer-framework --test grpc_client_test
```

**关键文件**:
- `dex-indexer-framework/src/ingestion/grpc_client.rs`
- `dex-indexer-framework/src/ingestion/mock.rs`

---

### 2.2 Framework - Pipeline 封装（3天）

**目标**: 封装 sui-indexer-alt-framework 的 Pipeline

**步骤**:
1. 定义 `DexProcessor` trait（泛化版本）:
   ```rust
   pub trait DexProcessor: Send + Sync {
       type Output: Send;
       fn process(&self, batch: &OnChainEventBatch) -> Vec<Self::Output>;
   }
   ```
2. 实现 `DexPipeline`:
   - 封装 sui-indexer-alt-framework 的 concurrent pipeline
   - 适配 `OnChainEventBatch` 作为输入
3. 实现 `DexStore` trait:
   - 封装 PostgreSQL 连接池
   - 实现 `Connection` trait
4. 实现 Watermark 管理:
   - 复用 sui-indexer-alt 的 watermark 机制

**测试验证**:
```bash
# 单元测试
cargo test -p dex-indexer-framework --lib

# 集成测试 - Pipeline + Mock Source + Test DB
DATABASE_URL=postgres://localhost/dex_indexer_test \
  cargo test -p dex-indexer-framework --test pipeline_test
```

**关键文件**:
- `dex-indexer-framework/src/pipeline/processor.rs`
- `dex-indexer-framework/src/pipeline/mod.rs`
- `dex-indexer-framework/src/store/postgres.rs`

---

### 2.3 Handlers 实现 - Fills（2天）

**目标**: 实现成交记录处理器

**步骤**:
1. 实现 `FillsHandler`:
   ```rust
   impl DexProcessor for FillsHandler {
       type Output = FillRow;
       fn process(&self, batch: &OnChainEventBatch) -> Vec<FillRow> {
           batch.fills.iter().map(FillRow::from).collect()
       }
   }
   ```
2. 实现批量写入（ON CONFLICT DO NOTHING）
3. 实现 Watermark 更新

**测试验证**:
```bash
# 单元测试 - 事件转换
cargo test -p dex-indexer-handlers fills::

# 集成测试 - 写入 PostgreSQL
DATABASE_URL=postgres://localhost/dex_indexer_test \
  cargo test -p dex-indexer-handlers --test fills_integration
```

**关键文件**:
- `dex-indexer-handlers/src/fills.rs`

---

### 2.4 Handlers 实现 - Positions & Balances（2天）

**目标**: 实现持仓和余额处理器

**步骤**:
1. 实现 `PositionsHandler`:
   - 处理 PositionUpdateEvent
   - Upsert 逻辑（按 owner + market_id）
2. 实现 `BalancesHandler`:
   - 处理 BalanceUpdateEvent
   - Upsert 逻辑（按 owner + asset）

**测试验证**:
```bash
# 单元测试
cargo test -p dex-indexer-handlers positions::
cargo test -p dex-indexer-handlers balances::

# 集成测试
DATABASE_URL=postgres://localhost/dex_indexer_test \
  cargo test -p dex-indexer-handlers --test positions_integration
```

**关键文件**:
- `dex-indexer-handlers/src/positions.rs`
- `dex-indexer-handlers/src/balances.rs`

---

### 2.5 Handlers 实现 - Candles（3天）

**目标**: 实现 K 线聚合处理器

**步骤**:
1. 实现 `CandleAggregator`:
   - 按时间窗口聚合 Fills
   - 支持 1m, 5m, 15m, 1h, 4h, 1d 周期
   - OHLCV 计算
2. 实现 `CandlesHandler`:
   - 接收 FillEvent，更新对应周期的 Candle
   - 处理跨周期边界情况
3. 实现 Candle 写入（Upsert by market_id + interval + open_time）

**测试验证**:
```bash
# 单元测试 - K 线聚合逻辑
cargo test -p dex-indexer-handlers candles::aggregator

# 集成测试 - 多周期 K 线
DATABASE_URL=postgres://localhost/dex_indexer_test \
  cargo test -p dex-indexer-handlers --test candles_integration
```

**关键文件**:
- `dex-indexer-handlers/src/candles.rs`

---

### 2.6 Handlers 实现 - Funding & Others（2天）

**目标**: 实现资金费率、转账、清算处理器

**步骤**:
1. 实现 `FundingHandler`:
   - 处理 FundingRateEvent
   - 写入 funding_rates 和 user_funding_records
2. 实现 `TransfersHandler`:
   - 处理 TransferEvent
3. 实现 `LiquidationsHandler`:
   - 处理 LiquidationEvent

**测试验证**:
```bash
# 单元测试
cargo test -p dex-indexer-handlers funding::
cargo test -p dex-indexer-handlers transfers::
cargo test -p dex-indexer-handlers liquidations::
```

**关键文件**:
- `dex-indexer-handlers/src/funding.rs`
- `dex-indexer-handlers/src/transfers.rs`
- `dex-indexer-handlers/src/liquidations.rs`

---

## Phase 3: REST API（第 3-4 周）

### 3.1 API 基础设施（2天）

**目标**: 搭建 Axum HTTP 服务器框架

**步骤**:
1. 实现 `server.rs`:
   - Axum Router 配置
   - 状态管理（DB Pool）
   - 中间件（logging, metrics）
2. 实现路由:
   - `POST /info` → `routes/info.rs`
   - `POST /exchange` → `routes/exchange.rs`
3. 实现错误处理:
   - 统一错误响应格式

**测试验证**:
```bash
# 单元测试 - 路由配置
cargo test -p dex-indexer-api --lib

# 启动服务器
cargo run -p dex-indexer-api -- --port 8080

# 手动测试
curl -X POST http://localhost:8080/info -d '{"type":"meta"}'
```

**关键文件**:
- `dex-indexer-api/src/server.rs`
- `dex-indexer-api/src/routes/info.rs`
- `dex-indexer-api/src/error.rs`

---

### 3.2 Info API - 市场数据（2天）

**目标**: 实现市场元数据查询

**步骤**:
1. 实现 `handlers/info/meta.rs`:
   - type=meta → 返回永续合约配置
   - type=metaAndAssetCtxs → 返回配置 + 实时数据
2. 实现 `handlers/info/spot.rs`:
   - type=spotMeta
   - type=spotMetaAndAssetCtxs
3. 实现 `handlers/info/mids.rs`:
   - type=allMids → 返回所有中间价

**测试验证**:
```bash
# 集成测试
DATABASE_URL=postgres://localhost/dex_indexer_test \
  cargo test -p dex-indexer-api --test info_meta

# 手动测试
curl -X POST http://localhost:8080/info \
  -H "Content-Type: application/json" \
  -d '{"type":"meta"}'
```

**关键文件**:
- `dex-indexer-api/src/handlers/info/meta.rs`

---

### 3.3 Info API - 用户数据（3天）

**目标**: 实现用户账户状态查询

**步骤**:
1. 实现 `handlers/info/clearinghouse.rs`:
   - type=clearinghouseState → 永续账户状态
   - type=spotClearinghouseState → 现货余额
2. 实现 `handlers/info/orders.rs`:
   - type=openOrders → 当前挂单（简化）
   - type=frontendOpenOrders → 当前挂单（完整）
   - type=historicalOrders → 历史订单
   - type=orderStatus → 单个订单状态
3. 实现 `handlers/info/fills.rs`:
   - type=userFills → 成交记录
   - type=userFillsByTime → 按时间查询

**测试验证**:
```bash
# 集成测试
DATABASE_URL=postgres://localhost/dex_indexer_test \
  cargo test -p dex-indexer-api --test info_user

# 手动测试
curl -X POST http://localhost:8080/info \
  -H "Content-Type: application/json" \
  -d '{"type":"clearinghouseState","user":"0x..."}'
```

**关键文件**:
- `dex-indexer-api/src/handlers/info/clearinghouse.rs`
- `dex-indexer-api/src/handlers/info/orders.rs`
- `dex-indexer-api/src/handlers/info/fills.rs`

---

### 3.4 Info API - 市场历史数据（2天）

**目标**: 实现 K 线和资金费率查询

**步骤**:
1. 实现 `handlers/info/candle.rs`:
   - type=candleSnapshot → K线数据
   - 支持 interval, startTime, endTime 参数
2. 实现 `handlers/info/funding.rs`:
   - type=fundingHistory → 资金费率历史
   - type=userFunding → 用户资金费记录
   - type=predictedFundings → 预测资金费率
3. 实现 `handlers/info/trades.rs`:
   - type=recentTrades → 最近成交

**测试验证**:
```bash
# 集成测试
DATABASE_URL=postgres://localhost/dex_indexer_test \
  cargo test -p dex-indexer-api --test info_market

# 手动测试 K 线
curl -X POST http://localhost:8080/info \
  -H "Content-Type: application/json" \
  -d '{"type":"candleSnapshot","coin":"BTC","interval":"1h","startTime":1706400000000}'
```

**关键文件**:
- `dex-indexer-api/src/handlers/info/candle.rs`
- `dex-indexer-api/src/handlers/info/funding.rs`

---

### 3.5 Exchange API（2天）

**目标**: 实现交易 API（转发到 DEX Engine）

**步骤**:
1. 实现 `handlers/exchange/order.rs`:
   - action.type=order → 下单（转发）
   - action.type=cancel → 撤单
   - action.type=cancelByCloid → 按 cloid 撤单
2. 实现 `handlers/exchange/leverage.rs`:
   - action.type=updateLeverage
   - action.type=updateIsolatedMargin

**测试验证**:
```bash
# 单元测试 - 请求解析
cargo test -p dex-indexer-api --lib exchange::

# 集成测试（需要 Mock DEX Engine）
cargo test -p dex-indexer-api --test exchange_integration
```

**关键文件**:
- `dex-indexer-api/src/handlers/exchange/order.rs`

---

## Phase 4: 集成与主程序（第 4 周）

### 4.1 主程序入口（2天）

**目标**: 实现 dex-indexer 主程序

**步骤**:
1. 实现 `config.rs`:
   - TOML 配置文件解析
   - 环境变量覆盖
2. 实现 `main.rs`:
   - 初始化 gRPC 客户端
   - 初始化 DB 连接池
   - 启动所有 Pipeline
   - 启动 REST API 服务器
3. 实现优雅关闭:
   - Signal handler
   - 等待 Pipeline 完成当前批次

**测试验证**:
```bash
# 编译主程序
cargo build -p dex-indexer

# 启动（配置文件模式）
./target/debug/dex-indexer --config config.toml

# 启动（环境变量模式）
DATABASE_URL=postgres://localhost/dex_indexer \
DEX_ENGINE_GRPC_URL=http://localhost:50051 \
./target/debug/dex-indexer
```

**关键文件**:
- `dex-indexer/src/main.rs`
- `dex-indexer/src/config.rs`

---

### 4.2 端到端集成测试（3天）

**目标**: 验证完整数据流

**步骤**:
1. 创建 `dex-indexer-e2e-tests` crate
2. 实现 Mock DEX Engine gRPC Server
3. 编写端到端测试:
   - 启动 PostgreSQL（TestContainers）
   - 启动 Mock gRPC Server
   - 启动 dex-indexer
   - 发送 Mock 事件
   - 验证 API 返回数据

**测试验证**:
```bash
# 端到端测试
cargo test -p dex-indexer-e2e-tests --test e2e

# 测试场景:
# 1. 事件流 → DB 写入 → API 查询
# 2. Watermark 断点续传
# 3. 重复事件幂等性
```

**关键文件**:
- `dex-indexer-e2e-tests/tests/e2e.rs`

---

### 4.3 与现有 DEX 测试集成（2天）

**目标**: 将 Indexer 集成到现有 DEX 测试框架

**步骤**:
1. 修改 `test_cluster` 支持启动 Indexer
2. 在 `dex_order_tests.rs` 中添加 Indexer 验证:
   - 下单后验证 Indexer 中有对应记录
   - 撤单后验证订单状态更新
3. 在 `dex_subaccount_tests.rs` 中添加验证:
   - 充值后验证 Balance 更新
   - 提现后验证 Transfer 记录

**测试验证**:
```bash
# 运行集成测试
cargo simtest -p sui-e2e-tests -- dex_order
cargo simtest -p sui-e2e-tests -- dex_subaccount
```

**关键文件**:
- `sui-e2e-tests/tests/dex_order_tests.rs`
- `sui-e2e-tests/tests/dex_subaccount_tests.rs`

---

## Phase 5: DEX Engine 事件发送端（第 5 周）

### 5.1 gRPC 服务端骨架（2天）

**目标**: 在 DEX Engine 中实现 gRPC 事件服务端

**步骤**:
1. 在 `sui-core` 或新建 `dex-engine-grpc` crate 中添加 gRPC Server:
   ```rust
   pub struct DexEventServer {
       event_rx: mpsc::Receiver<OnChainEventBatch>,
   }

   impl DexEventService for DexEventServer {
       type SubscribeOnChainEventsStream = ReceiverStream<OnChainEventBatch>;

       async fn subscribe_on_chain_events(
           &self,
           request: Request<SubscribeOnChainRequest>,
       ) -> Result<Response<Self::SubscribeOnChainEventsStream>, Status> {
           // 返回事件流
       }
   }
   ```
2. 配置 tonic gRPC 服务器
3. 实现基本的连接管理

**测试验证**:
```bash
# 启动 gRPC Server
cargo run -p dex-engine-grpc -- --port 50051

# 使用 grpcurl 测试
grpcurl -plaintext localhost:50051 dex.indexer.v1.DexEventService/GetLatestCheckpoint
```

**关键文件**:
- `dex-engine-grpc/src/server.rs`
- `dex-engine-grpc/src/service.rs`

---

### 5.2 下单事件发送（先验证）（2天）

**目标**: 实现最小可验证的事件发送 - 仅 FillEvent

**步骤**:
1. 在 DEX 撮合引擎中添加事件发送钩子:
   ```rust
   // 撮合成功后发送 FillEvent
   fn on_fill(&self, fill: &Fill) {
       let event = FillEvent::from(fill);
       self.event_tx.send(OnChainEventBatch {
           checkpoint_sequence: self.current_checkpoint,
           fills: vec![event],
           ..Default::default()
       });
   }
   ```
2. 在 Checkpoint 确认时批量发送事件
3. 实现事件缓冲区（在 Checkpoint 之间暂存）

**测试验证**:
```bash
# 启动完整链路
# 1. 启动 PostgreSQL
docker run -d -p 5432:5432 -e POSTGRES_PASSWORD=password postgres:15

# 2. 启动 DEX Engine (with gRPC)
cargo run -p sui-node -- --with-dex-grpc

# 3. 启动 Indexer
cargo run -p dex-indexer

# 4. 提交下单交易
cargo test -p sui-e2e-tests dex_order_tests::test_place_order

# 5. 验证 Indexer 收到 FillEvent
curl -X POST http://localhost:8080/info \
  -H "Content-Type: application/json" \
  -d '{"type":"recentTrades","coin":"BTC"}'
```

**关键文件**:
- `sui-core/src/dex/matching_engine.rs` (事件钩子)
- `sui-core/src/authority/checkpoint_handler.rs` (Checkpoint 触发)

---

### 5.3 完善其他事件发送（3天）

**目标**: 补充所有 OnChainUpdates 事件类型

**步骤**:
1. PositionUpdateEvent - 持仓变化时发送
2. BalanceUpdateEvent - 余额变化时发送
3. FundingRateEvent - 资金费结算时发送
4. TransferEvent - 充提完成时发送
5. LiquidationEvent - 清算发生时发送

**测试验证**:
```bash
# 验证各事件类型
# 1. 持仓更新
cargo test -p sui-e2e-tests dex_order_tests::test_position_update

# 2. 充值
cargo test -p sui-e2e-tests dex_subaccount_tests::test_deposit

# 3. 验证 API 返回
curl -X POST http://localhost:8080/info \
  -H "Content-Type: application/json" \
  -d '{"type":"clearinghouseState","user":"0x..."}'
```

---

## Phase 6: 部署与运维（第 6 周）

### 6.1 Docker Compose 配置（1天）

**目标**: 提供一键部署方案

**步骤**:
1. 创建 `docker-compose.yml`:
   ```yaml
   version: '3.8'
   services:
     postgres:
       image: postgres:15
       environment:
         POSTGRES_PASSWORD: ${DB_PASSWORD}
         POSTGRES_DB: dex_indexer
       volumes:
         - pg_data:/var/lib/postgresql/data
       ports:
         - "5432:5432"

     dex-indexer:
       build: .
       depends_on:
         - postgres
       environment:
         DATABASE_URL: postgres://postgres:${DB_PASSWORD}@postgres/dex_indexer
         DEX_ENGINE_GRPC_URL: ${DEX_ENGINE_GRPC_URL}
         API_PORT: 8080
       ports:
         - "8080:8080"

   volumes:
     pg_data:
   ```
2. 创建 Dockerfile（多阶段构建）
3. 编写 `.env.example`

**测试验证**:
```bash
# 启动服务
docker-compose up -d

# 检查健康状态
curl http://localhost:8080/health

# 查看日志
docker-compose logs -f dex-indexer
```

**关键文件**:
- `dex-indexer/docker-compose.yml`
- `dex-indexer/Dockerfile`

---

### 6.2 分区表自动管理（1天）

**目标**: 实现分区表自动创建和过期清理

**步骤**:
1. 创建分区管理脚本 `scripts/partition_manager.sql`:
   ```sql
   -- 创建下一个月的分区
   CREATE OR REPLACE FUNCTION create_next_partitions()
   RETURNS void AS $$
   DECLARE
       next_month DATE := DATE_TRUNC('month', NOW()) + INTERVAL '1 month';
       partition_name TEXT;
   BEGIN
       -- fills 按天分区
       FOR i IN 0..30 LOOP
           partition_name := 'fills_' || TO_CHAR(next_month + (i || ' days')::INTERVAL, 'YYYY_MM_DD');
           EXECUTE format(
               'CREATE TABLE IF NOT EXISTS %I PARTITION OF fills
                FOR VALUES FROM (%L) TO (%L)',
               partition_name,
               next_month + (i || ' days')::INTERVAL,
               next_month + ((i+1) || ' days')::INTERVAL
           );
       END LOOP;

       -- candles 按月分区
       partition_name := 'candles_' || TO_CHAR(next_month, 'YYYY_MM');
       EXECUTE format(
           'CREATE TABLE IF NOT EXISTS %I PARTITION OF candles
            FOR VALUES FROM (%L) TO (%L)',
           partition_name,
           next_month,
           next_month + INTERVAL '1 month'
       );
   END;
   $$ LANGUAGE plpgsql;
   ```
2. 配置 pg_cron 定时任务
3. 实现过期分区清理

**测试验证**:
```bash
# 手动执行分区创建
psql -c "SELECT create_next_partitions();"

# 验证分区
psql -c "\dt fills_*"
```

---

### 6.3 Prometheus + Grafana 监控（2天）

**目标**: 配置可观测性基础设施

**步骤**:
1. 在 dex-indexer 中暴露 Prometheus 指标:
   - `dex_indexer_events_processed_total`
   - `dex_indexer_checkpoint_lag`
   - `dex_indexer_api_request_duration_seconds`
   - `dex_indexer_db_query_duration_seconds`
2. 创建 Prometheus 配置 (`prometheus.yml`)
3. 创建 Grafana Dashboard JSON:
   - 事件处理速率
   - Checkpoint 延迟
   - API 响应时间
   - 数据库连接池状态

**测试验证**:
```bash
# 访问指标端点
curl http://localhost:8080/metrics

# 启动 Prometheus + Grafana
docker-compose -f docker-compose.monitoring.yml up -d

# 访问 Grafana
open http://localhost:3000
```

**关键文件**:
- `dex-indexer/monitoring/prometheus.yml`
- `dex-indexer/monitoring/grafana/dashboards/dex-indexer.json`

---

## Phase 7: API 文档与 SDK（第 6 周）

### 7.1 OpenAPI 规范生成（1天）

**目标**: 自动生成 API 文档

**步骤**:
1. 使用 `utoipa` crate 为 API 添加 OpenAPI 注解:
   ```rust
   #[utoipa::path(
       post,
       path = "/info",
       request_body = InfoRequest,
       responses(
           (status = 200, description = "Success", body = InfoResponse),
           (status = 400, description = "Bad Request")
       )
   )]
   async fn info_handler(Json(req): Json<InfoRequest>) -> impl IntoResponse {
       // ...
   }
   ```
2. 生成 `openapi.json`
3. 集成 Swagger UI (`/docs`)

**测试验证**:
```bash
# 访问 Swagger UI
open http://localhost:8080/docs

# 下载 OpenAPI 规范
curl http://localhost:8080/openapi.json > openapi.json
```

**关键文件**:
- `dex-indexer-api/src/docs.rs`

---

### 7.2 TypeScript SDK 示例（2天）

**目标**: 提供前端集成示例

**步骤**:
1. 创建 `sdk/typescript/` 目录
2. 基于 OpenAPI 生成类型定义:
   ```bash
   npx openapi-typescript openapi.json -o src/types.ts
   ```
3. 实现基础客户端:
   ```typescript
   // sdk/typescript/src/client.ts
   export class DexIndexerClient {
     constructor(private baseUrl: string) {}

     async getMeta(): Promise<MetaResponse> {
       return this.info({ type: 'meta' });
     }

     async getUserFills(user: string): Promise<UserFill[]> {
       return this.info({ type: 'userFills', user });
     }

     private async info<T>(request: InfoRequest): Promise<T> {
       const res = await fetch(`${this.baseUrl}/info`, {
         method: 'POST',
         headers: { 'Content-Type': 'application/json' },
         body: JSON.stringify(request),
       });
       return res.json();
     }
   }
   ```
4. 编写使用示例

**测试验证**:
```bash
# 安装依赖
cd sdk/typescript && npm install

# 运行示例
npx ts-node examples/basic.ts
```

**关键文件**:
- `sdk/typescript/src/client.ts`
- `sdk/typescript/examples/basic.ts`

---

## 验证检查清单

### Phase 1 完成标准
- [ ] 所有 crates 编译通过
- [ ] Proto 生成代码正确
- [ ] Types 序列化测试通过
- [ ] Schema 迁移执行成功
- [ ] 表结构与 DDL 一致

### Phase 2 完成标准
- [ ] gRPC 客户端可连接 Mock Server
- [ ] Pipeline 可处理 Mock 事件批次
- [ ] 所有 Handlers 单元测试通过
- [ ] 数据正确写入 PostgreSQL
- [ ] Watermark 断点续传工作正常

### Phase 3 完成标准
- [ ] REST API 启动成功
- [ ] 所有 Info API type 实现
- [ ] Exchange API 转发正常
- [ ] API 响应格式对标 Hyperliquid
- [ ] 性能满足 P99 < 100ms

### Phase 4 完成标准
- [ ] 主程序可启动运行
- [ ] 端到端测试全部通过
- [ ] 与现有 DEX 测试集成
- [ ] 优雅关闭正常工作

### Phase 5 完成标准
- [ ] gRPC Server 可启动并接受连接
- [ ] FillEvent 端到端验证通过（Engine → gRPC → Indexer → DB → API）
- [ ] 所有事件类型（Position/Balance/Funding/Transfer/Liquidation）发送正常
- [ ] Checkpoint 批量发送机制工作正常
- [ ] 事件缓冲区在异常情况下不丢数据

### Phase 6 完成标准
- [ ] `docker-compose up` 一键启动成功
- [ ] 分区表自动创建下月分区
- [ ] 过期分区自动清理
- [ ] Prometheus 指标正确暴露
- [ ] Grafana Dashboard 显示核心指标
- [ ] 健康检查端点 `/health` 正常响应

### Phase 7 完成标准
- [ ] `/docs` Swagger UI 可访问
- [ ] `openapi.json` 自动生成且与代码同步
- [ ] TypeScript SDK 类型定义完整
- [ ] SDK 示例代码可运行
- [ ] README 文档完整

---

## 问题记录

> 每个阶段完成后，记录遇到的问题和解决方案，便于后续参考。

### Phase 1 问题记录
| 问题 | 解决方案 | 日期 |
|------|----------|------|
| | | |

### Phase 2 问题记录
| 问题 | 解决方案 | 日期 |
|------|----------|------|
| | | |

### Phase 3 问题记录
| 问题 | 解决方案 | 日期 |
|------|----------|------|
| | | |

### Phase 4 问题记录
| 问题 | 解决方案 | 日期 |
|------|----------|------|
| | | |

### Phase 5 问题记录
| 问题 | 解决方案 | 日期 |
|------|----------|------|
| | | |

### Phase 6 问题记录
| 问题 | 解决方案 | 日期 |
|------|----------|------|
| | | |

### Phase 7 问题记录
| 问题 | 解决方案 | 日期 |
|------|----------|------|
| | | |

---

## 时间估算

| 阶段 | 内容 | 工作量 |
|------|------|--------|
| Phase 1 | 基础设施（Proto、Types、Schema） | ~9 天 |
| Phase 2 | 核心框架（gRPC Client、Pipeline、Handlers） | ~15 天 |
| Phase 3 | REST API（Info、Exchange） | ~11 天 |
| Phase 4 | 集成与主程序 | ~7 天 |
| Phase 5 | DEX Engine 事件发送端 | ~7 天 |
| Phase 6 | 部署与运维（Docker、监控） | ~4 天 |
| Phase 7 | API 文档与 SDK | ~3 天 |
| **总计** | | **~8 周** |

### 建议执行顺序

```
Phase 1 → Phase 2 → Phase 5.1-5.2（先验证事件流）→ Phase 3 → Phase 4 → Phase 5.3 → Phase 6 → Phase 7
```

**里程碑**:
1. **M1 (Week 2)**: Proto + Types + Schema 完成，数据库可用
2. **M2 (Week 4)**: Indexer 可接收 Mock 事件并写入 DB
3. **M3 (Week 5)**: DEX Engine 可发送 FillEvent，端到端验证通过
4. **M4 (Week 6)**: REST API 完整，与现有 DEX 测试集成
5. **M5 (Week 8)**: 生产就绪（Docker 部署、监控、文档）

---

## 相关文档

- 技术方案: `sui/mynotes/dex/tech/dex-indexer-tech-v3.md`
- 模块分析: `sui/mynotes/dex/tech/dex-indexer-module-analysis.md`
- dYdX 参考: `sui/mynotes/dex/analyst/dydx-indexer-analyst.md`
- 架构分析: `sui/mynotes/dex/analyst/dex-indexer-full-by-dydx-analysis.md`
