# Polymarket 5m Bot

BTC 5 分钟涨跌市场自动交易机器人。它从 Coinbase 获取 BTC 实时价格，从 Polymarket 获取 5 分钟市场报价，在市场情绪极端时反向下注，并在 live 模式下支持链上赎回。

## 策略概览

- 市场极端看涨时买 `DOWN`
- 市场极端看跌时买 `UP`
- 核心假设是 5 分钟窗口的公平价值接近 `0.50`
- 只有当 edge、风控和 momentum filter 同时通过时才会下单

详细逻辑见 `STRATEGY.md`。

## 架构

```text
Coinbase WS -> BTC 实时价格 (信号)
                |
Polymarket REST -> Yes/No 价格
                |
          +-------------------------+
          | Pipeline                |
          | 1. PriceSource          |
          | 2. Signal               |
          | 3. Decider              |
          | 4. Executor             |
          | 5. Settler              |
          +-------------------------+
                |
Chainlink Oracle -> BTC 结算价格 (Polygon)
                |
CTF Redeemer -> live 模式链上赎回
```

## 目录结构

```text
src/
|- main.rs                  # 主循环、CLI、日志初始化
|- config.rs                # 配置定义与默认值
|- data/
|  |- chainlink.rs          # Chainlink BTC/USD oracle RPC
|  |- coinbase.rs           # Coinbase Advanced Trade WS
|  |- market_discovery.rs   # Gamma API 市场发现
|  `- polymarket.rs         # CLOB 客户端与 CTF redeem
`- pipeline/
   |- price_source.rs       # BTC 价格缓冲
   |- signal.rs             # 市场信号计算
   |- decider.rs            # 交易决策与风控
   |- executor.rs           # paper/live 下单
   `- settler.rs            # 到期结算与交易日志

scripts/
`- watch.sh                 # 终端实时监控

deploy/
`- polybot.service          # systemd 服务文件

logs/                       # 运行时生成
|- bot.log                  # 主日志
|- trades.csv               # 开仓与结算交易记录
`- balance                  # 当前余额
```

## 快速开始

```bash
# 1. 编译
cargo build --release

# 2. 检查或修改配置
#    首次运行会自动生成 config.json

# 3. 以 paper 模式运行
cargo run --release

# 4. 监控日志
scripts/watch.sh
```

## CLI

```bash
# 正常运行 bot
cargo run --release

# 派生 Polymarket CLOB API 凭据（输出到终端，不写回 .env）
cargo run --release -- --derive-keys

# 手动扫描最近 24 小时市场并尝试链上 redeem
cargo run --release -- --redeem-all
```

## 运行模式

### Paper

- 默认模式，`config.json` 里的 `trading.mode` 默认为 `paper`
- 不要求 `PRIVATE_KEY`
- 下单使用本地模拟订单 ID
- 结算优先使用 Chainlink BTC/USD 价格，失败时回退到最新 Coinbase 价格

### Live

- 需要在 `.env` 里提供 `PRIVATE_KEY`
- 会创建 Polymarket 认证客户端并实际下单
- 会启用 CTF redeemer，对可赎回仓位进行链上 redeem
- Chainlink RPC 优先使用 `ALCHEMY_KEY`，否则回退公共 Polygon RPC

## 环境变量

程序启动时会读取仓库根目录的 `.env`。

| 变量 | 是否必需 | 说明 |
| --- | --- | --- |
| `PRIVATE_KEY` | live 模式必需 | 钱包私钥，用于 CLOB 认证和 CTF redeem |
| `ALCHEMY_KEY` | 可选 | live 模式 Polygon RPC，加速 Chainlink 查询和 redeem |

`--derive-keys` 会根据 `PRIVATE_KEY` 派生出 `POLY_API_KEY`、`POLY_API_SECRET` 和 `POLY_PASSPHRASE`，但这些值只输出到终端，不会写回 `.env`。

## 配置

当前示例配置见 `config.json`，完整默认值定义见 `src/config.rs`。

| 参数 | 默认值 | 说明 |
| --- | --- | --- |
| `trading.mode` | `paper` | 运行模式 |
| `market.event_url` | `""` | Polymarket event URL，用于自动解析 `series_id` |
| `market.series_id` | `""` | 事件序列 ID；如果设置了 `event_url`，优先从 URL 解析 |
| `market.window_minutes` | `5.0` | 市场窗口长度 |
| `polyclob.gamma_api_url` | `https://gamma-api.polymarket.com` | Gamma API 地址 |
| `strategy.max_position_size` | `50.0` | 单笔最大仓位（USDC） |
| `strategy.min_order_size` | `5.0` | 单笔最小仓位（USDC） |
| `strategy.extreme_threshold` | `0.80` | 极端阈值 |
| `strategy.fair_value` | `0.50` | 公平价值假设 |
| `strategy.btc_tiebreaker_usd` | `5.0` | 结算价差低于该值时的 tie-breaker |
| `strategy.momentum_threshold` | `0.001` | Momentum 过滤阈值（0.1%） |
| `strategy.momentum_lookback_ms` | `120000` | Momentum 回看窗口（2 分钟） |
| `edge.edge_threshold_early` | `0.15` | 前期 edge 门槛 |
| `edge.edge_threshold_mid` | `0.15` | 预留的中期 edge 配置项，当前主流程未使用 |
| `edge.edge_threshold_late` | `0.20` | 预留的后期 edge 配置项，当前主流程未使用 |
| `edge.min_prob_early` | `0.50` | 预留的最小概率配置项，当前主流程未使用 |
| `edge.min_prob_mid` | `0.50` | 预留的最小概率配置项，当前主流程未使用 |
| `edge.min_prob_late` | `0.50` | 预留的最小概率配置项，当前主流程未使用 |
| `risk.max_daily_loss_usdc` | `100.0` | 预留的绝对日亏损配置项，当前 decider 未使用 |
| `risk.max_consecutive_losses` | `8` | 连续亏损熔断阈值 |
| `risk.max_daily_loss_pct` | `0.10` | 日亏损比例上限 |
| `risk.cooldown_ms` | `5000` | 交易冷却期 |
| `risk.max_risk_fraction` | `0.10` | 单笔最多使用余额比例 |
| `polling.signal_interval_ms` | `1000` | 主循环信号间隔 |

注意：仓库里的 `config.json` 是当前运行示例值，不一定等于代码默认值。

## 数据源

- Coinbase Advanced Trade WS: BTC 实时价格
- Polymarket CLOB REST: Yes/No 报价和 live 下单
- Gamma API: 自动发现当前 5 分钟市场
- Chainlink BTC/USD Oracle on Polygon: 结算价格与 redeem 相关链上读操作

## 日志与监控

- `logs/bot.log`: 主运行日志
- `logs/trades.csv`: 开仓和结算都会追加的交易记录
- `logs/balance`: 当前余额快照
- `scripts/watch.sh`: 基于 `logs/bot.log` 的终端监控脚本

## 部署

仓库包含 `deploy/polybot.service`，是一个 systemd 服务文件，当前配置假设二进制位于 `/root/polymarket-5m-bot/target/release/polybot`。
