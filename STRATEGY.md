# Polymarket 5m Bot 交易策略文档

**版本:** 0.2.0  
**更新时间:** 2026-03-16

---

## 1. 概述

Bot 针对 Polymarket 的 BTC 5分钟涨跌预测市场进行自动化交易:
- 使用 Binance 实时 BTC 价格作为信号源
- 技术分析 + 市场价格融合计算 edge
- Kelly 动态仓位管理
- 自适应参数调整

---

## 2. 数据流

```
Binance WebSocket → BTC实时价格
                    ↓
              价格历史缓冲区 (1000条)
                    ↓
              ┌─ VWAP计算
              └─ RSI (14)
              └─ MACD (12, 26, 9)
              └─ Heiken Ashi
                    ↓
              技术指标
                    ↓
              ┌─ P(UP) 计算
              └─ Edge 计算
```

```
Polymarket REST API → Yes/No合约价格
                    ↓
              市场概率
                    ↓
              Edge = 模型概率 - 市场概率
```

## 3. 概率模型 (src/core/probability.rs)

### 3.1 技术指标评分

```python
momentum = Σ (indicator_weight × normalized_signal)

其中:
- Price vs VWAP: weight 2.0
- VWAP slope: weight 2.0  
- RSI deviation: weight 1.5
- MACD histogram: weight 2.0
- MACD line: weight 0.5
- Heiken Ashi streak: weight 1.0
- VWAP reclaim失败: weight -1.5 (bearish)

normalized_momentum = momentum / total_weight
P(UP)_raw = 0.5 + 0.5 * tanh(momentum × 3.0)
```

### 3.2 概率融合

```python
# 与市场价格融合
P(UP)_final = P(UP)_TA × ta_weight + P(Yes)_market × (1 - ta_weight)

ta_weight = 0.6  # 60% 权重给技术分析
```

### 3.3 均值回归调整

当市场价格极端时 (Yes > 85% 或 < 15%):
- 调整 P(UP) 向相反方向移动 ±15%
- 防止追涨杀跌

---

## 4. Edge 计算 (src/core/edge.rs)

```
Edge_up = P(UP)_model - P(Yes)_market
Edge_down = P(DOWN)_model - P(No)_market

# 选择 Edge 更大的方向
best_edge = max(Edge_up, Edge_down)
best_side = "UP" if Edge_up > Edge_down else "DOWN"
```

---

## 5. 决策流程

### 5.1 市场状态过滤

```python
# 极端价格过滤
if market_price < 0.02 or > 0.98:
    → NO TRADE ("extreme_market_price")

# Edge 异常过滤  
if edge > 0.50
    → NO TRADE ("edge_too_high_unrealistic")

# 价格过滤
if price < 0.05 and edge < 0.40
    → NO TRADE ("price_too_cheap_low_edge")

if price > 0.95
    → NO TRADE ("price_too_expensive")
```

### 5.2 时间阶段划分

```python
remaining_pct = time_remaining / window_minutes

if remaining_pct > 0.66:
    phase = EARLY
elif remaining_pct > 0.33:
    phase = MID
else:
    phase = LATE
```

### 5.3 阈值配置

| 阶段 | Edge阈值 | 最小概率 |
|------|----------|----------|
| EARLY | 8% | 58% |
| MID | 12% | 62% |
| LATE | 18% | 65% |

### 5.4 市场状态调整

```python
# 检测市场状态
regime = detect_regime(prices, vwap_crosses, volatility)

# 状态相关调整
if regime == CHOPPY:
    edge_threshold *= 1.5  # 需要更高edge
if regime == EXTREME_VOL
    edge_threshold *= 2.0  # 需要更高edge
if regime == LOW_VOLUME
    edge_threshold *= 1.3

# 跳过震荡市
if skip_chop and regime == CHOPPY
    → NO TRADE
```

### 5.5 决策逻辑

```python
if best_edge < threshold:
    → NO TRADE ("edge_below_threshold")

if best_model_prob < min_prob:
    → NO TRADE ("prob_below_threshold")

# 通过所有检查
→ TRADE (best_side, best_edge)
```

---

## 6. 仓位管理 (src/strategy.rs)

### 6.1 Kelly 公式

