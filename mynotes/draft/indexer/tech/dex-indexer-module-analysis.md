# DEX Indexer 模块修改与新增分析

> 基于 `dex-indexer-tech-v3.md` 技术方案，结合 Sui 项目模块结构分析

---

## 1. 核心结论

基于技术方案设计，DEX Indexer 采用 **借鉴但独立部署** 的策略：

| 策略 | 说明 |
|------|------|
| **不修改** Sui 核心模块 | sui-core、sui-types 等保持不变 |
| **借鉴** sui-indexer-alt 架构 | 复用 Pipeline 模式、Store trait 设计 |
| **新增** DEX 专用 crates | 独立的 dex-indexer-* 系列 crate |
| **替换** 数据源 | 从 Checkpoint gRPC → DEX Engine gRPC |

---

## 2. Sui 现有模块分析

### 2.1 sui-indexer-alt 系列（可借鉴）

```
sui/crates/
├── sui-indexer-alt-framework/          # ✅ 核心框架（借鉴 Pipeline 模式）
│   ├── src/
│   │   ├── ingestion/                  # 数据摄入（需替换为 DEX gRPC Client）
│   │   │   ├── streaming_client.rs     # gRPC 订阅 Checkpoint
│   │   │   └── broadcaster.rs          # 广播机制
│   │   ├── pipeline/                   # ✅ Pipeline 模式（直接借鉴）
│   │   │   ├── concurrent/             # 并发 Pipeline
│   │   │   │   ├── mod.rs
│   │   │   │   ├── collector.rs        # 批量收集
│   │   │   │   ├── committer.rs        # 批量提交
│   │   │   │   └── pruner.rs           # 数据清理
│   │   │   └── sequential/             # 顺序 Pipeline
│   │   └── postgres/                   # ✅ PostgreSQL Handler（借鉴）
│   │       └── handler.rs
│
├── sui-indexer-alt-framework-store-traits/  # ✅ Store 抽象（借鉴）
│   └── src/lib.rs                      # Store、Connection trait 定义
│
├── sui-indexer-alt-schema/             # 数据库 Schema（需替换为 DEX Schema）
│   └── src/
│       ├── schema.rs                   # Diesel schema
│       ├── checkpoints.rs
│       ├── events.rs
│       └── transactions.rs
│
├── sui-indexer-alt/                    # Handler 实现（参考模式）
│   └── src/handlers/
│       ├── mod.rs
│       ├── ev_emit_mod.rs              # 事件处理示例
│       ├── kv_transactions.rs          # KV 处理示例
│       └── tx_balance_changes.rs       # 余额处理示例
│
├── sui-indexer-alt-jsonrpc/            # JSON-RPC API（参考 API 设计）
├── sui-indexer-alt-metrics/            # Prometheus 指标（借鉴）
└── sui-indexer-alt-reader/             # 读取器（参考）
```

### 2.2 不需要修改的核心模块

```
sui/crates/
├── sui-core/                           # ❌ 不修改（DEX Engine 独立实现）
├── sui-types/                          # ❌ 不修改（使用 DEX 自定义类型）
├── sui-execution/                      # ❌ 不修改（DEX Engine 独立）
└── sui-node/                           # ❌ 不修改
```

---

## 3. DEX Indexer 新增模块规划

### 3.1 新增 Crate 总览

```
sui/crates/
├── dex-indexer/                        # 🆕 主程序入口
├── dex-indexer-framework/              # 🆕 核心框架（借鉴 sui-indexer-alt-framework）
├── dex-indexer-schema/                 # 🆕 数据库 Schema
├── dex-indexer-handlers/               # 🆕 事件处理器
├── dex-indexer-api/                    # 🆕 REST API（POST /info + /exchange）
├── dex-indexer-ws/                     # 🆕 WebSocket 服务（Phase 2）
├── dex-indexer-types/                  # 🆕 DEX 类型定义
├── dex-indexer-proto/                  # 🆕 gRPC Proto 定义
└── dex-indexer-metrics/                # 🆕 监控指标
```

### 3.2 各模块详细设计

#### 3.2.1 dex-indexer-proto（gRPC 定义）

```
dex-indexer-proto/
├── Cargo.toml
├── build.rs                            # prost-build 构建脚本
└── proto/
    └── dex/indexer/v1/
        ├── events.proto                # OnChainUpdates + OffChainUpdates 事件
        └── service.proto               # DexEventService gRPC 定义
```

