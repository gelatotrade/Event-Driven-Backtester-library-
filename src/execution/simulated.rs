use super::{ExecutionHandler, FeeModel, FundingModel, SlippageModel};
use crate::data::Bar;
use crate::error::BacktestError;
use crate::events::{FillEvent, OrderEvent};
use crate::types::{OrderSide, OrderType};

/// Default execution handler — fills market orders at the bar's open with
/// slippage applied, and treats limit orders as maker fills when the bar's
/// range crosses the limit price.
pub struct SimulatedExecutionHandler<S, F, FU>
where
    S: SlippageModel,
    F: FeeModel,
    FU: FundingModel,
{
    pub slippage: S,
    pub fees: F,
    pub funding: FU,
}

impl<S, F, FU> SimulatedExecutionHandler<S, F, FU>
where
    S: SlippageModel,
    F: FeeModel,
    FU: FundingModel,
{
    pub fn new(slippage: S, fees: F, funding: FU) -> Self {
        Self {
            slippage,
            fees,
            funding,
        }
    }
}

impl<S, F, FU> ExecutionHandler for SimulatedExecutionHandler<S, F, FU>
where
    S: SlippageModel,
    F: FeeModel,
    FU: FundingModel,
{
    fn execute(&mut self, order: &OrderEvent, bar: &Bar) -> Result<FillEvent, BacktestError> {
        if order.quantity <= 0.0 {
            return Err(BacktestError::InvalidOrder(
                "order quantity must be positive".into(),
            ));
        }

        let slip = self.slippage.slippage(order, bar);

        let (fill_price, is_maker) = match order.order_type {
            OrderType::Market => {
                // Crossing the spread; slippage moves the fill against us.
                let base = bar.open;
                let price = match order.side {
                    OrderSide::Buy => base + slip,
                    OrderSide::Sell => (base - slip).max(0.0),
                };
                (price, false)
            }
            OrderType::Limit => {
                let limit = order.limit_price.ok_or_else(|| {
                    BacktestError::InvalidOrder("limit order missing limit_price".into())
                })?;
                // Limit fills only when the bar's range touches the limit price.
                let touched = match order.side {
                    OrderSide::Buy => bar.low <= limit,
                    OrderSide::Sell => bar.high >= limit,
                };
                if !touched {
                    return Err(BacktestError::InvalidOrder(
                        "limit order not triggered on this bar".into(),
                    ));
                }
                (limit, true)
            }
        };

        let commission = self.fees.fee(order, fill_price, is_maker);

        Ok(FillEvent {
            timestamp: order.timestamp,
            symbol: order.symbol.clone(),
            side: order.side,
            quantity: order.quantity,
            fill_price,
            commission,
            slippage_cost: slip * order.quantity,
            is_maker,
        })
    }
}

impl<S, F, FU> SimulatedExecutionHandler<S, F, FU>
where
    S: SlippageModel,
    F: FeeModel,
    FU: FundingModel,
{
    /// Forward to the inner funding model so the engine can apply funding payments.
    pub fn funding_for(&mut self, bar: &Bar) -> Option<f64> {
        self.funding.funding(bar)
    }
}
