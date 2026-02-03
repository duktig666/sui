# DEX Indexer 技术方案 V5

> 版本: V5
> 日期: 2026-02-03
> 状态: 设计中
> 基于: V4 方案 + 模块分离架构

---

## 1. 版本演进与设计决策

### 1.1 V4 → V5 核心变化

| 维度 | V4 方案 | V5 方案 | 变化原因 |
|------|---------|---------|----------|
| 模块结构 | 单一 dex-indexer crate，两个二进制 | 四个独立 crate | 职责分离，独立部署 |
| API 服务 | dex-indexer 内置 | 独立 dex-api crate | 避免资源竞争 |
| 实时通道 | 未规划 | dex-realtime + dex-ws | 双通道架构 |
| 类型共享 | 内部定义 | sui-types + 各 crate 内部 | 减少依赖层级 |

### 1.2 V5 架构核心理念

**"职责分离，独立扩展"**：每个服务单一职责，可独立部署和扩展

1. **dex-indexer**：Checkpoint 事件处理，写入 PostgreSQL
2. **dex-api**：REST API 查询服务，读取 PostgreSQL
3. **dex-realtime**：RPC 实时监听，发布 Redis Stream
4. **dex-ws**：WebSocket 推送服务，消费 Redis Stream

### 1.3 设计决策总结

| 决策 | 选择 | 理由 |
|------|------|------|
| 模块粒度 | 四个独立 crate | 职责清晰，故障隔离 |
| 共享类型 | 不创建 dex-types | 事件类型已在 sui-types，API 类型各自定义 |
| REST+WS | 拆分为两个 crate | 避免长连接与短连接资源竞争 |
| handlers+realtime | 拆分为两个 crate | 保持一致性，独立扩展 |
| schema 位置 | 保留在 dex-indexer | dex-api 依赖 dex-indexer 获取 schema |

---

## 2. 四模块架构

### 2.1 Phase 1 结构（2 个 crate）

```
dex-sui/crates/
├── dex-indexer/         # Checkpoint 索引服务
│   └── src/
│       ├── lib.rs
│       ├── main.rs      # dex-indexer 二进制
│       ├── schema/      # 数据库定义 + migrations
│       │   ├── mod.rs
│       │   └── stored_types.rs
│       └── handlers/    # Checkpoint 事件处理
│           ├── mod.rs
│           ├── fills.rs
│           ├── balances.rs
│           └── positions.rs
│
└── dex-api/             # REST API 服务
    └── src/
        ├── lib.rs
        ├── main.rs      # dex-api 二进制
        ├── types.rs     # API 请求/响应类型
        ├── server.rs    # Axum 服务器
        ├── handlers.rs  # 查询处理
        └── cache/       # [Phase 3] Redis 缓存
            └── mod.rs
```

### 2.2 Phase 2 结构（4 个 crate）

```
dex-sui/crates/
├── dex-indexer/         # (不变)
├── dex-api/             # REST API (不变)
│
├── dex-realtime/        # [新增] 实时事件采集
│   └── src/
│       ├── lib.rs
│       ├── main.rs      # dex-realtime 二进制
│       ├── listener.rs  # Sui RPC 事件订阅
│       └── publisher.rs # Redis Stream 发布
│
└── dex-ws/              # [新增] WebSocket 推送服务
    └── src/
        ├── lib.rs
        ├── main.rs      # dex-ws 二进制
        ├── types.rs     # WS 订阅/消息类型
        ├── server.rs    # WebSocket 服务器
        ├── channels.rs  # 订阅频道管理
        └── subscriber.rs # Redis 订阅消费
```

### 2.3 类型定义策略

| 类型 | 位置 | 说明 |
|------|------|------|
| 事件类型 | `sui-types/src/dex_events.rs` | 已存在，FillEvent, PositionUpdateEvent 等 |
| API 类型 | `dex-api/src/types.rs` | InfoRequest, FillResponse 等 |
| WS 类型 | `dex-ws/src/types.rs` | Subscription, WsMessage 等 |

### 2.4 依赖关系

```
    sui-types
        │
        ├─────────────────────┐
        │                     │
    dex-indexer           dex-realtime
        │                     │
        │                Redis Stream
        │                     │
        ├─────────────────────┤
        │                     │
    dex-api               dex-ws
  (PostgreSQL)           (Redis)
```