**关键内容**：
- `FillEvent`, `PositionUpdateEvent`, `BalanceUpdateEvent`
- `FundingRateEvent`, `LiquidationEvent`, `TransferEvent`
- `OrderPlaceEvent`, `OrderUpdateEvent`, `OrderRemoveEvent`（Phase 2）
- `DexEventService` gRPC 服务定义

#### 3.2.2 dex-indexer-types（类型定义）

```
dex-indexer-types/
├── Cargo.toml
└── src/
    ├── lib.rs
    ├── events.rs                       # 事件 Rust 类型
    ├── api/
    │   ├── mod.rs
    │   ├── info.rs                     # InfoRequest/Response
    │   └── exchange.rs                 # ExchangeRequest/Response
    ├── models/
    │   ├── mod.rs
    │   ├── market.rs                   # PerpAsset, SpotAsset, AssetCtx
    │   ├── order.rs                    # OpenOrder, FrontendOrder
    │   ├── fill.rs                     # UserFill
    │   ├── position.rs                 # AssetPosition, PositionInfo
    │   ├── candle.rs                   # Candle
    │   └── funding.rs                  # FundingHistory
    └── enums.rs                        # Side, LeverageType, OrderStatus
```

#### 3.2.3 dex-indexer-schema（数据库 Schema）

```
dex-indexer-schema/
├── Cargo.toml
├── migrations/                         # Diesel 迁移文件
│   ├── 00000000000000_init/
│   │   └── up.sql                      # 初始化 Schema
│   └── 2026xxxx_create_tables/
│       └── up.sql                      # 创建 DEX 表
└── src/
    ├── lib.rs
    ├── schema.rs                       # Diesel schema 自动生成
    ├── models/
    │   ├── mod.rs
    │   ├── market.rs
    │   ├── order.rs
    │   ├── fill.rs
    │   ├── position.rs
    │   ├── balance.rs
    │   ├── candle.rs
    │   ├── funding.rs
    │   ├── transfer.rs
    │   ├── liquidation.rs
    │   └── watermark.rs
    └── queries/                        # 预定义查询
        ├── mod.rs
        ├── fills.rs
        └── positions.rs
```

**DDL 表**（来自 dex-indexer-tech-v3.md）：
- `markets` - 交易对配置
- `orders` - 活跃订单
- `fills` - 成交记录（按天分区）
- `perpetual_positions` - 永续持仓
- `balances` - 账户余额
- `candles` - K线数据（按月分区）
- `funding_rates` - 资金费率
- `user_funding_records` - 用户资金费记录
- `transfers` - 充提记录
- `liquidations` - 清算记录
- `dex_watermarks` - 断点续传标记

#### 3.2.4 dex-indexer-framework（核心框架）

```
dex-indexer-framework/
├── Cargo.toml
└── src/
    ├── lib.rs
    ├── ingestion/                      # 数据摄入（替换 sui-indexer-alt）
    │   ├── mod.rs
    │   ├── grpc_client.rs              # 🆕 DEX Engine gRPC Client
    │   ├── config.rs
    │   └── error.rs
    ├── pipeline/                       # 借鉴 sui-indexer-alt-framework
    │   ├── mod.rs
    │   ├── processor.rs                # Processor trait
    │   ├── concurrent/
    │   │   ├── mod.rs
    │   │   ├── handler.rs              # Handler trait
    │   │   ├── collector.rs
    │   │   ├── committer.rs
    │   │   └── watermark.rs
    │   └── sequential/
    │       └── mod.rs
    ├── store/                          # 借鉴 store-traits
    │   ├── mod.rs
    │   ├── traits.rs                   # Store, Connection trait
    │   └── postgres.rs                 # PostgreSQL 实现
    └── metrics.rs
```

**与 sui-indexer-alt-framework 的差异**：

| 组件 | sui-indexer-alt | dex-indexer |
|------|-----------------|-------------|
| 数据源 | Checkpoint gRPC (Full Node) | DEX Engine gRPC |
| 事件类型 | Sui TransactionEffects | DexEventBatch |
| Checkpoint | Sui Checkpoint | DEX Checkpoint Sequence |
| 数据模型 | Objects, Events | Fills, Positions, Balances |

#### 3.2.5 dex-indexer-handlers（事件处理器）

```
dex-indexer-handlers/
├── Cargo.toml
└── src/
    ├── lib.rs
    ├── mod.rs
    ├── fills.rs                        # FillsHandler
    ├── positions.rs                    # PositionsHandler
    ├── balances.rs                     # BalancesHandler
    ├── candles.rs                      # CandlesHandler（K线聚合）
    ├── funding.rs                      # FundingHandler
    ├── transfers.rs                    # TransfersHandler
    ├── liquidations.rs                 # LiquidationsHandler
    └── orders.rs                       # OrdersHandler（Phase 2）
```

