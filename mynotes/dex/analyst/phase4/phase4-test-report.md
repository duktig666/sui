# Phase 4: Exchange Write API 测试报告

## 测试日期: 2026-02-11

## 测试结果: ✅ 通过

Exchange API 端到端流程验证成功：
- EIP-712 签名 → POST `/exchange` → Sui 交易执行成功 → 订单 resting

### 成功的测试流程

```
1. secp256k1 测试密钥 (seed=[1,0,...]) → Sui 地址 0x8d83...0fa4
2. 构建 EIP-712 PlaceOrder 参数 (perpetual_id=0, buy, qty=100, price=50000)
3. 签名 → {r, s, v}
4. POST /exchange → 200 OK, status: "ok"
5. 链上验证 → sui_getTransactionBlock → status: success
6. 订单状态: resting（挂单成功）
```

### Exchange API 响应示例

```json
{
  "status": "ok",
  "response": {
    "type": "order",
    "data": {
      "statuses": [
        {
          "resting": {
            "digest": "FQr2rtGrCczfwNDRM9Ygni9jmH7pa9Zqtho6VTQGgps4"
          }
        }
      ]
    }
  }
}
```

---

## 遇到的问题及解决方案

### 问题 1: DexCommand 枚举变体不匹配

**现象**: `Deserialization error: invalid value: integer 8, expected variant index 0 <= i < 8`

**原因**: `DexCommand::PlaceOrderWithEip712` 是第 9 个变体（index=8），但 Docker 镜像中的 `sui` 二进制是在添加该变体之前编译的（2月9日），只识别 0-7 共 8 个变体。

**排查过程**:
1. 最初怀疑是 `TransactionKind::ProgrammableDex`（variant 11）的问题
2. 通过 gateway SDK 模式成功下单（使用 `PlaceOrder` variant 5），排除了 TransactionKind 层面的问题
3. 确认问题出在 `DexCommand` 层级：`PlaceOrderWithEip712` 是新增的 variant 8

**解决**: 重新编译 `sui` 二进制 (`cargo build -p sui`)，确保包含最新的 `DexCommand` 定义。

**教训**: 添加新的 BCS 序列化枚举变体后，必须重新编译所有使用该类型的二进制（包括 Docker 镜像中的节点）。

---

### 问题 2: InvariantViolation — 测试用户无余额

**现象**: `Transaction execution failed: InvariantViolation in command 0`

**原因**: 使用哑签名 (r=1, s=1, v=27) 测试时，恢复出的地址 `0x25f13c...` 没有 subaccount/balance。后续用真实密钥签名时，恢复出地址 `0x8d83...0fa4` 也没有余额。

**解决**:
1. 给 gateway 的 `DepositRequest` 添加可选 `owner` 字段
2. 通过 `POST /tx/deposit` 指定 `owner` 为 EIP-712 用户地址来充值
3. 修改 `handle_deposit` 中的 `SubaccountId` 构建逻辑

```rust
// gateway.rs 修改
pub struct DepositRequest {
    pub subaccount_number: u32,
    pub amount: String,
    #[serde(default)]
    pub sender_index: u8,
    pub owner: Option<String>,  // 新增：指定 subaccount 所有者
}

// handle_deposit 中
let owner = if let Some(ref addr) = req.owner {
    addr.parse::<SuiAddress>()?
} else {
    entry.client.sender()
};
let subaccount_id = SubaccountId::new(owner, req.subaccount_number);
```

**教训**: EIP-712 模式下，订单所有者由签名恢复而非交易发送者。测试前需要确保该地址已有充足余额。

---

### 问题 3: dex_sui_objects 表数据不完整

**现象**:
- `global_accounts` 记录缺失
- `perpetual_state` 的 `initial_shared_version` 为 0（占位值）

**原因**:
- indexer 的 `perpetuals` handler 在处理 `PerpetualCreatedEvent` 时写入 `dex_sui_objects`，但 `initial_shared_version` 设为 0（占位值）
- 没有 handler 处理 `GlobalAccounts` 创建事件

**临时解决**: 手动 SQL 操作
```sql
-- 插入 global_accounts
INSERT INTO dex_sui_objects (object_type, type_id, object_id, initial_shared_version)
VALUES ('global_accounts', 0, '\x...', 3);

-- 修正 perpetual_state 版本号
UPDATE dex_sui_objects SET initial_shared_version = 3
WHERE object_type = 'perpetual_state' AND type_id = 0;
```

