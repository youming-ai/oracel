# Polymarket 5m Bot

BTC 5 分钟涨跌预测市场自动交易机器人。检测 Polymarket 市场情绪极端时反向下注。

## 策略核心

不预测 BTC 走势，而是检测市场过度自信（>80%）时反向下注：

```
市场 80%+ 认为涨 → 买 DOWN（便宜的一边）
市场 80%+ 认为跌 → 买 UP（便宜的一边）
Edge = 0.50 - cheap_side_price
```

详见 [STRATEGY.md](STRATEGY.md)。

## 架构

```
Coinbase WS → BTC 实时价格
                  ↓
Polymarket REST → Yes/No 合约价格
                  ↓
            ┌─────────────────────────┐
            │  Pipeline               │
            │  1. PriceSource  价格流  │
            │  2. Signal  极端检测     │
            │  3. Decider  交易决策    │
            │  4. Executor  下单执行   │
            │  5. Settler  结算追踪    │
            └─────────────────────────┘
```

## 目录结构

```
src/
├── main.rs                 # 主循环、Bot 状态管理
├── config.rs               # 配置定义
├── signing.rs              # EIP-712 订单签名
├── data/
│   ├── coinbase.rs         # Coinbase Advanced Trade WS
│   ├── market_discovery.rs # Gamma API 自动发现市场
│   └── polymarket.rs       # CLOB REST 价格 + 下单
└── pipeline/
    ├── price_source.rs     # BTC 价格缓冲区
    ├── signal.rs           # 市场极端信号计算
    ├── decider.rs          # 交易决策 + 仓位 + 风控
    ├── executor.rs         # Paper/Live 下单
    └── settler.rs          # 5m 窗口到期结算

scripts/
├── run.sh                  # 启动 bot
├── watch.sh                # 实时终端监控
└── status.sh               # 状态报告

deploy/
└── polybot.service         # systemd 服务

logs/                       # 运行日志 (gitignored)
├── bot.log
├── trade_log.json
└── balance.json
```

## 快速开始

```bash
# 1. 编译
cargo build --release

# 2. 编辑配置
#    config.json 中设置 series_id、private_key (live 模式)
#    首次运行会生成默认 config.json

# 3. Paper 模式运行
scripts/run.sh

# 4. 实时监控
scripts/watch.sh
```

## 配置参数

| 参数 | 默认值 | 说明 |
|------|--------|------|
| `strategy.extreme_threshold` | 0.80 | 市场极端阈值 (>80% 触发) |
| `strategy.fair_value` | 0.50 | 公平价值假设 |
| `strategy.max_position_size` | $50 | 单笔最大仓位 |
| `edge.edge_threshold_early` | 15% | Edge 最低门槛 |
| `risk.max_consecutive_losses` | 8 | 熔断：连续亏损上限 |
| `risk.max_daily_loss_pct` | 10% | 日最大亏损比例 |
| `polling.signal_interval_ms` | 2000 | 信号计算间隔 |

## 数据源

- **Coinbase Advanced Trade WS** — `wss://advanced-trade-ws.coinbase.com` BTC-USD 实时价格
- **Polymarket CLOB REST** — `https://clob.polymarket.com` Yes/No 价格 + 下单
- **Gamma API** — `https://gamma-api.polymarket.com` 自动发现 5m 市场
