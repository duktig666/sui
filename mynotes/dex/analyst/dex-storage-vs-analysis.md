# DEX 存储方案对比分析：PostgreSQL vs TimescaleDB vs ClickHouse

> 分析 K线数据和历史存储的技术选型，对比不同方案的性能、运维复杂度和适用场景。

---

## 一、K线数据：TimescaleDB vs PostgreSQL

### 1.1 性能对比

| 场景 | PostgreSQL | TimescaleDB | 提升幅度 |
|-----|-----------|-------------|---------|
| **写入 1分钟K线** | ~5,000 rows/s | ~50,000 rows/s | **10x** |
| **查询最近24小时K线** | ~50ms | ~5ms | **10x** |
| **查询30天历史K线** | ~500ms | ~30ms | **15x** |
| **聚合计算 (5m→1h)** | 手动 SQL | 连续聚合自动 | **自动化** |
| **磁盘占用 (压缩后)** | 100% | ~30% | **3x 压缩** |

### 1.2 TimescaleDB 核心优势

```sql
-- 1. 自动分区 (Hypertable)
SELECT create_hypertable('candles', 'bucket',
    chunk_time_interval => INTERVAL '1 day');
-- PostgreSQL 需要手动分区表管理

-- 2. 连续聚合 (自动计算高周期K线)
CREATE MATERIALIZED VIEW candles_1h
WITH (timescaledb.continuous) AS
SELECT
    market_id,
    time_bucket('1 hour', bucket) AS bucket,
    first(open, bucket) AS open,
    max(high) AS high,
    min(low) AS low,
    last(close, bucket) AS close,
    sum(volume) AS volume
FROM candles_1m
GROUP BY market_id, time_bucket('1 hour', bucket);
-- PostgreSQL 需要定时任务手动聚合

-- 3. 自动压缩
SELECT add_compression_policy('candles', INTERVAL '7 days');
-- PostgreSQL 无原生支持
```

### 1.3 实际收益评估

**DEX 场景**（假设 50 个交易对）：
- 每分钟写入：50 × 7 周期 = 350 rows
- 每日写入：~500,000 rows
- 查询 QPS：~1,000 (K线查询)

| 指标 | PostgreSQL | TimescaleDB |
|-----|-----------|-------------|
| 写入延迟 P99 | ~20ms | ~2ms |
| 查询延迟 P99 | ~100ms | ~10ms |
| 30天数据量 | ~50GB | ~15GB |
| 运维复杂度 | 手动分区+清理 | 自动 |

**结论**：对于 K线这种典型时序数据，TimescaleDB 有**明显优势**（10x+ 性能提升）。

---

## 二、历史存储：ClickHouse vs PostgreSQL

### 2.1 性能对比

| 场景 | PostgreSQL | ClickHouse | 提升幅度 |
|-----|-----------|------------|---------|
| **批量写入** | ~10,000 rows/s | ~1,000,000 rows/s | **100x** |
| **全表扫描 1亿行** | ~60s | ~2s | **30x** |
| **多维聚合查询** | ~10s | ~0.5s | **20x** |
| **用户历史订单查询** | ~50ms | ~100ms | **PostgreSQL 更快** |
| **磁盘占用** | 100% | ~20% | **5x 压缩** |

### 2.2 ClickHouse 核心优势

```sql
-- 1. 列式存储 + 高压缩
CREATE TABLE orders (
    order_id String,
    market_id LowCardinality(String),  -- 低基数优化
    owner String,
    price Decimal(38, 18),
    quantity Decimal(38, 18),
    created_at DateTime64(3)
) ENGINE = MergeTree()
PARTITION BY toYYYYMM(created_at)
ORDER BY (market_id, owner, created_at);

-- 2. 物化视图 (实时聚合)
CREATE MATERIALIZED VIEW user_daily_volume
ENGINE = SummingMergeTree()
ORDER BY (owner, trade_date)
AS SELECT
    owner,
    toDate(created_at) AS trade_date,
    sum(quote_quantity) AS total_volume
FROM fills
GROUP BY owner, toDate(created_at);

-- 3. 超大范围查询
SELECT market_id, count(), sum(quantity)
FROM orders
WHERE created_at >= '2024-01-01'
GROUP BY market_id;
-- 1亿行，ClickHouse: ~2s, PostgreSQL: ~60s
```

