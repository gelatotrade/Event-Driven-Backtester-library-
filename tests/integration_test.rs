use chrono::{Duration, TimeZone, Utc};
use event_driven_backtester::data::DataHandler;
use event_driven_backtester::prelude::*;
use event_driven_backtester::strategy::Strategy;

fn ramp_bars(symbol: &str, n: usize, start_price: f64, step: f64) -> Vec<Bar> {
    let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    (0..n)
        .map(|i| {
            let close = start_price + step * (i as f64);
            let open = if i == 0 {
                close
            } else {
                start_price + step * ((i - 1) as f64)
            };
            Bar::new(
                start + Duration::days(i as i64),
                symbol,
                open,
                close.max(open) + 1.0,
                close.min(open) - 1.0,
                close,
                1_000.0,
            )
        })
        .collect()
}

fn fill(symbol: &str, side: OrderSide, qty: f64, price: f64, ts_offset_days: i64) -> FillEvent {
    let ts = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap() + Duration::days(ts_offset_days);
    FillEvent {
        timestamp: ts,
        symbol: symbol.into(),
        side,
        quantity: qty,
        fill_price: price,
        commission: 0.0,
        slippage_cost: 0.0,
        is_maker: false,
    }
}

#[test]
fn ma_crossover_runs_end_to_end() {
    let bars = ramp_bars("BTC", 100, 100.0, 1.0);
    let data = InMemoryDataHandler::from_bars(bars);
    let strategy = MovingAverageCrossover::new("BTC", 5, 20);
    let portfolio = Portfolio::new(10_000.0);
    let execution = SimulatedExecutionHandler::new(ZeroSlippage, FixedFees::new(0.0), NoFunding);

    let mut engine = BacktestEngine::new(data, strategy, portfolio, execution);
    let result = engine.run().expect("engine should run");

    assert!(!result.equity_curve.is_empty());
    assert!(
        result.fills >= 1,
        "should have opened at least one position on a rising ramp"
    );
    assert!(result.metrics.num_periods > 0);
}

/// A strategy that fires a single long signal on the first bar it sees.
struct BuyOnce {
    symbol: String,
    fired: bool,
}

impl Strategy for BuyOnce {
    fn name(&self) -> &str {
        "buy-once"
    }
    fn on_market(&mut self, ev: &MarketEvent, _d: &dyn DataHandler) -> Vec<SignalEvent> {
        if self.fired || ev.bar.symbol != self.symbol {
            return vec![];
        }
        self.fired = true;
        vec![SignalEvent {
            timestamp: ev.timestamp,
            symbol: self.symbol.clone(),
            direction: SignalDirection::Long,
            strength: 1.0,
        }]
    }
}

#[test]
fn fills_occur_at_next_bar_open_not_signal_bar() {
    // Signal fires on bar 0 (open=close=100). Next-bar-open execution must
    // fill at bar 1's open (= 200), NOT bar 0's open (= 100). Filling at 100
    // would be look-ahead bias.
    let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let bars = vec![
        Bar::new(start, "X", 100.0, 100.0, 100.0, 100.0, 1.0),
        Bar::new(
            start + Duration::days(1),
            "X",
            200.0,
            205.0,
            195.0,
            200.0,
            1.0,
        ),
    ];
    let data = InMemoryDataHandler::from_bars(bars);
    let strategy = BuyOnce {
        symbol: "X".into(),
        fired: false,
    };
    let portfolio = Portfolio::new(1_000_000.0);
    let execution = SimulatedExecutionHandler::new(ZeroSlippage, FixedFees::new(0.0), NoFunding);

    let mut engine = BacktestEngine::new(data, strategy, portfolio, execution);
    let result = engine.run().expect("engine should run");

    assert_eq!(result.fills, 1);
    let pos = engine.portfolio().position("X").expect("should hold X");
    approx::assert_relative_eq!(pos.avg_price, 200.0, max_relative = 1e-9);
}

