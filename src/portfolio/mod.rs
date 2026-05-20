//! Portfolio state, position tracking and signal-to-order conversion.

mod position;

pub use position::Position;

use std::collections::HashMap;

use crate::data::DataHandler;
use crate::error::BacktestError;
use crate::events::{FillEvent, OrderEvent, SignalDirection, SignalEvent};
use crate::types::{OrderSide, OrderType, Symbol, Timestamp};

/// Equity-curve sample.
#[derive(Debug, Clone, Copy)]
pub struct EquityPoint {
    pub timestamp: Timestamp,
    pub equity: f64,
    pub cash: f64,
    pub position_value: f64,
}

pub struct Portfolio {
    initial_cash: f64,
    cash: f64,
    positions: HashMap<Symbol, Position>,
    last_prices: HashMap<Symbol, f64>,
    equity_curve: Vec<EquityPoint>,
    /// Fraction of equity to allocate per position when sizing from signals.
    risk_per_trade: f64,
    /// Hard cap on absolute leverage (gross notional / equity).
    max_leverage: f64,
    /// Allow short positions.
    allow_short: bool,
    realized_pnl: f64,
    total_fees: f64,
    total_funding: f64,
    total_slippage: f64,
    trade_count: usize,
}

impl Portfolio {
    pub fn new(initial_cash: f64) -> Self {
        Self {
            initial_cash,
            cash: initial_cash,
            positions: HashMap::new(),
            last_prices: HashMap::new(),
            equity_curve: Vec::new(),
            risk_per_trade: 1.0,
            max_leverage: 1.0,
            allow_short: true,
            realized_pnl: 0.0,
            total_fees: 0.0,
            total_funding: 0.0,
            total_slippage: 0.0,
            trade_count: 0,
        }
    }

    pub fn with_risk_per_trade(mut self, fraction: f64) -> Self {
        assert!(fraction > 0.0 && fraction <= 1.0);
        self.risk_per_trade = fraction;
        self
    }

    pub fn with_max_leverage(mut self, leverage: f64) -> Self {
        assert!(leverage > 0.0);
        self.max_leverage = leverage;
        self
    }

    pub fn allow_short(mut self, allow: bool) -> Self {
        self.allow_short = allow;
        self
    }

    pub fn cash(&self) -> f64 {
        self.cash
    }

    pub fn equity(&self) -> f64 {
        self.cash + self.position_value()
    }

    pub fn position_value(&self) -> f64 {
        self.positions
            .iter()
            .map(|(sym, pos)| {
                let mark = self.last_prices.get(sym).copied().unwrap_or(pos.avg_price);
                pos.quantity * mark
            })
            .sum()
    }

    pub fn position(&self, symbol: &str) -> Option<&Position> {
        self.positions.get(symbol)
    }

    pub fn equity_curve(&self) -> &[EquityPoint] {
        &self.equity_curve
    }

    pub fn initial_cash(&self) -> f64 {
        self.initial_cash
    }

    pub fn realized_pnl(&self) -> f64 {
        self.realized_pnl
    }

    pub fn total_fees(&self) -> f64 {
        self.total_fees
    }

    pub fn total_funding(&self) -> f64 {
        self.total_funding
    }

    pub fn total_slippage(&self) -> f64 {
        self.total_slippage
    }

    pub fn trade_count(&self) -> usize {
        self.trade_count
    }

    /// Mark portfolio to market using the latest bar and snapshot the equity curve.
    pub fn mark_to_market(&mut self, timestamp: Timestamp, symbol: &str, price: f64) {
        self.last_prices.insert(symbol.to_string(), price);
        let position_value = self.position_value();
        self.equity_curve.push(EquityPoint {
            timestamp,
            equity: self.cash + position_value,
            cash: self.cash,
            position_value,
        });
    }

