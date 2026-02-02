# Phase 9: 真实节点测试执行计划

> 创建日期: 2026-01-30
> 目标: 使用 dex-node-test 向真实节点发送交易，验证完整数据流

---

## 1. 测试架构

```
┌──────────────────────────────────────────────────────────────────────────┐
│                    真实节点 E2E 验证架构                                 │
├──────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│  终端 1: Sui 节点 (Port 9001)                                            │
│  ┌────────────────────────────────────────────────────────────────────┐  │
│  │  sui start --network.config ... --fullnode-rpc-port 9001           │  │
│  └────────────────────────────────────────────────────────────────────┘  │
│                       │                                                  │
│                       v                                                  │
│  终端 4: dex-node-test (交易发送)                                        │
│  ┌────────────────────────────────────────────────────────────────────┐  │
│  │  cargo run -p dex-node-test --example fill_and_verify              │  │
│  │  → 创建子账户、存款、下单、撮合                                    │  │
│  │  → 验证 FillEvent 和 BalanceUpdateEvent                            │  │
│  └────────────────────────────────────────────────────────────────────┘  │
│                       │                                                  │
│                       v                                                  │
│  终端 2: dex-indexer (Checkpoint 处理)                                   │
│  ┌────────────────────────────────────────────────────────────────────┐  │
│  │  ./target/debug/dex-indexer --rpc-api-url http://127.0.0.1:9001    │  │
│  │  → 解析 FillEvent/BalanceUpdateEvent                               │  │
│  │  → 写入 PostgreSQL                                                 │  │
│  └────────────────────────────────────────────────────────────────────┘  │
│                       │                                                  │
│                       v                                                  │
│  终端 3: dex-api (REST API)                                              │
│  ┌────────────────────────────────────────────────────────────────────┐  │
│  │  ./target/debug/dex-api --api-listen-address 0.0.0.0:3000          │  │
│  │  → /health, /info (userFills, userBalances, recentFills)           │  │
│  └────────────────────────────────────────────────────────────────────┘  │
│                                                                          │
└──────────────────────────────────────────────────────────────────────────┘
```

---

## 2. 环境准备

### 2.1 编译

```bash
cd /home/rsw/code/dex/dex-sui
cargo build -p sui -p dex-indexer -p dex-node-test
```

编译产物:
- `./target/debug/sui`
- `./target/debug/dex-indexer`
- `./target/debug/dex-api`

### 2.2 PostgreSQL

```bash
cd /home/rsw/code/dex/dex-sui/crates/dex-indexer
docker-compose up -d

# 验证连接
psql postgres://dex:dex123@localhost:5432/dex_indexer -c "SELECT 1"
```

---

## 3. 启动服务

### 3.1 终端 1: Sui 节点

**方式 A: 持久化配置（推荐）**
```bash
cd /home/rsw/code/dex/dex-sui

# 初始化（首次执行）
export SUI_CHAIN_DIR="$PWD/local-network-config"
./target/debug/sui genesis \
  --with-faucet \
  --committee-size 1 \
  --working-dir "$SUI_CHAIN_DIR" \
  --force \
  --epoch-duration-ms 30000

# 启动节点
export SUI_CHAIN_DIR="$PWD/local-network-config"
RUST_LOG="off,sui_node=info,sui_execution=debug" \
  ./target/debug/sui start \
    --network.config "$SUI_CHAIN_DIR" \
    --with-faucet=0.0.0.0:9123 \
    --fullnode-rpc-port 9001
```

**方式 B: 临时配置**
```bash
RUST_LOG="off,sui_node=info,sui_execution=debug" \
  ./target/debug/sui start \
    --with-faucet \
    --force-regenesis \
    --fullnode-rpc-port 9001
```

### 3.2 终端 2: dex-indexer

```bash
cd /home/rsw/code/dex/dex-sui

./target/debug/dex-indexer \
  --database-url postgres://dex:dex123@localhost:5432/dex_indexer \
  --rpc-api-url http://127.0.0.1:9001 \
  --first-checkpoint 0
```

### 3.3 终端 3: dex-api

