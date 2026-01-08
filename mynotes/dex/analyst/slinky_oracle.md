Slinky（原名 Connect）是一个去中心化预言机系统，用于将链下数据安全地传递到链上应用。它利用链的原生验证者集合和共识机制来保证数据安全。

## 核心组件

根据代码结构，Slinky 包含以下核心包：

1. **oracle** - 主预言机，聚合外部数据源
2. **providers** - 数据提供商集合（支持 API 和 WebSocket）
3. **abci** - Vote Extensions 和 Proposal Handlers，用于与区块链集成
4. **x/oracle** - Cosmos SDK 模块，存储预言机数据
5. **x/marketmap** - Cosmos SDK 模块，存储市场配置
   1. 

## 核心逻辑

### 1. 价格数据获取

从多个数据提供商（如 Coinbase、Binance、Kraken 等）获取价格数据：

```Plain
func (o *OracleImpl) fetchAllPrices() {
        o.logger.Debug("starting price fetch loop")
        defer func() {
                if r := recover(); r != nil {
                        o.logger.Error("fetchAllPrices tick panicked", zap.Error(fmt.Errorf("%v", r)))
                }
        }()

        o.aggregator.Reset()

        // Retrieve the latest prices from each provider.
        o.mut.Lock()
        for _, provider := range o.priceProviders {
                o.fetchPrices(provider.Provider)
        }
        o.mut.Unlock()

        o.logger.Debug("oracle fetched prices from providers")

        // Compute aggregated prices and update the oracle.
        o.aggregator.AggregatePrices()
        o.setLastSyncTime(time.Now().UTC())

        // update the last sync time
        o.metrics.AddTick()
}
```

### 2. 价格聚合机制

使用中位数算法聚合多个数据源的价格：

```Plain
        for ticker, market := range m.cfg.Markets {
                if !market.Ticker.Enabled {
                        m.logger.Debug("skipping disabled market", zap.Any("market", market))
                        continue
                }

                // Get the converted prices for set of convertible markets.
                // ex. BTC/USDT * Index USDT/USD = BTC/USD
                //     BTC/USDC * Index USDC/USD = BTC/USD
                target := market.Ticker
                convertedPrices := m.CalculateConvertedPrices(market)
                m.metrics.AddProviderCountForMarket(target.String(), len(convertedPrices))

                // We need to have at least the minimum number of providers to calculate the median.
                if len(convertedPrices) < int(target.MinProviderCount) { //nolint:gosec
                        missingPrices = append(missingPrices, ticker)
                        m.logger.Debug(
                                "insufficient amount of converted prices",
                                zap.String("target_ticker", ticker),
                                zap.Int("num_converted_prices", len(convertedPrices)),
                                zap.Any("converted_prices", convertedPrices),
                                zap.Int("min_provider_count", int(target.MinProviderCount)), //nolint:gosec
                        )

                        continue
                }

                // Take the median of the converted prices. This takes the average of the middle two
                // prices if the number of prices is even.
                price := math.CalculateMedian(convertedPrices)
                indexPrices[target.String()] = new(big.Float).Copy(price)

                // Scale the price to the target ticker's decimals.
                scaledPrices[target.String()] = math.ScaleBigFloat(new(big.Float).Copy(price), target.Decimals)

                m.logger.Debug(
                        "calculated median price",
                        zap.String("target_ticker", ticker),

                        zap.String("unscaled_price", indexPrices[target.String()].String()),
                        zap.String("scaled_price", scaledPrices[target.String()].String()),
                        zap.Any("converted_prices", convertedPrices),
                )
                floatPrice, _ := price.Float64()
                m.metrics.AddTickerTick(target.String())
                m.metrics.UpdateAggregatePrice(target.String(), target.GetDecimals(), floatPrice)
        }
```

### 3. 市场映射（Market Map）

通过 Market Map 配置价格转换路径，支持间接价格计算：

- 直接价格：BTC/USD
- 间接价格：BTC/USDT × USDT/USD = BTC/USD
- 支持价格反转和归一化
  - 

### 4. ABCI++ 集成

通过 Vote Extensions 在共识层广播价格：

- Extend Vote：每个验证者从预言机获取价格，创建 Vote Extension 并广播
- Prepare Proposal：提案者聚合所有验证者的 Vote Extensions，生成最终价格
- Process Proposal：验证者验证提案中的价格数据
  - 

```Plain
        // Set Price Update Generators/Decoders for Slinky
        if appFlags.VEOracleEnabled {
                priceUpdateGenerator = prices.NewSlinkyPriceUpdateGenerator(
                        aggregator.NewDefaultVoteAggregator(
                                app.Logger(),
                                voteweighted.MedianFromContext(
                                        app.Logger(),
                                        app.StakingKeeper,
                                        voteweighted.DefaultPowerThreshold,
                                ),
                                strategy,
                        ),
                        extCommitCodec,
                        veCodec,
                        strategy,
                )
                priceUpdateDecoder = process.NewSlinkyMarketPriceDecoder(
                        priceUpdateDecoder,
                        priceUpdateGenerator,
                )
        }
        // Generate the dydx handlers
        dydxPrepareProposalHandler := prepare.PrepareProposalHandler(
                txConfig,
                app.BridgeKeeper,
                app.ClobKeeper,
                app.PerpetualsKeeper,
                priceUpdateGenerator,
        )

        // ProcessProposal setup.
        var dydxProcessProposalHandler = process.ProcessProposalHandler(
                txConfig,
                app.BridgeKeeper,
                app.ClobKeeper,
                app.StakingKeeper,
                app.PerpetualsKeeper,
                app.PricesKeeper,
                priceUpdateDecoder,
        )

        // Wrap dydx handlers with slinky handlers
        if appFlags.VEOracleEnabled {
                app.initOracle(priceUpdateDecoder)
                proposalHandler := slinkyproposals.NewProposalHandler(
                        app.Logger(),
                        dydxPrepareProposalHandler,
                        dydxProcessProposalHandler,
                        ve.NewDefaultValidateVoteExtensionsFn(app.StakingKeeper),
                        veCodec,
                        extCommitCodec,
                        strategy,
                        app.oracleMetrics,
                        slinkyproposals.RetainOracleDataInWrappedProposalHandler(),
                )
                return proposalHandler.PrepareProposalHandler(), proposalHandler.ProcessProposalHandler()
        }
        return dydxPrepareProposalHandler, dydxProcessProposalHandler
```

## 工作流程

1. 数据获取：从多个数据提供商（API/WebSocket）获取价格
2. 价格过滤：过滤过期数据（基于 `MaxPriceAge`）
3. 价格转换：根据 Market Map 配置转换间接价格路径
4. 价格聚合：使用中位数算法聚合多个数据源的价格
5. Vote Extension：验证者将聚合后的价格放入 Vote Extension
6. 共识聚合：提案者聚合所有验证者的 Vote Extensions，使用投票权重中位数
7. 链上存储：最终价格写入区块链状态
   1. 

## 关键特性

- 安全性：利用链的原生验证者集合，无需第三方信任
- 性能：支持超过 2000 个交易对和价格源
- 实时性：每个区块更新价格（通过 Vote Extensions）
- 去中心化：多个数据源 + 多个验证者，降低单点故障风险
- 灵活性：支持直接和间接价格路径，可配置最小提供商数量
  - 

问题：管理员直接上币调用哪个接口。