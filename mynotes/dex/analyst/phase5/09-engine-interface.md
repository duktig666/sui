# 09 与引擎工程师的接口约定

> 日期：2026-02-25
> 目的：明确 indexer/API/WS 层需要引擎层提供的接口，减少集成风险

---

## 一、接口总览

### 1.1 需要工程师 B 提供的新事件

| 事件 | 关键字段 | 发射时机 | 优先级 | 详细文档 |
|------|---------|---------|--------|---------|
| **MarkPriceUpdateEvent** | perpetual_id, mark_price, oracle_price, funding_rate, open_interest, premium | 每个 checkpoint | P1 | 03-mark-price.md |
| **LeverageUpdateEvent** | perpetual_id, subaccount, leverage_value, margin_mode, isolated_margin | updateLeverage 交易时 | P1 | 07-leverage-margin.md |

### 1.2 需要工程师 B 扩展的现有事件

| 事件 | 新增字段 | 优先级 | 详细文档 |
|------|---------|--------|---------|
| **OrderPlacedEventV1** | trigger_price, trigger_condition, parent_order_id, grouping | P1 | 02-order-types.md |
| **FillEvent**（可选） | is_liquidation | P2 | 05-liquidation.md |
| **PriceLevel**（可选） | num_orders | P2 | 08-api-enrichment.md |

### 1.3 需要工程师 B 提供的新 DexCommand

| Command | 参数 | 优先级 | 详细文档 |
|---------|------|--------|---------|
| **UpdateLeverage** | perpetual_id, subaccount_number, leverage_value, is_cross | P1 | 07-leverage-margin.md |
| **UpdateIsolatedMargin** | perpetual_id, subaccount_number, is_buy, amount | P1 | 07-leverage-margin.md |

### 1.4 需要工程师 A 确认的事项

| 问题 | 影响范围 | 优先级 | 详细文档 |
|------|---------|--------|---------|
| 跨链充提是否复用 BalanceUpdateEvent？ | balances handler | P2 | 06-deposit-withdraw.md |
| update_type 新增哪些值？ | balances handler + API | P2 | 06-deposit-withdraw.md |
| 是否需要 WithdrawalEvent？ | 新 handler + 表 | P2 | 06-deposit-withdraw.md |
| 提款有中间状态吗？ | 数据模型 | P2 | 06-deposit-withdraw.md |

---

## 二、MarkPriceUpdateEvent 详细规格

### 2.1 事件结构

```rust
/// 标记价格更新事件
/// 发射位置：引擎的 mark price 计算模块
/// 发射时机：每个 checkpoint（建议频率：≥ 每3秒）
pub struct MarkPriceUpdateEvent {
    /// 永续市场 ID
    pub perpetual_id: u32,

    /// 标记价格 (subticks)
    /// 用途：PnL 计算、清算判断
    /// 计算方式：综合 oracle price + orderbook impact price + 衰减 EMA
    pub mark_price: u64,

    /// Oracle 价格 (subticks)
    /// 来源：外部价格源（Pyth / Switchboard / 自建）
    pub oracle_price: u64,

    /// 预测资金费率 (scaled by 1e18)
    /// 含义：如果现在结算，费率是多少
    /// 计算方式：(mark_price - oracle_price) / oracle_price / funding_interval
    /// 正值 = 多头支付给空头
    pub funding_rate: i64,

    /// 全市场总持仓量 (quantums)
    /// 含义：所有多头仓位的绝对值之和（= 所有空头仓位的绝对值之和）
    pub open_interest: u64,

    /// 溢价 (scaled by 1e18)
    /// 含义：mark_price 与 oracle_price 的偏差率
    /// 计算方式：(mark_price - oracle_price) / oracle_price
    pub premium: i64,

    pub timestamp_ms: u64,
}
```

### 2.2 Indexer 消费方式

```
MarkPriceUpdateEvent
    ├─→ INSERT dex_mark_prices (历史记录)
    ├─→ HSET dex:mark_price:{perpetual_id} (最新状态 Hash)
    └─→ XADD dex:stream:mark_prices (实时 Stream)
```

### 2.3 API 消费方式

