# event_driven_backtester

Event-driven backtesting library in Rust. Built as the shared infrastructure
layer that strategy repos can plug into — so a portfolio of notebooks becomes
a single engineering stack.

## Why event-driven

A vectorized backtester gives you a fast equity curve and a lot of look-ahead
bugs. An event-driven backtester mirrors a live trading loop:

```
MarketEvent  ->  Strategy   ->  SignalEvent
SignalEvent  ->  Portfolio  ->  OrderEvent
OrderEvent   ->  Execution  ->  FillEvent
FillEvent    ->  Portfolio
```

Same strategy code, same data path, same fill semantics — historical or live.

## Feature overview

- **Pluggable components** behind traits: `DataHandler`, `Strategy`,
  `ExecutionHandler`, `SlippageModel`, `FeeModel`, `FundingModel`.
- **Realistic execution**: market + limit orders, maker/taker fees,
  linear and square-root slippage, perpetual funding.
- **Portfolio engine**: long/short positions, leverage cap, risk-per-trade
  sizing, realized vs unrealized PnL, full equity curve.
- **Walk-forward analysis** with rolling or expanding in-sample windows
  and stitched OOS metrics.
- **Performance metrics**: Sharpe, Sortino, Calmar, max drawdown
  (depth + duration), profit factor, win rate, total/annualized return.
- **CSV ingestion** (RFC3339 or unix-ms timestamps) plus an in-memory
  source for tests and synthetic data.
- **Zero `unsafe`**, MIT licensed, no async runtime required.

## Install

```toml
[dependencies]
event_driven_backtester = { git = "https://github.com/gelatotrade/event-driven-backtester-library-" }
```

## Quick start

```rust
use event_driven_backtester::prelude::*;

fn main() -> Result<(), BacktestError> {
    let data = CsvDataHandler::from_path("btc_1d.csv")?;
    let strategy = MovingAverageCrossover::new("BTCUSDT", 10, 30);
    let portfolio = Portfolio::new(100_000.0)
        .with_risk_per_trade(0.5)
        .with_max_leverage(1.0);
    let execution = SimulatedExecutionHandler::new(
        LinearSlippage::new(0.0005),         // 5 bps
        MakerTakerFees::new(0.0002, 0.0005), // 2 / 5 bps
        NoFunding,
    );

    let mut engine = BacktestEngine::new(data, strategy, portfolio, execution);
    let result = engine.run()?;

    println!("Sharpe   : {:.2}", result.metrics.sharpe);
    println!("Max DD   : {:.2}%", result.metrics.max_drawdown * 100.0);
    println!("Trades   : {}", result.trade_count);
    Ok(())
}
```

### CSV format

Either an RFC3339 `timestamp` column or `timestamp_ms` (unix ms) — pick one:

```csv
timestamp,symbol,open,high,low,close,volume,funding_rate
2024-01-01T00:00:00Z,BTCUSDT,42000,42500,41800,42200,1234.5,
2024-01-02T00:00:00Z,BTCUSDT,42200,42900,42100,42700,1500.0,
```

`funding_rate` is optional and only consumed when paired with
`PerpetualFunding` in the execution handler.

## Writing a strategy

```rust
use event_driven_backtester::prelude::*;
use event_driven_backtester::data::DataHandler;
use event_driven_backtester::events::{MarketEvent, SignalEvent, SignalDirection};
use event_driven_backtester::strategy::Strategy;

pub struct ZScoreReversion {
    symbol: String,
    window: usize,
    entry_z: f64,
}

impl Strategy for ZScoreReversion {
    fn name(&self) -> &str { "z-score-reversion" }

    fn on_market(&mut self, ev: &MarketEvent, data: &dyn DataHandler) -> Vec<SignalEvent> {
        let hist = data.history(&self.symbol, self.window);
        if hist.len() < self.window { return vec![]; }

        let mean = hist.iter().map(|b| b.close).sum::<f64>() / hist.len() as f64;
        let var  = hist.iter().map(|b| (b.close - mean).powi(2)).sum::<f64>() / hist.len() as f64;
        let std  = var.sqrt();
        if std == 0.0 { return vec![]; }

        let z = (ev.bar.close - mean) / std;
        let dir = if z >  self.entry_z { SignalDirection::Short }
                  else if z < -self.entry_z { SignalDirection::Long }
                  else { SignalDirection::Exit };

        vec![SignalEvent {
            timestamp: ev.timestamp,
            symbol: self.symbol.clone(),
            direction: dir,
            strength: (z.abs() / self.entry_z).min(1.0),
        }]
    }
}
```

