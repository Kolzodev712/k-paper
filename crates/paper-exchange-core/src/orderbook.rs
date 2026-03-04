//! Per-instrument orderbook state and snapshots.

use crate::types::{BookLevel, InstrumentId, OrderbookSnapshot};
use chrono::Utc;
use rust_decimal::Decimal;
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct InstrumentOrderbook {
    pub instrument_id: InstrumentId,
    bids: BTreeMap<Decimal, Decimal>,
    asks: BTreeMap<Decimal, Decimal>,
    sequence: u64,
}

impl InstrumentOrderbook {
    pub fn new(instrument_id: InstrumentId) -> Self {
        Self {
            instrument_id,
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            sequence: 0,
        }
    }
}

impl Default for InstrumentOrderbook {
    fn default() -> Self {
        Self::new(InstrumentId::new(String::new()))
    }
}

impl InstrumentOrderbook {
    pub fn apply_snapshot(&mut self, snapshot: OrderbookSnapshot) -> OrderbookSnapshot {
        self.bids = snapshot
            .bids
            .iter()
            .map(|l| (l.price, l.quantity))
            .collect();
        self.asks = snapshot
            .asks
            .iter()
            .map(|l| (l.price, l.quantity))
            .collect();
        self.sequence = snapshot.sequence;
        self.to_snapshot()
    }

    pub fn set_snapshot(
        &mut self,
        bids: Vec<BookLevel>,
        asks: Vec<BookLevel>,
    ) -> OrderbookSnapshot {
        self.sequence += 1;
        self.bids.clear();
        self.asks.clear();
        for level in &bids {
            *self.bids.entry(level.price).or_default() += level.quantity;
        }
        for level in &asks {
            *self.asks.entry(level.price).or_default() += level.quantity;
        }
        self.to_snapshot()
    }

    pub fn to_snapshot(&self) -> OrderbookSnapshot {
        let bids: Vec<BookLevel> = self
            .bids
            .iter()
            .rev()
            .map(|(price, qty)| BookLevel {
                price: *price,
                quantity: *qty,
            })
            .collect();
        let asks: Vec<BookLevel> = self
            .asks
            .iter()
            .map(|(price, qty)| BookLevel {
                price: *price,
                quantity: *qty,
            })
            .collect();
        OrderbookSnapshot {
            instrument_id: self.instrument_id.clone(),
            bids,
            asks,
            sequence: self.sequence,
            timestamp: Utc::now(),
        }
    }

    pub fn best_bid(&self) -> Option<Decimal> {
        self.bids.keys().next_back().copied()
    }

    pub fn best_ask(&self) -> Option<Decimal> {
        self.asks.keys().next().copied()
    }

    pub fn best_bid_qty(&self) -> Option<Decimal> {
        self.bids.iter().next_back().map(|(_, q)| *q)
    }

    pub fn best_ask_qty(&self) -> Option<Decimal> {
        self.asks.iter().next().map(|(_, q)| *q)
    }

    pub fn sequence(&self) -> u64 {
        self.sequence
    }
}

#[derive(Clone, Debug, Default)]
pub struct Orderbook {
    books: std::collections::HashMap<InstrumentId, InstrumentOrderbook>,
}

impl Orderbook {
    pub fn new() -> Self {
        Self {
            books: std::collections::HashMap::new(),
        }
    }

    pub fn ensure_instrument(&mut self, instrument_id: InstrumentId) {
        self.books
            .entry(instrument_id.clone())
            .or_insert_with(|| InstrumentOrderbook::new(instrument_id));
    }

    pub fn apply_snapshot(&mut self, snapshot: OrderbookSnapshot) -> OrderbookSnapshot {
        self.books
            .entry(snapshot.instrument_id.clone())
            .or_insert_with(|| InstrumentOrderbook::new(snapshot.instrument_id.clone()))
            .apply_snapshot(snapshot)
    }

    pub fn set_snapshot(
        &mut self,
        instrument_id: InstrumentId,
        bids: Vec<BookLevel>,
        asks: Vec<BookLevel>,
    ) -> OrderbookSnapshot {
        self.ensure_instrument(instrument_id.clone());
        self.books
            .get_mut(&instrument_id)
            .unwrap()
            .set_snapshot(bids, asks)
    }

    pub fn get(&self, instrument_id: &InstrumentId) -> Option<&InstrumentOrderbook> {
        self.books.get(instrument_id)
    }

    pub fn get_mut(&mut self, instrument_id: &InstrumentId) -> Option<&mut InstrumentOrderbook> {
        self.books.get_mut(instrument_id)
    }
}