```bash
cd /home/rsw/code/dex/dex-sui

./target/debug/dex-api \
  --database-url postgres://dex:dex123@localhost:5432/dex_indexer \
  --api-listen-address 0.0.0.0:3000
```

### 3.4 终端 4: dex-node-test

```bash
cd /home/rsw/code/dex/dex-sui

# 运行下单示例
cargo run -p dex-node-test --example place_order -- \
  --fullnode-url http://127.0.0.1:9001

# 运行撮合验证示例（Phase 9 需要实现）
cargo run -p dex-node-test --example fill_and_verify -- \
  --fullnode-url http://127.0.0.1:9001
```

---

## 4. 验证步骤

### 4.1 验证节点运行

```bash
curl http://127.0.0.1:9001 -X POST \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"sui_getLatestCheckpointSequenceNumber"}'
```

### 4.2 验证 API 服务

```bash
# 健康检查
curl http://localhost:3000/health
# 预期: "OK"
```

### 4.3 执行交易并验证数据

```bash
# 1. 运行 place_order 示例
cargo run -p dex-node-test --example place_order -- \
  --fullnode-url http://127.0.0.1:9001

# 2. 等待 indexer 处理（约 5-10 秒）

# 3. 查询余额变动
curl -X POST http://localhost:3000/info \
  -H "Content-Type: application/json" \
  -d '{"type": "userBalances", "subaccount": "0x<SUBACCOUNT_ID>", "limit": 10}'

# 4. 查询最近成交（需要撮合才有数据）
curl -X POST http://localhost:3000/info \
  -H "Content-Type: application/json" \
  -d '{"type": "recentFills", "perpetualId": 0, "limit": 10}'
```

---

## 5. 需要实现的代码

### 5.1 DexClient 扩展

**文件**: `crates/dex-node-test/src/client.rs`

添加方法以获取包含事件的完整响应:
```rust
pub async fn place_limit_order_with_response(...) -> Result<SuiTransactionBlockResponse>
```

### 5.2 撮合验证示例

**文件**: `crates/dex-node-test/examples/fill_and_verify.rs`

功能:
1. 创建 buyer 和 seller 两个客户端
2. 各自创建子账户并存款
3. 创建永续市场
4. buyer 下买单，seller 下卖单（价格相同触发撮合）
5. 验证交易响应中的 FillEvent

---

## 6. 验证清单

| 步骤 | 验证项 | 预期结果 | 状态 |
|------|--------|----------|------|
| 2.2 | PostgreSQL 启动 | 容器运行正常 | [ ] |
| 3.1 | Sui 节点启动 | RPC 端口 9001 可访问 | [ ] |
| 3.2 | dex-indexer 启动 | 开始处理 checkpoint | [ ] |
| 3.3 | dex-api 启动 | 端口 3000 可访问 | [ ] |
| 4.3 | place_order 执行 | 子账户创建、存款、下单成功 | [ ] |
| 4.3 | BalanceUpdateEvent | 存款事件被索引 | [ ] |
| 4.3 | userBalances 查询 | API 返回余额变动 | [ ] |
| 5.2 | fill_and_verify 执行 | 撮合成功 | [ ] |
| 5.2 | FillEvent | 撮合事件被索引 | [ ] |
| 5.2 | recentFills 查询 | API 返回成交记录 | [ ] |

---

## 7. 问题排查

| 问题 | 可能原因 | 解决方案 |
|------|----------|----------|
| 节点无法启动 | 端口被占用 | 检查 9001/9123 端口 |
| Indexer 无法连接节点 | RPC URL 错误 | 确认 `--rpc-api-url` |
| Indexer 无法获取 checkpoint | 节点未生成 checkpoint | 等待节点初始化完成 |
| API 返回空数据 | Indexer 未处理完 | 等待更长时间 |
| FillEvent 未发射 | 订单未撮合 | 确认买卖价格匹配 |
| 版本号不匹配 | 对象版本已更新 | 使用 `version.next()` |

---

## 8. 输出文档

执行完成后更新:
- `sui/mynotes/dex/summary/phase-9-real-node-test.md` - 测试结果总结