### 2.3 ClickHouse 劣势

| 场景 | 问题 |
|-----|------|
| **单行查询** | 比 PostgreSQL 慢 2-3x |
| **事务支持** | 无 ACID 事务 |
| **更新/删除** | 异步合并，不适合频繁更新 |
| **复杂 JOIN** | 性能较差 |
| **运维** | 相比 PostgreSQL 更复杂 |

### 2.4 实际收益评估

**DEX 场景**（假设日成交 100 万笔）：
- 每日写入：~1,000,000 fills + ~500,000 orders
- 历史数据量：1年 ~10亿行
- 分析查询：日报、周报、用户统计

| 指标 | PostgreSQL | ClickHouse |
|-----|-----------|------------|
| 写入吞吐 | 可能成为瓶颈 | 轻松应对 |
| 日报生成 | ~5分钟 | ~10秒 |
| 1年数据量 | ~500GB | ~100GB |
| 单用户历史查询 | ~50ms ✅ | ~100ms |
| 运维复杂度 | 低 | 中等 |

---

## 三、仍然使用 PostgreSQL 的利弊分析

### 3.1 架构对比

```
方案A: PostgreSQL Only
═══════════════════════
┌─────────────────────────────────────┐
│           PostgreSQL                │
│  ┌─────────────────────────────┐   │
│  │ orders, fills, positions    │   │
│  │ candles, funding_rates      │   │
│  │ 历史数据                     │   │
│  └─────────────────────────────┘   │
└─────────────────────────────────────┘
优点: 简单
缺点: 性能瓶颈


方案B: PostgreSQL + 分区表优化
═══════════════════════════════
┌─────────────────────────────────────┐
│        PostgreSQL (优化版)           │
│  ┌─────────────┐  ┌─────────────┐  │
│  │ 热数据分区   │  │ 冷数据分区   │  │
│  │ (近30天)    │  │ (历史归档)   │  │
│  └─────────────┘  └─────────────┘  │
└─────────────────────────────────────┘
优点: 相对简单
缺点: 手动管理分区，仍有性能上限


方案C: TimescaleDB + ClickHouse (当前设计)
════════════════════════════════════════
┌──────────────┐  ┌──────────────┐
│ TimescaleDB  │  │  ClickHouse  │
│ (时序数据)   │  │  (分析数据)  │
│ K线/成交/仓位 │  │  历史订单    │
└──────────────┘  └──────────────┘
优点: 最佳性能
缺点: 运维复杂
```

### 3.2 PostgreSQL Only 方案的可行性

| 数据规模 | PostgreSQL 能否应对 | 建议 |
|---------|-------------------|------|
| **小规模** (<10个交易对，日成交<10万) | ✅ 完全可以 | PostgreSQL 足够 |
| **中规模** (50个交易对，日成交100万) | ⚠️ 需优化 | 分区表 + 索引优化 |
| **大规模** (>100交易对，日成交>500万) | ❌ 瓶颈明显 | 必须 TimescaleDB/ClickHouse |

### 3.3 PostgreSQL 优化后的极限

**可以做到的优化**：