---

## 3. 各模块详细设计

### 3.1 dex-indexer

**职责**：从 Checkpoint 中提取 DEX 事件，写入 PostgreSQL

**核心组件**：

| 组件 | 职责 |
|------|------|
| `handlers/fills.rs` | 处理 FillEvent，写入 dex_fills 表 |
| `handlers/balances.rs` | 处理 BalanceUpdateEvent，写入 dex_balances 表 |
| `handlers/positions.rs` | 处理 PositionUpdateEvent，写入 dex_positions 表 |
| `schema/mod.rs` | Diesel 表定义，StoredFill, StoredBalance 等 |

**依赖**：
- `sui-indexer-alt-framework`：Checkpoint 处理框架
- `sui-types`：DEX 事件类型定义
- `diesel`：PostgreSQL ORM

### 3.2 dex-api

**职责**：提供 REST API 查询服务

**核心组件**：

| 组件 | 职责 |
|------|------|
| `types.rs` | InfoRequest 枚举，各种 Response 类型 |
| `server.rs` | Axum 服务器，路由配置 |
| `handlers.rs` | 查询处理，调用数据库 |
| `cache/` | [Phase 3] Redis 缓存层 |

**API 端点**（参考 Hyperliquid）：

| 端点 | 方法 | 请求类型 | 说明 |
|------|------|----------|------|
| `/health` | GET | - | 健康检查 |
| `/info` | POST | `type: "userFills"` | 用户成交历史 |
| `/info` | POST | `type: "userBalances"` | 用户余额变化 |
| `/info` | POST | `type: "recentFills"` | 市场最近成交 |
| `/info` | POST | `type: "clearinghouseState"` | 用户持仓状态 |

**依赖**：
- `dex-indexer`：仅用于 schema 定义
- `axum`：Web 框架
- `sui-pg-db`：数据库连接池

### 3.3 dex-realtime（Phase 2）

**职责**：订阅 Sui RPC 事件，发布到 Redis Stream

**核心组件**：

| 组件 | 职责 |
|------|------|
| `listener.rs` | Sui RPC WebSocket 订阅 |
| `publisher.rs` | Redis Stream 发布 |

**事件类型**：

| 事件 | Redis Stream Key |
|------|------------------|
| FillEvent | `dex:fills` |
| PositionUpdateEvent | `dex:positions` |
| OrderBookUpdate | `dex:orderbook:{perpetual_id}` |

**依赖**：
- `sui-sdk`：Sui RPC 客户端
- `redis`：Redis 客户端

### 3.4 dex-ws（Phase 2）

**职责**：WebSocket 推送服务

**核心组件**：

| 组件 | 职责 |
|------|------|
| `types.rs` | 订阅请求、推送消息类型 |
| `server.rs` | WebSocket 服务器 |
| `channels.rs` | 订阅频道管理 |
| `subscriber.rs` | Redis Stream 消费 |

**订阅频道**（参考 Hyperliquid）：

| 频道 | 数据 | 说明 |
|------|------|------|
| `l2Book:{perpetual_id}` | 订单簿增量 | 买卖盘变化 |
| `trades:{perpetual_id}` | 实时成交 | 市场成交记录 |
| `orderUpdates:{address}` | 订单状态 | 用户订单变化 |
| `fills:{address}` | 用户成交 | 用户成交通知 |

**依赖**：
- `tokio-tungstenite`：WebSocket 库
- `redis`：Redis 客户端

---

## 4. 数据流设计

### 4.1 Phase 1: OnChain 数据流

```
Sui Checkpoint ──► dex-indexer ──► PostgreSQL ──► dex-api
                                                     │
                                                REST 客户端
```

**延迟**：~3-5 秒（Checkpoint 确认时间）

**特点**：
- 数据一致性高（最终确认）
- 无需回滚处理
- 适合历史查询

### 4.2 Phase 2: 双通道数据流

```
Sui Node
    │
    ├── RPC 订阅 ──► dex-realtime ──► Redis Stream ──► dex-ws
    │   (<500ms)                                         │
    │                                              WebSocket 客户端
    │
    └── Checkpoint ──► dex-indexer ──► PostgreSQL ──► dex-api
        (3-5s)                                           │
                                                    REST 客户端
```