```python
# 估计胜率
est_win_prob = (entry_price + edge).clamp(0.1, 0.9)

# 赔率
payout_ratio = (1 - entry_price) / entry_price

# Kelly
kelly = (est_win_prob × payout_ratio - (1 - est_win_prob)) / payout_ratio

# 分数Kelly (25%)
fractional_kelly = kelly × 0.25

# 仓位大小
size = base_size × (1 + fractional_kelly × 3).max(0.3)
```

### 6.2 风控限制

```python
# 连续亏损
if consecutive_losses >= 3:
    size *= 0.5
elif consecutive_losses >= 2:
    size *= 0.75

# 日亏损限制
if daily_loss_ratio > 0.5:
    size *= (1 - daily_loss_ratio)

# 单市场限制
if entries_for_market >= 3:
    → NO TRADE

# 冷却期
if time_since_last_trade < 45s:
    → NO TRADE
```

---

## 7. 自适应调整 (src/adaptive.rs)

每5分钟根据实际表现调整参数

```python
# 计算滚动胜率
win_rate = wins / total_trades (最近20笔)

# 方向偏差
up_ratio = up_trades / total_trades
if up_ratio > 0.75 or up_ratio < 0.25:
    bias_detected = True
    edge_penalty += 5%

# Edge 倍数调整
if win_rate < 0.45:
    edge_multiplier = 1.0 + (0.45 - win_rate) / 0.25
elif win_rate > 0.45:
    edge_multiplier = 0.8  # 放宽

if win_rate < 0.20:
    emergency_stop = True  # 完全停止交易

# 连续亏损惩罚
if consecutive_losses >= 3:
    cooldown *= 2.0

# 回撤惩罚
if drawdown > 5%:
    skip_chop = True
```

---

## 8. 结算逻辑 (src/settlement.rs)

5分钟窗口结束时:
```python
btc_change = btc_settlement - btc_entry

if abs(btc_change) < $5:
    # 价格变化太小，使用市场价格决定
    winner = "UP" if market_yes > 0.5 else "DOWN"
else:
    winner = "UP" if btc_change > 0 else "DOWN"

# 判断交易结果
if trade_side == winner:
    payout = trade_size  # 赢了获得全部
else:
    payout = 0  # 输了全部损失

pnl = payout - cost
```

---

## 9. 配置参数

当前配置 (config.json):

| 参数 | 值 | 说明 |
|------|------|------|
| edge_threshold_early | 8% | 窗口前期edge阈值 |
| edge_threshold_mid | 12% | 窗口中期edge阈值 |
| edge_threshold_late | 18% | 窗口末期edge阈值 |
| min_prob_early | 58% | 窗口前期最小概率 |
| min_prob_mid | 62% | 窗口中期最小概率 |
| min_prob_late | 65% | 窗口末期最小概率 |
| cooldown_seconds | 45 | 交易冷却期 |
| max_position_size | $30 | 单笔最大仓位 |
| min_order_size | $5 | 单笔最小仓位 |
| max_daily_loss_usdc | $50 | 日最大亏损 |
| max_consecutive_losses | 3 | 最大连续亏损次数 |
| skip_chop | true | 跳过震荡市 |
| adaptive.enabled | true | 启用自适应调整 |

---

## 10. 关键设计理念

1. **概率中心化** — P(UP) 在中性条件下接近 50%，避免系统性偏差
2. **多因子融合** — 技术分析 (60%) + 市场价格 (40%)
3. **动态阈值** — 根据时间阶段和市场状态调整
4. **Kelly 仓位** — 根据edge和概率动态调整仓位大小
5. **多层风控** — 价格过滤 + 连续亏损缩仓 + 日止损
6. **自适应调整** — 根据实际表现自动优化参数

---

## 11. 文件结构

```
src/
├── main.rs              # 主循环、状态管理
├── config.rs            # 配置定义
├── adaptive.rs          # 自适应参数调整
├── strategy.rs          # 仓位管理、价格过滤
├── settlement.rs        # 结算逻辑
├── core/
│   ├── mod.rs           # 计算管线入口
│   ├── probability.rs   # 概率计算
│   └── edge.rs          # Edge计算、决策逻辑
├── data/
│   ├── binance.rs       # Binance价格数据
│   ├── polymarket.rs    # Polymarket市场数据
│   └── market_discovery.rs  # 市场发现
└── types.rs             # 类型定义
```