**Handler 实现模式**（借鉴 sui-indexer-alt）：

```rust
#[async_trait]
impl Handler for FillsHandler {
    type Store = DexStore;
    type Batch = Vec<FillRow>;

    fn batch(&self, batch: &mut Self::Batch, values: &mut IntoIter<Self::Value>) -> BatchStatus {
        batch.extend(values);
        BatchStatus::Ready
    }

    async fn commit<'a>(
        &self,
        batch: &Self::Batch,
        conn: &mut DexConnection<'a>,
    ) -> Result<usize> {
        // 批量插入 fills 表
    }
}
```

#### 3.2.6 dex-indexer-api（REST API）

```
dex-indexer-api/
├── Cargo.toml
└── src/
    ├── lib.rs
    ├── server.rs                       # Axum HTTP 服务器
    ├── routes/
    │   ├── mod.rs
    │   ├── info.rs                     # POST /info 路由
    │   └── exchange.rs                 # POST /exchange 路由
    ├── handlers/
    │   ├── mod.rs
    │   ├── info/
    │   │   ├── mod.rs
    │   │   ├── meta.rs                 # type=meta
    │   │   ├── l2_book.rs              # type=l2Book
    │   │   ├── candle.rs               # type=candleSnapshot
    │   │   ├── clearinghouse.rs        # type=clearinghouseState
    │   │   ├── orders.rs               # type=openOrders, frontendOpenOrders
    │   │   ├── fills.rs                # type=userFills, userFillsByTime
    │   │   └── funding.rs              # type=fundingHistory
    │   └── exchange/
    │       ├── mod.rs
    │       ├── order.rs                # action.type=order
    │       ├── cancel.rs               # action.type=cancel
    │       └── leverage.rs             # action.type=updateLeverage
    ├── middleware/
    │   ├── mod.rs
    │   ├── logging.rs
    │   └── metrics.rs
    └── error.rs
```

#### 3.2.7 dex-indexer-ws（WebSocket，Phase 2）

```
dex-indexer-ws/
├── Cargo.toml
└── src/
    ├── lib.rs
    ├── server.rs                       # WebSocket 服务器
    ├── subscriptions/
    │   ├── mod.rs
    │   ├── all_mids.rs                 # allMids 订阅
    │   ├── l2_book.rs                  # l2Book 订阅
    │   ├── trades.rs                   # trades 订阅
    │   ├── candle.rs                   # candle 订阅
    │   ├── order_updates.rs            # orderUpdates 订阅
    │   └── user_fills.rs               # userFills 订阅
    ├── broadcast/
    │   ├── mod.rs
    │   └── broadcaster.rs              # 消息广播器
    └── connection/
        ├── mod.rs
        └── manager.rs                  # 连接管理
```

#### 3.2.8 dex-indexer（主程序入口）

```
dex-indexer/
├── Cargo.toml
└── src/
    ├── main.rs
    ├── config.rs                       # 配置加载
    ├── args.rs                         # 命令行参数
    └── bootstrap.rs                    # 初始化 Pipeline
```

---

## 4. 依赖关系图

```
                         ┌─────────────────────────┐
                         │      dex-indexer        │
                         │      (main binary)      │
                         └───────────┬─────────────┘
                                     │
          ┌──────────────────────────┼──────────────────────────┐
          │                          │                          │
          ▼                          ▼                          ▼
┌─────────────────┐      ┌─────────────────────┐      ┌─────────────────┐
│ dex-indexer-api │      │ dex-indexer-handlers │      │  dex-indexer-ws │
│   (REST API)    │      │   (Event Handlers)   │      │   (WebSocket)   │
└────────┬────────┘      └──────────┬───────────┘      └────────┬────────┘
         │                          │                           │
         │                          ▼                           │
         │               ┌─────────────────────┐                │
         │               │dex-indexer-framework│                │
         │               │  (Pipeline Layer)   │                │
         │               └──────────┬──────────┘                │
         │                          │                           │
         └──────────────────────────┼───────────────────────────┘
                                    │
                     ┌──────────────┼──────────────┐
                     │              │              │
                     ▼              ▼              ▼
          ┌─────────────────┐ ┌───────────┐ ┌─────────────────┐
          │dex-indexer-schema│ │dex-indexer│ │ dex-indexer-    │
          │   (DB Models)   │ │  -types   │ │     proto       │
          └─────────────────┘ └───────────┘ └─────────────────┘
```