**双通道对比**：

| 通道 | 延迟 | 数据来源 | 用途 |
|------|------|----------|------|
| OnChain | 3-5s | Checkpoint | 历史查询、持久化 |
| OffChain | <500ms | RPC 事件 | 实时推送、订单簿 |

### 4.3 Phase 3: 缓存优化

```
dex-realtime ──► Redis Stream
                      │
              ┌───────┴───────┐
              │               │
          dex-ws         dex-api/cache
                              │
                          dex-api
```

**缓存策略**：

| 数据类型 | 优先数据源 | 回退数据源 |
|---------|-----------|-----------|
| 订单簿 | Redis | PostgreSQL 聚合 |
| 活跃订单 | Redis | PostgreSQL |
| 仓位快照 | Redis | PostgreSQL |
| 成交历史 | PostgreSQL | - |
| K线数据 | PostgreSQL | - |

---

## 5. 关键设计决策

### 5.1 为什么 REST 和 WebSocket 分离？

**问题分析**：

| 维度 | REST API | WebSocket |
|------|----------|-----------|
| 连接类型 | 短连接（请求/响应） | 长连接（持久） |
| 内存占用 | 低（无状态） | 高（每连接状态） |
| 并发模型 | 请求级 | 连接级 |

**分离优势**：
1. **资源隔离**：WebSocket 高并发不影响 REST 响应
2. **独立扩展**：可按需扩展 WebSocket 实例
3. **故障隔离**：一个服务异常不影响另一个

### 5.2 为什么 handlers 和 realtime 分离？

**问题分析**：

| 维度 | handlers (Checkpoint) | realtime (RPC) |
|------|----------------------|----------------|
| 数据源 | Checkpoint 批量 | RPC 事件流 |
| 故障恢复 | 可从任意点恢复 | 断连则丢失 |
| 存储后端 | PostgreSQL | Redis |

**分离优势**：
1. **故障隔离**：RPC 断连不影响 Checkpoint 索引
2. **独立扩展**：可部署多实例监听
3. **一致性**：与 REST/WS 分离保持一致

### 5.3 为什么不创建 dex-types？

**分析**：
- 事件类型已在 `sui-types/src/dex_events.rs`
- API 类型仅 dex-api 使用
- WS 类型仅 dex-ws 使用

**结论**：无需额外共享层，各 crate 内部定义类型更简洁

---

## 6. 技术栈

### 6.1 各模块依赖

| 模块 | 核心依赖 |
|------|----------|
| dex-indexer | sui-indexer-alt-framework, diesel, sui-types |
| dex-api | axum, sui-pg-db, dex-indexer (schema) |
| dex-realtime | sui-sdk, redis |
| dex-ws | tokio-tungstenite, redis |

### 6.2 存储系统

| 存储 | 用途 | 数据 |
|------|------|------|
| PostgreSQL | 持久化 | 成交历史、余额变化、持仓状态 |
| Redis Stream | 实时流 | 事件推送、订单簿更新 |
| Redis Hash | 缓存 | 活跃订单、当前仓位 |

---

## 7. 与 V4 的兼容性

### 7.1 保留的设计

| 设计 | 说明 |
|------|------|
| 虚拟 Package 地址 | DEX_EVENTS_PACKAGE 常量不变 |
| 事件类型定义 | FillEvent, PositionUpdateEvent 等不变 |
| API 风格 | POST /info 模式不变 |
| 数据库 schema | 表结构不变 |

### 7.2 迁移路径

1. **Step 1**：创建 dex-api crate，迁移 API 代码
2. **Step 2**：从 dex-indexer 删除 API 相关代码
3. **Step 3**：更新 workspace 配置
4. **Step 4**：验证编译和测试

---

## 8. 设计参考

| 设计维度 | 参考来源 | 说明 |
|---------|---------|------|
| **REST API** | Hyperliquid | POST /info 模式 |
| **WebSocket** | Hyperliquid | 订阅频道设计 |
| **事件设计** | dYdX v4 | OnChain/OffChain 双通道 |
| **模块分离** | 微服务架构 | 单一职责原则 |
