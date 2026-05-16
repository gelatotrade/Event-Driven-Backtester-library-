use crate::data::Bar;

/// Decides whether the current bar triggers a funding payment, and at what rate.
pub trait FundingModel {
    /// Return `Some(rate)` if funding settles on this bar, else `None`.
    /// Rate is expressed as a decimal fraction applied to notional (e.g. 0.0001 = 1 bps).
    fn funding(&mut self, bar: &Bar) -> Option<f64>;
}

#[derive(Debug, Clone, Copy)]
pub struct NoFunding;

impl FundingModel for NoFunding {
    fn funding(&mut self, _bar: &Bar) -> Option<f64> {
        None
    }
}

/// Funding settles whenever a bar carries a non-`None` `funding_rate`.
///
/// Typical perpetual-futures data has this set at fixed 8h intervals; the
/// model just forwards it.
#[derive(Debug, Clone, Copy)]
pub struct PerpetualFunding;

impl FundingModel for PerpetualFunding {
    fn funding(&mut self, bar: &Bar) -> Option<f64> {
        bar.funding_rate
    }
}
