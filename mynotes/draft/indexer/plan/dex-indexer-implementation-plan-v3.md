# DEX Indexer 实施计划 V3

> 基于 dex-indexer-tech-v5.md 技术方案的模块分离实施计划
> 创建日期: 2026-02-03
> 更新日期: 2026-02-05 (链上订单簿快照推送方案)
> 前置版本：V2（基础功能已完成）
> 阶段总结目录：`sui/mynotes/dex/summary/`

## 参考文档

Phase 2 详细分析文档位于 `sui/mynotes/dex/analyst/phase2_realtime/`：
- 实施清单：`06-implementation-checklist.md`（文件清单、测试清单）
- 设计决策：`04-design-decisions.md`（技术选型理由）
- 事件定义：`05-event-definitions.md`（事件结构详解）

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

> 详细实施清单见 `sui/mynotes/dex/analyst/phase2_realtime/06-implementation-checklist.md`

### Phase 2 依赖说明

> ✅ **阻塞已解除**：dex.rs 改造（subaccount 拆分）已完成
>
> 更新日期：2026-02-05

#### 依赖关系

| 任务 | 依赖 dex.rs | 当前状态 |
|------|:-----------:|----------|
| dex-realtime/dex-ws crate 框架 | - | ✅ 已完成 |
| 事件定义（*EventV1） | ✓ | 🟡 待实施 |
| 事件发射点修改 | ✓ | 🟡 待实施 |
| dex-indexer Orders Handler | ✓ | 🟡 待实施 |
| 链上订单簿快照推送 | ✓ | 🟡 待实施（方案 A - 复用 Sui Event） |

#### 节点连接策略

| 阶段 | 方案 | 延迟 |
|------|------|------|
| MVP | Full Node 标准订阅 | 2-4s |
| **生产** | **Validator + 同机器索引节点** | **400-650ms** |

> 详见 `sui/mynotes/dex/analyst/phase2_realtime/08-realtime-latency-analysis.md`

#### 已完成的框架部分

**dex-realtime crate** (`crates/dex-realtime/`)
- ✅ 目录结构和 Cargo.toml
- ✅ config.rs - 配置（RPC URL、Redis、重连策略）
- ✅ listener.rs - Sui RPC 事件订阅框架（事件过滤器 TODO）
- ✅ publisher.rs - Redis Stream 批量发布框架
- ✅ main.rs - CLI 入口

**dex-ws crate** (`crates/dex-ws/`)
- ✅ 目录结构和 Cargo.toml
- ✅ config.rs - 配置（端口、Redis、心跳）
- ✅ types.rs - 订阅/消息类型定义
- ✅ channels.rs - 频道订阅管理
- ✅ subscriber.rs - Redis Stream 消费框架
- ✅ server.rs - WebSocket 服务器
- ✅ main.rs - CLI 入口

#### 验证命令

```bash
# 编译验证
cargo build -p dex-realtime
cargo build -p dex-ws

# CLI 帮助
./target/debug/dex-realtime --help
./target/debug/dex-ws --help
```

---

### Phase 2.0: 事件定义与 dex-indexer 扩展

**目标**：新增订单事件，扩展 dex-indexer

#### 2.0.1 事件定义
- [ ] 在 `sui-types/src/dex_events.rs` 添加 OrderPlacedEventV1
- [ ] 在 `sui-types/src/dex_events.rs` 添加 OrderRemovedEventV1
- [ ] 现有事件添加 V1 后缀（FillEvent → FillEventV1 等）

#### 2.0.2 dex-indexer 扩展
- [ ] 创建 `dex-indexer/src/handlers/orders.rs`（处理订单事件）
- [ ] 新增数据库迁移：创建 dex_orders 表
- [ ] 在 `main.rs` 注册 Orders Handler

