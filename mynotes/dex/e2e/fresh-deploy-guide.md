# DEX 本地节点重新部署指南

本文档描述从零开始部署本地 Sui 节点和 DEX Indexer 的完整流程。

## psql
```sh
# 清空表                                                                                      
sudo docker exec dex-indexer-db psql -U dex -d dex_indexer -c "TRUNCATE TABLE dex_balances, dex_fills, dex_watermarks, watermarks CASCADE;"           
                                                                                            
# 交互式连接                                                                                  
sudo docker exec -it dex-indexer-db psql -U dex -d dex_indexer                                
                                                                                            
# 查询示例                                                                                    
sudo docker exec dex-indexer-db psql -U dex -d dex_indexer -c "SELECT * FROM watermarks;"   
```

## 前置条件

- PostgreSQL 已启动：`docker-compose up -d`
- 当前工作目录：`/home/rsw/code/dex/sui`

## 一、清理并重新编译

```sh
# 1. 编译 sui 二进制和相关组件
cargo build -p sui -p dex-indexer -p dex-node-test
cargo build --bin sui

# 2. 清理旧的网络配置和数据
sudo rm -rf local-network-config/**
rm -rf ~/.sui/sui_config/network.yaml
rm -rf ~/.sui/sui_config/authorities_db/
rm -rf ~/.sui/sui_config/consensus_db/
```

## 二、初始化网络配置

```sh
export SUI_CHAIN_DIR="$PWD/local-network-config"
sudo ./target/debug/sui genesis \
  --with-faucet \
  --committee-size 1 \
  --working-dir "$SUI_CHAIN_DIR" \
  --force \
  --epoch-duration-ms 30000
```

## 三、清理 Indexer 数据库

PostgreSQL 运行在 Docker 容器 `dex-indexer-db` 中，使用 `docker exec` 执行 psql 命令：

```sh
# 清空所有表
sudo docker exec dex-indexer-db psql -U dex -d dex_indexer -c "TRUNCATE TABLE dex_balances, dex_fills, dex_watermarks, watermarks CASCADE;"
```

**交互式连接数据库：**

```sh
sudo docker exec -it dex-indexer-db psql -U dex -d dex_indexer
```

## 四、启动服务

需要 4 个终端分别启动不同服务：

### 终端 1：Sui 节点

```sh
cd /home/rsw/code/dex/sui
export SUI_CHAIN_DIR="$PWD/local-network-config"
sudo RUST_LOG="off,sui_node=info" \
  ./target/debug/sui start \
    --network.config "$SUI_CHAIN_DIR" \
    --with-faucet=0.0.0.0:9123 \
    --fullnode-rpc-port 9001
```

**快速启动模式（临时节点，自动重置）：**

```sh
RUST_LOG="off,sui_node=info" \
  ./target/debug/sui start --with-faucet --force-regenesis --fullnode-rpc-port 9000
```

### 终端 2：DEX Indexer

等待 Sui 节点启动完成后执行：

```sh
cd /home/rsw/code/dex/sui
./target/debug/dex-indexer \
  --database-url postgres://dex:dex123@localhost:5432/dex_indexer \
  --rpc-api-url http://127.0.0.1:9001 \
  --first-checkpoint 0
```

### 终端 3：DEX API

```sh
cd /home/rsw/code/dex/sui
./target/debug/dex-api \
  --database-url postgres://dex:dex123@localhost:5432/dex_indexer \
  --api-listen-address 0.0.0.0:3000
```

### 终端 4：运行测试

```sh
cd /home/rsw/code/dex/sui

# 测试下单流程
cargo run -p dex-node-test --example place_order -- --fullnode-url http://127.0.0.1:9001

# 测试成交验证
cargo run -p dex-node-test --example fill_and_verify -- --fullnode-url http://127.0.0.1:9001
```

## 五、验证服务状态

### 查询 API 健康状态

```sh
curl -s http://127.0.0.1:3000/info -X POST \
  -H "Content-Type: application/json" \
  -d '{"type": "userBalances", "subaccount":"<SUBACCOUNT_ID>", "limit": 10}' | jq
```

### 检查 Indexer 同步进度

```sh
sudo docker exec dex-indexer-db psql -U dex -d dex_indexer -c "SELECT * FROM watermarks;"
```

### 检查已索引的余额

```sh
sudo docker exec dex-indexer-db psql -U dex -d dex_indexer -c "SELECT * FROM dex_balances;"
```

## 六、一键重启脚本

将以下内容保存为 `scripts/fresh-deploy.sh`：

```sh
#!/bin/bash
set -e

cd /home/rsw/code/dex/sui

echo "=== 清理旧数据 ==="
sudo rm -rf local-network-config/**
rm -rf ~/.sui/sui_config/network.yaml
rm -rf ~/.sui/sui_config/authorities_db/
rm -rf ~/.sui/sui_config/consensus_db/

echo "=== 清理数据库 ==="
sudo docker exec dex-indexer-db psql -U dex -d dex_indexer -c \
  "TRUNCATE TABLE dex_balances, dex_fills, dex_balance_events, watermarks CASCADE;" 2>/dev/null || true

echo "=== 初始化网络配置 ==="
export SUI_CHAIN_DIR="$PWD/local-network-config"
sudo ./target/debug/sui genesis \
  --with-faucet \
  --committee-size 1 \
  --working-dir "$SUI_CHAIN_DIR" \
  --force \
  --epoch-duration-ms 30000

echo "=== 完成 ==="
echo "请在各终端手动启动服务（见文档第四节）"
```

