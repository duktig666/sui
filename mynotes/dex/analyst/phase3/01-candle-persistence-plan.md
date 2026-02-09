# Phase 3: K 线持久化与数据完整性技术方案

## 背景

Phase 2 完成后，K 线（OHLCV candlestick）数据仅存于 Redis：
- **当前 candle**: Redis Hash `dex:candle:{perpetual_id}:{interval}`
- **历史 candle**: Redis Sorted Set `dex:candles:{perpetual_id}:{interval}`

存在以下问题：

| 编号 | 问题 | 影响 |
|------|------|------|
| 1 | Redis 重启 → 所有 candle 数据丢失 | K 线图清空 |
| 2 | 长时间无交易 → K 线图出现时间空洞 | 图表不连续 |
| 3 | current candle Hash 过期但 API 仍当作"当前"返回 | 数据不准确 |
| 4 | ZSet `INTERVAL_MAX_CANDLES` 裁剪后旧数据永久丢失 | 长期历史不可查 |

**不做的事**：
- 订单簿无需 PG 持久化（下一个 OrderbookSnapshotEvent 完全覆盖）
- K 线 WebSocket 推送机制本身正常，无需修改

---

## P0: Candle 持久化到 PostgreSQL（写穿模式）

### 设计思路

在跨间隔过渡（interval transition）时，将已完成的 candle 写入 PostgreSQL。
这是 **write-through** 模式——每次旧 candle 被归档到 ZSet 时，同步写入 PG。

**选择 write-through 的原因**：
1. Candle 写入只在间隔过渡时发生（1m 间隔最频繁 = 每分钟一次/市场），写频率低
2. 按需从 dex_fills 重建需要昂贵的聚合查询
3. PG 持久化使长期历史查询不受 Redis 内存限制

### 数据库表

```sql
CREATE TABLE dex_candles (
    perpetual_id    INT NOT NULL,
    interval        TEXT NOT NULL,
    timestamp_ms    BIGINT NOT NULL,
    open            BIGINT NOT NULL,
    high            BIGINT NOT NULL,
    low             BIGINT NOT NULL,
    close           BIGINT NOT NULL,
    volume          BIGINT NOT NULL,
    num_trades      BIGINT NOT NULL,
    PRIMARY KEY (perpetual_id, interval, timestamp_ms)
);

CREATE INDEX idx_dex_candles_query
ON dex_candles (perpetual_id, interval, timestamp_ms DESC);
```

### Lua 脚本修改

当前 Lua 脚本在跨间隔过渡时返回 6 个值（新 candle 的 OHLCV）。
修改为同时返回旧 candle 的完整数据：

- **同间隔更新**：返回 7 个值，第 7 位 = `0`
- **跨间隔过渡**：返回 14 个值，第 7 位 = `1`，后 7 位 = 旧 candle `[open, high, low, close, volume, num_trades, timestamp_ms]`

### Rust 侧处理

新增 `CompletedCandle` 结构体和 `StoredCandle` 表映射。

`update_interval()` 解析 Lua 返回值，跨间隔时构造 `CompletedCandle`。
`update_from_fill()` 收集所有 interval 的 `CompletedCandle`，返回给调用方。

### fills handler UPSERT

在 `commit()` 中，candle 聚合循环后批量 UPSERT：

```rust
diesel::insert_into(dex_candles::table)
    .values(&stored)
    .on_conflict((perpetual_id, interval, timestamp_ms))
    .do_update()
    .set(...)
    .execute(conn)
```

使用 `ON CONFLICT DO UPDATE` 而非 `DO NOTHING`，确保框架重试时正确覆盖。

---

## P1: API 层 Gap Filling

### 问题

长时间无交易时，连续 candle 之间出现时间空洞。前端图表显示不连续。

### 方案

在 `query_candle_snapshot()` 返回结果后处理：

1. 按 `timestamp_ms` 升序排列
2. 检测相邻 candle 间 gap > 1 个 `duration_ms`
3. 生成零成交量 candle：`open=high=low=close=前一根 close, volume=0, num_trades=0`
4. 恢复降序排列返回

**选择 API 层处理的原因**：
- 无需后台定时任务
- Gap candle 是纯计算（前一根 close 值 + 零成交量）
- 计算开销仅在实际请求时产生
- 前端无需任何修改

### 双数据源合并

`query_candle_snapshot()` 改为双数据源：

1. 先查 Redis ZSet（快路径，覆盖近期数据）
2. 如果请求的时间范围超出 Redis 范围，从 PG 补充
3. 按 `timestamp_ms` 去重合并

---

## P2: 启动时从 PG 恢复 Redis Candle

### 问题

Redis 重启后 candle 数据为空，需要从 PG 恢复。

### 方案

新增 `recover_candles_from_pg()` 函数：

1. 从 PG `SELECT DISTINCT perpetual_id, interval FROM dex_candles` 获取组合
2. 检查 Redis ZSet 是否为空（`ZCARD == 0`）
3. 如果空，从 PG 加载最近 N 根（N = `INTERVAL_MAX_CANDLES`）
4. 批量 `ZADD` 到 Redis ZSet
5. 最新一根设为 current candle Hash

### 调用时机

在 `main.rs` 中，migrations 完成后、`Indexer::new()` 之前调用。
此时 `store`（Db）还未被 move。

```rust
#[cfg(feature = "redis-publish")]
if let Some(publisher) = dex_indexer::redis::publisher() {
    if let Some(mut redis_conn) = publisher.aggregation_conn() {
        let mut pg_conn = store.connect().await?;
        recover_candles_from_pg(&mut redis_conn, &mut pg_conn).await?;
    }
}
```

---

## P3: 过期 Current Candle 检测

### 问题

无交易时 current candle Hash 的 `timestamp_ms` 指向旧间隔。
API 不加检查直接将其插入到结果最前面，导致数据错误。

### 方案

在读取 current candle Hash 后计算当前间隔起始时间：

```rust
let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).as_millis() as i64;
let current_interval_start = (now_ms / duration_ms) * duration_ms;

if candle_ts < current_interval_start {
    // 已过期，当作历史 candle 插入到正确的时间位置
} else {
    // 当前间隔，插入到最前面
}
```

---

## 实施顺序

```
批次一（核心持久化）：P0 + P1 + P3
批次二（恢复 + 合并）：P2 + recent_fills Redis+PG 合并
批次三（缓存 + 收尾）：API 缓存集成 + 文档更新
```

## 关键文件

| 文件 | 改动 |
|------|------|
| `migrations/2026-02-10-000001_dex_candles/up.sql` | 新建：建表 |
| `migrations/2026-02-10-000001_dex_candles/down.sql` | 新建：回滚 |
| `src/schema/mod.rs` | 添加 `dex_candles` 表 + `StoredCandle` |
| `src/aggregators/candles.rs` | Lua 返回值扩展 + `CompletedCandle` + 恢复函数 |
| `src/handlers/fills.rs` | `commit()` 中 UPSERT `dex_candles` |
| `dex-api/src/handlers.rs` | 双数据源 + gap filling + 过期检测 + recent_fills 合并 |
| `dex-api/src/server.rs` | `AppState` 添加 cache |
| `src/main.rs` | 启动恢复调用 |