```sql
-- 1. 分区表 (手动管理)
CREATE TABLE fills (
    id BIGSERIAL,
    created_at TIMESTAMPTZ NOT NULL,
    ...
) PARTITION BY RANGE (created_at);

CREATE TABLE fills_2024_01 PARTITION OF fills
    FOR VALUES FROM ('2024-01-01') TO ('2024-02-01');

-- 2. 部分索引 (减少索引大小)
CREATE INDEX idx_orders_active ON orders (owner, created_at)
    WHERE status IN ('open', 'partial');

-- 3. 物化视图 (预计算K线)
CREATE MATERIALIZED VIEW candles_1h AS
SELECT ... GROUP BY market_id, date_trunc('hour', created_at);
-- 需要定时刷新: REFRESH MATERIALIZED VIEW candles_1h;

-- 4. 读写分离
-- 主库写入，从库查询
```

**优化后的预期性能**：

| 场景 | 优化前 | 优化后 | vs TimescaleDB |
|-----|-------|-------|----------------|
| K线写入 | 5,000/s | 15,000/s | 50,000/s |
| K线查询 30天 | 500ms | 100ms | 30ms |
| 历史订单聚合 | 60s | 20s | 2s (ClickHouse) |

### 3.4 利弊总结

#### 继续使用 PostgreSQL 的优势

| 优势 | 说明 |
|-----|------|
| **运维简单** | 单一数据库，团队熟悉 |
| **事务支持** | 完整 ACID，数据一致性好 |
| **复杂查询** | JOIN、子查询支持完善 |
| **生态成熟** | 工具、监控、备份方案丰富 |
| **成本** | 无需额外学习成本 |
| **快速启动** | MVP 阶段可以先用 PostgreSQL |

#### 继续使用 PostgreSQL 的劣势

| 劣势 | 说明 |
|-----|------|
| **K线性能** | 无连续聚合，需手动刷新物化视图 |
| **分区管理** | 需手动创建/删除分区 |
| **压缩** | 无原生时序压缩 |
| **大范围扫描** | OLAP 查询慢 |
| **扩展性** | 垂直扩展有上限 |

---

## 四、建议的技术路线

### 4.1 分阶段演进

```
阶段1: MVP (0-3个月)
════════════════════
PostgreSQL Only + Redis

• 快速验证业务模型
• 日成交 <10万笔
• K线用物化视图


阶段2: 增长期 (3-12个月)
═════════════════════════
PostgreSQL → TimescaleDB + Redis

• 日成交 10-100万笔
• TimescaleDB 替换 K线存储
• 历史数据仍在 PostgreSQL


阶段3: 规模化 (12个月+)
═════════════════════════
TimescaleDB + ClickHouse + Redis

• 日成交 >100万笔
• ClickHouse 承担历史分析
• 完整三层架构
```

### 4.2 决策矩阵

| 如果你的情况是... | 建议方案 |
|-----------------|---------|
| 团队小，快速验证 | PostgreSQL Only |
| 重视 K线性能 | TimescaleDB (优先级高) |
| 重视历史分析 | ClickHouse (优先级中) |
| 追求最佳性能 | TimescaleDB + ClickHouse |
| 运维能力有限 | PostgreSQL + 分区表优化 |

### 4.3 折中方案

如果运维能力有限，可以考虑：

```
TimescaleDB Only (折中方案)
═══════════════════════════
┌─────────────────────────────────────┐
│           TimescaleDB               │
│  ┌─────────────────────────────┐   │
│  │ 所有数据 (时序 + 非时序)     │   │
│  │ • K线、成交 → Hypertable    │   │
│  │ • 订单、仓位 → 普通表        │   │
│  │ • 历史数据 → 压缩分区        │   │
│  └─────────────────────────────┘   │
└─────────────────────────────────────┘

优点:
• 单一数据库，运维简单
• K线性能好
• 100% PostgreSQL 兼容

缺点:
• OLAP 分析不如 ClickHouse
• 但对于中等规模已经够用
```

---

## 五、结论

### 5.1 核心结论

| 问题 | 答案 |
|-----|------|
| **TimescaleDB 替换 PostgreSQL 存 K线** | 提升 **10x+**，强烈建议 |
| **ClickHouse 替换 PostgreSQL 存历史** | 提升 **20-30x** (分析场景)，但增加运维复杂度 |
| **继续用 PostgreSQL** | **MVP 阶段可行**，但需要提前规划迁移路径 |

