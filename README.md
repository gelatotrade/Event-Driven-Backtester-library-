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

**No look-ahead by construction.** A signal computed from bar `T`'s close is
filled on the *next* bar of that symbol, at its open. The engine never lets an
order execute at a price from the same bar (or earlier) that produced the
signal — the single most common way a backtester lies to you.

## Feature overview

- **Pluggable components** behind traits: `DataHandler`, `Strategy`,
  `ExecutionHandler`, `SlippageModel`, `FeeModel`, `FundingModel`.
- **Next-bar-open execution** so the simulation is free of look-ahead bias.
- **Realistic execution**: market + limit orders, maker/taker fees,
  linear and square-root slippage, perpetual funding.
- **Portfolio engine**: long/short positions, gross-leverage cap,
  risk-per-trade sizing, realized vs unrealized PnL, full equity curve.
- **Margin & risk**: maintenance-margin liquidation, short borrow/financing
  cost, configurable liquidation penalty.
- **Walk-forward analysis** with rolling or expanding in-sample windows
  and stitched OOS metrics.
- **Curve metrics**: Sharpe, Sortino, Calmar, max drawdown (depth + duration),
  total/annualized return and volatility.
- **Trade metrics** (genuinely per round-trip, not per-bar): win rate,
  profit factor, average win/loss, largest win/loss.
- **CSV ingestion** (RFC3339 or unix-ms timestamps) and **CSV export** of the
  trade log and equity curve; in-memory source for tests and synthetic data.
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

    println!("Sharpe      : {:.2}", result.metrics.sharpe);
    println!("Max DD      : {:.2}%", result.metrics.max_drawdown * 100.0);
    println!("Win rate    : {:.1}%", result.trade_stats.win_rate * 100.0);
    println!("Trades      : {}", result.trade_stats.num_trades);
    println!("Liquidated  : {}", result.liquidated);

    result.write_trades_csv("trades.csv")?;
    result.write_equity_csv("equity.csv")?;
    Ok(())
}
```

`result.metrics` holds the equity-curve statistics; `result.trade_stats` holds
the per-trade statistics; `result.trades` is the full round-trip blotter.

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

## Costs, margin & liquidation

The portfolio enforces a gross-leverage cap across **all** symbols (not per
order), and optionally models margin and financing:

```rust
let portfolio = Portfolio::new(100_000.0)
    .with_risk_per_trade(0.75)        // allocate up to 75% of equity per signal
    .with_max_leverage(3.0)           // total gross notional <= 3x equity
    .with_maintenance_margin(0.005)   // liquidate below 0.5% maintenance margin
    .with_annual_borrow_rate(0.10)    // 10% p.a. financing on short notional
    .with_liquidation_fee(0.005);     // 50 bps penalty when liquidated
```

- **Leverage cap**: each new order is sized so that this symbol's notional plus
  every other open position stays within `max_leverage * equity`.
- **Liquidation**: after each bar is marked to market, if
  `equity < maintenance_margin_rate * gross_notional` the book is force-flattened
  at the current mark (plus the liquidation fee) and `result.liquidated` is set.
  No further trades are taken. Disabled when the rate is `0.0` (the default).
- **Borrow cost**: short notional is charged `annual_borrow_rate / periods_per_year`
  each bar, surfaced as `result.total_borrow`. Disabled when `0.0`.

## Results & export

`BacktestResult` carries the equity curve, the full trade blotter, aggregate
cost accounting (`total_fees`, `total_funding`, `total_borrow`, `total_slippage`)
and both metric structs. Persist the artifacts for downstream analysis:

```rust
result.write_trades_csv("trades.csv")?;   // one row per round-trip trade
result.write_equity_csv("equity.csv")?;   // timestamp, equity, cash, position_value
```

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
├── portfolio/          # positions, trades, sizing, margin, equity curve
├── execution/          # simulated handler, slippage / fee / funding models
└── metrics/            # curve metrics (Sharpe/Sortino/Calmar/DD) + trade stats
```

## Status

This crate is the engine layer for downstream strategy repos. In place and
covered by integration tests:

- Event loop with **next-bar-open execution** (no look-ahead)
- Realistic costs: slippage, maker/taker fees, perpetual funding, short borrow
- Margin model with maintenance-margin liquidation and gross-leverage cap
- Round-trip trade blotter with per-trade metrics; CSV export
- Walk-forward (rolling / expanding) with stitched OOS metrics
- CI: `fmt` + `clippy -D warnings` + tests + examples on every push/PR

Phase 2 candidates:

- Multi-asset equity accounting on a single shared clock (today one equity
  point is recorded per incoming bar, which is exact for single-symbol runs)
- Limit-order book simulation with queue position for higher-frequency strategies
- Parallel walk-forward and parameter-grid search
- Vol-targeting / drawdown-stop risk overlays
- Live trading adapter (Binance / Hyperliquid) reusing the same traits

## License

MIT
