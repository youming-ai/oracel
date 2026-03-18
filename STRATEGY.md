# 交易策略

## 1. 核心思路

Polymarket BTC 5 分钟涨跌市场是二元期权：YES = 涨，NO = 跌。

市场经常在某个方向过度自信（如 85% 认为涨）。此时便宜一边（15%）被严重低估。
假设真实概率接近 50/50，买入便宜一边就是正 EV。

```
Edge = fair_value(0.50) - cheap_side_price
```

## 2. 信号检测

从 Polymarket CLOB 获取 Yes/No 价格，计算市场倾向度：

```
mkt_up = yes_price / (yes_price + no_price)

if mkt_up > 80%:  → 市场极端看涨，买 DOWN
if mkt_up < 20%:  → 市场极端看跌，买 UP
else:             → 不交易 (市场没有极端偏差)
```

## 3. 决策流程

```
1. 该窗口是否已交易？ → 每 5m 窗口只交易一次
2. 风控检查
   - 余额 > 0
   - 冷却期已过
   - 未触发熔断 (连续亏损 < 8)
   - 日亏损 < 余额 10%
3. 市场是否极端？ → mkt_up > 80% 或 < 20%
4. Edge 是否足够？ → edge > 15%
5. Momentum filter → skip when BTC moves >0.1% in 2min against trade direction
6. 通过 → 下单
```

## 4. 仓位管理

Half-Kelly + Edge 缩放：

```
win_rate = 历史胜率.clamp(50%, 75%)
kelly = (2 × win_rate - 1) × 0.5
edge_mult = (1 + (edge - 0.15) / 0.15).clamp(1.0, 2.0)

size = balance × kelly × edge_mult
size = size.clamp(min_position, max_position)
size = min(size, balance × 10%)
```

## 5. 风控

| 机制 | 规则 |
|------|------|
| 单窗口限制 | 每 5m 市场最多 1 笔 |
| 冷却期 | 交易间隔 5s |
| 连亏暂停 | 4-5 连亏暂停 1m，6-7 连亏暂停 5m |
| 熔断 | 8 连亏停止交易 |
| 日止损 | 日亏损 > 余额 10% 停止 |

## 6. 结算

5 分钟窗口到期时：

```
btc_change = settle_price - entry_price

if |btc_change| < $5:  → 价差太小，用入场价格方向决定
if btc_change > 0:     → UP 赢
if btc_change < 0:     → DOWN 赢

赢 → payout = size_usdc, pnl = payout - cost
输 → payout = 0,         pnl = -cost
```

## 7. Momentum Filter

The core strategy assumes 50/50 fair value, but during strong trends the market's
extreme pricing may be justified. The momentum filter acts as a safety valve:
skip trades when short-term BTC movement strongly opposes the trade direction.

```
momentum = (current_price - lookback_start_price) / lookback_start_price
lookback = momentum_lookback_ms (default 120s, time-based window)

if shorting && momentum > +0.1%:  → skip (BTC pumping, don't bet against)
if longing  && momentum < -0.1%:  → skip (BTC dumping, don't bet against)
else:                              → pass
```

Both threshold and lookback window are configurable via `config.json`:
`strategy.momentum_threshold` and `strategy.momentum_lookback_ms`.

## 8. 关键假设

1. **50/50 公平价值** — 5 分钟内 BTC 涨跌概率接近均等
2. **均值回归** — 市场极端定价倾向于修正
3. **低成本入场** — 买便宜一边 (0.15-0.20)，赢了赚 5-8x，输了亏 cost
4. **高频轮转** — 每 5 分钟一个新市场，机会密度高
