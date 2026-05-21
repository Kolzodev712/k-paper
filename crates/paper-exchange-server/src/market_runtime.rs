//! Shared state between the multi-instrument market engine and scenario overlay scheduler.

use paper_exchange_core::types::InstrumentId;
use paper_exchange_scenarios::OverlayState;
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct InstrumentDynamics {
    pub mid: f64,
    pub sequence: u64,
}

#[derive(Debug)]
pub struct MarketRuntime {
    pub instruments: HashMap<InstrumentId, InstrumentDynamics>,
    pub overlay: OverlayState,
}

impl MarketRuntime {
    pub fn mid(&self, instrument_id: &InstrumentId) -> f64 {
        self.instruments
            .get(instrument_id)
            .map(|d| d.mid)
            .unwrap_or(0.0)
    }

    pub fn mids(&self) -> HashMap<InstrumentId, f64> {
        self.instruments
            .iter()
            .map(|(k, v)| (k.clone(), v.mid))
            .collect()
    }

    pub fn effective_mid(&self, instrument_id: &InstrumentId) -> f64 {
        self.mid(instrument_id) + self.overlay.offset(instrument_id)
    }
}