#### 2.0.3 数据库迁移
```sql
CREATE TABLE dex_orders (
    id BIGSERIAL PRIMARY KEY,
    perpetual_id INTEGER NOT NULL,
    order_id BYTEA NOT NULL UNIQUE,
    subaccount BYTEA NOT NULL,
    side SMALLINT NOT NULL,
    price BIGINT NOT NULL,
    original_quantity BIGINT NOT NULL,
    remaining_quantity BIGINT NOT NULL,
    status SMALLINT NOT NULL DEFAULT 0,  -- 0=open, 1=filled, 2=cancelled
    created_at TIMESTAMP WITH TIME ZONE NOT NULL,
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL
);
```

---

### Phase 2.1: dex-realtime 基础功能

**目标**：实现 Sui RPC 事件监听，发布到 Redis

#### 2.1.1 创建目录结构
```bash
dex-sui/crates/dex-realtime/
├── src/
│   ├── lib.rs
│   ├── main.rs
│   ├── config.rs       # 配置（RPC、Redis、重连策略）
│   ├── listener.rs     # Sui RPC 订阅
│   └── publisher.rs    # Redis Stream 发布
└── Cargo.toml
```

#### 2.1.2 核心实现
- [ ] 实现 `config.rs`：配置加载（环境变量、配置文件）
- [ ] 实现 `listener.rs`：sui_subscribeEvent 订阅，指数退避重连
- [ ] 实现 `publisher.rs`：Redis Stream 批量写入（10ms/100条）
- [ ] 实现 `main.rs`：命令行参数和启动逻辑

#### 2.1.3 验证
```bash
cargo build -p dex-realtime
# 验证事件可订阅并发布到 Redis
```

---

### Phase 2.2: 链上订单簿快照推送

**目标**：消费链上 OrderbookSnapshotEvent，直接写入 Redis

> **架构简化说明**：采用「链上内存订单簿快照推送」方案（方案 A - 复用 Sui Event），dex-realtime 直接消费链上快照事件，无需本地维护订单簿状态，也无需启动恢复逻辑。
>
> 详见 [设计决策文档](../../../sui/mynotes/dex/analyst/phase2_realtime/04-design-decisions.md) 和 [订单簿推送方案分析](../../../sui/mynotes/dex/analyst/phase2_realtime/12-orderbook-push-strategy-analysis.md)。

#### 2.2.1 链上实现（sui-types + sui-execution）
- [ ] 在 `sui-types/src/dex_events.rs` 定义 `OrderbookSnapshotEvent`
- [ ] 在 `sui-types/src/dex/perpetual.rs` 扩展 PerpetualState（添加快照字段）
- [ ] 在 `sui-execution/src/dex/helpers.rs` 实现 `generate_orderbook_snapshot()`
- [ ] 在 `sui-execution/src/dex/commands/order.rs` 添加快照检查和发射逻辑

#### 2.2.2 dex-realtime 实现
- [ ] 在 `publisher.rs` 添加 `on_orderbook_snapshot()` 处理器
- [ ] 直接将 `OrderbookSnapshotEvent` 写入 Redis Hash
- [ ] ~~删除 orderbook.rs~~（不再需要）
- [ ] ~~删除 recovery.rs~~（不再需要）

#### 2.2.3 快照参数

| 参数 | 值 | 说明 |
|------|-----|------|
| 推送频率 | 250ms | 与 Hyperliquid 对标，平均延迟 125ms |
| 快照深度 | 100 档 | 满足大多数交易需求，单快照约 4-5 KB |
| 触发方式 | 交易触发 | 实现简单，无需额外链下服务 |

#### 2.2.4 验证
```bash
# 验证链上事件发射
# 订阅 OrderbookSnapshotEvent 并检查内容

# 验证 Redis 写入
redis-cli HGETALL dex:orderbook:0
# 应包含 bids, asks, best_bid, best_ask, stats, sequence_number, updated_at
```

---

### Phase 2.3: K 线与市场统计

**目标**：实时 K 线聚合和市场统计

#### 2.3.1 扩展 dex-realtime
```bash
dex-realtime/src/
├── ...
├── candles.rs      # [新增] K 线聚合
└── market_stats.rs # [新增] 市场统计
```

