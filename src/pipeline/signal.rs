//! Stage 2: Signal Computer
//! Market extreme detection for Polymarket 5m prediction markets.
//!
//! Core insight: We don't predict BTC price direction. Instead, we detect
//! when the market is EXTREMELY overconfident in one direction, then bet
//! against it. This is market sentiment arbitrage, not price prediction.
//!
//! BTC price trend is used as a FILTER to avoid betting against
//! strong momentum (trend confirmation, not prediction).

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    Up,
    Down,
}

impl Direction {
    pub fn as_str(&self) -> &'static str {
        match self {
            Direction::Up => "UP",
            Direction::Down => "DOWN",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Signal {
    pub p_up: f64,
    pub p_down: f64,
    pub direction: Direction,
    pub confidence: f64,
}

/// BTC price trend info for filtering
#[derive(Debug, Clone)]
pub struct TrendInfo {
    pub btc_change_pct: f64,
    pub clear_trend: bool,
    pub strong_trend: bool,
    pub trend_direction: f64,
    pub timeframe_alignment: u8,
}

pub fn compute_trend(prices: &[f64]) -> Option<TrendInfo> {
    if prices.len() < 30 {
        return None;
    }

    let len = prices.len();

    let changes = [
        calc_change(prices, len, 15),   // ~30s
        calc_change(prices, len, 30),   // ~1m
        calc_change(prices, len, 60),   // ~2m
        calc_change(prices, len, 120),  // ~4m
    ];

    let weights = [0.15, 0.25, 0.35, 0.25];
    let weighted_trend: f64 = changes.iter().zip(weights.iter())
        .map(|(c, w)| c * w)
        .sum();

    let up_count = changes.iter().filter(|&&c| c > 0.00005).count();
    let down_count = changes.iter().filter(|&&c| c < -0.00005).count();
    let alignment = up_count.max(down_count) as u8;

    // Clear trend: 2+ timeframes agree AND weighted trend > 0.02%
    let clear_trend = alignment >= 2 && weighted_trend.abs() > 0.0002;
    // Strong trend: 3+ timeframes agree AND weighted trend > 0.05%
    let strong_trend = alignment >= 3 && weighted_trend.abs() > 0.0005;

    Some(TrendInfo {
        btc_change_pct: weighted_trend * 100.0,
        clear_trend,
        strong_trend,
        trend_direction: weighted_trend,
        timeframe_alignment: alignment,
    })
}

fn calc_change(prices: &[f64], len: usize, lookback: usize) -> f64 {
    let lb = lookback.min(len - 1);
    if lb < 5 { return 0.0; }
    let past = prices[len - 1 - lb];
    let current = prices[len - 1];
    if past > 0.0 { (current - past) / past } else { 0.0 }
}

/// Compute signal based on MARKET PRICES (not BTC price).
pub fn compute_signal(
    _prices: &[f64],
    market_yes: Option<f64>,
    market_no: Option<f64>,
) -> Option<Signal> {
    let (yes, no) = match (market_yes, market_no) {
        (Some(y), Some(n)) if y > 0.01 && n > 0.01 => (y, n),
        _ => return None,
    };

    let total = yes + no;
    if total <= 0.0 { return None; }

    let mkt_up = yes / total;

    const EXTREME_THRESHOLD: f64 = 0.80;
    const BASE_PROB: f64 = 0.50;

    let (p_up, confidence) = if mkt_up > EXTREME_THRESHOLD {
        let edge = BASE_PROB - (1.0 - mkt_up);
        let p = BASE_PROB - edge * 0.8;
        (p.clamp(0.10, 0.50), edge * 2.0)
    } else if mkt_up < (1.0 - EXTREME_THRESHOLD) {
        let edge = BASE_PROB - mkt_up;
        let p = BASE_PROB + edge * 0.8;
        (p.clamp(0.50, 0.90), edge * 2.0)
    } else {
        (0.50, 0.0)
    };

    let p_down = 1.0 - p_up;
    let direction = if p_up > 0.5 { Direction::Up } else { Direction::Down };

    Some(Signal {
        p_up,
        p_down,
        direction,
        confidence: confidence.clamp(0.0, 1.0),
    })
}
