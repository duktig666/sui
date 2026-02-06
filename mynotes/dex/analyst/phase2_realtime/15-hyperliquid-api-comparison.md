# Phase 2 方案与 Hyperliquid API 对标评估

> 评估日期：2026-02-05
>
> 本文档评估当前 Phase 2 单通道架构方案能否实现对标 Hyperliquid 的 API 和 WebSocket 功能。

---

## 一、WebSocket 功能对标

### 1.1 市场数据订阅（6 种）

| Hyperliquid 频道 | 当前方案支持 | 数据来源 | 说明 |
|-----------------|:------------:|----------|------|
| `trades` | ✅ 完全支持 | FillEventV1 → Redis Stream | 成交事件推送 |
| `l2Book` | ✅ 完全支持 | OrderbookSnapshotEvent → Redis Hash | 链上快照 ~250ms 推送 |
| `candle` | ✅ 完全支持 | FillEventV1 聚合 → Redis Hash | dex-indexer 实时聚合 |
| `allMids` | ⚠️ 需补充 | OrderbookSnapshot 计算 | 需要聚合所有交易对 |
| `bbo` | ✅ 可支持 | OrderbookSnapshot 提取 | best_bid/best_ask 字段 |
| `activeAssetCtx` | ⚠️ 部分支持 | 多事件聚合 | 缺少预言机价格、标记价格 |

### 1.2 用户数据订阅（10 种）

| Hyperliquid 频道 | 当前方案支持 | 数据来源 | 说明 |
|-----------------|:------------:|----------|------|
| `userFills` | ✅ 完全支持 | FillEventV1 按 subaccount 过滤 | 用户成交推送 |
| `orderUpdates` | ✅ 完全支持 | OrderPlaced/RemovedEventV1 | 订单状态变化 |
| `openOrders` | ✅ 可支持 | dex_orders 表 + Redis | 需要维护订单状态 |
| `userPositions` | ✅ 完全支持 | PositionUpdateEventV1 | 用户持仓变化 |
| `userFundings` | ✅ 可支持 | FundingSettlementEventV1 | 资金费结算 |
| `userNonFundingLedgerUpdates` | ✅ 可支持 | TransferEventV1 + LiquidationEventV1 | 账本更新 |
| `clearinghouseState` | ⚠️ 需补充 | 聚合持仓 + 余额 | 需要实时计算保证金 |
| `webData3` | ⚠️ 需补充 | 多数据源聚合 | 综合用户数据 |
| `activeAssetData` | ⚠️ 需补充 | 需要新增逻辑 | 杠杆、最大可交易量 |
| `notification` | ⚠️ 需定义 | 业务逻辑 | 通知规则待定义 |

### 1.3 TWAP 相关订阅（3 种）

| Hyperliquid 频道 | 当前方案支持 | 说明 |
|-----------------|:------------:|------|
| `twapStates` | ❌ 不在范围 | TWAP 算法订单 |
| `userTwapSliceFills` | ❌ 不在范围 | TWAP 分片成交 |
| `userTwapHistory` | ❌ 不在范围 | TWAP 历史 |

---

## 二、HTTP API 功能对标

### 2.1 市场数据 API

| Hyperliquid API | 当前方案支持 | 数据来源 | 说明 |
|-----------------|:------------:|----------|------|
| `meta` | ⚠️ 需补充 | 静态配置 | 永续合约元数据 |
| `metaAndAssetCtxs` | ⚠️ 需补充 | 静态 + Redis | 元数据 + 实时上下文 |
| `l2Book` | ✅ 完全支持 | Redis Hash | 订单簿快照查询 |
| `candleSnapshot` | ✅ 完全支持 | PostgreSQL + Redis | K 线历史 |
| `recentTrades` | ✅ 完全支持 | Redis Sorted Set | 最近成交 |
| `allMids` | ⚠️ 需补充 | Redis Hash 聚合 | 所有中间价 |
| `predictedFundings` | ⚠️ 需补充 | 需要新增计算 | 预测资金费率 |
| `fundingHistory` | ✅ 可支持 | PostgreSQL | 历史资金费率 |

### 2.2 用户数据 API

