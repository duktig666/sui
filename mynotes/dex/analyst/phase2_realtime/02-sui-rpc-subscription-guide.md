# Sui RPC 订阅指南

## 概述

本文档详细说明如何使用 Sui RPC 的事件订阅功能实现 dex-realtime 的实时事件监听，包括 API 对比、配置指南和代码示例。

---

## 1. 订阅 API 对比

### 1.1 sui_subscribeEvent vs sui_subscribeTransaction

| 特性 | sui_subscribeEvent | sui_subscribeTransaction |
|------|-------------------|-------------------------|
| 数据粒度 | 单个事件 | 完整交易（含所有事件） |
| 过滤能力 | MoveEventType、Sender、Package 等 | TransactionFilter |
| 数据量 | 小（仅匹配事件） | 大（完整交易数据） |
| 适用场景 | 监听特定类型事件 | 需要交易完整上下文 |
| **推荐** | ✓ 用于 dex-realtime | 用于需要交易关联的场景 |

### 1.2 选择理由

选择 `sui_subscribeEvent` 的原因：
1. **数据量小**：只接收感兴趣的事件类型
2. **过滤精确**：通过 MoveEventType 精确匹配 DEX 事件
3. **处理简单**：直接获取事件数据，无需解析交易结构

---

## 2. EventFilter 配置指南

### 2.1 EventFilter 类型

```rust
pub enum EventFilter {
    /// 匹配任意事件（无过滤）
    All,
    /// 匹配发送者地址
    Sender(SuiAddress),
    /// 匹配 Package ID
    Package(ObjectID),
    /// 匹配 Module 名称
    Module { package: ObjectID, module: Identifier },
    /// 匹配事件类型（推荐）
    MoveEventType(StructTag),
    /// 匹配事件字段
    MoveEventField { path: String, value: Value },
    /// 组合过滤：任一匹配
    Any(Vec<EventFilter>),
    /// 组合过滤：全部匹配
    And(Vec<EventFilter>),
    /// 时间范围
    TimeRange { start_time: u64, end_time: u64 },
}
```

### 2.2 MoveEventType 过滤示例

```rust
use sui_sdk::types::base_types::ObjectID;
use sui_types::event::EventFilter;

// DEX Package ID（部署后确定）
const DEX_PACKAGE: &str = "0x1234...";

// 构建事件类型过滤器
fn build_event_filters(package_id: &str) -> Vec<EventFilter> {
    let events = vec![
        "FillEventV1",
        "OrderPlacedEventV1",
        "OrderRemovedEventV1",
        "PositionUpdateEventV1",
        "LiquidationEventV1",
    ];

    events.into_iter()
        .map(|event| {
            let type_str = format!("{}::dex_events::{}", package_id, event);
            EventFilter::MoveEventType(type_str.parse().unwrap())
        })
        .collect()
}

// 组合为 Any 过滤器
let filters = build_event_filters(DEX_PACKAGE);
let combined_filter = EventFilter::Any(filters);
```

### 2.3 过滤器性能考虑

| 过滤方式 | 性能 | 说明 |
|----------|------|------|
| MoveEventType | 最优 | 节点端过滤，数据量最小 |
| Package | 较好 | 接收 Package 所有事件 |
| Module | 较好 | 接收 Module 所有事件 |
| All | 最差 | 接收所有事件，需客户端过滤 |

---

## 3. sui-sdk 代码示例

### 3.1 基础订阅示例

```rust
use sui_sdk::SuiClientBuilder;
use sui_types::event::EventFilter;
use futures::StreamExt;

async fn subscribe_events() -> Result<(), Box<dyn std::error::Error>> {
    // 创建 Sui 客户端
    let sui_client = SuiClientBuilder::default()
        .ws_url("wss://sui-testnet.mystenlabs.com")
        .build()
        .await?;

    // 构建事件过滤器
    let filters = vec![
        EventFilter::MoveEventType(
            "0x1234::dex_events::FillEventV1".parse()?
        ),
        EventFilter::MoveEventType(
            "0x1234::dex_events::OrderPlacedEventV1".parse()?
        ),
        EventFilter::MoveEventType(
            "0x1234::dex_events::OrderRemovedEventV1".parse()?
        ),
    ];

    let filter = EventFilter::Any(filters);

    // 订阅事件流
    let mut stream = sui_client
        .event_api()
        .subscribe_event(filter)
        .await?;

    // 处理事件
    while let Some(event_result) = stream.next().await {
        match event_result {
            Ok(event) => {
                println!("Received event: {:?}", event.type_);
                // 处理事件...
            }
            Err(e) => {
                eprintln!("Event error: {}", e);
            }
        }
    }

    Ok(())
}
```

### 3.2 事件解析示例

