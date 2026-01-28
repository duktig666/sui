# Sui DEX Indexer 架构结构图

> 基于 `dex-indexer-analyst.md` 文档生成的完整架构图

## 架构全景图

```mermaid
flowchart TB
    subgraph Client["客户端层"]
        direction LR
        C1["交易前端"]
        C2["做市商系统"]
        C3["分析工具"]
    end

    subgraph API["API 服务层"]
        direction LR
        REST["REST API<br/>(Axum)"]
        WS["WebSocket Server<br/>(实时推送)"]
    end

    subgraph Storage["存储层 (三层架构)"]
        direction LR
        subgraph L1["L1 实时层"]
            Redis["Redis Cluster<br/>订单簿/活跃订单/仓位<br/>&lt;1ms"]
        end
        subgraph L2["L2 时序层"]
            TimescaleDB["TimescaleDB<br/>K线/成交/资金费率<br/>&lt;10ms"]
        end
        subgraph L3["L3 分析层"]
            ClickHouse["ClickHouse<br/>历史订单/交易分析<br/>&lt;500ms"]
        end
    end

    subgraph Processing["处理服务层"]
        direction LR
        OP["Orderbook Processor<br/>(实时订单簿)"]
        TP["Trade Processor<br/>(K线/成交)"]
        HP["History Processor<br/>(历史数据)"]
        RS["Reconciliation Service<br/>(数据对账)"]
    end

    subgraph MQ["消息队列层"]
        direction LR
        subgraph Realtime["实时通道"]
            RedisStream["Redis Stream<br/>realtime-orders<br/>realtime-trades<br/>realtime-positions"]
        end
        subgraph Batch["批量通道"]
            Kafka["Kafka/Pulsar<br/>checkpoint-data"]
        end
    end

    subgraph Ingestion["数据摄取层 (双通道)"]
        direction LR
        subgraph FastPath["FastPath 通道<br/>&lt;500ms 延迟"]
            FPL["FastPath Listener<br/>订阅 Sui RPC"]
        end
        subgraph CheckpointPath["Checkpoint 通道<br/>3-5s 延迟"]
            CP["Checkpoint Processor<br/>sui-indexer-alt Pipeline"]
        end
    end

    subgraph OnChain["链上层 (On-chain)"]
        direction LR
        DEX["自定义 DEX 引擎"]
        Match["订单匹配引擎"]
        Events["事件发射<br/>(OrderPlaced/Matched/...)"]
    end

    subgraph SuiNetwork["Sui 网络"]
        direction LR
        Validators["Validator 集群"]
        FullNode["Indexer Full Node<br/>(专用全节点)"]
        CheckpointStore["Checkpoint Store<br/>(RocksDB)"]
    end

    %% 链上层数据流
    DEX --> Match --> Events

    %% Sui 网络
    Events -->|"打包进 Checkpoint"| Validators
    Validators -->|"同步"| FullNode
    FullNode --> CheckpointStore

    %% 数据摄取
    FullNode -->|"RPC 订阅<br/>TransactionEffects"| FPL
    CheckpointStore -->|"批量读取"| CP

    %% 消息队列
    FPL -->|"OffChainUpdate"| RedisStream
    CP -->|"Move Events"| Kafka

    %% 处理服务
    RedisStream --> OP
    RedisStream --> TP
    Kafka --> TP
    Kafka --> HP
    TP -.->|"StateFilledQuantumsCache"| RS
    RS -.->|"对账补偿"| OP

    %% 存储写入
    OP -->|"订单簿/订单状态"| Redis
    TP -->|"K线/成交记录"| TimescaleDB
    TP -->|"已确认成交量"| Redis
    HP -->|"历史订单/仓位快照"| ClickHouse

    %% API 读取
    REST -->|"查询"| Redis
    REST -->|"历史K线"| TimescaleDB
    REST -->|"历史订单"| ClickHouse
    WS -->|"订阅"| Redis

    %% 客户端
    C1 <-->|"行情订阅"| WS
    C1 -->|"历史查询"| REST
    C2 <-->|"订单簿"| WS
    C3 -->|"分析查询"| REST

    %% 直连 Sui (下单)
    C1 -.->|"下单/取消<br/>直连 Sui RPC"| FullNode
    C2 -.->|"下单/取消<br/>直连 Sui RPC"| FullNode

    %% 样式
    classDef onchain fill:#e1f5fe,stroke:#01579b
    classDef ingestion fill:#f3e5f5,stroke:#4a148c
    classDef mq fill:#fff3e0,stroke:#e65100
    classDef processing fill:#e8f5e9,stroke:#1b5e20
    classDef storage fill:#fce4ec,stroke:#880e4f
    classDef api fill:#e3f2fd,stroke:#0d47a1
    classDef client fill:#f5f5f5,stroke:#424242

    class DEX,Match,Events onchain
    class FPL,CP ingestion
    class RedisStream,Kafka mq
    class OP,TP,HP,RS processing
    class Redis,TimescaleDB,ClickHouse storage
    class REST,WS api
    class C1,C2,C3 client
```

## 图例说明

| 颜色 | 层级 | 说明 |
|------|------|------|
| 浅蓝 | 链上层 | 自定义 DEX 引擎及事件发射 |
| 浅紫 | 数据摄取层 | 双通道数据摄取 (FastPath + Checkpoint) |
| 浅橙 | 消息队列层 | Redis Stream (实时) + Kafka (批量) |
| 浅绿 | 处理服务层 | 各类 Processor 业务处理 |
| 浅粉 | 存储层 | 三层数据库 (Redis/TimescaleDB/ClickHouse) |
| 浅蓝 | API 层 | REST + WebSocket |
| 灰色 | 客户端 | 交易前端/做市商/分析工具 |

## 关键数据流

### 实时通道 (FastPath)
```
Sui RPC → FastPath Listener → Redis Stream → Orderbook Processor → Redis
延迟: <500ms
用途: 订单簿实时更新、活跃订单状态
```

### 批量通道 (Checkpoint)
```
Checkpoint Store → sui-indexer-alt → Kafka → Trade/History Processor → TimescaleDB/ClickHouse
延迟: 3-5s
用途: K线聚合、成交持久化、历史数据
```

### 客户端连接
| 操作 | 连接目标 | 协议 |
|------|----------|------|
| 订阅行情/订单簿 | Indexer WebSocket | WS |
| 查询历史数据 | Indexer REST API | HTTP |
| **下单/取消** | **Sui Full Node RPC** | **HTTP/WS** |