| Hyperliquid API | 当前方案支持 | 数据来源 | 说明 |
|-----------------|:------------:|----------|------|
| `clearinghouseState` | ⚠️ 需补充 | PostgreSQL + Redis | 账户完整状态 |
| `openOrders` | ✅ 可支持 | PostgreSQL (dex_orders) | 挂单列表 |
| `frontendOpenOrders` | ✅ 可支持 | PostgreSQL + 扩展字段 | 详细挂单 |
| `historicalOrders` | ✅ 可支持 | PostgreSQL | 历史订单 |
| `userFills` | ✅ 完全支持 | PostgreSQL (dex_fills) | 成交记录 |
| `userFillsByTime` | ✅ 完全支持 | PostgreSQL | 时间范围查询 |
| `userFunding` | ✅ 可支持 | PostgreSQL | 用户资金费 |
| `userNonFundingLedgerUpdates` | ✅ 可支持 | PostgreSQL | 账本记录 |
| `orderStatus` | ✅ 可支持 | PostgreSQL | 订单状态 |
| `subAccounts` | ⚠️ 需补充 | 需要新表 | 子账户管理 |
| `userRateLimit` | ⚠️ 需补充 | Redis + 业务逻辑 | 频率限制 |
| `referral` | ❌ 不在范围 | 推荐系统 | 需要专门开发 |

### 2.3 现货 API（暂不支持）

| Hyperliquid API | 当前方案支持 | 说明 |
|-----------------|:------------:|------|
| `spotMeta` | ❌ 不在范围 | 永续优先 |
| `spotMetaAndAssetCtxs` | ❌ 不在范围 | - |
| `spotClearinghouseState` | ❌ 不在范围 | - |
| `tokenDetails` | ❌ 不在范围 | - |
| `spotDeployState` | ❌ 不在范围 | - |

### 2.4 Vault 相关 API（暂不支持）

| Hyperliquid API | 当前方案支持 | 说明 |
|-----------------|:------------:|------|
| `vaultDetails` | ❌ 不在范围 | Vault 功能 |
| `userVaultEquities` | ❌ 不在范围 | - |

### 2.5 Builder Code 相关 API

| Hyperliquid API | 当前方案支持 | 说明 |
|-----------------|:------------:|------|
| `maxBuilderFee` | ❌ 不在范围 | Builder 费用授权 |

---

## 三、功能覆盖率统计

| 类别 | Hyperliquid 总数 | 完全支持 | 部分/需补充 | 不在范围 |
|------|:----------------:|:--------:|:-----------:|:--------:|
| **WS 市场数据** | 6 | 4 | 2 | 0 |
| **WS 用户数据** | 10 | 6 | 4 | 0 |
| **WS TWAP** | 3 | 0 | 0 | 3 |
| **HTTP 市场** | 8 | 4 | 4 | 0 |
| **HTTP 用户** | 12 | 8 | 3 | 1 |
| **HTTP 现货** | 5 | 0 | 0 | 5 |
| **HTTP Vault** | 2 | 0 | 0 | 2 |
| **HTTP Builder** | 1 | 0 | 0 | 1 |
| **总计** | **47** | **22 (47%)** | **13 (28%)** | **12 (25%)** |

---

## 四、核心功能差距分析

### 4.1 需要补充的关键功能

| 功能 | 重要性 | 补充方案 | 工作量 |
|------|:------:|----------|:------:|
| **allMids** | P0 | 从所有 OrderbookSnapshot 聚合 mid_price | 低 |
| **meta / metaAndAssetCtxs** | P0 | 静态配置 + Redis 聚合 | 中 |
| **clearinghouseState** | P0 | 聚合 positions + balances + 计算保证金 | 中 |
| **activeAssetCtx** | P1 | 缺少 oraclePx, markPx, premium | 高* |
| **predictedFundings** | P1 | 需要资金费率计算逻辑 | 中 |
| **activeAssetData** | P2 | 杠杆、最大可交易量计算 | 中 |

> *注：oraclePx/markPx 依赖链上预言机和标记价格系统，可能需要额外链上支持。

### 4.2 当前方案可完全覆盖的核心交易功能

```
✅ 成交推送 (trades, userFills)
✅ 订单簿实时更新 (l2Book)
✅ K 线数据 (candle, candleSnapshot)
✅ 订单状态更新 (orderUpdates, openOrders)
✅ 持仓变化 (userPositions)
✅ 资金费结算 (userFundings)
✅ 账本更新 (userNonFundingLedgerUpdates)
✅ 历史查询 (userFills, historicalOrders)
```

