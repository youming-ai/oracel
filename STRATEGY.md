# 交易策略

## 1. 核心思路

Polymarket BTC 5 分钟市场是二元结果市场：`UP` 代表 BTC 在窗口结束时更高，`DOWN` 代表更低。

策略不预测中短期方向，而是利用市场在极端情绪下的错价：

- 市场过度看涨时，买 `DOWN`
- 市场过度看跌时，买 `UP`
- 用 `0.50` 作为 5 分钟窗口的公平价值近似

```text
edge = fair_value - cheap_side_price
```

## 2. 信号检测

程序从 Polymarket 读取 yes/no 报价，计算市场倾向：

```text
mkt_up = yes_price / (yes_price + no_price)

if mkt_up > 0.80: buy DOWN
if mkt_up < 0.20: buy UP
else: no trade
```

只有便宜一边相对公平价值存在足够 edge 时，信号才会进入下单阶段。

## 3. 决策流程

```text
1. 当前 5m 窗口是否已经交易过
2. 风控是否允许继续交易
3. 市场是否达到 extreme threshold
4. edge 是否达到当前启用阈值
5. momentum filter 是否允许逆势入场
6. 通过后再计算仓位并下单
```

程序会对同一个结算窗口只交易一次。

## 4. Momentum Filter

极端价格有时来自真实趋势而不是情绪失衡，所以策略会检查 BTC 的短时动量，避免直接逆强趋势开仓。

```text
momentum = (current_price - lookback_price) / lookback_price

if trade is DOWN and momentum > +threshold: skip
if trade is UP   and momentum < -threshold: skip
```

默认配置：

- `strategy.momentum_threshold = 0.001`（0.1%）
- `strategy.momentum_lookback_ms = 120000`（2 分钟）

## 5. 仓位管理

仓位使用基于历史胜率的 half-Kelly 近似，并叠加 edge 强度缩放：

```text
win_rate = overall_win_rate.clamp(0.50, 0.75)
kelly_fraction = max(2 * win_rate - 1, 0.05)
half_kelly = kelly_fraction * 0.5

edge_multiplier = clamp(1 + (edge - 0.15) / 0.15, 1.0, 2.0)

size = balance * half_kelly * edge_multiplier
size = clamp(size, min_position, max_position)
size = min(size, balance * max_risk_fraction)
```

默认上限包括：

- `strategy.min_order_size = 5.0`
- `strategy.max_position_size = 50.0`
- `risk.max_risk_fraction = 0.10`

## 6. 风控

| 机制 | 规则 |
| --- | --- |
| 单窗口限制 | 每个结算窗口最多交易一次 |
| 冷却期 | 两次交易至少间隔 `5000ms` |
| 连亏暂停 | 4-5 连亏暂停 1 分钟，6-7 连亏暂停 5 分钟 |
| 熔断 | 8 连亏后停止交易 |
| 日止损 | 当 `daily_pnl <= -balance * max_daily_loss_pct` 时停止 |
| 仓位上限 | 单笔不超过 `balance * max_risk_fraction` |

说明：代码里同时保留了 `risk.max_daily_loss_usdc` 配置项，但当前 decider 的阻断逻辑实际使用的是比例限制 `risk.max_daily_loss_pct`。

## 7. 下单执行

### Paper mode

- 不向 Polymarket 发送真实订单
- 生成本地 UUID 作为订单 ID
- 订单成本按 `size_usdc` 记录

### Live mode

- 使用认证后的 Polymarket CLOB 客户端发单
- 下单前会把 `size_usdc / price` 转成 shares，并向下截断到 2 位小数
- FOK 无法成交或无流动性时直接放弃该次交易

## 8. 结算

窗口到期后，程序优先使用 Polygon 上的 Chainlink BTC/USD oracle 判断结果；如果 Chainlink 查询失败，则回退到最新 Coinbase 价格继续结算。

```text
btc_change = chainlink_btc_price - entry_btc_price

if abs(btc_change) < btc_tiebreaker_usd:
    btc_went_up = entry_price > 0.5
else:
    btc_went_up = btc_change > 0
```

随后根据持仓方向判断输赢：

```text
if won:
    shares = size_usdc / entry_price
    payout = shares
    pnl = payout - cost
else:
    payout = 0
    pnl = -cost
```

本地 settlement 会把结果追加到 `logs/trades.csv`；开仓时也会往同一个文件追加记录。

## 9. Live 模式赎回

在 live 模式下，程序会初始化 CTF redeemer，用于对可赎回的 winning positions 进行链上 redeem。

另外也支持手动命令：

```bash
cargo run --release -- --redeem-all
```

这个命令会扫描最近 24 小时的市场并尝试逐个 redeem。

## 10. 关键假设

1. 5 分钟窗口的公平价值可以近似看作 `0.50`
2. 市场极端情绪会带来可利用的错价
3. 强趋势时错价可能是合理定价，因此需要 momentum filter
4. 结算应尽量贴近链上实际结果，因此使用 Chainlink oracle 和 live redeem 流程
