//! Scenario timeline: timed overlay effects (JSON or presets).

use crate::overlay::OverlayState;
use paper_exchange_core::types::InstrumentId;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EffectSpec {
    Drop {
        instrument_id: String,
        #[serde(default = "default_pct_drop")]
        pct: f64,
        #[serde(default = "default_over_ticks")]
        over_ticks: u32,
    },
    Rally {
        instrument_id: String,
        #[serde(default = "default_pct_rally")]
        pct: f64,
        #[serde(default = "default_over_ticks")]
        over_ticks: u32,
    },
}

fn default_pct_drop() -> f64 {
    -0.05
}
fn default_pct_rally() -> f64 {
    0.05
}
fn default_over_ticks() -> u32 {
    10
}

#[derive(Clone, Debug, Deserialize)]
pub struct TimedEffect {
    pub at_ms: u64,
    pub effect: EffectSpec,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ScenarioTimeline {
    #[serde(default)]
    pub name: Option<String>,
    pub events: Vec<TimedEffect>,
}

impl ScenarioTimeline {
    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }

    pub fn apply_effect(
        &self,
        spec: &EffectSpec,
        overlay: &mut OverlayState,
        mids: &HashMap<InstrumentId, f64>,
    ) {
        match spec {
            EffectSpec::Drop {
                instrument_id,
                pct,
                over_ticks,
            } => {
                let id = InstrumentId::new(instrument_id);
                let mid = mids.get(&id).copied().unwrap_or(50_000.0);
                overlay.start_drop(id, mid, *pct, *over_ticks);
            }
            EffectSpec::Rally {
                instrument_id,
                pct,
                over_ticks,
            } => {
                let id = InstrumentId::new(instrument_id);
                let mid = mids.get(&id).copied().unwrap_or(50_000.0);
                overlay.start_rally(id, mid, *pct, *over_ticks);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_timeline_json() {
        let j = r#"{"events":[{"at_ms":1000,"effect":{"type":"drop","instrument_id":"BTC-USDT","pct":-0.1,"over_ticks":5}}]}"#;
        let t = ScenarioTimeline::from_json(j).unwrap();
        assert_eq!(t.events.len(), 1);
        assert_eq!(t.events[0].at_ms, 1000);
    }
}