## 注意事项

1. **启动顺序**：Sui 节点 → DEX Indexer → DEX API → 运行测试
2. **等待时间**：每个服务启动后等待几秒确认稳定再启动下一个
3. **端口冲突**：确保 9001、9123、3000、5432 端口未被占用
4. **权限问题**：部分命令需要 sudo 权限

---

## 七、运行事件验证示例

dex-node-test 提供多个示例程序，用于验证不同类型的 DEX 事件：

```sh
cd /home/rsw/code/dex/sui

# 下单和成交验证（FillEvent + BalanceUpdateEvent）
cargo run -p dex-node-test --example fill_and_verify -- --fullnode-url http://127.0.0.1:9001

# 市场创建事件验证（PerpetualCreatedEvent）
cargo run -p dex-node-test --example perpetual_and_verify -- --fullnode-url http://127.0.0.1:9001

# 持仓变化事件验证（PositionUpdateEvent）
cargo run -p dex-node-test --example position_and_verify -- --fullnode-url http://127.0.0.1:9001
```

## 八、事件索引验证

运行示例后，使用以下命令验证事件是否被正确索引到数据库：

### 验证 Indexer 处理进度

```sh
sudo docker exec dex-indexer-db psql -U dex -d dex_indexer -c "SELECT * FROM watermarks;"
```

### 验证各事件表数据

```sh
# 成交记录
sudo docker exec dex-indexer-db psql -U dex -d dex_indexer -c "SELECT * FROM dex_fills ORDER BY timestamp_ms DESC LIMIT 5;"

# 余额变化
sudo docker exec dex-indexer-db psql -U dex -d dex_indexer -c "SELECT * FROM dex_balances ORDER BY timestamp_ms DESC LIMIT 5;"

# 持仓状态（当前快照）
sudo docker exec dex-indexer-db psql -U dex -d dex_indexer -c "SELECT * FROM dex_positions;"

# 持仓变化历史
sudo docker exec dex-indexer-db psql -U dex -d dex_indexer -c "SELECT * FROM dex_position_updates ORDER BY timestamp_ms DESC LIMIT 5;"

# 永续市场
sudo docker exec dex-indexer-db psql -U dex -d dex_indexer -c "SELECT * FROM dex_perpetuals;"
```

## 九、API 查询验证

通过 DEX API 验证索引数据可被正确查询。

> **注意**：`<SUBACCOUNT_ID>` 需要替换为示例程序输出中的实际子账户 ID（如 `0x1234...`）

### perpetual_and_verify 验证

```sh
# 查询市场元数据（验证 PerpetualCreatedEvent）
curl -s http://127.0.0.1:3000/info -X POST \
  -H "Content-Type: application/json" \
  -d '{"type": "meta"}' | jq
```

### position_and_verify / fill_and_verify 验证

```sh
# 查询最近成交（验证 FillEvent）
curl -s http://127.0.0.1:3000/info -X POST \
  -H "Content-Type: application/json" \
  -d '{"type": "recentFills", "perpetualId": 0, "limit": 10}' | jq

# 查询用户持仓和保证金（验证 PositionUpdateEvent）
curl -s http://127.0.0.1:3000/info -X POST \
  -H "Content-Type: application/json" \
  -d '{"type": "clearinghouseState", "subaccount": "<SUBACCOUNT_ID>"}' | jq

# 查询用户成交历史
curl -s http://127.0.0.1:3000/info -X POST \
  -H "Content-Type: application/json" \
  -d '{"type": "userFills", "subaccount": "<SUBACCOUNT_ID>", "limit": 10}' | jq

# 查询用户余额变化（验证 BalanceUpdateEvent）
curl -s http://127.0.0.1:3000/info -X POST \
  -H "Content-Type: application/json" \
  -d '{"type": "userBalances", "subaccount": "<SUBACCOUNT_ID>", "limit": 10}' | jq
```

### API 支持的查询类型

| 类型 | 说明 | 必需参数 |
|------|------|----------|
| `meta` | 市场元数据 | 无 |
| `recentFills` | 市场最近成交 | `perpetualId` |
| `clearinghouseState` | 用户持仓和保证金 | `subaccount` |
| `userFills` | 用户成交历史 | `subaccount` |
| `userBalances` | 用户余额变化 | `subaccount` |
| `userTransfers` | 用户转账历史 | `subaccount` |

## 十、事件对应关系

| 操作 | 触发的事件 | 索引表 |
|------|-----------|--------|
| 存款 (deposit) | BalanceUpdateEvent | dex_balances |
| 取款 (withdraw) | BalanceUpdateEvent | dex_balances |
| 创建市场 | PerpetualCreatedEvent | dex_perpetuals |
| 订单成交 | FillEvent + PositionUpdateEvent + BalanceUpdateEvent | dex_fills, dex_positions, dex_balances |

## 十一、常见问题排查

### 事件未被索引

1. 检查 Indexer 是否正常运行
2. 查看 watermarks 表确认处理进度
3. 检查 Indexer 日志是否有错误

### 数据库查询无数据

1. 确认示例程序执行成功
2. 等待几秒让 Indexer 处理新的 checkpoint
3. 检查 Indexer 是否连接到正确的 RPC 端点

### API 返回空结果

1. 确认 subaccount ID 正确
2. 检查 API 是否连接到正确的数据库
3. 验证数据库中确实有数据