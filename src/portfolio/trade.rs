use serde::Serialize;

use crate::types::{Side, Symbol, Timestamp};

/// A closed (or partially closed) round-trip trade.
///
/// One is recorded each time a position is reduced or flattened. `pnl` is
/// gross realized PnL for the closed quantity (fees are accounted separately
/// on the portfolio).
#[derive(Debug, Clone, Serialize)]
pub struct Trade {
    pub symbol: Symbol,
    /// Direction of the position that was closed.
    pub side: Side,
    pub entry_time: Timestamp,
    pub exit_time: Timestamp,
    pub entry_price: f64,
    pub exit_price: f64,
    pub quantity: f64,
    pub pnl: f64,
    /// Return on the closed notional, signed by direction.
    pub return_pct: f64,
}

impl Trade {
    pub fn is_win(&self) -> bool {
        self.pnl > 0.0
    }
}