### 5.2 实用建议

| 优先级 | 建议 | 原因 |
|-------|------|------|
| **必做** | K线用 TimescaleDB | 收益大，迁移简单（PostgreSQL 兼容） |
| **可选** | 历史数据用 ClickHouse | 取决于分析需求和运维能力 |
| **折中** | 全部用 TimescaleDB | 单一数据库，兼顾性能和运维 |

### 5.3 按规模推荐

| 规模 | 日成交量 | 推荐方案 |
|-----|---------|---------|
| 小 | <10万 | PostgreSQL Only |
| 中 | 10-100万 | TimescaleDB + PostgreSQL |
| 大 | >100万 | TimescaleDB + ClickHouse |

---

## 六、sui-indexer-alt 存储分析与替换评估

> 分析复用 sui-indexer-alt 时的存储方案，评估替换工作量和建议。

### 6.1 sui-indexer-alt 当前存储方案

**数据库**: PostgreSQL + Diesel ORM (Rust)

**默认连接**: `postgres://postgres:postgrespw@localhost:5432/sui_indexer_alt`

**核心表结构** (通用区块链索引数据):

| 表名 | 用途 | 数据格式 | 说明 |
|-----|------|---------|------|
| kv_transactions | 交易存储 | BCS 序列化 | tx_digest 为主键 |
| kv_objects | 对象存储 | BCS 序列化 | object_id + version 复合主键 |
| kv_checkpoints | Checkpoint | BCS 序列化 | sequence_number 为主键 |
| kv_epoch_starts | Epoch 开始 | 结构化 | 协议版本、Gas 价格等 |
| kv_epoch_ends | Epoch 结束 | 结构化 | 质押、费用统计等 |
| tx_affected_addresses | 地址索引 | 索引表 | 按地址查询交易 |
| tx_affected_objects | 对象索引 | 索引表 | 按对象查询交易 |
| obj_info | 对象元信息 | 结构化 | 类型、所有者信息 |
| obj_versions | 对象版本 | 索引表 | 版本追踪 |
| watermarks | 进度追踪 | 结构化 | Pipeline 水位线 |

**关键特点**:

| 特性 | sui-indexer-alt | DEX 需求 |
|-----|----------------|---------|
| 时序优化 | ❌ 无 Hypertable | ✅ K线需要 |
| DEX 专用表 | ❌ 无 | ✅ orders, fills, candles |
| 连续聚合 | ❌ 无 | ✅ K线聚合需要 |
| 数据格式 | BCS 二进制 | 结构化字段 |
| 查询模式 | KV 查找 | 范围查询、聚合 |

### 6.2 替换存储工作量评估

#### 方案 A: 保持 PostgreSQL，新增 DEX 表

| 工作项 | 工作量 | 说明 |
|-------|-------|------|
| sui-indexer-alt 代码修改 | 无 | 保持原样 |
| 新增 DEX 专用表 | 中 | orders, fills, candles, positions |
| 手动分区管理 | **高** | 需要自己实现分区创建/删除脚本 |
| K线聚合 | **高** | 需要定时任务刷新物化视图 |
| 总工作量 | **中-高** | 后期维护成本高 |

#### 方案 B: 升级为 TimescaleDB (推荐)

| 工作项 | 工作量 | 说明 |
|-------|-------|------|
| 数据库升级 | **低** | TimescaleDB 100% PostgreSQL 兼容 |
| sui-indexer-alt 代码修改 | **零** | Diesel ORM 完全兼容 |
| 现有表迁移 | **零** | 普通表无需变化 |
| 新增 DEX Hypertable | 低 | fills, candles 作为 Hypertable |
| 连续聚合配置 | 低 | SQL 配置即可 |
| 总工作量 | **低** | 运维简单 |

#### 方案 C: PostgreSQL + ClickHouse

