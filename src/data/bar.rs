use crate::types::{Price, Symbol, Timestamp};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bar {
    pub timestamp: Timestamp,
    pub symbol: Symbol,
    pub open: Price,
    pub high: Price,
    pub low: Price,
    pub close: Price,
    pub volume: f64,
    /// Funding rate for perpetuals (per funding interval, e.g. 8h). `None` for spot.
    pub funding_rate: Option<f64>,
}

impl Bar {
    pub fn new(
        timestamp: Timestamp,
        symbol: impl Into<Symbol>,
        open: Price,
        high: Price,
        low: Price,
        close: Price,
        volume: f64,
    ) -> Self {
        Self {
            timestamp,
            symbol: symbol.into(),
            open,
            high,
            low,
            close,
            volume,
            funding_rate: None,
        }
    }

    pub fn with_funding(mut self, rate: f64) -> Self {
        self.funding_rate = Some(rate);
        self
    }

    /// Bar range — useful for volatility-aware slippage models.
    pub fn range(&self) -> f64 {
        self.high - self.low
    }

    pub fn typical_price(&self) -> Price {
        (self.high + self.low + self.close) / 3.0
    }
}