| 端点 | 使用字段 | 数据源 |
|------|---------|-------|
| `metaAndAssetCtxs` | 全部字段 | Redis Hash |
| `clearinghouseState` | mark_price | Redis Hash |
| `activeAssetCtx` WS | 全部字段 | Redis Stream |

### 2.4 注意事项

- **频率**：频率过低（>30s）会导致 PnL 计算不够实时；频率过高（<1s）会增加存储压力
- **精度**：mark_price 和 oracle_price 使用 subticks 单位，与 FillEvent.price 一致
- **funding_rate 与 FundingSettlementEvent.funding_rate 的区别**：
  - MarkPriceUpdateEvent.funding_rate = 预测值（实时变化）
  - FundingSettlementEvent.funding_rate = 实际结算值（每 8h 一次）
- **open_interest**：引擎需要维护全市场持仓量的实时汇总

---

## 三、LeverageUpdateEvent 详细规格

### 3.1 事件结构

```rust
/// 杠杆更新事件
/// 发射位置：引擎的 updateLeverage / updateIsolatedMargin 命令处理
/// 发射时机：用户调整杠杆或逐仓保证金时
pub struct LeverageUpdateEvent {
    /// 永续市场 ID
    pub perpetual_id: u32,

    /// 子账户 (36 bytes = 32 address + 4 subaccount_number)
    pub subaccount: Vec<u8>,

    /// 杠杆倍数（如 20 表示 20x）
    /// 范围：1 到 max_leverage
    pub leverage_value: u32,

    /// 保证金模式：0=cross（全仓），1=isolated（逐仓）
    pub margin_mode: u8,

    /// 逐仓保证金金额 (quantums)
    /// 全仓模式时为 0
    /// 逐仓模式时为该仓位锁定的保证金
    pub isolated_margin: i128,

    pub timestamp_ms: u64,
}
```

### 3.2 Indexer 消费方式

```
LeverageUpdateEvent
    └─→ UPDATE dex_positions SET
            leverage_value = event.leverage_value,
            margin_mode = event.margin_mode,
            isolated_margin = event.isolated_margin
        WHERE account + subaccount + perpetual_id 匹配
```

### 3.3 注意事项

- **无仓位时的杠杆设置**：用户可以在开仓前设置杠杆。此时 dex_positions 中可能没有该用户的行。需要考虑是否 UPSERT 创建一个 size=0 的 position 行。
- **isolated_margin 精度**：使用 i128（与 BalanceUpdateEvent.delta 一致），存储为 16 字节 LE BYTEA
- **切换模式**：从 cross→isolated 或 isolated→cross 都通过同一事件处理

---

## 四、OrderPlacedEventV1 扩展详细规格

### 4.1 新增字段

```rust
pub struct OrderPlacedEventV1 {
    // ===== 现有字段（不变）=====
    pub perpetual_id: u32,
    pub order_id: Vec<u8>,
    pub subaccount: Vec<u8>,
    pub side: u8,
    pub price: u64,
    pub quantity: u64,
    pub order_type: u8,       // 0=Limit, 1=Market, 2=StopLimit, 3=StopMarket
    pub time_in_force: u8,
    pub reduce_only: bool,
    pub client_id: u64,
    pub timestamp_ms: u64,

    // ===== 新增字段 =====

    /// 触发价格 (subticks)
    /// 0 表示无触发价（普通限价单）
    /// 非 0 表示条件单的触发价格
    pub trigger_price: u64,

    /// 触发条件
    /// 0 = None（普通单）
    /// 1 = TakeProfit（止盈）
    /// 2 = StopLoss（止损）
    pub trigger_condition: u8,

    /// 父订单 ID
    /// 空 Vec 表示独立订单
    /// 非空表示该单是父单的 TP/SL 子单
    pub parent_order_id: Vec<u8>,

    /// 分组类型
    /// 0 = na（普通单）
    /// 1 = normalTpsl（主单 + TP/SL 组合）
    /// 2 = positionTpsl（仓位级 TP/SL）
    pub grouping: u8,
}
```

### 4.2 兼容性

- 新字段使用默认值（0 / 空 Vec）时，行为与当前一致
- Indexer 可以检查 `trigger_price == 0` 判断是否为普通单
- 旧版引擎生成的事件不含新字段时，反序列化需要默认值支持

### 4.3 注意：向后兼容

