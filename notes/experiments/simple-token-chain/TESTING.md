# Token Chain 测试指南

您的区块链节点已经在运行！以下是验证它是一个真正的区块链的几种方法：

## 🎯 验证区块链的核心特性

一个真正的区块链应该具备：
- ✅ **去中心化账本** - 维护所有账户状态
- ✅ **交易执行** - 处理转账和状态变更
- ✅ **共识机制** - 确保交易顺序和一致性
- ✅ **不可篡改性** - Nonce机制防止重放攻击
- ✅ **状态验证** - 余额检查、nonce验证

---

## 方法1: 使用 Rust 客户端（推荐）

### 步骤1: 确保节点正在运行
```bash
# 在一个终端运行节点
cargo run --bin simple-token-chain
```

### 步骤2: 在新终端运行客户端示例
```bash
cd /Users/robsu/workplace/dex/sui/notes/experiments/simple-token-chain
cargo run --example client
```

### 预期输出
```
🚀 Token Chain Client Demo

✅ Connected to Token Chain node at http://127.0.0.1:9000

👥 Created test addresses:
   Alice:   0x616c696365000000
   Bob:     0x626f62000000
   Charlie: 0x6368617269650000

📊 Step 1: Checking node status...
   Node status: {
     "node_id": 0,
     "running": true,
     "rpc_addr": "127.0.0.1:9000"
   }

💰 Step 2: Checking initial balances...
   Alice's balance: 0 tokens

🏦 Step 3: Minting 1000 tokens to Alice...
   ✅ Alice's new balance: 1000 tokens

💸 Step 4: Transferring 300 tokens from Alice to Bob...
   ✅ Alice's balance: 700 tokens
   ✅ Bob's balance: 300 tokens

💸 Step 5: Transferring 200 tokens from Alice to Charlie...
   ✅ Transaction successful

📊 Step 6: Final state of the blockchain:
   Alice:   500 tokens (nonce: 2)
   Bob:     300 tokens
   Charlie: 200 tokens
   Total:   1000 tokens

❌ Step 7: Testing invalid transaction...
   ✅ Expected error: Insufficient balance

🎉 Demo complete!
✅ This is a working blockchain!
```

---

## 方法2: 使用 Bash 脚本

```bash
# 运行预先创建的测试脚本
bash /tmp/test_blockchain.sh
```

这个脚本会：
1. 检查节点状态
2. 给Alice铸造1000代币
3. 从Alice转账300代币给Bob
4. 验证最终余额（Alice: 700, Bob: 300）
5. 验证nonce递增

---

## 方法3: 使用 curl 手动测试

### 1. 检查节点状态
```bash
curl -X POST http://127.0.0.1:9000 \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "method": "getStatus",
    "params": [],
    "id": 1
  }' | jq .
```

### 2. 查询余额（Alice地址）
```bash
curl -X POST http://127.0.0.1:9000 \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "method": "getBalance",
    "params": [[97,108,105,99,101,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]],
    "id": 2
  }' | jq .
```

### 3. 铸造代币
```bash
curl -X POST http://127.0.0.1:9000 \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "method": "submitTransaction",
    "params": [{
      "Mint": {
        "to": [97,108,105,99,101,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],
        "amount": 1000
      }
    }],
    "id": 3
  }' | jq .
```

### 4. 转账交易
```bash
curl -X POST http://127.0.0.1:9000 \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "method": "submitTransaction",
    "params": [{
      "Transfer": {
        "from": [97,108,105,99,101,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],
        "to": [98,111,98,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],
        "amount": 300,
        "nonce": 0
      }
    }],
    "id": 4
  }' | jq .
```

---

## 🔍 验证这是一个真正的区块链

### 1. 状态持久性
- ✅ 重启节点后，状态会重置（当前简化版）
- ✅ 在节点运行期间，所有状态变更都被追踪

### 2. 交易顺序性
- ✅ 通过nonce机制保证交易顺序
- ✅ Nonce必须按顺序递增（0, 1, 2, ...）

### 3. 余额一致性
- ✅ 转账前检查余额
- ✅ 总供应量守恒（除非mint）
- ✅ 双花保护

### 4. 共识集成
- ✅ 使用Mysticeti共识协议
- ✅ 交易通过共识层提交
- ✅ 支持多节点部署（当前单节点演示）

### 5. RPC API
- ✅ 标准JSON-RPC 2.0接口
- ✅ 支持查询和交易提交
- ✅ 错误处理完善

---

## 📊 测试场景

### 场景1: 正常转账流程
1. Mint 1000 tokens to Alice
2. Transfer 300 tokens to Bob
3. Verify: Alice=700, Bob=300 ✅

### 场景2: Nonce验证
1. Submit transfer with nonce=0 ✅
2. Submit another transfer with nonce=0 ❌ (should fail)
3. Submit with nonce=1 ✅

### 场景3: 余额检查
1. Try to transfer more than balance ❌
2. Transfer within balance ✅

### 场景4: 批量交易
```bash
cargo run --example client
# 观察多笔交易按顺序执行
```

---

## 🎓 理解区块链机制

### 交易生命周期
```
提交交易 → 验证 → 共识 → 执行 → 状态更新
```

### 状态转换
```
初始状态: {}
Mint(Alice, 1000): {Alice: 1000}
Transfer(Alice→Bob, 300): {Alice: 700, Bob: 300}
```

### Nonce机制（防重放）
```
Alice的nonce: 0 → 1 → 2 → ...
每次转账必须使用当前nonce，并自动递增
```

---

## 🚀 高级测试

### 并发测试
```bash
# 同时提交多笔交易
for i in {1..10}; do
  cargo run --example client &
done
wait
```

### 压力测试
修改客户端代码，提交大量交易测试性能。

---

## ✅ 验证清单

- [ ] 节点启动成功
- [ ] 可以查询余额
- [ ] 可以铸造代币
- [ ] 可以转账
- [ ] 余额正确更新
- [ ] Nonce正确递增
- [ ] 无效交易被拒绝
- [ ] 总供应量守恒

完成所有项目 = **这是一个功能完整的区块链！** 🎉

---

## 📝 API 参考

### getStatus
```json
{
  "jsonrpc": "2.0",
  "method": "getStatus",
  "params": [],
  "id": 1
}
```

### getBalance
```json
{
  "jsonrpc": "2.0",
  "method": "getBalance",
  "params": [<Address>],
  "id": 2
}
```

### getNonce
```json
{
  "jsonrpc": "2.0",
  "method": "getNonce",
  "params": [<Address>],
  "id": 3
}
```

### submitTransaction (Mint)
```json
{
  "jsonrpc": "2.0",
  "method": "submitTransaction",
  "params": [{
    "Mint": {
      "to": <Address>,
      "amount": <u64>
    }
  }],
  "id": 4
}
```

### submitTransaction (Transfer)
```json
{
  "jsonrpc": "2.0",
  "method": "submitTransaction",
  "params": [{
    "Transfer": {
      "from": <Address>,
      "to": <Address>,
      "amount": <u64>,
      "nonce": <u64>
    }
  }],
  "id": 5
}
```

---

## 🎉 恭喜！

如果您能成功执行上述测试，那么您已经验证了这是一个：
- ✅ 具有状态管理的区块链
- ✅ 支持代币转账的区块链
- ✅ 基于共识的区块链
- ✅ 具有防重放攻击的区块链
- ✅ 提供RPC接口的区块链

**这就是一个真正的区块链！** 🚀