#### 2.3.2 核心实现
- [ ] 实现 `candles.rs`：
  - 多周期聚合（1m, 5m, 15m, 1h, 4h, 1d）
  - 基于 FillEventV1 更新 OHLCV
  - 实时 K 线存 Redis，历史 K 线存 PostgreSQL（可选）
- [ ] 实现 `market_stats.rs`：
  - 中间价：(best_bid + best_ask) / 2
  - 24h 成交量：滚动窗口累加
  - 定期更新到 Redis（~1s）

#### 2.3.3 验证
```bash
# 验证 K 线数据
redis-cli ZRANGE dex:candles:1:1m 0 -1
```

---

### Phase 2.4: dex-ws WebSocket 服务

**目标**：实现 WebSocket 推送服务

#### 2.4.1 创建目录结构
```bash
dex-sui/crates/dex-ws/
├── src/
│   ├── lib.rs
│   ├── main.rs
│   ├── config.rs       # 配置
│   ├── types.rs        # 订阅/消息类型
│   ├── server.rs       # WebSocket 服务器
│   ├── channels.rs     # 订阅频道管理
│   └── subscriber.rs   # Redis Stream 消费
└── Cargo.toml
```

#### 2.4.2 核心实现
- [ ] 实现 `types.rs`：订阅请求、推送消息类型
- [ ] 实现 `channels.rs`：频道管理（trades, orderbook, candle, user）
- [ ] 实现 `subscriber.rs`：Redis Stream 消费者组
- [ ] 实现 `server.rs`：WebSocket 服务器 + 心跳检测

#### 2.4.3 验证
```bash
cargo build -p dex-ws
wscat -c ws://localhost:8080
# 发送: {"type": "subscribe", "channel": "trades:1"}
```

---

### Phase 2.5: dex-api 扩展

**目标**：新增订单簿、K 线查询接口

#### 2.5.1 新增 API 端点
- [ ] `POST /info` type: "l2Book" - 订单簿查询（从 Redis）
- [ ] `POST /info` type: "candleSnapshot" - K 线查询（从 Redis/PostgreSQL）
- [ ] `POST /info` type: "marketStats" - 市场统计（从 Redis）
- [ ] `POST /info` type: "openOrders" - 用户活跃订单（从 PostgreSQL）

#### 2.5.2 验证
```bash
curl -X POST http://localhost:3000/info \
  -H "Content-Type: application/json" \
  -d '{"type": "l2Book", "perpetualId": 1}'
```

---

## 4. Phase 3: 缓存优化

### Phase 3 架构说明

**双层数据架构**：
```
Client → dex-api → Redis（热数据，低延迟）
                 ↘ PostgreSQL（冷数据/历史/回退）
```

**数据来源差异**：
- **Redis 数据**：来自 dex-realtime 的 RPC 实时订阅（延迟 ~200ms）
- **PostgreSQL 数据**：来自 dex-indexer 的 Checkpoint（延迟 ~2-3s）

---

### Phase 3.0: 数据源分类与合并策略

#### 3.0.1 数据源分类

| 分类 | 数据类型 | 数据源 | 合并 | 说明 |
|------|----------|--------|:----:|------|
| **仅 Redis** | 订单簿快照 | Redis | ❌ | 实时状态，历史无意义 |
| | 中间价/BBO | Redis | ❌ | 实时行情 |
| | 市场统计 | Redis | ❌ | 24h 成交量等聚合值 |
| **仅 PostgreSQL** | 用户成交历史 | PostgreSQL | ❌ | 完整性优先，无实时需求 |
| | 用户余额历史 | PostgreSQL | ❌ | 完整性优先 |
| | 转账记录 | PostgreSQL | ❌ | 完整性优先 |
| **需要合并** | 最近成交 | Redis + PostgreSQL | ✅ | 实时性 + 完整性 |
| | K 线数据 | Redis + PostgreSQL | ✅ | 当前周期 + 历史 |
| | 用户持仓 | Redis 优先 | ⚠️ | 实时性优先，PostgreSQL 回退 |