建议引擎使用 BCS 序列化时确保新字段有默认值，或者使用新的事件版本（如 OrderPlacedEventV2）。

**选择 A**：扩展 V1（所有新字段有默认值）
- 优点：简单，一个事件处理所有订单
- 缺点：旧数据反序列化可能失败（如果 BCS 不支持可选字段）

**选择 B**：新建 OrderPlacedEventV2
- 优点：完全兼容
- 缺点：Indexer 需要处理两种事件

**推荐选择 B**，参照已有的 V1 命名模式。

---

## 五、FillEvent 扩展（可选）

### 5.1 新增字段

```rust
pub struct FillEvent {
    // 现有字段...

    /// 是否为清算成交
    /// false = 普通成交
    /// true = 清算触发的成交
    pub is_liquidation: bool,
}
```

### 5.2 用途

- `userFills` API 响应中增加 `isLiquidation` 标记
- 前端可以在成交历史中标注清算成交
- `userNonFundingLedgerUpdates` 可以更精确地分类

### 5.3 优先级

**P2** — 可以通过时间窗口关联 LiquidationEvent 和 FillEvent 来近似判断。

---

## 六、PriceLevel 扩展（可选）

### 6.1 新增字段

```rust
pub struct PriceLevel {
    pub price: u64,
    pub quantity: u64,
    pub num_orders: u32,    // 该价格档位的订单数量
}
```

### 6.2 用途

- `l2Book` API 返回每个价格档位的订单数
- 对标 Hyperliquid 的 `n` 字段

### 6.3 优先级

**P2** — 不影响核心交易功能。

---

## 七、DexCommand 新增

### 7.1 UpdateLeverage

```rust
/// 更新用户杠杆设置
DexCommand::UpdateLeverage {
    /// 永续市场 ID
    perpetual_id: u32,
    /// 子账户编号
    subaccount_number: u32,
    /// 杠杆倍数 (1 到 max_leverage)
    leverage_value: u32,
    /// 保证金模式：true=cross, false=isolated
    is_cross: bool,
}
```

**验证规则**：
- `leverage_value` 必须在 1..=max_leverage 范围内
- 切换到 isolated 模式时，需要确保有足够保证金
- 有持仓时降低杠杆可能导致保证金不足 → 拒绝或自动调整

**触发事件**：LeverageUpdateEvent

### 7.2 UpdateIsolatedMargin

```rust
/// 调整逐仓保证金
DexCommand::UpdateIsolatedMargin {
    /// 永续市场 ID
    perpetual_id: u32,
    /// 子账户编号
    subaccount_number: u32,
    /// 方向（仅用于确定调整哪个方向的仓位）
    is_buy: bool,
    /// 保证金变化量（正=增加，负=减少）
    amount: i64,
}
```

**验证规则**：
- 必须是 isolated 模式
- 增加保证金：从账户余额转入
- 减少保证金：不能低于维持保证金要求

**触发事件**：LeverageUpdateEvent（更新后的 isolated_margin）

### 7.3 EIP-712 签名

两个新命令都需要 EIP-712 签名支持：

```rust
// sui-types/src/eip712/types.rs 需要新增

pub struct Eip712UpdateLeverageParams {
    pub subaccount_number: u32,
    pub perpetual_id: u32,
    pub leverage: u32,
    pub is_cross: bool,
    pub nonce: u64,
    pub deadline: u64,
}

pub struct Eip712UpdateIsolatedMarginParams {
    pub subaccount_number: u32,
    pub perpetual_id: u32,
    pub is_buy: bool,
    pub amount: i64,
    pub nonce: u64,
    pub deadline: u64,
}
```

---

## 八、集成时间线

### Phase 1：Indexer 准备（现在，不依赖引擎）

```
Week 1-2:
  ├── DB migrations (M1-M5)
  ├── Liquidation handler
  ├── FundingPayments Redis publish
  ├── 消除 clearinghouseState 硬编码
  └── 类型定义准备（responses.rs, requests.rs, exchange.rs, ws/types.rs）
```

### Phase 2：引擎集成（引擎交付后）

