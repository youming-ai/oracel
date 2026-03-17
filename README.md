# Polymarket 5m Bot

BTC 5分钟市场自动交易机器人，使用 Chainlink 预言机数据 + Polymarket CLOB。

移植自 [orakel](https://github.com/youming-ai/orakel) 的 15 分钟策略，针对 5 分钟市场优化。

## 架构

```
┌─────────────────────────────────────────────────────┐
│                   数据层                              │
│                                                      │
│  Chainlink RPC ← Polygon BTC/USD 预言机价格          │
│  Binance WS   ← 实时 BTC/USDT 价格                  │
│  Polymarket CLOB WS ← 订单簿 + 市场概率               │
└──────────────────┬───────────────────────────────────┘
                   ↓
┌─────────────────────────────────────────────────────┐
│               计算引擎 (core/)                        │
│                                                      │
│  1. score_direction()                                │
│     - VWAP 距离 + 斜率                               │
│     - RSI + RSI 斜率                                 │
│     - MACD 柱状图扩张                                │
│     - Heiken Ashi 连续计数                            │
│     → TA 概率                                        │
│                                                      │
│  2. estimate_price_to_beat_probability()             │
│     - Sigmoid 模型: (价格-目标)/波动率                │
│     → PtB 概率                                       │
│                                                      │
│  3. blend_probabilities()                            │
│     - 自适应加权: 早期信赖 TA, 晚期信赖 PtB           │
│     → 最终概率                                       │
│                                                      │
│  4. compute_edge()                                   │
│     - edge = model_prob - market_prob                │
│     → 套利机会检测                                    │
│                                                      │
│  5. decide()                                         │
│     - 阶段过滤: EARLY/MID/LATE                       │
│     - 波动体制过滤: Trending/Chop/LowVolume           │
│     → 交易决策                                       │
└──────────────────┬───────────────────────────────────┘
                   ↓
┌─────────────────────────────────────────────────────┐
│               执行层                                  │
│                                                      │
│  Paper 模式 → 日志记录                                │
│  Live 模式  → Polymarket CLOB API 下单               │
└─────────────────────────────────────────────────────┘
```

## 与 Orakel (15m) 的区别

| 维度 | Orakel (15m) | 5m Bot |
|------|-------------|--------|
| 时间窗口 | 15 分钟 | 5 分钟 |
| 决策频率 | ~60 秒 | ~2 秒 |
| TA 权重 | 0.6-0.7 | 0.4-0.6 (自适应) |
| Edge 阈值 | 5-20% | 4-10% |
| 数据源 | Binance + Chainlink | Chainlink + Binance + CLOB |
| 语言 | TypeScript/Bun | Rust |

## 快速开始

### 1. 安装依赖

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### 2. 编辑配置

```bash
cp config.example.json config.json
# 编辑 config.json:
#   - market.settlement_time: 市场结算时间
#   - market.price_to_beat: 结算目标价格
#   - market.token_id_yes/no: 从 Polymarket 获取
#   - trading.private_key: 私钥 (live 模式需要)
```

### 3. 自动获取市场 Token IDs

```bash
# 使用 market_slug 自动查询
# 在 config.json 中设置 market_slug，bot 启动时会自动获取 token IDs
```

### 4. Paper 模式运行

```bash
cargo run --release
```

### 5. Live 模式

```bash
# 在 config.json 中设置:
#   trading.mode = "live"
#   trading.private_key = "0x..."
cargo run --release
```

## 配置说明

### 时间阶段阈值

```
时间剩余 > 66% → EARLY:  edge ≥ 4%,  prob ≥ 54%
时间剩余 > 33% → MID:    edge ≥ 7%,  prob ≥ 57%
时间剩余 ≤ 33% → LATE:   edge ≥ 10%, prob ≥ 60%
```

### 波动体制

- **Trending**: 正常交易
- **Chop**: edge 阈值 × 1.5 (可配置 skip_chop 跳过)
- **LowVolume**: 正常但需要注意流动性

### 核心参数

| 参数 | 默认值 | 说明 |
|------|--------|------|
| `ta_weight_early` | 0.6 | 窗口开始时的 TA 权重 |
| `ta_weight_late` | 0.4 | 窗口结束时的 TA 权重 |
| `signal_interval_ms` | 2000 | 信号计算间隔 |
| `chainlink.poll_interval_ms` | 5000 | Chainlink 价格轮询间隔 |
| `max_position_size` | 50.0 | 最大仓位 (USDC) |
| `cooldown_seconds` | 30 | 交易冷却时间 |

## 数据源

### Chainlink BTC/USD (Polygon)

合约地址: `0xc907E116054Ad103354f2D350FD2514433D57F6f`

通过 `eth_call` 读取 `latestRoundData()` 获取最新价格。

### Polymarket CLOB

- WebSocket: `wss://clob.polymarket.com/ws` - 实时订单簿
- REST: `https://clob.polymarket.com` - 价格查询 + 下单

## 还需要实现

- [ ] Polymarket EIP-712 订单签名
- [ ] VWAP cross count 统计
- [ ] Volume ratio 计算
- [ ] Failed VWAP reclaim 检测
- [ ] 自动发现新市场 (每 5 分钟)
- [ ] 止盈止损
- [ ] Web dashboard
