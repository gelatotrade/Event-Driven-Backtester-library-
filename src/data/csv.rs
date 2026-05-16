use std::path::Path;

use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use serde::Deserialize;

use super::{Bar, DataHandler, InMemoryDataHandler};
use crate::error::BacktestError;
use crate::events::MarketEvent;
use crate::types::Symbol;

/// CSV columns expected by [`CsvDataHandler::from_path`].
///
/// Either `timestamp` (RFC3339) or `timestamp_ms` (unix milliseconds) is accepted.
#[derive(Debug, Deserialize)]
struct CsvRow {
    timestamp: Option<String>,
    timestamp_ms: Option<i64>,
    symbol: String,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    volume: f64,
    funding_rate: Option<f64>,
}

pub struct CsvDataHandler {
    inner: InMemoryDataHandler,
}

impl CsvDataHandler {
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, BacktestError> {
        let mut rdr = csv::Reader::from_path(path)?;
        let mut bars = Vec::new();
        for row in rdr.deserialize::<CsvRow>() {
            let row = row?;
            let ts = parse_timestamp(&row)?;
            bars.push(Bar {
                timestamp: ts,
                symbol: row.symbol,
                open: row.open,
                high: row.high,
                low: row.low,
                close: row.close,
                volume: row.volume,
                funding_rate: row.funding_rate,
            });
        }
        Ok(Self {
            inner: InMemoryDataHandler::from_bars(bars),
        })
    }
}

fn parse_timestamp(row: &CsvRow) -> Result<DateTime<Utc>, BacktestError> {
    if let Some(ms) = row.timestamp_ms {
        return Utc
            .timestamp_millis_opt(ms)
            .single()
            .ok_or_else(|| BacktestError::Parse(format!("invalid timestamp_ms: {ms}")));
    }
    if let Some(s) = &row.timestamp {
        if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
            return Ok(dt.with_timezone(&Utc));
        }
        if let Ok(naive) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
            return Ok(Utc.from_utc_datetime(&naive));
        }
        if let Ok(naive) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
            return Ok(Utc.from_utc_datetime(&naive));
        }
        return Err(BacktestError::Parse(format!("unrecognized timestamp: {s}")));
    }
    Err(BacktestError::Parse(
        "row missing both `timestamp` and `timestamp_ms`".into(),
    ))
}

impl DataHandler for CsvDataHandler {
    fn next(&mut self) -> Option<MarketEvent> {
        self.inner.next()
    }

    fn current_bar(&self, symbol: &str) -> Option<&Bar> {
        self.inner.current_bar(symbol)
    }

    fn history(&self, symbol: &str, n: usize) -> Vec<Bar> {
        self.inner.history(symbol, n)
    }

    fn symbols(&self) -> Vec<Symbol> {
        self.inner.symbols()
    }

    fn reset(&mut self) {
        self.inner.reset();
    }
}
