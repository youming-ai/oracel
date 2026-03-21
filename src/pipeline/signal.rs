//! Signal types — trade direction.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
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