    /// Apply perpetual funding payment for the given symbol.
    ///
    /// Longs pay positive funding to shorts; signs flip when funding is negative.
    pub fn apply_funding(&mut self, symbol: &str, funding_rate: f64) {
        let Some(pos) = self.positions.get(symbol) else {
            return;
        };
        let Some(mark) = self.last_prices.get(symbol).copied() else {
            return;
        };
        let notional = pos.quantity * mark;
        let payment = notional * funding_rate;
        self.cash -= payment;
        self.total_funding += payment;
    }

    /// Convert a signal into a target order. Returns `None` if no trade is required.
    pub fn on_signal(
        &self,
        signal: &SignalEvent,
        data: &dyn DataHandler,
    ) -> Result<Option<OrderEvent>, BacktestError> {
        let bar = data
            .current_bar(&signal.symbol)
            .ok_or_else(|| BacktestError::UnknownSymbol(signal.symbol.clone()))?;
        let price = bar.close;

        let current_qty = self
            .positions
            .get(&signal.symbol)
            .map(|p| p.quantity)
            .unwrap_or(0.0);

        let target_qty = match signal.direction {
            SignalDirection::Hold => return Ok(None),
            SignalDirection::Exit => 0.0,
            SignalDirection::Long => self.target_quantity(price, signal.strength, 1.0),
            SignalDirection::Short => {
                if !self.allow_short {
                    0.0
                } else {
                    self.target_quantity(price, signal.strength, -1.0)
                }
            }
        };

        let delta = target_qty - current_qty;
        if delta.abs() < 1e-9 {
            return Ok(None);
        }

        let side = if delta > 0.0 {
            OrderSide::Buy
        } else {
            OrderSide::Sell
        };

        Ok(Some(OrderEvent {
            timestamp: signal.timestamp,
            symbol: signal.symbol.clone(),
            side,
            quantity: delta.abs(),
            order_type: OrderType::Market,
            limit_price: None,
        }))
    }

    fn target_quantity(&self, price: f64, strength: f64, sign: f64) -> f64 {
        let strength = strength.clamp(0.0, 1.0);
        let equity = self.equity();
        let allocation = equity * self.risk_per_trade * strength;
        let leveraged_cap = equity * self.max_leverage;
        let notional = allocation.min(leveraged_cap);
        sign * (notional / price.max(1e-12))
    }

    /// Apply a fill, updating cash, positions and realized PnL.
    pub fn on_fill(&mut self, fill: &FillEvent) {
        let sign = fill.side.sign();
        let signed_qty = sign * fill.quantity;
        let cost = fill.fill_price * fill.quantity;

        let entry = self.positions.entry(fill.symbol.clone()).or_insert(Position {
            symbol: fill.symbol.clone(),
            quantity: 0.0,
            avg_price: 0.0,
        });

        let old_qty = entry.quantity;
        let new_qty = old_qty + signed_qty;

        // Realized PnL: only the portion of the trade that closes/reduces an existing position.
        let closing_qty = if old_qty.signum() != signed_qty.signum() && old_qty != 0.0 {
            old_qty.abs().min(signed_qty.abs())
        } else {
            0.0
        };
        if closing_qty > 0.0 {
            let pnl = (fill.fill_price - entry.avg_price) * closing_qty * old_qty.signum();
            self.realized_pnl += pnl;
        }

        if new_qty.abs() < 1e-12 {
            entry.quantity = 0.0;
            entry.avg_price = 0.0;
        } else if old_qty.signum() == new_qty.signum() && old_qty != 0.0 {
            // Adding to existing position; volume-weight the average price.
            entry.avg_price =
                (entry.avg_price * old_qty.abs() + fill.fill_price * fill.quantity) / new_qty.abs();
            entry.quantity = new_qty;
        } else {
            // Flipped through zero — remainder starts fresh at fill price.
            entry.avg_price = fill.fill_price;
            entry.quantity = new_qty;
        }

        self.cash -= sign * cost;
        self.cash -= fill.commission;
        self.total_fees += fill.commission;
        self.total_slippage += fill.slippage_cost;
        self.trade_count += 1;
    }
}