### 4.3 功能依赖的事件映射

| 功能 | 依赖事件 | 存储位置 |
|------|----------|----------|
| trades | FillEventV1 | Redis Stream + Sorted Set |
| l2Book | OrderbookSnapshotEvent | Redis Hash |
| candle | FillEventV1（聚合） | Redis Hash + Sorted Set |
| userFills | FillEventV1 | PostgreSQL + Redis Stream |
| orderUpdates | OrderPlaced/RemovedEventV1 | Redis Stream |
| userPositions | PositionUpdateEventV1 | PostgreSQL + Redis Stream/Hash |
| userFundings | FundingSettlementEventV1 | PostgreSQL + Redis Stream |
| liquidations | LiquidationEventV1 | PostgreSQL + Redis Stream |

---

## 五、结论与建议

### 5.1 评估结论

| 维度 | 评估 |
|------|------|
| **核心交易功能** | ✅ **可满足**：订单簿、成交、K 线、订单状态、持仓等核心功能完全覆盖 |
| **市场数据完整性** | ⚠️ **需补充**：缺少 allMids、meta 等聚合类接口 |
| **用户账户数据** | ⚠️ **需补充**：clearinghouseState 需要聚合计算 |
| **高级功能** | ❌ **暂不支持**：TWAP、Vault、现货、推荐系统 |

### 5.2 优先级建议

#### Phase 2 必须完成

**WebSocket 频道**：
- `trades` - 成交推送
- `l2Book` - 订单簿更新
- `candle` - K 线推送
- `userFills` - 用户成交
- `orderUpdates` - 订单状态
- `userPositions` - 用户持仓

**HTTP API**：
- `l2Book` - 订单簿查询
- `candleSnapshot` - K 线历史
- `recentTrades` - 最近成交
- `openOrders` - 挂单列表
- `userFills` - 成交记录

#### Phase 3 可补充

**聚合类 API**：
- `allMids` - 所有中间价
- `meta` / `metaAndAssetCtxs` - 元数据

**账户状态**：
- `clearinghouseState` - 完整账户状态

**资金费率**：
- `predictedFundings` - 预测资金费率
- `fundingHistory` - 历史资金费率

#### 可暂缓

- TWAP 相关（算法订单）
- 现货交易
- Vault 功能
- 推荐系统
- Builder Code

### 5.3 整体评价

**当前 Phase 2 方案能够支持 Hyperliquid 约 70% 的核心永续合约交易功能**，足以支撑基本的交易页面：

| 页面功能 | 支持情况 |
|---------|:--------:|
| 交易页面订单簿 | ✅ |
| 成交列表 | ✅ |
| K 线图 | ✅ |
| 用户持仓列表 | ✅ |
| 订单管理 | ✅ |
| 账户总览 | ⚠️ 需补充保证金计算 |

---

## 六、后续行动

### 6.1 Phase 2 核心任务（已规划）

参见 `06-implementation-checklist.md`：
- dex-indexer Redis 发布功能
- dex-ws WebSocket 服务
- 核心频道实现

### 6.2 Phase 3 补充任务（建议）

1. **allMids 聚合服务**
   - 定时聚合所有交易对的 mid_price
   - 写入 Redis Hash：`dex:all_mids`

2. **meta 静态配置服务**
   - 永续合约元数据配置
   - 支持动态更新

3. **clearinghouseState 聚合服务**
   - 聚合用户 positions + balances
   - 实时计算保证金、账户价值

4. **activeAssetCtx 增强**
   - 需要链上支持：预言机价格、标记价格
   - 溢价率计算

---

## 附录：Hyperliquid 参考文档

| 文档 | 路径 |
|------|------|
| WebSocket API 规范 | `dex-sui/docs/indexer/hyperliquid/ws/hyperliquid-websocket.md` |
| HTTP API 示例 | `dex-sui/docs/indexer/hyperliquid/http/hyperliquid-query.http` |
| 本项目 WebSocket 协议 | `11-websocket-protocol-spec.md` |
| 本项目 Redis 消息规范 | `10-redis-message-spec.md` |