#[test]
fn no_fill_when_signal_has_no_following_bar() {
    // Signal on the only bar -> order queued for a next bar that never comes.
    let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let bars = vec![Bar::new(start, "X", 100.0, 100.0, 100.0, 100.0, 1.0)];
    let data = InMemoryDataHandler::from_bars(bars);
    let strategy = BuyOnce {
        symbol: "X".into(),
        fired: false,
    };
    let portfolio = Portfolio::new(1_000.0);
    let execution = SimulatedExecutionHandler::new(ZeroSlippage, FixedFees::new(0.0), NoFunding);

    let mut engine = BacktestEngine::new(data, strategy, portfolio, execution);
    let result = engine.run().expect("engine should run");
    assert_eq!(result.fills, 0);
}

#[test]
fn linear_slippage_moves_buy_price_up_and_sell_price_down() {
    let bar = Bar::new(Utc::now(), "BTC", 100.0, 110.0, 95.0, 105.0, 1_000.0);

    let order_buy = OrderEvent {
        timestamp: bar.timestamp,
        symbol: "BTC".into(),
        side: OrderSide::Buy,
        quantity: 1.0,
        order_type: OrderType::Market,
        limit_price: None,
    };
    let order_sell = OrderEvent {
        timestamp: bar.timestamp,
        symbol: "BTC".into(),
        side: OrderSide::Sell,
        quantity: 1.0,
        order_type: OrderType::Market,
        limit_price: None,
    };

    let mut exec =
        SimulatedExecutionHandler::new(LinearSlippage::new(0.01), FixedFees::new(0.0), NoFunding);

    let buy_fill = exec.execute(&order_buy, &bar).unwrap();
    let sell_fill = exec.execute(&order_sell, &bar).unwrap();

    // 1% of close = 1.05, applied around bar.open = 100.0
    approx::assert_relative_eq!(buy_fill.fill_price, 101.05, max_relative = 1e-9);
    approx::assert_relative_eq!(sell_fill.fill_price, 98.95, max_relative = 1e-9);
    assert!(!buy_fill.is_maker);
}

#[test]
fn maker_taker_fees_charge_correct_rate() {
    let bar = Bar::new(Utc::now(), "BTC", 100.0, 110.0, 95.0, 105.0, 1_000.0);
    let order = OrderEvent {
        timestamp: bar.timestamp,
        symbol: "BTC".into(),
        side: OrderSide::Buy,
        quantity: 2.0,
        order_type: OrderType::Limit,
        limit_price: Some(96.0),
    };

    let mut exec =
        SimulatedExecutionHandler::new(ZeroSlippage, MakerTakerFees::new(0.0001, 0.001), NoFunding);

    let fill = exec.execute(&order, &bar).unwrap();
    // Limit fills are maker -> notional * 0.0001 = 96 * 2 * 0.0001 = 0.0192
    approx::assert_relative_eq!(fill.commission, 0.0192, max_relative = 1e-9);
    assert!(fill.is_maker);
}

#[test]
fn funding_payments_reduce_long_cash() {
    let mut portfolio = Portfolio::new(10_000.0);
    portfolio.on_fill(&fill("ETH", OrderSide::Buy, 10.0, 1_000.0, 0));
    portfolio.update_mark("ETH", 1_000.0);

    let cash_before = portfolio.cash();
    portfolio.apply_funding("ETH", 0.0001); // 1 bps
    let cash_after = portfolio.cash();

    // Long pays positive funding: notional * rate = 10 * 1000 * 0.0001 = 1.0
    approx::assert_relative_eq!(cash_before - cash_after, 1.0, max_relative = 1e-9);
}

#[test]
fn borrow_cost_reduces_short_cash() {
    let mut portfolio = Portfolio::new(10_000.0).with_annual_borrow_rate(0.365);
    portfolio.on_fill(&fill("X", OrderSide::Sell, 10.0, 100.0, 0)); // short
    portfolio.update_mark("X", 100.0);

    let cash_before = portfolio.cash();
    portfolio.apply_borrow_cost(365.0); // per-bar 0.001, notional 1000 -> cost 1.0
    let cash_after = portfolio.cash();

    approx::assert_relative_eq!(cash_before - cash_after, 1.0, max_relative = 1e-9);
    approx::assert_relative_eq!(portfolio.total_borrow(), 1.0, max_relative = 1e-9);
}

