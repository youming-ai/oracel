//! Stage 2: Signal — detect market extreme pricing.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Direction {
    Up,
    Down,
}

impl Direction {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Direction::Up => "UP",
            Direction::Down => "DOWN",
        }
    }
}

/// Returns true if the market is in an extreme state (one side > threshold).
pub(crate) fn is_market_extreme(
    market_yes: Option<f64>,
    market_no: Option<f64>,
    extreme_threshold: f64,
) -> bool {
    let (yes, no) = match (market_yes, market_no) {
        (Some(y), Some(n)) if y > 0.01 && n > 0.01 => (y, n),
        _ => return false,
    };
    let total = yes + no;
    if total <= 0.0 {
        return false;
    }
    let mkt_up = yes / total;
    mkt_up > extreme_threshold || mkt_up < (1.0 - extreme_threshold)
}

#[cfg(test)]
mod tests {
    use super::is_market_extreme;

    #[test]
    fn test_extreme_bullish() {
        assert!(is_market_extreme(Some(0.85), Some(0.15), 0.80));
    }

    #[test]
    fn test_extreme_bearish() {
        assert!(is_market_extreme(Some(0.15), Some(0.85), 0.80));
    }

    #[test]
    fn test_not_extreme() {
        assert!(!is_market_extreme(Some(0.55), Some(0.45), 0.80));
    }

    #[test]
    fn test_missing_data() {
        assert!(!is_market_extreme(None, Some(0.45), 0.80));
        assert!(!is_market_extreme(Some(0.55), None, 0.80));
        assert!(!is_market_extreme(None, None, 0.80));
    }

    #[test]
    fn test_zero_prices() {
        assert!(!is_market_extreme(Some(0.0), Some(0.85), 0.80));
        assert!(!is_market_extreme(Some(0.85), Some(0.0), 0.80));
        assert!(!is_market_extreme(Some(0.0), Some(0.0), 0.80));
    }
}
