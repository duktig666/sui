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
        direction TB
        subgraph L1["L1 实时层 - Redis Cluster (&lt;1ms)"]
            Redis["Redis"]
            R1["订单簿价格层级"]
            R2["活跃订单状态"]
            R3["仓位快照"]
            R4["中间价缓存"]
        end
        subgraph L2["L2 时序层 - TimescaleDB (&lt;10ms)"]
            TimescaleDB["TimescaleDB"]
            T1["K线数据 (candles)"]
            T2["成交记录 (fills)"]
            T3["资金费率 (funding_rates)"]
            T4["订单记录 (orders)"]
            T5["仓位历史 (positions)"]
        end
        subgraph L3["L3 分析层 - ClickHouse (&lt;500ms)"]
            ClickHouse["ClickHouse"]
            CH1["历史订单归档"]
            CH2["仓位快照归档"]
            CH3["交易分析视图"]
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
        subgraph RealtimeChannel["实时通道 (RPC 订阅)<br/>&lt;500ms 延迟"]
            RTL["Realtime Listener<br/>订阅 Sui RPC"]
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
        FullNode["Full Node<br/>(公共节点)"]
        IndexNode["Index Full Node<br/>(索引专用)"]
        CheckpointStore["Checkpoint Store<br/>(RocksDB)"]
    end

    %% 链上层数据流
    DEX --> Match --> Events

    %% Sui 网络
    Events -->|"打包进 Checkpoint"| Validators
    Validators -->|"同步"| FullNode
    FullNode -->|"同步"| IndexNode
    IndexNode --> CheckpointStore

    %% 数据摄取
    IndexNode -->|"RPC 订阅<br/>TransactionEffects"| RTL
    CheckpointStore -->|"批量读取"| CP

    %% 消息队列
    RTL -->|"OffChainUpdate"| RedisStream
    CP -->|"OnChainUpdate"| Kafka

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
    classDef network fill:#e8eaf6,stroke:#3f51b5

    class DEX,Match,Events onchain
    class RTL,CP ingestion
    class RedisStream,Kafka mq
    class OP,TP,HP,RS processing
    class Redis,TimescaleDB,ClickHouse,R1,R2,R3,R4,T1,T2,T3,T4,T5,CH1,CH2,CH3 storage
    class REST,WS api
    class C1,C2,C3 client
    class Validators,FullNode,IndexNode,CheckpointStore network
```

## 图例说明

| 颜色 | 层级 | 说明 |
|------|------|------|
| 浅蓝 (#e1f5fe) | 链上层 | 自定义 DEX 引擎及事件发射 |
| 浅靛 (#e8eaf6) | Sui 网络层 | Validator/Full Node/Index Node/Checkpoint Store |
| 浅紫 | 数据摄取层 | 双通道数据摄取 (实时 RPC 订阅 + Checkpoint) |
| 浅橙 | 消息队列层 | Redis Stream (实时) + Kafka (批量) |
| 浅绿 | 处理服务层 | 各类 Processor 业务处理 |
| 浅粉 | 存储层 | 三层数据库 (Redis/TimescaleDB/ClickHouse) |
| 浅蓝 (#e3f2fd) | API 层 | REST + WebSocket |
| 灰色 | 客户端 | 交易前端/做市商/分析工具 |

## 关键数据流

### 实时通道 (RPC 订阅)
```
Sui RPC → Realtime Listener → Redis Stream → Orderbook Processor → Redis
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

## 数据存储映射

| 数据类型 | 来源通道 | 存储目标 | 用途 |
|---------|---------|---------|------|
| 订单簿价格层级 | 实时通道 | Redis | 实时行情展示 |
| 活跃订单状态 | 实时通道 | Redis | 用户订单查询 |
| 仓位快照 | 实时通道 | Redis | 实时仓位展示 |
| 中间价缓存 | 实时通道 | Redis | 快速价格查询 |
| K线数据 (candles) | Checkpoint | TimescaleDB | 历史K线查询 |
| 成交记录 (fills) | Checkpoint | TimescaleDB | 成交历史 |
| 资金费率 (funding_rates) | Checkpoint | TimescaleDB | 资金费率历史 |
| 订单记录 (orders) | Checkpoint | TimescaleDB | 订单历史 |
| 仓位历史 (positions) | Checkpoint | TimescaleDB | 仓位变更记录 |
| 历史订单归档 | Checkpoint | ClickHouse | 长期存储/分析 |
| 仓位快照归档 | Checkpoint | ClickHouse | 长期存储/分析 |
| 交易分析视图 | Checkpoint | ClickHouse | OLAP 分析 |

## 网络节点说明

| 节点类型 | 用途 | 连接方 |
|---------|------|--------|
| **Full Node (公共节点)** | 接收客户端交易请求 | 交易前端、做市商 |
| **Index Full Node (索引专用)** | 为索引器提供数据 | Realtime Listener |
| **Checkpoint Store** | 存储 Checkpoint 数据 | Checkpoint Processor |
