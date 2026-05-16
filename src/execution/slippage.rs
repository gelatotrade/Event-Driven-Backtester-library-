use crate::data::Bar;
use crate::events::OrderEvent;

/// Computes price impact for an order. The result is the *adverse* price
/// movement in absolute terms (always >= 0); the simulator adds it to a buy
/// fill or subtracts it from a sell fill.
pub trait SlippageModel {
    fn slippage(&self, order: &OrderEvent, bar: &Bar) -> f64;
}

/// Zero slippage — useful for unit tests and theoretical upper bounds.
#[derive(Debug, Clone, Copy)]
pub struct ZeroSlippage;

impl SlippageModel for ZeroSlippage {
    fn slippage(&self, _order: &OrderEvent, _bar: &Bar) -> f64 {
        0.0
    }
}

/// Linear slippage as a fraction of price: `bps * price`.
#[derive(Debug, Clone, Copy)]
pub struct LinearSlippage {
    bps: f64,
}

impl LinearSlippage {
    /// `bps` is expressed as a decimal fraction (e.g. 0.0005 = 5 basis points).
    pub fn new(bps: f64) -> Self {
        assert!(bps >= 0.0);
        Self { bps }
    }
}

impl SlippageModel for LinearSlippage {
    fn slippage(&self, _order: &OrderEvent, bar: &Bar) -> f64 {
        bar.close * self.bps
    }
}

/// Square-root market-impact model: `coeff * price * sqrt(quantity / adv_volume)`.
///
/// This matches the Almgren-Chriss style impact widely used for equity/futures
/// execution research. Falls back to a flat term when bar volume is zero.
#[derive(Debug, Clone, Copy)]
pub struct SquareRootSlippage {
    coeff: f64,
    floor_bps: f64,
}

impl SquareRootSlippage {
    pub fn new(coeff: f64, floor_bps: f64) -> Self {
        assert!(coeff >= 0.0 && floor_bps >= 0.0);
        Self { coeff, floor_bps }
    }
}

impl SlippageModel for SquareRootSlippage {
    fn slippage(&self, order: &OrderEvent, bar: &Bar) -> f64 {
        let floor = bar.close * self.floor_bps;
        if bar.volume <= 0.0 {
            return floor;
        }
        let participation = (order.quantity / bar.volume).max(0.0);
        let impact = self.coeff * bar.close * participation.sqrt();
        impact.max(floor)
    }
}
