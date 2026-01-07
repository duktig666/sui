# sui-devnet-local 项目指导

## 项目定位

本地 Sui 开发测试网络搭建指南，**不涉及任何源码修改**。

## 适用场景

- 本地开发和测试 Move 合约
- 多验证者网络实验
- 运维流程验证
- CI/CD 集成测试环境

## 关键约束

- **零源码修改**：仅使用官方 Sui CLI 和配置
- 如需研究 Sui 源码级修改，请参考 [`triton-network`](../triton-network/) 项目

## 目录结构

| 目录/文件 | 说明 |
|----------|------|
| `README.md` | 完整操作手册 |
| `observability/` | Prometheus + Grafana 配置示例 |

## 常用命令

```bash
# 快速模式（每次新链）
sui start --committee-size 4 --with-faucet --force-regenesis

# 持久化模式
sui genesis --with-faucet --committee-size 4 --working-dir ./config
sui start --network.config ./config --with-faucet

# 健康检查
curl -X POST http://127.0.0.1:9000 -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"sui_getTotalTransactionBlocks","params":[]}'
```

## 相关项目

- [`triton-network`](../triton-network/) - 基于 Sui 源码二次开发的定制链研究
