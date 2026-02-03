# sui-indexer-alt 部署架构

## 概述

sui-indexer-alt 是 Sui 区块链的新一代索引器服务，是一个**完全链下的独立服务**，不参与共识。它从 Checkpoint 数据中提取、转换、存储区块链数据，为 RPC 服务（GraphQL、JSON-RPC）提供数据支撑。

**关键特性**：
- 链下独立运行，不影响链上性能
- 支持多种数据源（远程存储、本地文件、RPC、gRPC）
- 高度可扩展的 Pipeline 架构
- 支持水平扩展和高可用部署

## 数据获取架构

### 核心问题：连接全节点还是验证者节点？

**答案：都不是。sui-indexer-alt 不直接连接节点，而是从 Checkpoint Store 获取数据。**

Checkpoint 是 Sui 的状态快照机制：
- 生成频率：每 ~3 秒
- 内容：1000-5000 条交易
- 签名：2f+1 个验证者签名

### 支持的 4 种数据源

sui-indexer-alt 通过 `IngestionClient` 支持 4 种数据源，按优先级排列：

| 数据源 | 启动参数 | 适用场景 | 延迟 |
|--------|---------|---------|------|
| **Remote Store** | `--remote-store-url` | 生产环境（S3/GCS） | 秒级 |
| **Local Path** | `--local-ingestion-path` | 开发测试 | 无延迟 |
| **RPC API** | `--rpc-api-url` | 从全节点 RPC 获取 | 秒级 |
| **gRPC Streaming** | `--streaming-url` | 实时低延迟同步 | 亚秒级 |

#### 数据源详解

**1. Remote Store（推荐生产环境）**
```bash
sui-indexer-alt indexer \
  --remote-store-url https://checkpoints.mainnet.sui.io \
  --database-url postgres://localhost:5432/sui_indexer
```
- Checkpoint 由验证者/全节点上传到 S3/GCS
- 索引器从云存储拉取，无需直连节点
- 高可用、易扩展

**2. Local Path（开发测试）**
```bash
sui-indexer-alt indexer \
  --local-ingestion-path ./checkpoints \
  --database-url postgres://localhost:5432/sui_indexer
```
- 从本地文件系统读取 Checkpoint
- 适合离线分析或测试

**3. RPC API**
```bash
sui-indexer-alt indexer \
  --rpc-api-url http://sui-node:9000 \
  --database-url postgres://localhost:5432/sui_indexer
```
- 通过全节点的 RPC 接口获取 Checkpoint
- 延迟较高，但部署简单

**4. gRPC Streaming（低延迟场景）**
```bash
sui-indexer-alt indexer \
  --rpc-api-url http://sui-node:9000 \
  --streaming-url grpc://sui-node:50051 \
  --database-url postgres://localhost:5432/sui_indexer
```
- 实时接收 Checkpoint 流
- 延迟最低，适合对实时性要求高的场景

### 数据流架构图

```
┌─────────────────────────────────────────────────────────────┐
│                 Sui 区块链网络                              │
│  ┌──────────────┐    ┌──────────────┐                       │
│  │  Validators  │    │  Full Nodes  │  ← 生成 Checkpoints  │
│  └──────────────┘    └──────────────┘                       │
└─────────────────────────────────────────────────────────────┘
           │                    │
           └────────┬───────────┘
                    ▼
        ┌───────────────────────┐
        │  Checkpoint Store     │
        │  (S3/GCS/Local/RPC)   │
        └───────────┬───────────┘
                    │
     ┌──────────────┴──────────────┐
     │   sui-indexer-alt          │  ← 完全链下
     │   Ingestion Service        │
     └───────────┬─────────────────┘
                 │
     ┌───────────┴─────────────────┐
     │       Pipeline System       │
     │  (21+ 并发/顺序 Handlers)   │
     └───────────┬─────────────────┘
                 │
     ┌───────────┴─────────────────┐
     │       PostgreSQL            │
     └───────────┬─────────────────┘
                 │
     ┌───────────┴─────────────────┐
     │   GraphQL / JSON-RPC        │
     └─────────────────────────────┘
```

## 部署模式

### 核心问题：每个节点都需要运行 sui-indexer-alt 吗？

**答案：不需要。**

sui-indexer-alt 是独立的链下服务：
- 不参与共识
- 一个实例可以为整个应用提供查询服务
- 可以部署多个实例实现高可用

### 部署方案对比

| 方案 | 拓扑结构 | 优点 | 缺点 |
|------|---------|------|------|
| **单实例** | 1 indexer + 1 PG | 简单、成本低 | 单点故障 |
| **高可用** | 2+ indexer + PG 主从 | 容错能力强 | 运维复杂 |
| **分片** | N indexer 各负责不同范围 | 高吞吐 | 架构复杂 |

### 典型生产部署

```
                        ┌─────────────────┐
                        │  Load Balancer  │
                        └────────┬────────┘
                                 │
              ┌──────────────────┼──────────────────┐
              │                  │                  │
      ┌───────┴───────┐  ┌───────┴───────┐  ┌───────┴───────┐
      │  GraphQL #1   │  │  GraphQL #2   │  │  GraphQL #3   │
      └───────┬───────┘  └───────┬───────┘  └───────┬───────┘
              │                  │                  │
              └──────────────────┼──────────────────┘
                                 │
                        ┌────────┴────────┐
                        │  PostgreSQL     │
                        │  (Primary)      │
                        └────────┬────────┘
                                 │
                        ┌────────┴────────┐
                        │  PostgreSQL     │
                        │  (Replica)      │
                        └─────────────────┘
                                 ▲
              ┌──────────────────┼──────────────────┐
              │                  │                  │
      ┌───────┴───────┐  ┌───────┴───────┐  ┌───────┴───────┐
      │  Indexer #1   │  │  Indexer #2   │  │  Indexer #3   │
      │  (Active)     │  │  (Standby)    │  │  (Standby)    │
      └───────────────┘  └───────────────┘  └───────────────┘
                                 ▲
                        ┌────────┴────────┐
                        │ Checkpoint Store│
                        │ (S3/GCS)        │
                        └─────────────────┘
```

