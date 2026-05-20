use crate::events::OrderEvent;

/// Compute fees paid for a given fill notional.
pub trait FeeModel {
    fn fee(&self, order: &OrderEvent, fill_price: f64, is_maker: bool) -> f64;
}

/// Constant per-trade commission, regardless of size or maker/taker status.
#[derive(Debug, Clone, Copy)]
pub struct FixedFees {
    per_trade: f64,
}

impl FixedFees {
    pub fn new(per_trade: f64) -> Self {
        assert!(per_trade >= 0.0);
        Self { per_trade }
    }
}

impl FeeModel for FixedFees {
    fn fee(&self, _order: &OrderEvent, _fill_price: f64, _is_maker: bool) -> f64 {
        self.per_trade
    }
}

/// Different fee rates for makers vs takers, expressed as a fraction of notional.
#[derive(Debug, Clone, Copy)]
pub struct MakerTakerFees {
    maker: f64,
    taker: f64,
}

impl MakerTakerFees {
    pub fn new(maker: f64, taker: f64) -> Self {
        assert!(maker >= 0.0 && taker >= 0.0);
        Self { maker, taker }
    }
}

impl FeeModel for MakerTakerFees {
    fn fee(&self, order: &OrderEvent, fill_price: f64, is_maker: bool) -> f64 {
        let notional = order.quantity * fill_price;
        let rate = if is_maker { self.maker } else { self.taker };
        notional * rate
    }
}