| 工作项 | 工作量 | 说明 |
|-------|-------|------|
| sui-indexer-alt 保持不变 | 无 | 继续用 PostgreSQL |
| 新增 ClickHouse | 中 | 新部署 + 驱动集成 |
| 数据同步机制 | **中** | PostgreSQL → ClickHouse 同步 |
| 运维复杂度 | **中** | 多系统维护 |
| 总工作量 | **中** | 适合大规模分析需求 |

### 6.3 推荐方案: 升级为 TimescaleDB

**核心理由**:

1. **零代码修改**: sui-indexer-alt 的 Diesel 代码完全兼容 TimescaleDB
   ```rust
   // sui-indexer-alt 使用 diesel-async + postgres
   // TimescaleDB 作为 PostgreSQL 扩展，连接字符串相同
   diesel-async = { features = ["bb8", "postgres"] }
   ```

2. **透明升级**: 只需安装扩展，现有表无需任何改动
   ```sql
   -- 在现有 PostgreSQL 数据库中启用 TimescaleDB
   CREATE EXTENSION IF NOT EXISTS timescaledb;
   -- sui-indexer-alt 的表继续作为普通表使用
   ```

3. **DEX 表独立优化**: 新增的 DEX 表可享受时序特性
   ```sql
   -- DEX 专用表使用 Hypertable
   SELECT create_hypertable('fills', 'created_at');
   SELECT create_hypertable('candles', 'bucket');

   -- sui-indexer-alt 表保持普通表
   -- kv_transactions, kv_objects 等无需改动
   ```

4. **运维简单**: 单一数据库，无需多系统同步

### 6.4 实施步骤

```
Phase 1: 数据库升级 (1天)
═══════════════════════════
1. 安装 TimescaleDB 扩展
   $ apt install timescaledb-2-postgresql-15

2. 启用扩展
   CREATE EXTENSION IF NOT EXISTS timescaledb;

3. 验证 sui-indexer-alt 正常运行
   -- 无需任何代码修改


Phase 2: 添加 DEX 专用表 (2-3天)
═════════════════════════════════
1. 创建 orders 表 (普通表)
2. 创建 fills 表 (Hypertable)
3. 创建 candles 表 (Hypertable + 连续聚合)
4. 创建 positions 表 (普通表)


Phase 3 (可选): 引入 ClickHouse
════════════════════════════════
- 如果后期需要 OLAP 分析能力
- 通过 Kafka 或 FDW 同步历史数据
```

### 6.5 决策建议

| 场景 | 建议 | 理由 |
|-----|------|------|
| **复用 sui-indexer-alt** | ✅ 升级为 TimescaleDB | 零修改成本，DEX 性能提升 10x+ |
| **sui-indexer-alt 保持原样** | ⚠️ 可行但不推荐 | 需要单独部署 DEX 数据库，增加运维复杂度 |
| **完全不替换存储** | ❌ 不推荐 | K线性能瓶颈明显，手动分区维护成本高 |

### 6.6 风险评估

| 风险 | 可能性 | 影响 | 缓解措施 |
|-----|-------|------|---------|
| TimescaleDB 兼容性问题 | 低 | 低 | 100% PostgreSQL 兼容，官方支持 Diesel |
| 性能回退 | 极低 | 中 | 普通表性能与 PostgreSQL 相同 |
| 运维学习成本 | 低 | 低 | TimescaleDB 命令与 PostgreSQL 基本一致 |

---

## 参考资料

- TimescaleDB 文档: https://docs.timescale.com/
- ClickHouse 文档: https://clickhouse.com/docs/
- PostgreSQL 分区表: https://www.postgresql.org/docs/current/ddl-partitioning.html
- DEX Indexer 详细设计: `dex-indexer-analyst.md`
- sui-indexer-alt 源码: `crates/sui-indexer-alt/`
- sui-indexer-alt-schema: `crates/sui-indexer-alt-schema/`