---

## 5. 代码复用分析

### 5.1 可直接复用（借鉴后少量修改）

| 来源 | 目标 | 复用程度 |
|------|------|---------|
| `sui-indexer-alt-framework/pipeline/concurrent/` | `dex-indexer-framework/pipeline/concurrent/` | 80% |
| `sui-indexer-alt-framework/pipeline/sequential/` | `dex-indexer-framework/pipeline/sequential/` | 80% |
| `sui-indexer-alt-framework-store-traits` | `dex-indexer-framework/store/traits.rs` | 70% |
| `sui-indexer-alt-metrics` | `dex-indexer-metrics` | 60% |

### 5.2 需要重写的模块

| 模块 | 原因 |
|------|------|
| `ingestion/` | 数据源从 Checkpoint gRPC 改为 DEX Engine gRPC |
| `schema/` | 完全不同的数据模型（Fills vs Transactions） |
| `handlers/` | 处理 DEX 特有事件（FillEvent vs TransactionEffects） |
| `api/` | Hyperliquid 风格 API（POST /info vs JSON-RPC） |

### 5.3 全新开发的模块

| 模块 | 说明 |
|------|------|
| `dex-indexer-proto` | DEX 专用 gRPC Proto 定义 |
| `dex-indexer-types` | DEX 数据模型（对标 Hyperliquid） |
| `dex-indexer-ws` | WebSocket 实时推送（Phase 2） |
| `handlers/candles.rs` | K线聚合逻辑（非简单存储） |

---

## 6. 实施建议

### 6.1 Phase 1 开发顺序

```
1. dex-indexer-proto          # 定义 gRPC 接口
2. dex-indexer-types          # 定义数据类型
3. dex-indexer-schema         # 定义数据库 Schema
4. dex-indexer-framework      # 实现 Pipeline 框架
   └── 先复制 sui-indexer-alt-framework，再修改 ingestion/
5. dex-indexer-handlers       # 实现各 Handler
6. dex-indexer-api            # 实现 REST API
7. dex-indexer                # 主程序集成
```

### 6.2 工作量估算

| 模块 | 预估工作量 | 说明 |
|------|-----------|------|
| `dex-indexer-proto` | 1-2 天 | Proto 定义 + prost 构建 |
| `dex-indexer-types` | 2-3 天 | 类型定义 + serde |
| `dex-indexer-schema` | 2-3 天 | DDL + Diesel 集成 |
| `dex-indexer-framework` | 5-7 天 | Pipeline 复用 + gRPC Client |
| `dex-indexer-handlers` | 5-7 天 | 7 个 Handler 实现 |
| `dex-indexer-api` | 5-7 天 | 20+ 个 API 端点 |
| `dex-indexer` | 1-2 天 | 主程序集成 |
| **Phase 1 总计** | **~4 周** | |

### 6.3 Phase 2 新增

| 模块 | 预估工作量 |
|------|-----------|
| `dex-indexer-ws` | 5-7 天 |
| Redis 集成 | 2-3 天 |
| OffChainUpdates Handler | 3-4 天 |
| **Phase 2 总计** | **~2 周** |

---

## 7. 总结

### 7.1 不修改的模块

- `sui-core`
- `sui-types`
- `sui-execution`
- `sui-node`
- `sui-indexer-alt-*`（保持原样，仅参考）

### 7.2 新增的模块

| Crate | 类型 | 用途 |
|-------|------|------|
| `dex-indexer-proto` | 新增 | gRPC Proto 定义 |
| `dex-indexer-types` | 新增 | DEX 数据类型 |
| `dex-indexer-schema` | 新增 | 数据库 Schema |
| `dex-indexer-framework` | 借鉴新增 | Pipeline 框架 |
| `dex-indexer-handlers` | 新增 | 事件处理器 |
| `dex-indexer-api` | 新增 | REST API |
| `dex-indexer-ws` | 新增 (Phase 2) | WebSocket |
| `dex-indexer-metrics` | 借鉴新增 | 监控指标 |
| `dex-indexer` | 新增 | 主程序 |

### 7.3 核心设计原则

1. **独立部署**：DEX Indexer 作为独立服务，不侵入 Sui 核心代码
2. **借鉴成熟模式**：复用 sui-indexer-alt 的 Pipeline 架构
3. **替换数据源**：从 Sui Checkpoint gRPC 改为 DEX Engine gRPC
4. **对标 Hyperliquid**：API 设计完全对标 Hyperliquid 风格
