# Phase 5 总览：功能适配层（Indexer / API / WebSocket）

> 日期：2026-02-25
> 状态：规划中
> 范围：Indexer / API / WebSocket 层适配，对标 Hyperliquid 规范

---

## 一、背景

两个工程师正在实现 DEX 核心引擎功能：
- **工程师 A**：真实跨链充值和提现（替代当前 MintCoin 模拟充值）
- **工程师 B**：逐仓保证金、TP/SL 订单、Oracle 标记价格、资金费率、清算、杠杆调整

本文档规划 indexer、API、WebSocket 层的适配工作，确保引擎功能交付后能快速对接上层服务。

---

## 二、现有基础

### 2.1 事件层（dex_events.rs）

| 事件 | 状态 | Handler | DB 表 | Redis |
|------|------|---------|-------|-------|
| FillEvent | ✅ | fills.rs | dex_fills | Stream + Candle + MarketStats |
| PositionUpdateEvent | ✅ | positions.rs | dex_positions + dex_position_updates | Stream |
| BalanceUpdateEvent | ✅ | balances.rs | dex_balances | Stream |
| TransferEvent | ✅ | transfers.rs | dex_transfers | Stream |
| FundingSettlementEvent | ✅ | funding_payments.rs | dex_funding_payments | **无 Redis** |
| LiquidationEvent | ✅ 定义 | **无 handler** | **无表** | **无** |
| OrderUpdateEvent | ✅ | order_updates.rs | dex_orders (UPDATE) | Stream |
| OrderPlacedEventV1 | ✅ | orders.rs | dex_orders (INSERT) | Stream |
| OrderRemovedEventV1 | ✅ | order_removals.rs | dex_orders (UPDATE) | Stream |
| OrderbookSnapshotEvent | ✅ | orderbook_snapshots.rs | 无（纯 Redis） | Hash + Stream |
| PerpetualCreatedEvent | ✅ | perpetuals.rs | dex_perpetuals + dex_sui_objects | 无 |
| GlobalAccountsCreatedEvent | ✅ | perpetuals.rs | dex_sui_objects | 无 |

### 2.2 API 层

**已有 19 个 info 端点**：userFills, userBalances, userTransfers, recentFills, clearinghouseState, meta, openOrders, l2Book, candleSnapshot, marketStats, allMids, orderStatus, historicalOrders, subAccounts, userNonFundingLedgerUpdates, userFillsByTime, userFunding, fundingHistory, userRateLimit

**已有 4 个 exchange action**：Order, Cancel, CancelByCloid（未实现）, ClosePosition

### 2.3 WebSocket 层

**已有 10 个频道**：trades, orderbook, candle, user, allMids, bbo, orderUpdates, clearinghouseState, openOrders, notification

**已有 8 个 Redis Stream**：fills, positions, balances, transfers, orders, orderbook, candles, market_stats

### 2.4 数据库

**已有 10 张表** + 1 个 watermarks 表：dex_fills, dex_balances, dex_positions, dex_position_updates, dex_perpetuals, dex_transfers, dex_orders, dex_candles, dex_sui_objects, dex_funding_payments, dex_watermarks

---

## 三、关键差距

### 3.1 高优先级（交易页面核心）

| 差距 | 影响 | 依赖 | 详细文档 |
|------|------|------|---------|
| clearinghouseState.unrealizedPnl 硬编码 0 | 持仓盈亏不可见 | 需 mark price | 03-mark-price.md |
| clearinghouseState.leverage 硬编码 cross/1x | 杠杆信息不准 | 需 DB migration | 07-leverage-margin.md |
| meta 缺 szDecimals/maxLeverage | 前端下单/杠杆选择器 | 需 DB migration | 08-api-enrichment.md |
| metaAndAssetCtxs 端点缺失 | 交易页面核心数据 | 需 mark price | 03-mark-price.md |
| activeAssetCtx WS 频道缺失 | 实时 funding/mark price | 需 mark price | 03-mark-price.md |

### 3.2 中优先级（功能完整性）

| 差距 | 影响 | 依赖 | 详细文档 |
|------|------|------|---------|
| LiquidationEvent 无 handler | 清算记录不可查 | 无依赖 | 05-liquidation.md |
| FundingPayments 不发 Redis | 资金费无 WS 推送 | 无依赖 | 04-funding-rate.md |
| clearinghouseState.liquidationPx 始终 null | 清算价不可见 | 需 margin 数据 | 05-liquidation.md |
| cumFunding 系列字段缺失 | 累计资金费不可查 | 需 DB migration | 04-funding-rate.md |