`strength` (0..1) feeds position sizing: the portfolio scales the
`risk_per_trade` allocation by it.

## Execution models

| Component    | Implementations                                                  |
|--------------|------------------------------------------------------------------|
| Slippage     | `ZeroSlippage`, `LinearSlippage`, `SquareRootSlippage`           |
| Fees         | `FixedFees`, `MakerTakerFees`                                    |
| Funding      | `NoFunding`, `PerpetualFunding`                                  |

`SquareRootSlippage` follows the Almgren-Chriss style impact:
`impact = coeff * price * sqrt(quantity / bar_volume)`, with a configurable
basis-point floor for thin or zero-volume bars.

`PerpetualFunding` reads `Bar::funding_rate` and lets the portfolio apply
the payment to the open notional — longs pay when the rate is positive,
shorts pay when negative.

## Walk-forward analysis

```rust
let wf = WalkForward::new(90, 30).with_mode(WindowMode::Rolling);

let result = wf.run(
    bars,
    || MovingAverageCrossover::new("BTCUSDT", 5, 15),
    || Portfolio::new(100_000.0).with_risk_per_trade(0.5),
    || SimulatedExecutionHandler::new(
        LinearSlippage::new(0.0005),
        MakerTakerFees::new(0.0002, 0.0005),
        NoFunding,
    ),
)?;

for fold in &result.folds {
    println!("fold {}: IS Sharpe {:.2}  OOS Sharpe {:.2}",
        fold.fold,
        fold.in_sample.metrics.sharpe,
        fold.out_sample.metrics.sharpe);
}
println!("aggregate OOS Sharpe: {:.2}", result.aggregate_oos.sharpe);
```

Each fold gets a fresh strategy/portfolio/execution from the factories you
pass in, so state never leaks across windows. The aggregate is built by
stitching the per-fold OOS equity curves multiplicatively.

## Examples

```bash
cargo run --release --example ma_crossover      # MA crossover on synthetic spot data
cargo run --release --example perpetual_funding # ETHPERP with 8h funding payments
cargo run --release --example walk_forward      # Rolling 90/60 walk-forward
```

## Project layout

```
src/
├── lib.rs              # crate root + prelude
├── types.rs            # Side / OrderSide / OrderType / Price / Symbol
├── events.rs           # Market / Signal / Order / Fill events
├── error.rs            # BacktestError
├── engine.rs           # event loop + BacktestResult
├── walkforward.rs      # rolling / expanding walk-forward
├── data/               # DataHandler trait, CSV + in-memory sources
├── strategy/           # Strategy trait + MA crossover reference
├── portfolio/          # positions, sizing, equity curve
├── execution/          # simulated handler, slippage / fee / funding models
└── metrics/            # Sharpe / Sortino / Calmar / max-DD / win rate
```

## Status

This crate is the engine layer for downstream strategy repos. The Phase 1
goals — event loop, realistic costs, walk-forward, metrics — are in place
and covered by integration tests. Phase 2 candidates:

- Multi-asset portfolio with cross-asset position netting
- Limit-order book simulation for higher-frequency strategies
- Parallel walk-forward and parameter-grid search
- Pluggable risk-management overlays (max-drawdown stops, vol targeting)
- Live trading adapter (Binance / Hyperliquid) reusing the same traits

## License

MIT