**根本解决方案（待实现）**:
1. indexer 在写入 `perpetual_state` 时，通过 checkpoint 数据获取正确的 `initial_shared_version`
2. 添加 `GlobalAccounts` 创建事件的处理，或在 API 启动参数中通过 `--global-accounts-id` 传入

---

### 问题 4: Gas Sponsor 无余额

**现象**: `No gas coins for sponsor address 0x73a6...adc0`

**原因**: 干净启动后 sponsor 地址没有 SUI。

**解决**: 通过 faucet 给 sponsor 地址充值
```bash
curl -X POST http://localhost:9123/gas \
  -H "Content-Type: application/json" \
  -d '{"FixedAmountRequest":{"recipient":"0x73a6b3c33e2d63383de5c6786cbaca231ff789f4c853af6d54cb883d8780adc0"}}'
```

**根本解决方案（待实现）**: dex-api 启动时自动检查 sponsor 余额，不足时报告明确错误信息。

---

### 问题 5: Sui 节点 Epoch 过渡导致交易超时

**现象**: `Failed to confirm tx status within 60 seconds`，faucet 返回 `Failed to execute transaction after 2 retries`

**原因**: Docker 重建时旧节点数据未清理，节点处于快速 epoch 过渡状态。

**解决**: 完全重置
```bash
docker compose down -v
rm -rf data/
docker compose up --build -d
# 等待 30 秒后再操作
```

---

### 问题 6: Object Cache 为空

**现象**: `Object cache refreshed cached_objects=0`，Exchange API 返回 `Object not found: global_accounts:0`

**原因**: dex-api 启动时从 DB 加载 object 缓存，但 DB 中还没有数据。

**解决**: 先确保 indexer 已处理到包含创建事件的 checkpoint，且 DB 有正确数据，然后重启 dex-api。

---

## 完整测试环境搭建步骤

```bash
# 1. 干净启动
cd dex-sui/docker/dex-dev
sudo docker compose down -v
sudo rm -rf data/
sudo docker compose up --build -d

# 2. 等待节点稳定
sleep 30

# 3. Faucet + Setup + Deposit (gateway 用户)
curl -X POST http://localhost:3200/tx/faucet -H "Content-Type: application/json" -d '{}'
curl -X POST http://localhost:3200/tx/setup -H "Content-Type: application/json" -d '{}'
curl -X POST http://localhost:3200/tx/deposit -H "Content-Type: application/json" \
  -d '{"subaccount_number": 0, "amount": "10000000000"}'

# 4. 查询 shared object 版本号
curl -s http://localhost:9001 -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"sui_getObject","params":["<GLOBAL_ACCOUNTS_ID>",{"showOwner":true}]}'

# 5. 填充 dex_sui_objects 表
docker exec dex-dev-db psql -U dex -d dex_indexer -c \
  "INSERT INTO dex_sui_objects (object_type, type_id, object_id, initial_shared_version) VALUES ('global_accounts', 0, '\\x...', 3);"
docker exec dex-dev-db psql -U dex -d dex_indexer -c \
  "UPDATE dex_sui_objects SET initial_shared_version = 3 WHERE object_type = 'perpetual_state';"

# 6. Faucet gas 给 sponsor
curl -X POST http://localhost:9123/gas -H "Content-Type: application/json" \
  -d '{"FixedAmountRequest":{"recipient":"0x73a6b3c33e2d63383de5c6786cbaca231ff789f4c853af6d54cb883d8780adc0"}}'

# 7. 重启 dex-api 刷新 cache
docker restart dex-dev-api

# 8. Deposit 给 EIP-712 测试用户
curl -X POST http://localhost:3200/tx/deposit -H "Content-Type: application/json" \
  -d '{"subaccount_number": 0, "amount": "10000000000", "owner": "0x8d83fad8bc8c02119c58ab81d6a1a4ee710ffb16d66723ebed171f3f92ef0fa4"}'

# 9. 运行 Exchange API 测试
cargo run -p dex-node-test --example exchange_api_test -- --api-url http://localhost:9100
```

---

## 待改进项

| 项目 | 优先级 | 说明 |
|------|--------|------|
| `initial_shared_version` 自动获取 | 高 | indexer 应从 checkpoint 数据获取，而非使用占位值 0 |
| `global_accounts` 自动写入 | 高 | 添加 handler 或 API 启动参数 |
| openOrders 查询适配 EIP-712 用户 | 中 | 当前按 sender 查询，EIP-712 订单 owner 不同 |
| Sponsor 余额自动检查 | 低 | API 启动时检查并报告 |
| 测试环境一键初始化脚本 | 低 | 自动化上述步骤 4-8 |
