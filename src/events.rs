use crate::data::Bar;
use crate::types::{OrderSide, OrderType, Price, Quantity, Symbol, Timestamp};
use serde::{Deserialize, Serialize};

/// All events that flow through the engine.
#[derive(Debug, Clone)]
pub enum Event {
    Market(MarketEvent),
    Signal(SignalEvent),
    Order(OrderEvent),
    Fill(FillEvent),
}

impl Event {
    pub fn timestamp(&self) -> Timestamp {
        match self {
            Event::Market(e) => e.timestamp,
            Event::Signal(e) => e.timestamp,
            Event::Order(e) => e.timestamp,
            Event::Fill(e) => e.timestamp,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MarketEvent {
    pub timestamp: Timestamp,
    pub bar: Bar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignalDirection {
    /// Open or maintain a long position.
    Long,
    /// Open or maintain a short position.
    Short,
    /// Close any existing position.
    Exit,
    /// No action.
    Hold,
}

#[derive(Debug, Clone)]
pub struct SignalEvent {
    pub timestamp: Timestamp,
    pub symbol: Symbol,
    pub direction: SignalDirection,
    /// Strategy confidence in [0, 1]; portfolio uses this to size positions.
    pub strength: f64,
}

#[derive(Debug, Clone)]
pub struct OrderEvent {
    pub timestamp: Timestamp,
    pub symbol: Symbol,
    pub side: OrderSide,
    pub quantity: Quantity,
    pub order_type: OrderType,
    pub limit_price: Option<Price>,
}

#[derive(Debug, Clone)]
pub struct FillEvent {
    pub timestamp: Timestamp,
    pub symbol: Symbol,
    pub side: OrderSide,
    pub quantity: Quantity,
    pub fill_price: Price,
    pub commission: f64,
    pub slippage_cost: f64,
    /// True if filled as maker (passive); false if taker (aggressive).
    pub is_maker: bool,
}

impl FillEvent {
    /// Signed notional value (positive for buys, negative for sells), excluding costs.
    pub fn signed_notional(&self) -> f64 {
        self.side.sign() * self.quantity * self.fill_price
    }
}
