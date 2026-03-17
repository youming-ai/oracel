//! Stage 2: Signal — detect market extreme pricing.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    Up,
    Down,
}

impl Direction {
    pub fn as_str(&self) -> &'static str {
        match self { Direction::Up => "UP", Direction::Down => "DOWN" }
    }
}

/// Returns true if the market is in an extreme state (one side > threshold).
pub fn is_market_extreme(
    market_yes: Option<f64>,
    market_no: Option<f64>,
    extreme_threshold: f64,
) -> bool {
    let (yes, no) = match (market_yes, market_no) {
        (Some(y), Some(n)) if y > 0.01 && n > 0.01 => (y, n),
        _ => return false,
    };
    let total = yes + no;
    if total <= 0.0 { return false; }
    let mkt_up = yes / total;
    mkt_up > extreme_threshold || mkt_up < (1.0 - extreme_threshold)
}
