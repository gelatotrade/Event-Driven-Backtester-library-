//! Portfolio state, position tracking and signal-to-order conversion.

mod position;
mod trade;

pub use position::Position;
pub use trade::Trade;

use std::collections::HashMap;

use serde::Serialize;

use crate::data::DataHandler;
use crate::error::BacktestError;
use crate::events::{FillEvent, OrderEvent, SignalDirection, SignalEvent};
use crate::types::{OrderSide, OrderType, Side, Symbol, Timestamp};

/// Equity-curve sample.
#[derive(Debug, Clone, Copy, Serialize)]
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
    trades: Vec<Trade>,
    /// Fraction of equity to allocate per position when sizing from signals.
    risk_per_trade: f64,
    /// Hard cap on gross leverage (gross notional / equity).
    max_leverage: f64,
    /// Liquidate when equity falls below `maintenance_margin_rate * gross_notional`.
    /// Zero disables liquidation.
    maintenance_margin_rate: f64,
    /// Annualized financing cost charged on short notional per bar. Zero disables.
    annual_borrow_rate: f64,
    /// Fee charged on gross notional when a liquidation flattens the book.
    liquidation_fee_rate: f64,
    /// Allow short positions.
    allow_short: bool,
    liquidated: bool,
    realized_pnl: f64,
    total_fees: f64,
    total_funding: f64,
    total_borrow: f64,
    total_slippage: f64,
    fills: usize,
}

