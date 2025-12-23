# OrderBook DEX V4：Shared Nothing 架构设计

**版本**: v4.2 (Shared Nothing)
**日期**: 2025-12-24
**状态**: 架构设计文档

---

## 摘要

本文档探讨基于 **Shared Nothing** 架构思想设计 OrderBook DEX。Shared Nothing 的核心原则是：**每个处理单元拥有独立数据，通过消息传递协调，最大化并行处理能力**。

在 Sui 的语境下，这意味着：
- 所有对象都是 **Owned Object**（用户独占）
- **无 Shared Object**（避免共识瓶颈）
- 通过 **链下撮合 + 链上结算** 或 **点对点匹配** 实现交易

---

## 目录

1. [Shared Nothing 架构原理](#1-shared-nothing-架构原理)
2. [DEX 场景的挑战](#2-dex-场景的挑战)
3. [Shared Nothing DEX 设计](#3-shared-nothing-dex-设计)
4. [核心机制详解](#4-核心机制详解)
5. [与其他方案对比](#5-与其他方案对比)
6. [实现方案](#6-实现方案)
7. [性能分析](#7-性能分析)
8. [适用场景与限制](#8-适用场景与限制)
9. [结论](#9-结论)

---

## 1. Shared Nothing 架构原理

### 1.1 核心思想

```
┌─────────────────────────────────────────────────────────────────────────┐
│                    Shared Nothing vs Shared Everything                   │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│  Shared Everything:                                                      │
│  ┌─────────────────────────────────────────────────────────────────┐    │
│  │                     Global Shared State                          │    │
│  │  ┌─────────┐  ┌─────────┐  ┌─────────┐  ┌─────────┐            │    │
│  │  │ User A  │  │ User B  │  │ User C  │  │ User D  │            │    │
│  │  └────┬────┘  └────┬────┘  └────┬────┘  └────┬────┘            │    │
│  │       └───────────┬┴───────────┬┴───────────┘                   │    │
│  │                   ▼            ▼                                 │    │
│  │           ┌──────────────────────────┐                          │    │
│  │           │   Shared OrderBook       │  ← 竞争点，需要共识       │    │
│  │           └──────────────────────────┘                          │    │
│  └─────────────────────────────────────────────────────────────────┘    │
│                                                                          │
│  Shared Nothing:                                                         │
│  ┌─────────────────────────────────────────────────────────────────┐    │
│  │                     Independent Units                            │    │
│  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐              │    │
│  │  │   User A    │  │   User B    │  │   User C    │              │    │
│  │  │  ┌───────┐  │  │  ┌───────┐  │  │  ┌───────┐  │              │    │
│  │  │  │Balance│  │  │  │Balance│  │  │  │Balance│  │              │    │
│  │  │  │Orders │  │  │  │Orders │  │  │  │Orders │  │              │    │
│  │  │  └───────┘  │  │  └───────┘  │  │  └───────┘  │              │    │
│  │  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘              │    │
│  │         │                │                │                      │    │
│  │         └────────────────┼────────────────┘                      │    │
│  │                          ▼                                       │    │
│  │              Message Passing / Matching                          │    │
│  │              (Off-chain or P2P)                                  │    │
│  └─────────────────────────────────────────────────────────────────┘    │
│                                                                          │
└─────────────────────────────────────────────────────────────────────────┘
```

### 1.2 Shared Nothing 的核心原则

| 原则 | 说明 | Sui 映射 |
|-----|------|---------|
| **数据独立** | 每个单元拥有独立数据 | Owned Object |
| **无共享状态** | 单元之间不共享可变状态 | 无 Shared Object |
| **消息传递** | 通过消息/事件协调 | Sui Events + 链下通信 |
| **并行处理** | 最大化并行能力 | 完全并发交易 |
| **无锁设计** | 避免锁竞争 | 无共识排序需求 |

### 1.3 在 Sui 上的体现

```rust
// Shared Nothing: 所有对象都是 Owned
struct UserAccount has key {
    id: UID,
    owner: address,           // 独占所有权
    balances: Table<Token, u64>,
    orders: vector<Order>,    // 订单也属于用户
    pending_trades: vector<PendingTrade>,
}

// 对比 Shared Everything
struct DexState has key {
    id: UID,
    // 所有用户共享的订单簿 - Shared Object
    orderbook: OrderBook,     // ← 竞争点
    all_balances: Table<address, Balance>,
}
```

---

## 2. DEX 场景的挑战

### 2.1 核心矛盾

**订单簿本质上是一个全局共享结构**：
- 所有买家竞争最优卖价
- 所有卖家竞争最优买价
- 价格发现需要全局视图

**Shared Nothing 要求**：
- 无共享状态
- 独立处理

**如何调和？**

### 2.2 解决思路

```
┌─────────────────────────────────────────────────────────────────────────┐
│                    Shared Nothing DEX 解决方案                           │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│  思路 1: 链下撮合 + 链上结算                                             │
│  ┌─────────────────────────────────────────────────────────────────┐    │
│  │  链下:                          链上:                            │    │
│  │  • 维护订单簿                   • 验证匹配                       │    │
│  │  • 执行撮合                     • 原子结算                       │    │
│  │  • 产生匹配对                   • 资金划转                       │    │
│  └─────────────────────────────────────────────────────────────────┘    │
│                                                                          │
│  思路 2: 点对点订单匹配                                                  │
│  ┌─────────────────────────────────────────────────────────────────┐    │
│  │  • 买方发布意向（链上）                                          │    │
│  │  • 卖方发现并接受（链下/链上）                                   │    │
│  │  • 双方签名确认                                                  │    │
│  │  • 原子交换执行                                                  │    │
│  └─────────────────────────────────────────────────────────────────┘    │
│                                                                          │
│  思路 3: 批量拍卖（Batch Auction）                                       │
│  ┌─────────────────────────────────────────────────────────────────┐    │
│  │  • 收集时间窗口内的订单                                          │    │
│  │  • 计算统一清算价格                                              │    │
│  │  • 批量执行所有匹配                                              │    │
│  └─────────────────────────────────────────────────────────────────┘    │
│                                                                          │
└─────────────────────────────────────────────────────────────────────────┘
```

---

## 3. Shared Nothing DEX 设计

### 3.1 架构概览

```
┌─────────────────────────────────────────────────────────────────────────┐
│                 Shared Nothing DEX V4 架构                               │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│                              用户层                                      │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐    │
│  │   User A    │  │   User B    │  │   User C    │  │   User D    │    │
│  │  (Owned)    │  │  (Owned)    │  │  (Owned)    │  │  (Owned)    │    │
│  │             │  │             │  │             │  │             │    │
│  │ • Balance   │  │ • Balance   │  │ • Balance   │  │ • Balance   │    │
│  │ • Orders    │  │ • Orders    │  │ • Orders    │  │ • Orders    │    │
│  │ • Intents   │  │ • Intents   │  │ • Intents   │  │ • Intents   │    │
│  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘    │
│         │                │                │                │            │
│         └────────────────┼────────────────┼────────────────┘            │
│                          │                │                             │
│                          ▼                ▼                             │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │                    链下撮合层 (Off-chain Matcher)                 │   │
│  │                                                                   │   │
│  │  • 监听链上订单意向事件                                           │   │
│  │  • 维护订单簿视图（非权威）                                       │   │
│  │  • 发现匹配对                                                     │   │
│  │  • 生成结算凭证                                                   │   │
│  │                                                                   │   │
│  │  注意: 这一层是无状态的，可水平扩展，任何人都可以运行             │   │
│  └───────────────────────────────┬───────────────────────────────────┘   │
│                                  │                                       │
│                                  │ Settlement Proofs                     │
│                                  ▼                                       │
│  ┌─────────────────────────────────────────────────────────────────┐    │
│  │                    链上结算层 (On-chain Settlement)               │    │
│  │                                                                   │    │
│  │  • 验证匹配有效性                                                 │    │
│  │  • 验证双方签名                                                   │    │
│  │  • 原子交换资产                                                   │    │
│  │  • 更新订单状态                                                   │    │
│  │                                                                   │    │
│  │  关键: 结算只涉及交易双方的 Owned Objects，无 Shared Object      │    │
│  └─────────────────────────────────────────────────────────────────┘    │
│                                                                          │
└─────────────────────────────────────────────────────────────────────────┘
```

### 3.2 数据模型

```rust
module dex_shared_nothing::types {
    use sui::object::{Self, UID};
    use sui::table::Table;
    use sui::coin::Coin;

    /// 用户账户 - Owned Object（每用户独立）
    struct UserAccount has key, store {
        id: UID,
        owner: address,

        // 可用余额（每种代币）
        available_balances: Table<TypeName, Coin>,

        // 冻结余额（已下单）
        frozen_balances: Table<TypeName, u64>,

        // 活跃订单列表
        active_orders: vector<Order>,

        // 已完成订单（历史）
        completed_orders: vector<OrderId>,

        // Nonce（防重放）
        nonce: u64,
    }

    /// 订单 - 存储在用户账户中（非独立对象）
    struct Order has store, copy, drop {
        // 订单标识
        order_id: u64,

        // 交易对
        pair: TradingPair,

        // 订单参数
        side: OrderSide,
        order_type: OrderType,
        price: u64,
        quantity: u64,
        remaining: u64,

        // 时间戳
        created_at: u64,
        expires_at: u64,

        // 冻结金额
        frozen_amount: u64,
    }

    /// 订单意向（链上广播）- 通过事件发布
    struct OrderIntent has copy, drop {
        // 订单所有者
        owner: address,
        account_id: ID,
        order_id: u64,

        // 订单参数
        pair: TradingPair,
        side: OrderSide,
        price: u64,
        quantity: u64,

        // 签名（用于验证）
        signature: vector<u8>,

        // 有效期
        expires_at: u64,
    }

    /// 结算凭证 - 由链下撮合器生成
    struct SettlementProof has copy, drop {
        // 匹配信息
        maker_account: ID,
        maker_order_id: u64,
        taker_account: ID,
        taker_order_id: u64,

        // 成交参数
        price: u64,
        quantity: u64,

        // 双方签名
        maker_signature: vector<u8>,
        taker_signature: vector<u8>,

        // 时间戳
        matched_at: u64,
    }

    /// 交易对配置 - 可以是 Shared（只读配置）或通过 Capability 管理
    struct TradingPairConfig has key {
        id: UID,
        base_token: TypeName,
        quote_token: TypeName,
        min_order_size: u64,
        tick_size: u64,
        maker_fee_rate: u64,
        taker_fee_rate: u64,
    }
}
```

### 3.3 核心流程

```
┌─────────────────────────────────────────────────────────────────────────┐
│                    Shared Nothing 交易流程                               │
└─────────────────────────────────────────────────────────────────────────┘

Step 1: 下单（链上，Owned Object）
┌─────────────────────────────────────────────────────────────────────────┐
│  User A 想买 1 BTC @ 50,000 USDT                                        │
│                                                                          │
│  交易内容:                                                               │
│  ├── 输入: UserAccount (Owned by A)                                     │
│  ├── 操作:                                                              │
│  │   1. 验证余额: available[USDT] >= 50,000                            │
│  │   2. 冻结资金: available[USDT] -= 50,000, frozen[USDT] += 50,000    │
│  │   3. 创建订单: orders.push(Order { side: Buy, price: 50000, ... })  │
│  │   4. 发送事件: emit OrderIntentEvent { ... }                        │
│  └── 输出: 更新后的 UserAccount                                         │
│                                                                          │
│  特性:                                                                   │
│  ✓ 只操作 Owned Object                                                  │
│  ✓ 无需共识排序                                                         │
│  ✓ 完全并发（不同用户互不影响）                                         │
│  ✓ 延迟: ~200-400ms                                                     │
│  ✓ 吞吐: 100K+ TPS                                                      │
└─────────────────────────────────────────────────────────────────────────┘
                                │
                                │ OrderIntentEvent
                                ▼
Step 2: 链下撮合
┌─────────────────────────────────────────────────────────────────────────┐
│  Matcher 节点（任何人都可运行）                                          │
│                                                                          │
│  工作流程:                                                               │
│  1. 监听 OrderIntentEvent                                               │
│  2. 维护本地订单簿视图:                                                  │
│     OrderBook {                                                          │
│       bids: [(50000, [OrderIntent{A, ...}]), ...]                       │
│       asks: [(50100, [OrderIntent{B, ...}]), ...]                       │
│     }                                                                    │
│  3. 发现匹配:                                                            │
│     当 User B 发布卖单 @ 50,000 时:                                     │
│     - 检测到 A 的买单 @ 50,000 可以匹配                                 │
│  4. 请求双方签名:                                                        │
│     - 发送匹配提议给 A 和 B                                             │
│     - 收集双方签名                                                       │
│  5. 生成 SettlementProof                                                │
│                                                                          │
│  特性:                                                                   │
│  ✓ 完全链下，无 Gas                                                     │
│  ✓ 可水平扩展（多个 Matcher 竞争）                                      │
│  ✓ 无需信任（结果需链上验证）                                           │
│  ✓ 延迟: ~10-100ms                                                      │
└─────────────────────────────────────────────────────────────────────────┘
                                │
                                │ SettlementProof
                                ▼
Step 3: 链上结算（原子交换）
┌─────────────────────────────────────────────────────────────────────────┐
│  Settlement Transaction                                                  │
│                                                                          │
│  交易内容:                                                               │
│  ├── 输入:                                                              │
│  │   - UserAccount A (Owned by A)                                       │
│  │   - UserAccount B (Owned by B)                                       │
│  │   - SettlementProof                                                  │
│  │                                                                       │
│  ├── 验证:                                                              │
│  │   1. 验证 maker 签名（A 确实同意这笔交易）                           │
│  │   2. 验证 taker 签名（B 确实同意这笔交易）                           │
│  │   3. 验证订单存在且状态有效                                          │
│  │   4. 验证价格和数量匹配                                              │
│  │   5. 验证未过期                                                       │
│  │                                                                       │
│  ├── 执行（原子）:                                                       │
│  │   // A 是买方                                                         │
│  │   A.frozen[USDT] -= 50,000                                           │
│  │   A.available[BTC] += 1                                              │
│  │   A.orders[order_id].remaining -= 1                                  │
│  │                                                                       │
│  │   // B 是卖方                                                         │
│  │   B.frozen[BTC] -= 1                                                 │
│  │   B.available[USDT] += 50,000                                        │
│  │   B.orders[order_id].remaining -= 1                                  │
│  │                                                                       │
│  └── 输出:                                                              │
│      - 更新后的 UserAccount A                                           │
│      - 更新后的 UserAccount B                                           │
│      - TradeExecutedEvent                                               │
│                                                                          │
│  关键设计:                                                               │
│  ✓ 只涉及双方的 Owned Objects                                           │
│  ✓ 需要双方签名授权（通过 Proof 中的签名）                              │
│  ✓ 原子执行（全成功或全失败）                                           │
│  ✓ 无 Shared Object，无共识排序                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

---

## 4. 核心机制详解

### 4.1 订单意向发布

```move
module dex_shared_nothing::order {
    use sui::event;
    use sui::clock::Clock;

    /// 发布买单意向
    public entry fun place_buy_order(
        account: &mut UserAccount,
        pair: TradingPair,
        price: u64,
        quantity: u64,
        expires_in_ms: u64,
        clock: &Clock,
        ctx: &mut TxContext,
    ) {
        // 1. 计算需要冻结的金额
        let freeze_amount = price * quantity;
        let quote_token = get_quote_token(&pair);

        // 2. 验证余额
        let available = table::borrow_mut(&mut account.available_balances, quote_token);
        assert!(coin::value(available) >= freeze_amount, E_INSUFFICIENT_BALANCE);

        // 3. 冻结资金
        let frozen_coin = coin::split(available, freeze_amount, ctx);
        let frozen = table::borrow_mut(&mut account.frozen_balances, quote_token);
        *frozen = *frozen + freeze_amount;
        // 注意：这里简化处理，实际需要管理 Coin 对象

        // 4. 创建订单
        let order_id = account.nonce;
        account.nonce = account.nonce + 1;

        let now = clock::timestamp_ms(clock);
        let order = Order {
            order_id,
            pair,
            side: ORDER_SIDE_BUY,
            order_type: ORDER_TYPE_LIMIT,
            price,
            quantity,
            remaining: quantity,
            created_at: now,
            expires_at: now + expires_in_ms,
            frozen_amount: freeze_amount,
        };

        vector::push_back(&mut account.active_orders, order);

        // 5. 发布订单意向事件（供链下 Matcher 监听）
        event::emit(OrderIntentEvent {
            owner: tx_context::sender(ctx),
            account_id: object::id(account),
            order_id,
            pair,
            side: ORDER_SIDE_BUY,
            price,
            quantity,
            expires_at: now + expires_in_ms,
        });
    }

    /// 发布卖单意向
    public entry fun place_sell_order(
        account: &mut UserAccount,
        pair: TradingPair,
        price: u64,
        quantity: u64,
        expires_in_ms: u64,
        clock: &Clock,
        ctx: &mut TxContext,
    ) {
        // 类似买单，但冻结 base token
        let base_token = get_base_token(&pair);
        // ... 省略类似逻辑
    }
}
```

### 4.2 链下撮合器

```rust
/// 链下撮合器（Rust 实现）
pub struct OffchainMatcher {
    /// 订单簿（按交易对）
    orderbooks: HashMap<TradingPair, OrderBook>,

    /// Sui 客户端（监听事件）
    sui_client: SuiClient,

    /// 待确认的匹配
    pending_matches: Vec<PendingMatch>,
}

impl OffchainMatcher {
    /// 主循环
    pub async fn run(&mut self) {
        loop {
            // 1. 监听新订单事件
            let events = self.sui_client.subscribe_events(
                EventFilter::MoveEventType("OrderIntentEvent")
            ).await;

            for event in events {
                let intent: OrderIntentEvent = event.parse();

                // 2. 更新本地订单簿
                self.update_orderbook(&intent);

                // 3. 尝试匹配
                if let Some(matches) = self.try_match(&intent) {
                    for m in matches {
                        // 4. 请求双方签名
                        let proof = self.request_signatures(m).await;

                        if let Some(proof) = proof {
                            // 5. 提交结算交易
                            self.submit_settlement(proof).await;
                        }
                    }
                }
            }
        }
    }

    /// 尝试匹配订单
    fn try_match(&mut self, new_order: &OrderIntentEvent) -> Option<Vec<Match>> {
        let orderbook = self.orderbooks.get_mut(&new_order.pair)?;

        let mut matches = Vec::new();
        let mut remaining = new_order.quantity;

        let opposite_side = if new_order.side == OrderSide::Buy {
            &mut orderbook.asks
        } else {
            &mut orderbook.bids
        };

        // 价格-时间优先匹配
        while remaining > 0 {
            let best = opposite_side.peek()?;

            // 检查价格是否匹配
            if !self.prices_match(new_order.side, new_order.price, best.price) {
                break;
            }

            let fill_qty = std::cmp::min(remaining, best.remaining);

            matches.push(Match {
                maker: best.clone(),
                taker: new_order.clone(),
                price: best.price,  // 使用 maker 价格
                quantity: fill_qty,
            });

            remaining -= fill_qty;

            // 更新或移除挂单
            if fill_qty == best.remaining {
                opposite_side.pop();
            } else {
                opposite_side.peek_mut().unwrap().remaining -= fill_qty;
            }
        }

        // 如果是限价单且有剩余，加入订单簿
        if remaining > 0 && new_order.order_type == OrderType::Limit {
            let side = if new_order.side == OrderSide::Buy {
                &mut orderbook.bids
            } else {
                &mut orderbook.asks
            };
            side.insert(OrderIntentEvent {
                remaining,
                ..new_order.clone()
            });
        }

        if matches.is_empty() {
            None
        } else {
            Some(matches)
        }
    }

    /// 请求双方签名
    async fn request_signatures(&self, m: Match) -> Option<SettlementProof> {
        // 构建签名消息
        let message = SettlementMessage {
            maker_account: m.maker.account_id,
            maker_order_id: m.maker.order_id,
            taker_account: m.taker.account_id,
            taker_order_id: m.taker.order_id,
            price: m.price,
            quantity: m.quantity,
            timestamp: current_timestamp(),
        };

        // 请求 maker 签名（可以是预签名或实时请求）
        let maker_sig = self.request_user_signature(
            m.maker.owner,
            &message,
        ).await?;

        // 请求 taker 签名
        let taker_sig = self.request_user_signature(
            m.taker.owner,
            &message,
        ).await?;

        Some(SettlementProof {
            maker_account: m.maker.account_id,
            maker_order_id: m.maker.order_id,
            taker_account: m.taker.account_id,
            taker_order_id: m.taker.order_id,
            price: m.price,
            quantity: m.quantity,
            maker_signature: maker_sig,
            taker_signature: taker_sig,
            matched_at: current_timestamp(),
        })
    }
}
```

### 4.3 链上结算

```move
module dex_shared_nothing::settlement {
    use sui::ed25519;

    /// 执行结算（原子交换）
    ///
    /// 关键: 此函数需要双方签名授权
    /// 通过 SettlementProof 中的签名验证
    public entry fun settle(
        maker_account: &mut UserAccount,
        taker_account: &mut UserAccount,
        proof: SettlementProof,
        clock: &Clock,
        ctx: &mut TxContext,
    ) {
        // 1. 验证未过期
        let now = clock::timestamp_ms(clock);
        assert!(now <= proof.matched_at + SETTLEMENT_TIMEOUT, E_EXPIRED);

        // 2. 构建签名消息
        let message = build_settlement_message(&proof);

        // 3. 验证 maker 签名
        let maker_pubkey = get_owner_pubkey(maker_account);
        assert!(
            ed25519::verify(&proof.maker_signature, &maker_pubkey, &message),
            E_INVALID_MAKER_SIGNATURE
        );

        // 4. 验证 taker 签名
        let taker_pubkey = get_owner_pubkey(taker_account);
        assert!(
            ed25519::verify(&proof.taker_signature, &taker_pubkey, &message),
            E_INVALID_TAKER_SIGNATURE
        );

        // 5. 验证订单存在且有效
        let maker_order = find_order(&maker_account.active_orders, proof.maker_order_id);
        let taker_order = find_order(&taker_account.active_orders, proof.taker_order_id);

        assert!(option::is_some(&maker_order), E_ORDER_NOT_FOUND);
        assert!(option::is_some(&taker_order), E_ORDER_NOT_FOUND);

        let maker_order = option::borrow(&maker_order);
        let taker_order = option::borrow(&taker_order);

        // 6. 验证订单参数匹配
        assert!(maker_order.pair == taker_order.pair, E_PAIR_MISMATCH);
        assert!(maker_order.side != taker_order.side, E_SAME_SIDE);
        assert!(proof.quantity <= maker_order.remaining, E_QUANTITY_EXCEEDED);
        assert!(proof.quantity <= taker_order.remaining, E_QUANTITY_EXCEEDED);

        // 7. 验证价格
        // Maker 是挂单方，价格应该更优或相等
        if (maker_order.side == ORDER_SIDE_BUY) {
            assert!(proof.price <= maker_order.price, E_PRICE_MISMATCH);
            assert!(proof.price >= taker_order.price, E_PRICE_MISMATCH);
        } else {
            assert!(proof.price >= maker_order.price, E_PRICE_MISMATCH);
            assert!(proof.price <= taker_order.price, E_PRICE_MISMATCH);
        };

        // 8. 执行资金交换
        let pair = maker_order.pair;
        let base_token = get_base_token(&pair);
        let quote_token = get_quote_token(&pair);
        let trade_value = proof.price * proof.quantity;

        // 确定买卖方
        let (buyer_account, seller_account) = if (maker_order.side == ORDER_SIDE_BUY) {
            (maker_account, taker_account)
        } else {
            (taker_account, maker_account)
        };

        // 买方: 解冻 quote token，获得 base token
        let buyer_frozen = table::borrow_mut(&mut buyer_account.frozen_balances, quote_token);
        *buyer_frozen = *buyer_frozen - trade_value;

        let buyer_available = table::borrow_mut(&mut buyer_account.available_balances, base_token);
        coin::join(buyer_available, /* transfer base token */);

        // 卖方: 解冻 base token，获得 quote token
        let seller_frozen = table::borrow_mut(&mut seller_account.frozen_balances, base_token);
        *seller_frozen = *seller_frozen - proof.quantity;

        let seller_available = table::borrow_mut(&mut seller_account.available_balances, quote_token);
        coin::join(seller_available, /* transfer quote token */);

        // 9. 更新订单状态
        update_order_remaining(&mut maker_account.active_orders, proof.maker_order_id, proof.quantity);
        update_order_remaining(&mut taker_account.active_orders, proof.taker_order_id, proof.quantity);

        // 10. 发送成交事件
        event::emit(TradeExecutedEvent {
            maker: object::id(maker_account),
            taker: object::id(taker_account),
            pair,
            price: proof.price,
            quantity: proof.quantity,
            timestamp: now,
        });
    }
}
```

### 4.4 预签名机制（优化延迟）

为了减少实时签名的延迟，可以使用**预签名**机制：

```rust
/// 预签名订单意向
struct PreSignedIntent {
    intent: OrderIntent,

    /// 预先签署的结算授权
    /// 允许任何符合条件的匹配进行结算
    settlement_authorization: SettlementAuthorization,
}

struct SettlementAuthorization {
    /// 允许的价格范围
    min_price: u64,  // 买单: 0, 卖单: 最低接受价
    max_price: u64,  // 买单: 最高接受价, 卖单: u64::MAX

    /// 允许的数量范围
    min_quantity: u64,
    max_quantity: u64,

    /// 有效期
    expires_at: u64,

    /// 用户签名（覆盖上述所有条件）
    signature: vector<u8>,
}

// 使用预签名后，Matcher 可以直接生成有效的 SettlementProof
// 无需实时请求用户签名
impl OffchainMatcher {
    fn generate_proof_with_presig(
        &self,
        m: Match,
        maker_presig: &PreSignedIntent,
        taker_presig: &PreSignedIntent,
    ) -> SettlementProof {
        // 验证匹配在预签名授权范围内
        assert!(m.price >= maker_presig.settlement_authorization.min_price);
        assert!(m.price <= maker_presig.settlement_authorization.max_price);
        // ... 其他验证

        SettlementProof {
            // ... 使用预签名
            maker_signature: maker_presig.settlement_authorization.signature,
            taker_signature: taker_presig.settlement_authorization.signature,
            // ...
        }
    }
}
```

---

## 5. 与其他方案对比

### 5.1 完整对比表

| 维度 | V4 三阶段 | Shared Everything | **Shared Nothing** |
|-----|----------|-------------------|-------------------|
| **下单延迟** | ~200-400ms | ~400-500ms | **~200-400ms** ✓ |
| **撮合延迟** | ~400-500ms (共识) | ~400-500ms (共识) | **~10-100ms (链下)** ✓ |
| **结算延迟** | 与撮合一起 | 与撮合一起 | ~200-400ms |
| **下单吞吐** | 100K+ TPS | ~10K TPS | **100K+ TPS** ✓ |
| **撮合吞吐** | ~10K TPS | ~10K TPS | **无限 (链下)** ✓ |
| **结算吞吐** | ~10K TPS | ~10K TPS | **100K+ TPS** ✓ |
| **共识依赖** | 撮合需要 | 全部需要 | **不需要** ✓ |
| **去中心化** | 高 | 最高 | 中 (Matcher 可中心化) |
| **实现复杂度** | 中 | 低 | **高** |
| **用户体验** | 需等待共识 | 需等待共识 | **最快反馈** ✓ |

### 5.2 关键差异分析

```
┌─────────────────────────────────────────────────────────────────────────┐
│                    关键差异对比                                          │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│  共识依赖:                                                               │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │  V4 三阶段:        下单(无) → 撮合(有) → 结算(有)               │   │
│  │  Shared Everything: 下单(有) → 撮合(有) → 结算(有)               │   │
│  │  Shared Nothing:   下单(无) → 撮合(无) → 结算(无)               │   │
│  │                                          ↑                       │   │
│  │                            完全不依赖共识排序                     │   │
│  └─────────────────────────────────────────────────────────────────┘   │
│                                                                          │
│  信任模型:                                                               │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │  V4/Shared Everything:                                          │   │
│  │  - 信任 Mysticeti 共识                                          │   │
│  │  - 2/3+ 验证者诚实                                              │   │
│  │                                                                  │   │
│  │  Shared Nothing:                                                 │   │
│  │  - 信任双方签名（密码学保证）                                    │   │
│  │  - 不信任 Matcher（可验证）                                      │   │
│  │  - 信任 Sui 执行（验证签名）                                     │   │
│  └─────────────────────────────────────────────────────────────────┘   │
│                                                                          │
│  可扩展性:                                                               │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │  V4/Shared Everything:                                          │   │
│  │  - 受共识吞吐限制 (~10K TPS)                                    │   │
│  │  - 需要分片扩展                                                  │   │
│  │                                                                  │   │
│  │  Shared Nothing:                                                 │   │
│  │  - 链下撮合无限扩展                                              │   │
│  │  - 链上结算 100K+ TPS（Owned Object 并发）                       │   │
│  │  - 天然水平扩展                                                  │   │
│  └─────────────────────────────────────────────────────────────────┘   │
│                                                                          │
└─────────────────────────────────────────────────────────────────────────┘
```

---

## 6. 实现方案

### 6.1 模块划分

```
dex-shared-nothing/
├── contracts/                    # Move 智能合约
│   ├── sources/
│   │   ├── account.move         # 用户账户管理
│   │   ├── order.move           # 订单管理
│   │   ├── settlement.move      # 结算逻辑
│   │   └── types.move           # 类型定义
│   └── tests/
│
├── matcher/                      # 链下撮合器
│   ├── src/
│   │   ├── main.rs
│   │   ├── orderbook.rs         # 订单簿实现
│   │   ├── matching.rs          # 撮合算法
│   │   ├── event_listener.rs    # 事件监听
│   │   └── settlement.rs        # 结算提交
│   └── Cargo.toml
│
├── sdk/                          # 客户端 SDK
│   ├── src/
│   │   ├── client.rs            # DEX 客户端
│   │   ├── signing.rs           # 签名工具
│   │   └── types.rs             # 类型定义
│   └── Cargo.toml
│
└── docs/                         # 文档
```

### 6.2 实施路线图

```
Phase 1: 核心合约 (3-4 周)
├── Week 1-2: 账户和订单管理
│   ├── UserAccount 结构
│   ├── 下单/撤单逻辑
│   └── 事件发布
└── Week 3-4: 结算合约
    ├── 签名验证
    ├── 原子交换
    └── 单元测试

Phase 2: 链下撮合器 (3-4 周)
├── Week 1-2: 基础功能
│   ├── 事件监听
│   ├── 订单簿维护
│   └── 撮合算法
└── Week 3-4: 高级功能
    ├── 预签名支持
    ├── 批量结算
    └── 性能优化

Phase 3: 集成测试 (2-3 周)
├── 端到端测试
├── 性能基准
└── 安全审计

Phase 4: 优化与部署 (2-3 周)
├── Matcher 高可用
├── 监控告警
└── 文档完善

总计: 10-14 周
```

---

## 7. 性能分析

### 7.1 延迟分解

```
Shared Nothing 延迟分析:

下单 (链上，Owned Object):
├── 网络传输: ~50ms
├── 验证者执行: ~30ms
├── 签名收集: ~200ms
└── 总计: ~280ms

撮合 (链下):
├── 事件传播: ~10ms
├── 订单簿匹配: <1ms
├── 签名请求: ~50ms (预签名: 0ms)
└── 总计: ~60ms (预签名: ~10ms)

结算 (链上，双 Owned Object):
├── 网络传输: ~50ms
├── 签名验证: ~5ms
├── 状态更新: ~20ms
├── 签名收集: ~200ms
└── 总计: ~275ms

端到端 (无预签名):
下单 → 撮合 → 结算 ≈ 280 + 60 + 275 = ~615ms

端到端 (有预签名):
下单 → 撮合 → 结算 ≈ 280 + 10 + 275 = ~565ms

对比:
V4 三阶段: 200-400 + 400-500 = ~600-900ms
Shared Everything: ~400-500ms
Shared Nothing: ~565-615ms (可并行优化)
```

### 7.2 吞吐量分析

```
下单吞吐:
- 类型: Owned Object 交易
- 限制: Sui 简单交易吞吐
- 预估: 100K+ TPS

撮合吞吐:
- 类型: 链下计算
- 限制: Matcher 性能
- 预估: 1M+ matches/s (单节点)

结算吞吐:
- 类型: 双 Owned Object 交易
- 限制: Sui 简单交易吞吐
- 预估: 50K+ TPS (需要双方 Object)

整体吞吐:
- 瓶颈: 结算
- 预估: ~50K TPS
- 优化: 批量结算可提升
```

### 7.3 批量结算优化

```move
/// 批量结算 - 多笔交易一次执行
public entry fun batch_settle(
    accounts: &mut vector<UserAccount>,
    proofs: vector<SettlementProof>,
    clock: &Clock,
    ctx: &mut TxContext,
) {
    let len = vector::length(&proofs);
    let mut i = 0;

    while (i < len) {
        let proof = vector::borrow(&proofs, i);

        // 找到对应账户
        let maker_account = find_account_mut(accounts, proof.maker_account);
        let taker_account = find_account_mut(accounts, proof.taker_account);

        // 执行单笔结算
        settle_internal(maker_account, taker_account, proof, clock);

        i = i + 1;
    };
}
```

---

## 8. 适用场景与限制

### 8.1 适用场景

```
✅ 适合 Shared Nothing 的场景:

1. 高频交易
   - 需要最低延迟
   - 对链下撮合接受度高
   - 有专业做市商

2. 大宗交易
   - 交易量大，愿意等待匹配
   - 对手方明确（OTC 风格）
   - 对价格发现要求不高

3. 专业交易者
   - 理解预签名机制
   - 有技术能力运行客户端
   - 追求性能极致

4. 跨链交易
   - 不同链资产交换
   - 原子交换需求
   - 无中心化信任
```

### 8.2 限制与挑战

```
⚠️ Shared Nothing 的限制:

1. 价格发现
   ┌─────────────────────────────────────────────────────────────────┐
   │  问题: 链下订单簿不是权威的                                      │
   │  - 不同 Matcher 可能有不同视图                                  │
   │  - 订单可能被撤销但 Matcher 不知道                              │
   │  - 价格可能不是"真实"市场价                                     │
   │                                                                  │
   │  缓解:                                                           │
   │  - 多个 Matcher 竞争提供更好价格                                │
   │  - 用户选择最优报价                                             │
   │  - 链上验证确保公平                                             │
   └─────────────────────────────────────────────────────────────────┘

2. 订单同步
   ┌─────────────────────────────────────────────────────────────────┐
   │  问题: 用户撤单后，链下 Matcher 可能还在尝试匹配                 │
   │                                                                  │
   │  缓解:                                                           │
   │  - 撤单事件实时传播                                             │
   │  - 结算时验证订单有效性                                         │
   │  - 短有效期订单                                                  │
   └─────────────────────────────────────────────────────────────────┘

3. 用户体验
   ┌─────────────────────────────────────────────────────────────────┐
   │  问题:                                                           │
   │  - 需要用户签署多次（或理解预签名）                              │
   │  - 匹配可能失败（对手方撤单）                                    │
   │  - 比传统 DEX 更复杂                                            │
   │                                                                  │
   │  缓解:                                                           │
   │  - 前端封装复杂性                                               │
   │  - 默认启用预签名                                               │
   │  - 清晰的状态反馈                                               │
   └─────────────────────────────────────────────────────────────────┘

4. Matcher 中心化风险
   ┌─────────────────────────────────────────────────────────────────┐
   │  问题: 如果只有少数 Matcher，可能导致:                           │
   │  - 审查风险（Matcher 拒绝某些订单）                              │
   │  - 抢跑风险（Matcher 优先匹配自己）                              │
   │                                                                  │
   │  缓解:                                                           │
   │  - 开源 Matcher，任何人可运行                                   │
   │  - 用户可以自己运行 Matcher                                     │
   │  - 链上结算确保最终公平                                         │
   │  - 声誉系统                                                      │
   └─────────────────────────────────────────────────────────────────┘
```

### 8.3 与 V4 其他变体的定位

```
┌─────────────────────────────────────────────────────────────────────────┐
│                    V4 架构变体定位                                        │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│  V4.0 三阶段 (原始)                                                      │
│  └── 定位: 平衡方案                                                      │
│      └── 适合: 大多数场景                                                │
│                                                                          │
│  V4.1 Shared Everything                                                  │
│  └── 定位: 最简单实现                                                    │
│      └── 适合: MVP、低吞吐场景                                           │
│                                                                          │
│  V4.2 Shared Nothing                                                     │
│  └── 定位: 最高性能                                                      │
│      └── 适合: 高频交易、专业交易者                                      │
│                                                                          │
│  选择建议:                                                               │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │  简单优先 → V4.1 Shared Everything                              │   │
│  │  平衡性能 → V4.0 三阶段 或 混合架构                             │   │
│  │  极致性能 → V4.2 Shared Nothing                                 │   │
│  └─────────────────────────────────────────────────────────────────┘   │
│                                                                          │
└─────────────────────────────────────────────────────────────────────────┘
```

---

## 9. 结论

### 9.1 Shared Nothing 的核心优势

1. **最高吞吐量**：无共识瓶颈，链上交易完全并发
2. **最低延迟**：链下撮合 ~10ms，端到端 ~600ms
3. **无限可扩展**：Matcher 水平扩展，链上并行处理
4. **无需信任 Matcher**：链上签名验证保证安全

### 9.2 适用建议

| 场景 | 推荐方案 | 理由 |
|-----|---------|------|
| **MVP/原型** | Shared Everything | 最简单 |
| **通用 DEX** | V4 三阶段/混合 | 平衡性能与复杂度 |
| **高频交易** | **Shared Nothing** | 最低延迟、最高吞吐 |
| **专业机构** | **Shared Nothing** | 可定制 Matcher |
| **跨链交易** | **Shared Nothing** | 原子交换友好 |

### 9.3 实施建议

1. **如果追求极致性能**：采用 Shared Nothing
2. **如果需要快速上线**：从 Shared Everything 开始
3. **如果需要平衡**：采用 V4 三阶段或混合架构
4. **可以混合使用**：普通用户用共识方案，专业用户用 Shared Nothing

---

## 附录

### A. 签名消息格式

```rust
/// 结算消息（用于签名）
#[derive(Serialize)]
struct SettlementMessage {
    /// 消息类型标识
    message_type: &'static str,  // "DEX_SETTLEMENT_V1"

    /// 链 ID（防跨链重放）
    chain_id: String,

    /// Maker 信息
    maker_account: ObjectID,
    maker_order_id: u64,

    /// Taker 信息
    taker_account: ObjectID,
    taker_order_id: u64,

    /// 交易参数
    pair: TradingPair,
    price: u64,
    quantity: u64,

    /// 时间戳
    timestamp: u64,

    /// Nonce（防重放）
    nonce: u64,
}
```

### B. 事件定义

```move
/// 订单意向事件
struct OrderIntentEvent has copy, drop {
    owner: address,
    account_id: ID,
    order_id: u64,
    pair: TradingPair,
    side: u8,
    order_type: u8,
    price: u64,
    quantity: u64,
    expires_at: u64,
}

/// 订单取消事件
struct OrderCancelledEvent has copy, drop {
    owner: address,
    account_id: ID,
    order_id: u64,
}

/// 成交事件
struct TradeExecutedEvent has copy, drop {
    maker_account: ID,
    taker_account: ID,
    pair: TradingPair,
    price: u64,
    quantity: u64,
    timestamp: u64,
}
```

### C. 安全考虑

```
1. 签名安全
   - 使用 Ed25519 或 Secp256k1
   - 包含 chain_id 防跨链重放
   - 包含 nonce 防交易重放
   - 包含 timestamp 限制有效期

2. 订单验证
   - 结算时验证订单存在
   - 验证订单未取消
   - 验证剩余数量足够
   - 验证价格范围

3. Matcher 安全
   - Matcher 无法伪造签名
   - Matcher 只能提交有效匹配
   - 用户可以选择 Matcher
   - 开源 Matcher 代码

4. 抢跑防护
   - 预签名包含价格范围
   - 短有效期限制
   - 声誉惩罚机制
```

---

**文档状态**: ✅ 完成
**版本**: v4.2 (Shared Nothing)
**最后更新**: 2025-12-24