### 3.3 低优先级（完整对标）

| 差距 | 影响 | 依赖 | 详细文档 |
|------|------|------|---------|
| candleSnapshot 仅 6 种周期 | 少于 HL 14 种 | 无依赖 | 08-api-enrichment.md |
| l2Book 缺每档订单数 n | 数据不够丰富 | 需 orderbook 事件扩展 | 08-api-enrichment.md |
| frontendOpenOrders 端点缺失 | TP/SL 条件单详情 | 需 TP/SL 订单 | 02-order-types.md |
| userFills/userFundings WS 缺失 | 用户级推送不完整 | 无依赖 | 08-api-enrichment.md |

---

## 四、工作分区

### Part 1：现阶段可做的准备工作（不依赖引擎）

| 序号 | 工作项 | 详细文档 | 估计复杂度 |
|------|--------|---------|-----------|
| 1.1 | DB migrations（5 个） | 01-pre-work.md §1 | 中 |
| 1.2 | Liquidation handler 实现 | 05-liquidation.md | 低 |
| 1.3 | FundingPayments Redis 发布 | 04-funding-rate.md | 低 |
| 1.4 | clearinghouseState 消除硬编码 | 08-api-enrichment.md | 中 |
| 1.5 | Schema + 类型定义更新 | 01-pre-work.md §5 | 中 |
| 1.6 | WS 频道类型注册 | 01-pre-work.md §6 | 低 |
| 1.7 | Redis key 设计 | 01-pre-work.md §7 | 低 |
| 1.8 | 额外 candle 周期 | 08-api-enrichment.md | 低 |

### Part 2：引擎交付后的适配工作

| 序号 | 工作项 | 详细文档 | 依赖 |
|------|--------|---------|------|
| 2.1 | TP/SL 订单字段扩展 | 02-order-types.md | 工程师 B |
| 2.2 | Mark price handler + API + WS | 03-mark-price.md | 工程师 B |
| 2.3 | 资金费率完善 | 04-funding-rate.md | 工程师 B |
| 2.4 | 清算完善 | 05-liquidation.md | 工程师 B |
| 2.5 | 跨链充提适配 | 06-deposit-withdraw.md | 工程师 A |
| 2.6 | 杠杆与保证金模式 | 07-leverage-margin.md | 工程师 B |

---

## 五、优先级排序

### P0（核心，现在开始）
1. DB migrations（M1-M5）+ Schema 更新
2. Liquidation handler 实现
3. clearinghouseState 消除硬编码（szDecimals / maxLeverage / leverage）
4. API/WS 类型定义准备

### P1（引擎交付后立即）
5. Mark price handler + metaAndAssetCtxs 端点 + activeAssetCtx WS
6. Leverage handler + updateLeverage exchange action
7. cumFunding 维护 + clearinghouseState 完善
8. TP/SL 订单字段扩展 + frontendOpenOrders

### P2（完整对标）
9. 额外 candle 周期（8 种新增）
10. 跨链充提状态追踪
11. userFills / userFundings WS 频道
12. WS 数据格式对标（array vs object）

---

## 六、文档索引

| 编号 | 文件 | 内容 |
|------|------|------|
| 00 | 本文件 | 总览与优先级 |
| 01 | 01-pre-work.md | 现阶段可做的准备工作 |
| 02 | 02-order-types.md | 新订单类型适配（TP/SL） |
| 03 | 03-mark-price.md | 标记价格 / Oracle |
| 04 | 04-funding-rate.md | 资金费率完善 |
| 05 | 05-liquidation.md | 清算适配 |
| 06 | 06-deposit-withdraw.md | 跨链充提适配 |
| 07 | 07-leverage-margin.md | 杠杆与保证金模式 |
| 08 | 08-api-enrichment.md | API 丰富化 |
| 09 | 09-engine-interface.md | 与引擎工程师的接口约定 |

---

## 七、验证方案

### 基础验证（Part 1 完成后）
```bash
# DB migration
cd dex-sui/crates/dex-indexer
diesel migration run --database-url postgres://dex:dex123@localhost:5432/dex_indexer

# 编译检查
cargo check -p dex-indexer -p dex-api -p dex-types

# 运行现有测试确保不破坏
SUI_SKIP_SIMTESTS=1 cargo nextest run -p dex-indexer -p dex-api -p dex-types

# Docker 全栈验证
cd docker/dex-dev && make rebuild
```

### 集成验证（Part 2 完成后）
- dex-node-test 发送各种订单类型验证 indexer 正确索引
- API .http 文件验证每个新端点
- WS 连接验证新频道推送
