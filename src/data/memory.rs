use std::collections::{HashMap, VecDeque};

use super::{Bar, DataHandler};
use crate::events::MarketEvent;
use crate::types::Symbol;

const HISTORY_LIMIT: usize = 4096;

pub struct InMemoryDataHandler {
    bars: Vec<Bar>,
    cursor: usize,
    history: HashMap<Symbol, VecDeque<Bar>>,
}

impl InMemoryDataHandler {
    /// Build from a list of bars. The handler sorts them by timestamp before emitting.
    pub fn from_bars(mut bars: Vec<Bar>) -> Self {
        bars.sort_by_key(|b| b.timestamp);
        Self {
            bars,
            cursor: 0,
            history: HashMap::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.bars.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bars.is_empty()
    }
}

impl DataHandler for InMemoryDataHandler {
    fn next(&mut self) -> Option<MarketEvent> {
        if self.cursor >= self.bars.len() {
            return None;
        }
        let bar = self.bars[self.cursor].clone();
        self.cursor += 1;

        let entry = self.history.entry(bar.symbol.clone()).or_default();
        entry.push_back(bar.clone());
        if entry.len() > HISTORY_LIMIT {
            entry.pop_front();
        }

        Some(MarketEvent {
            timestamp: bar.timestamp,
            bar,
        })
    }

    fn current_bar(&self, symbol: &str) -> Option<&Bar> {
        self.history.get(symbol).and_then(|h| h.back())
    }

    fn history(&self, symbol: &str, n: usize) -> Vec<Bar> {
        match self.history.get(symbol) {
            Some(h) => {
                let start = h.len().saturating_sub(n);
                h.iter().skip(start).cloned().collect()
            }
            None => Vec::new(),
        }
    }

    fn symbols(&self) -> Vec<Symbol> {
        let mut out: Vec<Symbol> = self
            .bars
            .iter()
            .map(|b| b.symbol.clone())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        out.sort();
        out
    }

    fn reset(&mut self) {
        self.cursor = 0;
        self.history.clear();
    }
}
