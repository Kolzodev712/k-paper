//! Overlay state: active effects mutate per-instrument mid offset on each market tick.

use paper_exchange_core::types::InstrumentId;
use std::collections::HashMap;

#[derive(Clone, Debug)]
struct ActiveEffect {
    instrument_id: InstrumentId,
    offset_per_tick: f64,
    ticks_remaining: u32,
}

/// Per-instrument overlay offset applied on top of baseline random-walk mid.
#[derive(Clone, Debug, Default)]
pub struct OverlayState {
    offsets: HashMap<InstrumentId, f64>,
    active: Vec<ActiveEffect>,
}

impl OverlayState {
    pub fn offset(&self, instrument_id: &InstrumentId) -> f64 {
        self.offsets.get(instrument_id).copied().unwrap_or(0.0)
    }

    /// Advance one market tick: apply active effects, return per-instrument delta this tick.
    pub fn tick(&mut self) -> HashMap<InstrumentId, f64> {
        let mut deltas = HashMap::new();
        for effect in &mut self.active {
            if effect.ticks_remaining == 0 {
                continue;
            }
            *deltas.entry(effect.instrument_id.clone()).or_insert(0.0) += effect.offset_per_tick;
            effect.ticks_remaining -= 1;
        }
        self.active.retain(|e| e.ticks_remaining > 0);
        for (id, delta) in &deltas {
            *self.offsets.entry(id.clone()).or_insert(0.0) += delta;
        }
        deltas
    }

    /// Price drop over `over_ticks`: total move = `current_mid * pct` (pct negative for drop).
    pub fn start_drop(
        &mut self,
        instrument_id: InstrumentId,
        current_mid: f64,
        pct: f64,
        over_ticks: u32,
    ) {
        self.start_ramp(instrument_id, current_mid * pct, over_ticks);
    }

    /// Price rally over `over_ticks`: total move = `current_mid * pct` (pct positive).
    pub fn start_rally(
        &mut self,
        instrument_id: InstrumentId,
        current_mid: f64,
        pct: f64,
        over_ticks: u32,
    ) {
        self.start_ramp(instrument_id, current_mid * pct, over_ticks);
    }

    fn start_ramp(&mut self, instrument_id: InstrumentId, total_offset: f64, over_ticks: u32) {
        if over_ticks == 0 {
            return;
        }
        let per_tick = total_offset / over_ticks as f64;
        self.active.push(ActiveEffect {
            instrument_id,
            offset_per_tick: per_tick,
            ticks_remaining: over_ticks,
        });
    }

    pub fn active_effects(&self) -> usize {
        self.active.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drop_applies_over_ticks() {
        let id = InstrumentId::new("BTC-USDT");
        let mut overlay = OverlayState::default();
        overlay.start_drop(id.clone(), 50_000.0, -0.1, 10);
        let mut total = 0.0;
        for _ in 0..10 {
            let d = overlay.tick();
            total += d.get(&id).copied().unwrap_or(0.0);
        }
        assert!((total - (-5_000.0)).abs() < 1.0);
        assert!((overlay.offset(&id) - (-5_000.0)).abs() < 1.0);
        assert_eq!(overlay.active_effects(), 0);
    }
}