#### 3.0.2 合并策略详解

**策略 1：最近成交合并**

```
查询流程：
1. 从 Redis 获取最新 N 条成交
2. 如果数量不足，从 PostgreSQL 补充更早的记录
3. 按时间戳排序、去重（tx_digest）
4. 返回合并结果

边界处理：
- Redis 保留最近 1000 条（ZREMRANGEBYRANK 裁剪）
- PostgreSQL 保留完整历史
- 用 oldest_redis_time 作为 PostgreSQL 查询上界
```

**策略 2：K 线数据合并**

```
查询流程：
1. 计算当前周期边界（current_period_start）
2. 历史 K 线（已完结周期）：从 PostgreSQL 查询
3. 当前 K 线（未完结周期）：从 Redis 查询
4. 合并返回

边界处理：
- PostgreSQL 存储已完结 K 线
- Redis 存储当前周期 K 线（dex-realtime 实时更新）
- 当前周期完结时，dex-realtime 写入 PostgreSQL
```

**策略 3：用户持仓合并**

```
查询流程：
1. 优先从 Redis 读取最新持仓快照
2. 如果 Redis 无数据或过期，回退到 PostgreSQL
3. 使用 Checkpoint 序列号判断数据新鲜度

边界处理：
- Redis 快照带 cp_sequence_number
- 如果 Redis 版本 < PostgreSQL 版本，使用 PostgreSQL
```

#### 3.0.3 一致性保障机制

| 机制 | 说明 | 适用场景 |
|------|------|----------|
| **时间戳边界** | Redis 数据带 updated_at，用于比较新鲜度 | 最近成交、K 线 |
| **Checkpoint 序列号** | 使用 cp_sequence_number 判断数据新旧 | 持仓、余额 |
| **TTL 过期** | Redis 数据设置 TTL，过期后强制回退 | 订单簿、持仓 |
| **去重** | 使用 tx_digest/order_id 去重 | 成交、订单 |

---

### Phase 3.1: 实现 dex-api 缓存层

**目标**：dex-api 支持 Redis 缓存查询

#### 3.1.1 cache 模块结构

```
dex-api/src/cache/
├── mod.rs          # 模块导出
├── client.rs       # Redis 客户端封装
├── keys.rs         # 键命名规范
├── queries.rs      # 缓存查询逻辑
└── types.rs        # 缓存数据类型
```

#### 3.1.2 实现任务

**Redis 客户端** (`cache/client.rs`)
- [ ] 连接池管理
- [ ] 读写封装（get/set/hget/zrange）
- [ ] 序列化/反序列化

**键命名规范** (`cache/keys.rs`)
- [ ] 订单簿：`dex:orderbook:{perpetual_id}`
- [ ] 市场统计：`dex:market:{perpetual_id}`
- [ ] K 线：`dex:candles:{perpetual_id}:{interval}`
- [ ] 最近成交：`dex:trades:{perpetual_id}`

**缓存查询逻辑** (`cache/queries.rs`)
- [ ] `get_l2_book` - 订单簿（仅 Redis）
- [ ] `get_recent_fills` - 最近成交（合并策略）
- [ ] `get_candles` - K 线数据（合并策略）
- [ ] `get_user_positions` - 用户持仓（优先 Redis）
- [ ] `get_market_stats` - 市场统计（仅 Redis）

#### 3.1.3 更新查询处理

- [ ] 修改 `handlers.rs`，按数据类型分发
- [ ] 实现合并逻辑（最近成交、K 线）
- [ ] 实现回退逻辑（缓存未命中/过期）

---

### Phase 3.2: 合并逻辑实现

#### 3.2.1 最近成交合并

