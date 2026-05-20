use crate::types::{Quantity, Symbol, Timestamp};

#[derive(Debug, Clone)]
pub struct Position {
    pub symbol: Symbol,
    /// Signed quantity: positive for long, negative for short, zero for flat.
    pub quantity: Quantity,
    pub avg_price: f64,
    /// Timestamp at which the current (non-flat) position was opened. Stale when flat.
    pub opened_at: Timestamp,
}

impl Position {
    pub fn is_long(&self) -> bool {
        self.quantity > 0.0
    }

    pub fn is_short(&self) -> bool {
        self.quantity < 0.0
    }

    pub fn is_flat(&self) -> bool {
        self.quantity.abs() < 1e-12
    }

    pub fn notional(&self, mark_price: f64) -> f64 {
        self.quantity.abs() * mark_price
    }

    pub fn unrealized_pnl(&self, mark_price: f64) -> f64 {
        (mark_price - self.avg_price) * self.quantity
    }
}
