# Decider 优化设计

## 概述

优化 `src/pipeline/decider.rs` 交易决策模块，新增以下功能：

1. **最小 Edge 阈值** - 过滤低质量交易 (默认 5%)
2. **多时间框架动量** - 30s/60s/180s 三框架同向确认
3. **动态 Fair Value** - 基于 BTC 历史涨跌概率动态调整

## 架构

```
src/pipeline/
├── mod.rs              # 导出新模块
├── decider.rs          # 修改：添加 min_edge 检查，调用新模块
├── btc_history.rs      # 新增：记录 BTC 5分钟窗口涨跌，计算动态 FV
├── momentum.rs         # 新增：多时间框架动量计算
├── price_source.rs     # 现有：价格缓冲
├── signal.rs           # 现有：方向定义
├── executor.rs         # 现有：执行
├── settler.rs          # 现有：结算
└── test_helpers.rs     # 现有：测试工具
```

**数据流：**
```
price_source (BTC价格) 
    → btc_history (记录窗口结果，计算动态FV)
    → momentum (计算30s/60s/180s动量)
    → decider (综合决策，min_edge 过滤)
    → executor
    → settler → btc_history (反馈窗口结果)
```

## 模块设计

### 1. btc_history.rs

**数据结构：**
```rust
pub(crate) struct BtcWindow {
    pub start_time_ms: i64,
    pub end_time_ms: i64,
    pub start_price: Decimal,
    pub end_price: Decimal,
    pub up_won: bool,  // true = BTC 上涨
}

pub(crate) struct BtcHistory {
    windows: VecDeque<BtcWindow>,
    max_windows: usize,  // 默认 1000 个窗口 (~83小时)
}
```

**核心方法：**
```rust
impl BtcHistory {
    pub fn new(max_windows: usize) -> Self;
    
    // 每个市场周期结束时调用
    pub fn record_window(&mut self, start_price: Decimal, end_price: Decimal, 
                          start_time_ms: i64, end_time_ms: i64);
    
    // 计算动态 fair value，样本不足返回 None
    pub fn dynamic_fair_value(&self, min_samples: usize) -> Option<Decimal>;
    
    // 持久化/恢复
    pub fn to_json(&self) -> String;
    pub fn from_json(json: &str) -> Self;
}
```

**动态 FV 计算：**
```rust
fn dynamic_fair_value(&self, min_samples: usize) -> Option<Decimal> {
    if self.windows.len() < min_samples {
        return None;  // 样本不足，使用默认 0.50
    }
    let up_count = self.windows.iter().filter(|w| w.up_won).count();
    Some(Decimal::from(up_count) / Decimal::from(self.windows.len()))
}
```

**调用时机：**
- `settler.rs` 结算时，调用 `record_window` 记录窗口结果
- `decider.rs` 决策时，调用 `dynamic_fair_value` 获取动态 FV

### 2. momentum.rs

**数据结构：**
```rust
pub(crate) struct MomentumSignal {
    pub short: Decimal,   // 30s 框架
    pub medium: Decimal,  // 60s 框架
    pub long: Decimal,    // 180s 框架
}

pub(crate) enum MomentumMode {
    AllAligned,  // 三个同向才通过
}
```

**核心方法：**
```rust
// 从现有的 compute_momentum 重构，保持原有签名
pub fn compute_momentum(prices: &[PriceTick], window_secs: u64) -> Decimal;

// 新增：多时间框架计算
pub fn compute_multi_frame_momentum(
    prices: &[PriceTick],
    short_secs: u64,
    medium_secs: u64,
    long_secs: u64,
) -> MomentumSignal;

// 检查动量是否与方向一致
pub fn momentum_aligned(signal: &MomentumSignal, direction: Direction, mode: MomentumMode) -> bool;
```

**对齐逻辑：**
```rust
fn momentum_aligned(signal: &MomentumSignal, direction: Direction, mode: MomentumMode) -> bool {
    match mode {
        MomentumMode::AllAligned => {
            let all_up = signal.short > Decimal::ZERO 
                      && signal.medium > Decimal::ZERO 
                      && signal.long > Decimal::ZERO;
            let all_down = signal.short < Decimal::ZERO 
                        && signal.medium < Decimal::ZERO 
                        && signal.long < Decimal::ZERO;
            
            match direction {
                Direction::Up => all_up,
                Direction::Down => all_down,
            }
        }
    }
}
```