```rust
// 伪代码
pub async fn get_recent_fills(perpetual_id: u32, limit: usize) -> Result<Vec<Fill>> {
    // 1. Redis 获取最新
    let redis_fills = cache.zrevrange("dex:trades:{id}", 0, limit).await?;

    // 2. 不足则 PostgreSQL 补充
    if redis_fills.len() < limit {
        let oldest_time = redis_fills.last().map(|f| f.timestamp_ms);
        let db_fills = query_fills_before(db, perpetual_id, oldest_time, limit - redis_fills.len()).await?;

        // 3. 合并去重
        return merge_and_dedup(redis_fills, db_fills, limit);
    }

    Ok(redis_fills)
}
```

#### 3.2.2 K 线合并

```rust
// 伪代码
pub async fn get_candles(perpetual_id: u32, interval: &str, start: u64, end: u64) -> Result<Vec<Candle>> {
    let current_period_start = get_period_start(end, interval);

    // 1. 历史 K 线（PostgreSQL）
    let historical = query_candles_from_db(db, perpetual_id, interval, start, current_period_start).await?;

    // 2. 当前周期（Redis）
    let current = cache.hget("dex:candle:{id}:{interval}", "current").await?;

    // 3. 合并
    let mut candles = historical;
    if let Some(c) = current {
        candles.push(c);
    }
    Ok(candles)
}
```

#### 3.2.3 验证

- [ ] 单元测试：合并逻辑正确性
- [ ] 边界测试：Redis 数据为空时回退
- [ ] 边界测试：时间边界正确处理
- [ ] 性能测试：合并查询延迟 < 50ms

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

#### 5.2.1 编译验证
```bash
cargo build -p dex-realtime
cargo build -p dex-ws
./target/debug/dex-realtime --help
./target/debug/dex-ws --help
```

#### 5.2.2 单元测试
```bash
cargo test -p dex-realtime
cargo test -p dex-ws
```

#### 5.2.3 集成测试
- [ ] Sui RPC 订阅测试（testnet）
- [ ] Redis Stream 发布/消费测试
- [ ] OrderbookSnapshotEvent 接收测试
- [ ] Redis Hash 订单簿写入测试

#### 5.2.4 端到端测试
- [ ] 下单 → FillEvent → dex-realtime → Redis → dex-ws → 客户端
- [ ] 下单 → OrderPlacedEvent → 订单簿更新 → 订单簿快照推送
- [ ] 取消订单 → OrderRemovedEvent → 订单簿更新
- [ ] 连续成交 → K 线更新 → K 线推送

#### 5.2.5 故障恢复测试
- [ ] dex-realtime 重启后等待下一个 OrderbookSnapshotEvent（无需手动恢复）
- [ ] Redis 连接断开后重连
- [ ] Sui RPC 断开后重连（指数退避 1s→30s）

#### 5.2.6 性能测试
- [ ] 事件端到端延迟 < 500ms
- [ ] 订单簿快照延迟 < 250ms（链上发射频率）
- [ ] Redis 写入延迟 < 10ms
- [ ] dex-ws 支持 1000 并发连接

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
| **Phase 2.0** | **事件定义 + dex-indexer 扩展** | Phase 1 完成 |
| **Phase 2.1** | **dex-realtime 基础功能** | Phase 2.0 |
| **Phase 2.2** | **链上订单簿快照推送** | Phase 2.1 |
| **Phase 2.3** | **K 线与市场统计** | Phase 2.1 |
| **Phase 2.4** | **dex-ws WebSocket 服务** | Phase 2.1 |
| **Phase 2.5** | **dex-api 扩展** | Phase 2.2 + 2.3 |
| Phase 3.1 | dex-api 缓存层 | Phase 2 完成 |

---

## 8. 风险与缓解

| 风险 | 影响 | 缓解措施 |
|------|------|----------|
| 依赖循环 | 编译失败 | dex-api 仅依赖 dex-indexer 的 schema |
| 测试失败 | 回归 | 分步验证，每步确保测试通过 |
| workspace 配置 | 构建失败 | 参考现有 crate 配置 |
