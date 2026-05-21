//! Named scenario presets for CI and local runs.
//!
//! Each preset layers timed [`EffectSpec`] events on the baseline market engine (random walk
//! keeps running on all pairs). Mechanics are approximate mid shifts, not full microstructure.

use crate::timeline::{EffectSpec, ScenarioTimeline, TimedEffect};

pub fn preset_names() -> &'static [&'static str] {
    &[
        "flash_drop_v1",
        "btc_rally_v1",
        "dead_cat_bounce_v1",
        "risk_off_v1",
        "bull_trap_v1",
    ]
}

pub fn preset_by_name(name: &str) -> Option<ScenarioTimeline> {
    match name {
        "flash_drop_v1" => Some(flash_drop_v1()),
        "btc_rally_v1" => Some(btc_rally_v1()),
        "dead_cat_bounce_v1" => Some(dead_cat_bounce_v1()),
        "risk_off_v1" => Some(risk_off_v1()),
        "bull_trap_v1" => Some(bull_trap_v1()),
        _ => None,
    }
}

/// Sharp selloff (flash-style) on BTC.
fn flash_drop_v1() -> ScenarioTimeline {
    ScenarioTimeline {
        name: Some("flash_drop_v1".into()),
        events: vec![TimedEffect {
            at_ms: 30_000,
            effect: EffectSpec::Drop {
                instrument_id: "BTC-USDT".into(),
                pct: -0.05,
                over_ticks: 10,
            },
        }],
    }
}

fn btc_rally_v1() -> ScenarioTimeline {
    ScenarioTimeline {
        name: Some("btc_rally_v1".into()),
        events: vec![TimedEffect {
            at_ms: 15_000,
            effect: EffectSpec::Rally {
                instrument_id: "BTC-USDT".into(),
                pct: 0.03,
                over_ticks: 8,
            },
        }],
    }
}

/// **Dead-cat bounce:** sharp drop then partial recovery — tests bots that wait for stabilization
/// before re-risking (do not treat first green tick as trend reversal).
fn dead_cat_bounce_v1() -> ScenarioTimeline {
    ScenarioTimeline {
        name: Some("dead_cat_bounce_v1".into()),
        events: vec![
            TimedEffect {
                at_ms: 20_000,
                effect: EffectSpec::Drop {
                    instrument_id: "BTC-USDT".into(),
                    pct: -0.08,
                    over_ticks: 8,
                },
            },
            TimedEffect {
                at_ms: 45_000,
                effect: EffectSpec::Rally {
                    instrument_id: "BTC-USDT".into(),
                    pct: 0.04,
                    over_ticks: 12,
                },
            },
        ],
    }
}

/// **Risk-off correlation:** staggered drops on BTC → ETH → SOL (lead–lag beta book).
/// Tests cross-asset hedging and portfolio heat when majors sell off in sequence.
fn risk_off_v1() -> ScenarioTimeline {
    ScenarioTimeline {
        name: Some("risk_off_v1".into()),
        events: vec![
            TimedEffect {
                at_ms: 25_000,
                effect: EffectSpec::Drop {
                    instrument_id: "BTC-USDT".into(),
                    pct: -0.04,
                    over_ticks: 10,
                },
            },
            TimedEffect {
                at_ms: 28_000,
                effect: EffectSpec::Drop {
                    instrument_id: "ETH-USDT".into(),
                    pct: -0.04,
                    over_ticks: 10,
                },
            },
            TimedEffect {
                at_ms: 31_000,
                effect: EffectSpec::Drop {
                    instrument_id: "SOL-USDT".into(),
                    pct: -0.05,
                    over_ticks: 10,
                },
            },
        ],
    }
}

/// **Bull trap / failed breakout:** rally then rejection — tests bots that chase breakouts
/// and need stop or size-down when price reverses through the entry zone.
fn bull_trap_v1() -> ScenarioTimeline {
    ScenarioTimeline {
        name: Some("bull_trap_v1".into()),
        events: vec![
            TimedEffect {
                at_ms: 15_000,
                effect: EffectSpec::Rally {
                    instrument_id: "BTC-USDT".into(),
                    pct: 0.03,
                    over_ticks: 6,
                },
            },
            TimedEffect {
                at_ms: 35_000,
                effect: EffectSpec::Drop {
                    instrument_id: "BTC-USDT".into(),
                    pct: -0.05,
                    over_ticks: 10,
                },
            },
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_presets_resolve() {
        for name in preset_names() {
            assert!(preset_by_name(name).is_some(), "missing preset {name}");
        }
    }

    #[test]
    fn risk_off_has_three_legs() {
        let t = risk_off_v1();
        assert_eq!(t.events.len(), 3);
    }
}