### 3. decider.rs 修改

**配置新增：**
```rust
pub(crate) struct DeciderConfig {
    // 现有字段...
    
    // 新增
    pub min_edge: Decimal,  // 默认 0.05
    
    // 多时间框架动量配置
    pub momentum_short_secs: u64,   // 30
    pub momentum_medium_secs: u64,  // 60
    pub momentum_long_secs: u64,    // 180
    
    // 动态 FV 配置
    pub dynamic_fv_min_samples: usize,  // 20
}
```

**decide() 函数修改：**
```rust
pub(crate) fn decide(
    ctx: &DecideContext, 
    account: &AccountState, 
    cfg: &DeciderConfig,
    btc_history: &BtcHistory,  // 新增参数
) -> Decision {
    // 1-4. 现有检查不变

    // 5. 多时间框架动量 (替换原单一动量)
    let momentum_signal = compute_multi_frame_momentum(
        &ctx.btc_prices,
        cfg.momentum_short_secs,
        cfg.momentum_medium_secs,
        cfg.momentum_long_secs,
    );
    
    if cfg.momentum_filter_enabled {
        if !momentum_aligned(&momentum_signal, base_direction, MomentumMode::AllAligned) {
            return Decision::Pass(format!(
                "momentum_not_aligned_{:+.1}%_{:+.1}%_{:+.1}%",
                momentum_signal.short * 100,
                momentum_signal.medium * 100,
                momentum_signal.long * 100
            ));
        }
    }

    // 6. 动态 Fair Value (替换原固定 FV)
    let effective_fair_value = if cfg.dynamic_fv_enabled {
        btc_history.dynamic_fair_value(cfg.dynamic_fv_min_samples)
            .unwrap_or(cfg.fair_value)
    } else {
        cfg.fair_value
    };

    // 7. Edge 计算 + 最小 Edge 检查 (新增)
    let edge = effective_fair_value - cheap_price;
    
    if edge < cfg.min_edge {
        return Decision::Pass(format!("edge_too_low_{:.1}%", edge * 100));
    }

    // 8. 后续逻辑不变
}
```

**函数签名变更影响：**
- `main.rs` 调用 `decide()` 处需传入 `btc_history`
- `decider.rs` 测试需 mock `BtcHistory`

## 持久化

**main.rs BotState 修改：**
```rust
pub(crate) struct BotState {
    // 现有字段...
    btc_history: BtcHistory,  // 新增
}
```

**保存/恢复：**
- 保存时调用 `btc_history.to_json()` 序列化
- 恢复时调用 `BtcHistory::from_json()` 反序列化

**settler.rs 修改：**
- 结算完成后从 `market_slug` 解析窗口时间
- 调用 `btc_history.record_window()` 记录结果

## 配置变更

```json
{
  "strategy": {
    "extreme_threshold": 0.88,
    "fair_value": 0.5,
    "position_size_usdc": 1.0,
    "min_edge": 0.05,
    "momentum_filter": {
      "enabled": true,
      "short_secs": 30,
      "medium_secs": 60,
      "long_secs": 180
    },
    "dynamic_fair_value": {
      "enabled": true,
      "min_samples": 20
    }
  }
}
```

## 测试计划

1. **btc_history.rs 单元测试**
   - `test_record_window`
   - `test_dynamic_fv_returns_none_when_insufficient_samples`
   - `test_dynamic_fv_computes_correct_ratio`
   - `test_to_json_from_json_roundtrip`

2. **momentum.rs 单元测试**
   - `test_compute_multi_frame_momentum`
   - `test_momentum_aligned_all_up`
   - `test_momentum_aligned_all_down`
   - `test_momentum_not_aligned_mixed`

3. **decider.rs 修改测试**
   - `test_min_edge_rejects_low_edge`
   - `test_min_edge_allows_high_edge`
   - `test_multi_frame_momentum_filter`
   - `test_dynamic_fv_uses_history_when_available`

## 不做的事 (Out of Scope)

- 动态仓位调整 (Kelly Criterion)
- 时间段过滤
- 订单簿深度分析
- Spread 敏感度调整