```rust
use serde::Deserialize;
use sui_sdk::rpc_types::SuiEvent;

#[derive(Debug, Deserialize)]
pub struct FillEventV1 {
    pub perpetual_id: u32,
    pub fill_id: Vec<u8>,
    pub maker_order_id: Vec<u8>,
    pub taker_order_id: Vec<u8>,
    pub maker_subaccount: Vec<u8>,
    pub taker_subaccount: Vec<u8>,
    pub price: u64,
    pub quantity: u64,
    pub maker_fee: i64,
    pub taker_fee: i64,
    pub timestamp_ms: u64,
}

fn parse_event(event: &SuiEvent) -> Result<(), Box<dyn std::error::Error>> {
    let type_name = event.type_.name.as_str();

    match type_name {
        "FillEventV1" => {
            let fill: FillEventV1 = bcs::from_bytes(&event.bcs)?;
            println!("Fill: perpetual={}, price={}, qty={}",
                fill.perpetual_id, fill.price, fill.quantity);
        }
        "OrderPlacedEventV1" => {
            // 解析 OrderPlacedEventV1...
        }
        "OrderRemovedEventV1" => {
            // 解析 OrderRemovedEventV1...
        }
        _ => {
            println!("Unknown event type: {}", type_name);
        }
    }

    Ok(())
}
```

### 3.3 完整的 dex-realtime 监听器示例

```rust
use std::time::Duration;
use sui_sdk::SuiClientBuilder;
use sui_types::event::EventFilter;
use futures::StreamExt;
use tokio::time::sleep;

pub struct RealtimeListener {
    ws_url: String,
    package_id: String,
    reconnect_delay: Duration,
    max_reconnect_delay: Duration,
}

impl RealtimeListener {
    pub fn new(ws_url: String, package_id: String) -> Self {
        Self {
            ws_url,
            package_id,
            reconnect_delay: Duration::from_secs(1),
            max_reconnect_delay: Duration::from_secs(30),
        }
    }

    pub async fn run(&self) -> Result<(), Box<dyn std::error::Error>> {
        let mut current_delay = self.reconnect_delay;

        loop {
            match self.connect_and_subscribe().await {
                Ok(_) => {
                    // 正常退出，重置延迟
                    current_delay = self.reconnect_delay;
                }
                Err(e) => {
                    eprintln!("Connection error: {}. Reconnecting in {:?}...",
                        e, current_delay);

                    // 指数退避
                    sleep(current_delay).await;
                    current_delay = std::cmp::min(
                        current_delay * 2,
                        self.max_reconnect_delay
                    );
                }
            }
        }
    }

    async fn connect_and_subscribe(&self) -> Result<(), Box<dyn std::error::Error>> {
        let client = SuiClientBuilder::default()
            .ws_url(&self.ws_url)
            .build()
            .await?;

        let filter = self.build_filter()?;
        let mut stream = client.event_api().subscribe_event(filter).await?;

        println!("Connected to {}", self.ws_url);

        while let Some(event_result) = stream.next().await {
            match event_result {
                Ok(event) => {
                    self.handle_event(event).await?;
                }
                Err(e) => {
                    return Err(e.into());
                }
            }
        }

        Ok(())
    }

    fn build_filter(&self) -> Result<EventFilter, Box<dyn std::error::Error>> {
        let event_types = vec![
            "FillEventV1",
            "OrderPlacedEventV1",
            "OrderRemovedEventV1",
            "PositionUpdateEventV1",
            "LiquidationEventV1",
        ];

        let filters: Vec<EventFilter> = event_types
            .into_iter()
            .map(|name| {
                let type_str = format!("{}::dex_events::{}", self.package_id, name);
                EventFilter::MoveEventType(type_str.parse().unwrap())
            })
            .collect();

        Ok(EventFilter::Any(filters))
    }

    async fn handle_event(&self, event: sui_sdk::rpc_types::SuiEvent)
        -> Result<(), Box<dyn std::error::Error>>
    {
        // TODO: 发布到 Redis Stream
        // TODO: 更新内存订单簿
        // TODO: 更新 K 线数据
        println!("Event: {:?}", event.type_);
        Ok(())
    }
}
```

---

## 4. 错误处理和重连策略

### 4.1 常见错误类型

| 错误类型 | 原因 | 处理方式 |
|----------|------|----------|
| 连接失败 | 网络问题、节点不可用 | 指数退避重连 |
| 订阅失败 | 无效过滤器、权限问题 | 检查配置后重试 |
| 流中断 | 节点重启、网络抖动 | 立即重连 |
| 解析失败 | 事件格式变更 | 记录日志，跳过该事件 |

### 4.2 指数退避重连策略