**说明**：
- 多个 Indexer 可以同时写入同一 PostgreSQL（幂等写入保证）
- GraphQL 服务可水平扩展
- Checkpoint Store 作为共享数据源

## RPC 服务层

### 对外提供的服务

sui-indexer-alt 提供三种 RPC 服务：

| 服务 | 默认端口 | 协议 | 用途 |
|------|---------|------|------|
| **GraphQL** | 7000 | HTTP | 主要查询接口（推荐） |
| **JSON-RPC** | 9000 | HTTP | 兼容旧版接口 |
| **Consistent API** | 50052 | gRPC | 强一致性快照查询 |

### GraphQL 服务

```bash
# 启动
sui-indexer-alt-graphql rpc \
  --indexer-config indexer_alt_config.toml

# 查询示例
curl http://localhost:7000/graphql \
  -X POST \
  -H "Content-Type: application/json" \
  -d '{"query":"{ checkpoint(id:1000000) { sequenceNumber timestamp } }"}'
```

支持的查询：
- Checkpoint 信息
- 交易历史
- 对象状态
- 事件查询
- 余额查询

### JSON-RPC 服务

```bash
sui-indexer-alt-jsonrpc rpc \
  --database-url postgres://localhost:5432/sui_indexer
```

### Consistent Store（强一致性查询）

基于 RocksDB，提供某个 checkpoint 时刻的强一致性视图：
- `ListOwnedObjects`: 列出某地址拥有的对象
- `ListObjectsByType`: 列出某类型的对象
- `GetBalance`: 获取地址余额
- `GetAvailableRange`: 获取可查询范围

## 配置参数说明

### 生成配置文件

```bash
sui-indexer-alt generate-config > indexer_alt_config.toml
```

### 核心配置项

```toml
[ingestion]
checkpoint_buffer_size = 5000          # Checkpoint 缓冲区大小
ingest_concurrency = 200               # 并发下载数
retry_interval_ms = 200                # 重试间隔

[committer]
write_concurrency = 5                  # 并发写入数据库连接数
collect_interval_ms = 500              # 批次收集间隔
watermark_interval_ms = 500            # 水位更新间隔

[pruner]
interval_ms = 300000                   # Prune 检查间隔（5分钟）
delay_ms = 120000                      # Prune 延迟（2分钟）
retention = 4000000                    # 保留 Checkpoint 数量
max_chunk_size = 2000                  # 单次删除数量
```

### 性能调优建议

| 场景 | 参数调整 | 说明 |
|------|---------|------|
| **追赶历史** | `write_concurrency = 10`<br/>`collect_interval_ms = 100` | 高吞吐 |
| **实时同步** | `collect_interval_ms = 50`<br/>`watermark_interval_ms = 100` | 低延迟 |
| **内存紧张** | `checkpoint_buffer_size = 1000` | 限制缓冲 |

## DEX 场景推荐方案

### 推荐架构

```
┌─────────────────────────────────────────────────────────────┐
│                    Checkpoint Store                         │
│                    (S3 或全节点 RPC)                        │
└────────────────────────────┬────────────────────────────────┘
                             │
┌────────────────────────────┴────────────────────────────────┐
│               sui-indexer-alt 实例                          │
│  - 配置: --remote-store-url 或 --rpc-api-url               │
│  - Pipeline: 启用 kv_transactions, ev_emit_mod 等          │
└────────────────────────────┬────────────────────────────────┘
                             │
┌────────────────────────────┴────────────────────────────────┐
│                    PostgreSQL                               │
│  - 主从部署保证高可用                                        │
│  - 定期备份                                                 │
└────────────────────────────┬────────────────────────────────┘
                             │
┌────────────────────────────┴────────────────────────────────┐
│                 GraphQL API 服务                            │
│  - 多实例 + 负载均衡                                        │
│  - 提供交易查询、事件查询、余额查询等                        │
└────────────────────────────┬────────────────────────────────┘
                             │
┌────────────────────────────┴────────────────────────────────┐
│                    DEX 应用层                               │
│  - 查询订单历史、成交记录、资金变动                          │
│  - 与链下订单簿引擎配合                                     │
└─────────────────────────────────────────────────────────────┘
```

### 关键考量

1. **延迟**：Checkpoint 生成 ~3 秒，索引延迟 1-5 秒，总延迟 4-8 秒
2. **实时数据**：订单簿、K 线等实时数据建议链下引擎直接提供
3. **链上数据**：成交结算、资金划转通过 sui-indexer-alt 索引
4. **扩展性**：可自定义 Pipeline 处理 DEX 特定事件

## 总结

| 问题 | 答案 |
|------|------|
| 连接全节点还是验证者？ | 都不是，从 Checkpoint Store 获取数据 |
| 每个节点都需要运行吗？ | 不需要，是独立的链下服务 |
| 如何提供 RPC 服务？ | GraphQL（推荐）+ JSON-RPC，可独立扩展 |
| 部署模式？ | 单机/HA/分片，按需选择 |

## 参考资料

- 源码：`sui/crates/sui-indexer-alt-framework/`
- 配置：`sui/crates/sui-indexer-alt/src/config.rs`
- 深度分析：`sui/mynotes/dex/analyst/sui-indexer-alt-analyst.md`