```
引擎交付顺序（建议）:
  1. MarkPriceUpdateEvent  → 解锁 metaAndAssetCtxs + PnL + activeAssetCtx
  2. LeverageUpdateEvent   → 解锁杠杆/保证金模式
  3. OrderPlacedEventV1 扩展 → 解锁 TP/SL 订单
  4. 跨链充提事件         → 解锁真实资产流转

每个事件交付后的集成步骤:
  1. 更新 dex_events.rs 事件定义（引擎 PR merge 后自动获得）
  2. 实现/修改 handler（1-2 天）
  3. API 端点对接（1 天）
  4. WS 频道对接（0.5 天）
  5. 端到端测试（1 天）
```

---

## 九、事件版本控制约定

### 9.1 现有模式

- `OrderPlacedEventV1` — 使用 V1 后缀
- `OrderRemovedEventV1` — 使用 V1 后缀
- 其他事件无版本后缀

### 9.2 建议约定

| 场景 | 策略 |
|------|------|
| 新增独立事件 | 无版本后缀（如 `MarkPriceUpdateEvent`） |
| 扩展现有事件（兼容） | 保持原名，新字段有默认值 |
| 扩展现有事件（不兼容） | 新建 V2（如 `OrderPlacedEventV2`） |
| 废弃旧事件 | 保留 V1 handler 一段时间，新数据用 V2 |

### 9.3 事件注册

所有事件在 `dex_events.rs` 中注册，使用虚拟包地址 `DEX_EVENTS_PACKAGE`：

```rust
const DEX_EVENTS_PACKAGE: &str = "0x...0000000044455800";
const DEX_EVENTS_MODULE: &str = "dex_events";

// 每个事件的 struct tag
pub fn mark_price_update_event_tag() -> StructTag {
    StructTag {
        address: DEX_EVENTS_PACKAGE.parse().unwrap(),
        module: DEX_EVENTS_MODULE.into(),
        name: "MarkPriceUpdateEvent".into(),
        type_params: vec![],
    }
}
```

---

## 十、接口确认检查表

### 工程师 B 确认项

| # | 接口 | 状态 | 确认内容 |
|---|------|------|---------|
| 1 | MarkPriceUpdateEvent | ⬜ 待确认 | 字段定义、发射频率、OI 计算方式 |
| 2 | LeverageUpdateEvent | ⬜ 待确认 | 字段定义、无仓位时的行为 |
| 3 | OrderPlacedEventV1 扩展 | ⬜ 待确认 | 扩展 V1 还是新建 V2？TP/SL 字段定义 |
| 4 | UpdateLeverage DexCommand | ⬜ 待确认 | 参数定义、验证规则 |
| 5 | UpdateIsolatedMargin DexCommand | ⬜ 待确认 | 参数定义、验证规则 |
| 6 | FillEvent.is_liquidation | ⬜ 待确认 | 是否值得扩展？ |
| 7 | PriceLevel.num_orders | ⬜ 待确认 | 实现成本评估 |
| 8 | EIP-712 新签名类型 | ⬜ 待确认 | TypeHash 定义 |

### 工程师 A 确认项

| # | 接口 | 状态 | 确认内容 |
|---|------|------|---------|
| 1 | 充值事件设计 | ⬜ 待确认 | 复用 BalanceUpdateEvent 还是新事件？ |
| 2 | 提款事件设计 | ⬜ 待确认 | 是否需要 WithdrawalEvent？ |
| 3 | update_type 扩展 | ⬜ 待确认 | 新增哪些类型值？ |
| 4 | 提款中间状态 | ⬜ 待确认 | Pending → Confirmed → Failed 流程 |
| 5 | 跨链桥 txHash | ⬜ 待确认 | 是否需要在事件中携带？ |

---

## 十一、风险与应对

| 风险 | 影响 | 应对 |
|------|------|------|
| BCS 序列化不兼容 | 旧 checkpoint 反序列化失败 | 使用 V2 事件 + 保留 V1 handler |
| mark price 频率不够 | PnL 更新延迟 | 回退使用 mid price |
| OI 计算不准 | metaAndAssetCtxs 数据不准 | 从 dex_positions 表聚合 |
| 引擎交付延迟 | Part 2 工作推迟 | Part 1 先完成，不阻塞 |
| EIP-712 签名不兼容 | 前端无法调用 | 统一签名库版本 |