```rust
pub struct ReconnectConfig {
    /// 初始重连延迟（默认 1 秒）
    pub initial_delay_ms: u64,
    /// 最大重连延迟（默认 30 秒）
    pub max_delay_ms: u64,
    /// 退避乘数（默认 2）
    pub backoff_multiplier: u32,
    /// 成功连接后重置延迟的时间（默认 60 秒）
    pub reset_after_success_secs: u64,
}

impl Default for ReconnectConfig {
    fn default() -> Self {
        Self {
            initial_delay_ms: 1000,
            max_delay_ms: 30000,
            backoff_multiplier: 2,
            reset_after_success_secs: 60,
        }
    }
}
```

### 4.3 重连状态机

```
┌─────────────┐
│  Connecting │
└──────┬──────┘
       │
       ▼ 成功
┌─────────────┐
│  Subscribed │ ◄────────────────────┐
└──────┬──────┘                      │
       │                             │
       ▼ 错误/断开                    │
┌─────────────┐                      │
│   Waiting   │ ─── 延迟到期 ─────────┘
│  (指数退避)  │
└─────────────┘
```

---

## 5. 节点连接配置

### 5.1 分阶段节点策略

| 阶段 | 连接方式 | 配置示例 |
|------|----------|----------|
| 开发/测试 | 公共 RPC | `wss://sui-testnet.mystenlabs.com` |
| 生产初期 | 自有 Full Node | `wss://my-sui-fullnode:9000` |
| 生产成熟 | 多节点冗余 | 配置列表 + 故障转移 |

### 5.2 配置文件示例

```toml
# dex-realtime/config.toml

[sui]
# 主节点
ws_url = "wss://sui-testnet.mystenlabs.com"
# 备用节点（可选）
backup_ws_urls = []

# DEX Package ID
package_id = "0x1234567890abcdef..."

[reconnect]
initial_delay_ms = 1000
max_delay_ms = 30000
backoff_multiplier = 2

[redis]
url = "redis://localhost:6379"
stream_prefix = "dex:stream:"
```

### 5.3 多节点故障转移（后续扩展）

```rust
pub struct MultiNodeConfig {
    /// 主节点
    pub primary: String,
    /// 备用节点列表
    pub backups: Vec<String>,
    /// 健康检查间隔
    pub health_check_interval_secs: u64,
    /// 故障转移阈值（连续失败次数）
    pub failover_threshold: u32,
}
```

---

## 6. 性能优化建议

### 6.1 批量处理

```rust
use tokio::sync::mpsc;
use tokio::time::{interval, Duration};

async fn batch_processor(
    mut rx: mpsc::Receiver<SuiEvent>,
    batch_size: usize,
    flush_interval: Duration,
) {
    let mut batch = Vec::with_capacity(batch_size);
    let mut timer = interval(flush_interval);

    loop {
        tokio::select! {
            Some(event) = rx.recv() => {
                batch.push(event);
                if batch.len() >= batch_size {
                    process_batch(&mut batch).await;
                }
            }
            _ = timer.tick() => {
                if !batch.is_empty() {
                    process_batch(&mut batch).await;
                }
            }
        }
    }
}

async fn process_batch(batch: &mut Vec<SuiEvent>) {
    // 批量写入 Redis Stream
    // 批量更新订单簿
    batch.clear();
}
```

### 6.2 建议配置

| 参数 | 建议值 | 说明 |
|------|--------|------|
| 批处理大小 | 100 | 累积 100 个事件后处理 |
| 刷新间隔 | 10ms | 最大延迟 10ms |
| 连接超时 | 10s | WebSocket 连接超时 |
| 读取超时 | 30s | 无消息超时（心跳检测） |

---

## 7. 监控指标

### 7.1 关键指标

| 指标 | 类型 | 说明 |
|------|------|------|
| `realtime_events_received_total` | Counter | 接收事件总数 |
| `realtime_events_processed_total` | Counter | 处理事件总数 |
| `realtime_reconnect_total` | Counter | 重连次数 |
| `realtime_event_latency_ms` | Histogram | 事件处理延迟 |
| `realtime_connection_status` | Gauge | 连接状态（0/1） |

### 7.2 Prometheus 指标示例

```rust
use prometheus::{Counter, Gauge, Histogram};

lazy_static! {
    static ref EVENTS_RECEIVED: Counter = Counter::new(
        "realtime_events_received_total",
        "Total events received from Sui RPC"
    ).unwrap();

    static ref CONNECTION_STATUS: Gauge = Gauge::new(
        "realtime_connection_status",
        "WebSocket connection status (1=connected, 0=disconnected)"
    ).unwrap();

    static ref EVENT_LATENCY: Histogram = Histogram::with_opts(
        HistogramOpts::new(
            "realtime_event_latency_ms",
            "Event processing latency in milliseconds"
        ).buckets(vec![1.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0])
    ).unwrap();
}
```