#[test]
fn partial_reduction_keeps_avg_price_and_records_trade() {
    let mut portfolio = Portfolio::new(100_000.0);
    portfolio.on_fill(&fill("BTC", OrderSide::Buy, 10.0, 100.0, 0)); // long 10 @ 100
    portfolio.on_fill(&fill("BTC", OrderSide::Sell, 3.0, 120.0, 1)); // reduce to 7

    let pos = portfolio.position("BTC").unwrap();
    approx::assert_relative_eq!(pos.quantity, 7.0, max_relative = 1e-9);
    // Average price must NOT change on a partial reduction.
    approx::assert_relative_eq!(pos.avg_price, 100.0, max_relative = 1e-9);
    // Realized PnL on the closed 3 units: 3 * (120 - 100) = 60.
    approx::assert_relative_eq!(portfolio.realized_pnl(), 60.0, max_relative = 1e-9);
    assert_eq!(portfolio.trades().len(), 1);
    approx::assert_relative_eq!(portfolio.trades()[0].pnl, 60.0, max_relative = 1e-9);
}

#[test]
fn round_trip_records_full_trade() {
    let mut portfolio = Portfolio::new(10_000.0);
    portfolio.on_fill(&fill("BTC", OrderSide::Buy, 1.0, 100.0, 0));
    portfolio.on_fill(&fill("BTC", OrderSide::Sell, 1.0, 120.0, 1));

    approx::assert_relative_eq!(portfolio.realized_pnl(), 20.0, max_relative = 1e-9);
    assert!(portfolio
        .position("BTC")
        .map(|p| p.is_flat())
        .unwrap_or(false));
    assert_eq!(portfolio.trades().len(), 1);
    let t = &portfolio.trades()[0];
    assert!(t.is_win());
    approx::assert_relative_eq!(t.return_pct, 0.2, max_relative = 1e-9);
}

#[test]
fn liquidation_flattens_book_when_equity_below_maintenance() {
    // 5x long: 200 equity, 1000 notional. Maintenance 10% allows up to 10x.
    let mut portfolio = Portfolio::new(200.0)
        .with_max_leverage(10.0)
        .with_maintenance_margin(0.1);
    portfolio.on_fill(&fill("X", OrderSide::Buy, 10.0, 100.0, 0));

    portfolio.update_mark("X", 100.0);
    assert!(
        !portfolio.check_and_liquidate(Utc::now()),
        "healthy at entry"
    );

    // Price drops to 85: equity 50, gross 850, maintenance 85 -> liquidate.
    portfolio.update_mark("X", 85.0);
    assert!(portfolio.check_and_liquidate(Utc::now()));
    assert!(portfolio.is_liquidated());
    assert!(portfolio
        .position("X")
        .map(|p| p.is_flat())
        .unwrap_or(false));
}

#[test]
fn trade_stats_compute_win_rate_and_profit_factor() {
    let mut portfolio = Portfolio::new(1_000_000.0);
    // Three round trips: +100, -50, +25.
    for (i, (qty, entry, exit)) in [(1.0, 100.0, 200.0), (1.0, 100.0, 50.0), (1.0, 100.0, 125.0)]
        .into_iter()
        .enumerate()
    {
        let d = (i as i64) * 2;
        portfolio.on_fill(&fill("X", OrderSide::Buy, qty, entry, d));
        portfolio.on_fill(&fill("X", OrderSide::Sell, qty, exit, d + 1));
    }
    let stats = TradeStats::from_trades(portfolio.trades());
    assert_eq!(stats.num_trades, 3);
    approx::assert_relative_eq!(stats.win_rate, 2.0 / 3.0, max_relative = 1e-9);
    // gross profit 125, gross loss 50 -> PF 2.5
    approx::assert_relative_eq!(stats.profit_factor, 2.5, max_relative = 1e-9);
}

#[test]
fn walk_forward_produces_folds() {
    let bars = ramp_bars("BTC", 360, 100.0, 0.5);

    let wf = WalkForward::new(60, 30);
    let result = wf
        .run(
            bars,
            || MovingAverageCrossover::new("BTC", 5, 20),
            || Portfolio::new(10_000.0),
            || SimulatedExecutionHandler::new(ZeroSlippage, FixedFees::new(0.0), NoFunding),
        )
        .expect("walk-forward should run");

    assert!(
        result.folds.len() >= 5,
        "expected several folds, got {}",
        result.folds.len()
    );
    for fold in &result.folds {
        assert!(fold.in_sample_end == fold.out_sample_start);
    }
}