impl Portfolio {
    pub fn new(initial_cash: f64) -> Self {
        Self {
            initial_cash,
            cash: initial_cash,
            positions: HashMap::new(),
            last_prices: HashMap::new(),
            equity_curve: Vec::new(),
            trades: Vec::new(),
            risk_per_trade: 1.0,
            max_leverage: 1.0,
            maintenance_margin_rate: 0.0,
            annual_borrow_rate: 0.0,
            liquidation_fee_rate: 0.0,
            allow_short: true,
            liquidated: false,
            realized_pnl: 0.0,
            total_fees: 0.0,
            total_funding: 0.0,
            total_borrow: 0.0,
            total_slippage: 0.0,
            fills: 0,
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

    /// Enable liquidation: the book is force-flattened when account equity
    /// drops below `rate * gross_notional`. `rate` is the maintenance margin
    /// fraction (e.g. 0.005 = 0.5%, allowing up to 200x before liquidation).
    pub fn with_maintenance_margin(mut self, rate: f64) -> Self {
        assert!(rate >= 0.0);
        self.maintenance_margin_rate = rate;
        self
    }

    /// Charge an annualized financing cost on short notional, applied pro-rata
    /// each bar. Models securities-borrow / margin financing on shorts.
    pub fn with_annual_borrow_rate(mut self, rate: f64) -> Self {
        assert!(rate >= 0.0);
        self.annual_borrow_rate = rate;
        self
    }

    /// Fee charged on gross notional at liquidation (e.g. 0.005 = 50 bps penalty).
    pub fn with_liquidation_fee(mut self, rate: f64) -> Self {
        assert!(rate >= 0.0);
        self.liquidation_fee_rate = rate;
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

    /// Sum of absolute position notionals at current marks.
    pub fn gross_notional(&self) -> f64 {
        self.positions
            .iter()
            .map(|(sym, pos)| {
                let mark = self.last_prices.get(sym).copied().unwrap_or(pos.avg_price);
                pos.notional(mark)
            })
            .sum()
    }

    fn gross_notional_excluding(&self, symbol: &str) -> f64 {
        self.positions
            .iter()
            .filter(|(sym, _)| sym.as_str() != symbol)
            .map(|(sym, pos)| {
                let mark = self.last_prices.get(sym).copied().unwrap_or(pos.avg_price);
                pos.notional(mark)
            })
            .sum()
    }

    pub fn position(&self, symbol: &str) -> Option<&Position> {
        self.positions.get(symbol)
    }

    pub fn equity_curve(&self) -> &[EquityPoint] {
        &self.equity_curve
    }

    pub fn trades(&self) -> &[Trade] {
        &self.trades
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

    pub fn total_borrow(&self) -> f64 {
        self.total_borrow
    }

    pub fn total_slippage(&self) -> f64 {
        self.total_slippage
    }

    /// Number of executed fills (not round-trip trades; see [`Portfolio::trades`]).
    pub fn fills(&self) -> usize {
        self.fills
    }

    pub fn is_liquidated(&self) -> bool {
        self.liquidated
    }

    /// Update the latest mark price for a symbol without snapshotting equity.
    pub fn update_mark(&mut self, symbol: &str, price: f64) {
        self.last_prices.insert(symbol.to_string(), price);
    }

    /// Append a point to the equity curve at the current marks.
    pub fn snapshot_equity(&mut self, timestamp: Timestamp) {
        let position_value = self.position_value();
        self.equity_curve.push(EquityPoint {
            timestamp,
            equity: self.cash + position_value,
            cash: self.cash,
            position_value,
        });
    }

    /// Update the mark and snapshot equity in one step.
    pub fn mark_to_market(&mut self, timestamp: Timestamp, symbol: &str, price: f64) {
        self.update_mark(symbol, price);
        self.snapshot_equity(timestamp);
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

    /// Charge one bar of financing cost on short notional.
    pub fn apply_borrow_cost(&mut self, periods_per_year: f64) {
        if self.annual_borrow_rate <= 0.0 || periods_per_year <= 0.0 {
            return;
        }
        let per_bar = self.annual_borrow_rate / periods_per_year;
        let mut cost = 0.0;
        for (sym, pos) in &self.positions {
            if pos.quantity < 0.0 {
                let mark = self.last_prices.get(sym).copied().unwrap_or(pos.avg_price);
                cost += pos.notional(mark) * per_bar;
            }
        }
        if cost > 0.0 {
            self.cash -= cost;
            self.total_borrow += cost;
        }
    }

    /// If liquidation is enabled and equity has fallen below the maintenance
    /// requirement, flatten every position at current marks and return `true`.
    pub fn check_and_liquidate(&mut self, timestamp: Timestamp) -> bool {
        if self.liquidated || self.maintenance_margin_rate <= 0.0 {
            return false;
        }
        let gross = self.gross_notional();
        if gross <= 0.0 {
            return false;
        }
        let required = gross * self.maintenance_margin_rate;
        if self.equity() >= required {
            return false;
        }

        let to_close: Vec<(Symbol, f64, f64)> = self
            .positions
            .iter()
            .filter(|(_, p)| !p.is_flat())
            .map(|(sym, p)| {
                let mark = self.last_prices.get(sym).copied().unwrap_or(p.avg_price);
                (sym.clone(), p.quantity, mark)
            })
            .collect();

        for (sym, qty, mark) in to_close {
            let side = if qty > 0.0 {
                OrderSide::Sell
            } else {
                OrderSide::Buy
            };
            let fee = qty.abs() * mark * self.liquidation_fee_rate;
            self.on_fill(&FillEvent {
                timestamp,
                symbol: sym,
                side,
                quantity: qty.abs(),
                fill_price: mark,
                commission: fee,
                slippage_cost: 0.0,
                is_maker: false,
            });
        }

        self.liquidated = true;
        true
    }

    /// Convert a signal into a target order. Returns `None` if no trade is required.
    pub fn on_signal(
        &self,
        signal: &SignalEvent,
        data: &dyn DataHandler,
    ) -> Result<Option<OrderEvent>, BacktestError> {
        if self.liquidated {
            return Ok(None);
        }
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
            SignalDirection::Long => {
                self.target_quantity(&signal.symbol, price, signal.strength, 1.0)
            }
            SignalDirection::Short => {
                if !self.allow_short {
                    0.0
                } else {
                    self.target_quantity(&signal.symbol, price, signal.strength, -1.0)
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

    /// Target signed quantity for `symbol`, capped so that total gross exposure
    /// (this symbol plus all others) stays within `max_leverage`.
    fn target_quantity(&self, symbol: &str, price: f64, strength: f64, sign: f64) -> f64 {
        let strength = strength.clamp(0.0, 1.0);
        let equity = self.equity();
        let allocation = equity * self.risk_per_trade * strength;
        let max_gross = (equity * self.max_leverage).max(0.0);
        let other_notional = self.gross_notional_excluding(symbol);
        let available = (max_gross - other_notional).max(0.0);
        let notional = allocation.min(available);
        sign * (notional / price.max(1e-12))
    }

    /// Apply a fill, updating cash, positions, realized PnL and the trade log.
    pub fn on_fill(&mut self, fill: &FillEvent) {
        let sign = fill.side.sign();
        let signed_qty = sign * fill.quantity;
        let cost = fill.fill_price * fill.quantity;

        let entry = self
            .positions
            .entry(fill.symbol.clone())
            .or_insert(Position {
                symbol: fill.symbol.clone(),
                quantity: 0.0,
                avg_price: 0.0,
                opened_at: fill.timestamp,
            });

        let old_qty = entry.quantity;
        let old_avg = entry.avg_price;
        let old_opened = entry.opened_at;
        let new_qty = old_qty + signed_qty;

        // The portion of this fill that closes/reduces an existing position.
        let closing_qty = if old_qty != 0.0 && old_qty.signum() != signed_qty.signum() {
            old_qty.abs().min(signed_qty.abs())
        } else {
            0.0
        };
        if closing_qty > 0.0 {
            let dir = old_qty.signum();
            let pnl = (fill.fill_price - old_avg) * closing_qty * dir;
            self.realized_pnl += pnl;
            let return_pct = if old_avg.abs() > 1e-12 {
                (fill.fill_price - old_avg) / old_avg * dir
            } else {
                0.0
            };
            self.trades.push(Trade {
                symbol: fill.symbol.clone(),
                side: if dir > 0.0 { Side::Long } else { Side::Short },
                entry_time: old_opened,
                exit_time: fill.timestamp,
                entry_price: old_avg,
                exit_price: fill.fill_price,
                quantity: closing_qty,
                pnl,
                return_pct,
            });
        }

        if new_qty.abs() < 1e-12 {
            // Flat.
            entry.quantity = 0.0;
            entry.avg_price = 0.0;
        } else if old_qty != 0.0 && old_qty.signum() == new_qty.signum() {
            if old_qty.signum() == signed_qty.signum() {
                // Scaling in — volume-weight the average price.
                entry.avg_price =
                    (old_avg * old_qty.abs() + fill.fill_price * fill.quantity) / new_qty.abs();
            }
            // Partial reduction leaves the average price unchanged.
            entry.quantity = new_qty;
        } else {
            // Opened from flat, or flipped through zero — remainder starts fresh.
            entry.avg_price = fill.fill_price;
            entry.quantity = new_qty;
            entry.opened_at = fill.timestamp;
        }

        self.cash -= sign * cost;
        self.cash -= fill.commission;
        self.total_fees += fill.commission;
        self.total_slippage += fill.slippage_cost;
        self.fills += 1;
    }
}
