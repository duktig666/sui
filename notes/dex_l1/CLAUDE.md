# DEX L1 编码宪法 / DEX L1 Coding Constitution

## 核心禁令 / Absolute Prohibitions

1. **禁止 `unwrap()` / `expect()`** - 生产代码必须使用 `?` 或显式错误处理
2. **禁止重复造轮子** - 必须优先复用 Sui 组件（见下方列表）
3. **禁止关键路径同步 I/O** - 使用异步 + 批量写入
4. **禁止关键路径运行时分配** - 使用预分配或对象池
5. **禁止 `Arc<RwLock<HashMap>>>`** - 使用 `DashMap`
6. **禁止日志敏感数据** - 私钥、余额明细等

## 必须复用 / Must Reuse

| 需求 | 使用 | 禁止自实现 |
|-----|------|-----------|
| KV 存储 | `typed-store` | RocksDB 封装 |
| P2P 网络 | `mysten-network` | 网络层 |
| 签名验证 | `shared-crypto` | 签名逻辑 |
| 指标采集 | `mysten-metrics` | Prometheus 封装 |
| 序列化 | `bcs` | 二进制格式 |
| 并发 Map | `dashmap` | 锁 + HashMap |

## 性能红线 / Performance Boundaries

- 端到端延迟: **< 50ms**
- 单次撮合: **< 10μs**
- 目标 TPS: **200,000**

## 测试要求 / Testing Requirements

- 公开函数必须有测试
- 异步测试用 `#[tokio::test]`
- 提交前: `cargo nextest run -p dex-*`

## 提交前检查 / Pre-commit

```bash
cargo fmt && cargo clippy -p dex-* && cargo nextest run -p dex-*
```
