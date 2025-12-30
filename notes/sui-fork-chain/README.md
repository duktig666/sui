# Sui 私链 / Fork Chain 指导手册（本机多验证者 + 最小观测）

本文把“研究笔记”升级为**可复制执行的操作手册**：在一台机器上启动一条“自己的 Sui 链”（4 个 validator + 1 个 fullnode + faucet），并用 **日志 / RPC / Prometheus** 观测其运行。

> 说明：这不是从主网/测试网状态“真分叉”（state fork），而是用自定义 genesis 启一条独立链（最适合做私链/开发链/验证运维流程）。

## 你将得到什么

- **Fullnode JSON-RPC**：`http://127.0.0.1:9000`（默认，可通过 `--fullnode-rpc-port` 修改）
- **Faucet**：`http://0.0.0.0:9123`（默认，可通过 `--with-faucet=<FAUCET_HOST_PORT>` 修改，例如 `--with-faucet=0.0.0.0:9123`）
- **Prometheus metrics**：每个 `sui-node` 都会启动 `http://<metrics_address>/metrics`（端口通常是自动分配的，见后文如何查找）

## 0. 前置条件

### 0.1 构建或安装 `sui` CLI

先确认你能运行 `sui`：

```bash
sui --version
```

若未安装，可在本仓库根目录构建（推荐 debug 便于排错）：

```bash
cargo build -p sui
./target/debug/sui --version
```

或安装到 PATH（更像“系统安装”）：

```bash
cargo install --path crates/sui --bin sui
sui --version
```

### 0.2（可选）日志级别

在 bash/WSL：

```bash
export RUST_LOG="off,sui_node=info"
```

在 PowerShell：

```powershell
$env:RUST_LOG="off,sui_node=info"
```

## 1. 启动方式 A：快速模式（每次新链，不持久化）

适合第一次跑通流程、验证端口/依赖是否齐全。

### 1.1 启动（4 验证者 + faucet）

```bash
# 可选：把临时目录固定到当前目录，方便找到运行期生成的数据/配置
mkdir -p ./tmp
TMPDIR="$PWD/tmp" RUST_LOG="off,sui_node=info" \
  sui start --committee-size 4 --with-faucet --force-regenesis
```

关键行为说明：

- `--force-regenesis`：每次启动都会生成新 genesis，不保留历史（停止后数据视为一次性）。  
- `--committee-size 4`：在 `--force-regenesis` 模式下生效，会起 4 个 validator。  
- **互斥**：`--force-regenesis` 与 `--network.config` 不能同时使用（这是 CLI 硬性检查）。  

### 1.2 你应该在输出里看到什么

- `Fullnode RPC URL: http://127.0.0.1:9000`
- 对每个节点，都会出现类似日志（用于找 metrics 端口）：
  - `Started Prometheus HTTP endpoint at 127.0.0.1:<port>`

## 2. 启动方式 B：持久化模式（推荐：作为“自己的链”）

适合做长期运行的本机私链：可反复重启、保留历史（除非你主动重置）。

### 2.1 生成 genesis + 网络配置

```bash
export SUI_CHAIN_DIR="$PWD/fork-chain-config"
sui genesis --with-faucet --committee-size 4 --working-dir "$SUI_CHAIN_DIR" --force
ls -la "$SUI_CHAIN_DIR"
```

你会在该目录看到（关键文件名在仓库里是固定的）：

- `genesis.blob`
- `network.yaml`
- `fullnode.yaml`
- `client.yaml`
- 若干 validator 配置（文件名通常带端口，例如 `127.0.0.1-<port>.yaml`）

### 2.2 启动（可重启保留状态）

```bash
RUST_LOG="off,sui_node=info" sui start --network.config "$SUI_CHAIN_DIR" --with-faucet
```

重置整条链（从头再来）：

- 停止进程后删除 `$SUI_CHAIN_DIR`，再重新执行 `sui genesis ... --force`。

## 3. 连接钱包并验证链真的在跑

### 3.1 连接到本地 RPC

```bash
sui client new-env --alias local --rpc http://127.0.0.1:9000
sui client switch --env local
sui client active-env
```

### 3.2 领取测试币并查看 Gas

```bash
sui client active-address
sui client chain-identifier
sui client faucet
sui client gas
```

如果 `faucet` 失败，优先检查：

- 你当前 `active-env` 是否指向本地 RPC；  
- 启动命令里是否包含 `--with-faucet`；  
- 端口是否冲突（默认 faucet `9123`）。  

## 4. 最小观测（不引入 Indexer/Postgres）

本节只依赖：**日志 + JSON-RPC + Prometheus /metrics**。

### 4.1 JSON-RPC 健康检查（curl）

总交易数（示例来自 OpenRPC 规范）：

```bash
curl --location --request POST 'http://127.0.0.1:9000' \
  --header 'Content-Type: application/json' \
  --data-raw '{
    "jsonrpc": "2.0",
    "id": 1,
    "method": "sui_getTotalTransactionBlocks",
    "params": []
  }'
```

最新 checkpoint 序号：

```bash
curl --location --request POST 'http://127.0.0.1:9000' \
  --header 'Content-Type: application/json' \
  --data-raw '{
    "jsonrpc": "2.0",
    "id": 1,
    "method": "sui_getLatestCheckpointSequenceNumber",
    "params": []
  }'
```

系统状态（可看到 epoch、validator 等信息）：

```bash
curl --location --request POST 'http://127.0.0.1:9000' \
  --header 'Content-Type: application/json' \
  --data-raw '{
    "jsonrpc": "2.0",
    "id": 1,
    "method": "suix_getLatestSuiSystemState",
    "params": []
  }'
```

### 4.2 日志：建议关注的关键字

- 启动与连通性：`Cluster started` / `Fullnode RPC URL: ...`
- metrics 端口：`Started Prometheus HTTP endpoint at ...`
- 异常：`ERROR` / `panic` / `unhealthy`（`sui start` 有健康检查循环）

### 4.3 Prometheus 指标：直接拉取 `/metrics`

拿到某个节点的 `metrics_address` 后：

```bash
curl "http://127.0.0.1:<metrics_port>/metrics" | head
```

如何找到 `metrics_port`：

- **优先**：从启动日志里搜 `Started Prometheus HTTP endpoint at`（每个节点都会打印）。  
- **其次（持久化模式）**：在 `$SUI_CHAIN_DIR` 下打开 `fullnode.yaml` 与各 validator 的 yaml，查 `metrics_address` 字段。  

## 5. 可选：Prometheus + Grafana（示例文件已提供）

本目录包含开箱即用的示例（你只需填 metrics targets）：  

- `notes/sui-fork-chain/observability/prometheus.yml`
- `notes/sui-fork-chain/observability/docker-compose.yml`

### 5.1 使用步骤（最短路径）

1) 从日志或配置中收集每个节点的 `metrics_address`（例如 `127.0.0.1:9184`、`127.0.0.1:39123` 等）。  
2) 根据 Prometheus 的运行方式填写 `observability/prometheus.yml` 的 `targets`：  
   - Prometheus 跑在 `docker compose`：通常填 `host.docker.internal:<port>`（如果节点 metrics 绑定在 `127.0.0.1`，见 5.2）。  
   - Prometheus 跑在宿主机进程：直接填 `127.0.0.1:<port>`。  
3) 启动：

```bash
cd notes/sui-fork-chain/observability
docker compose up -d
```

4) 打开：

- Prometheus：`http://127.0.0.1:9090`
- Grafana：`http://127.0.0.1:3000`（默认账号/密码：`admin`/`admin`）

### 5.2 Docker 抓不到 metrics？（最常见原因）

如果你的节点 metrics 绑定在 `127.0.0.1`，而 Prometheus 在容器里运行，容器可能无法访问宿主机的 loopback。

解决思路（任选其一）：

- **最简单**：把 Prometheus 跑在宿主机网络（例如使用 host network，或直接本机安装 Prometheus）。  
- **稍进阶**：把节点配置里的 `metrics_address` 从 `127.0.0.1:<port>` 改成 `0.0.0.0:<port>` 后重启（注意：多节点必须保证端口不冲突）。  

## 6. 可选：区块浏览器（最小可用）

很多线上浏览器依赖它们自己的后端，不一定能完整支持你的私链；但你仍可尝试以下路径：

- **可自建/可本地运行的 Explorer**：
  - [Polymedia Explorer](https://github.com/juzybits/polymedia-explorer)
  - [Sui Explorer（社区 fork）](https://github.com/suiware/sui-explorer)
- **在线扫描器（可能支持 Custom RPC URL）**：
  - [suiscan](https://suiscan.xyz/)
  - [SuiVision](https://suivision.xyz/)

更推荐的策略：

- 先用本手册的 **RPC + Prometheus/Grafana** 把链跑稳；  
- 若确实需要更像“区块浏览器”的检索体验，再升级启用 `sui start --with-indexer --with-graphql`（需要 PostgreSQL），用 indexer/GraphQL 做更丰富的查询与 UI。  

## 7. 常见坑与排查

- 端口冲突：
  - RPC 默认 `9000`（可用 `--fullnode-rpc-port` 改）
  - faucet 默认 `9123`（可用 `--with-faucet=<FAUCET_HOST_PORT>` 改，例如 `--with-faucet=6124`）
- 参数互斥：`--force-regenesis` 与 `--network.config` 不能同时使用。
- `--committee-size` 被忽略：当已有网络配置存在时，CLI 会警告并忽略该参数；要修改 validator 数量，请用全新目录或重新生成 genesis。
- `/tmp` 问题：如果 `/tmp` 挂载到 `/tmpfs` 或权限/容量异常，优先用 `TMPDIR=./tmp` 指定临时目录。

## 8. 参考与实现位置（便于深挖）

- 本地网络与可选服务（faucet/indexer/graphql）：`docs/content/guides/developer/sui-101/local-network.mdx`
- `sui start`/参数校验/默认端口：`crates/sui/src/sui_commands.rs`
- 节点默认端口（RPC 9000、metrics 9184）：`crates/sui-config/src/node.rs`
- OpenRPC 方法名（用于 curl 校验）：`crates/sui-open-rpc/spec/openrpc.json`

---

# 研究笔记（保留）

本目录也用于研究如何成功运行一个 Sui 的 fork 链（分叉链）。

## 后续研究目标

- 理解 Sui fork 链的概念和实现方式
- 研究多节点网络的配置和连接（跨机器）
- 探索从主链分叉出独立链的方法（state fork）

## 待办事项

- [ ] 深入研究 `GenesisConfig` 结构
- [ ] 研究 `NetworkConfig` 和节点配置
- [ ] 探索自定义协议参数的方法
- [ ] 研究如何持久化 fork 链状态

