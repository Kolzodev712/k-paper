//! Server runtime: load YAML config, resolve lab/dev profile from environment.

use crate::market_engine::{
    default_initial_mid, default_ticks, InstrumentMarketSpec, MarketEngineConfig,
};
use crate::scenario::{resolve_scenario, ScenarioLoad};
use paper_exchange_core::config::{Config, FillModelConfig, InitialBalance};
use paper_exchange_core::types::Instrument;
use serde::Deserialize;
use std::path::PathBuf;

/// Default HTTP/WS listen port when `PAPER_EXCHANGE_PORT` is unset (fixed, not ephemeral).
pub const DEFAULT_PAPER_EXCHANGE_PORT: u16 = 19999;

/// Resolve bind port from `PAPER_EXCHANGE_PORT` or [`DEFAULT_PAPER_EXCHANGE_PORT`].
pub fn listen_port() -> u16 {
    std::env::var("PAPER_EXCHANGE_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(DEFAULT_PAPER_EXCHANGE_PORT)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeProfile {
    /// Scripted scenario and/or no random dummy OB (deterministic-friendly).
    Lab,
    /// Random-walk dummy orderbook (default when dummy OB enabled).
    Dev,
}

#[derive(Debug, Deserialize)]
struct ExchangeConfigFile {
    instruments: Vec<Instrument>,
    initial_balances: Vec<InitialBalance>,
    #[serde(default)]
    fill_model: FillModelConfig,
    #[serde(default = "default_max_trade_history")]
    max_trade_history: usize,
    #[serde(default)]
    market: Option<MarketFile>,
    /// Legacy section name; merged with `market` if present.
    #[serde(default)]
    dummy_ob: Option<MarketFile>,
    #[serde(default)]
    linear: Option<paper_exchange_core::linear::LinearContract>,
}

fn default_max_trade_history() -> usize {
    10_000
}

#[derive(Debug, Deserialize)]
struct MarketFile {
    /// Max random-walk move per interval, in ticks of each instrument. When absent, derived
    /// per instrument as ~20 bps of its initial mid.
    #[serde(default)]
    step_ticks: Option<u32>,
    /// Half the quoted spread, in ticks. When absent, derived as ~10 bps of the initial mid.
    #[serde(default)]
    spread_half_ticks: Option<u32>,
    /// Removed absolute-price keys; present only so they can be rejected with a clear error.
    #[serde(default)]
    step: Option<f64>,
    #[serde(default)]
    spread_half: Option<f64>,
    #[serde(default = "default_levels")]
    levels: usize,
    #[serde(default = "default_base_qty")]
    base_qty_per_level: f64,
    #[serde(default = "default_interval_ms")]
    interval_ms: u64,
    #[serde(default)]
    seed: Option<u64>,
    #[serde(default = "default_trade_probability")]
    trade_probability: f64,
    #[serde(default = "default_trade_max_qty")]
    trade_max_qty: f64,
    /// Per-instrument mid overrides (optional).
    #[serde(default)]
    instruments: Vec<MarketInstrumentFile>,
}

#[derive(Debug, Deserialize)]
struct MarketInstrumentFile {
    id: String,
    #[serde(default)]
    initial_mid: Option<f64>,
    #[serde(default)]
    step_ticks: Option<u32>,
    #[serde(default)]
    spread_half_ticks: Option<u32>,
}

fn default_levels() -> usize {
    5
}
fn default_base_qty() -> f64 {
    0.5
}
fn default_interval_ms() -> u64 {
    500
}
fn default_trade_probability() -> f64 {
    0.25
}
fn default_trade_max_qty() -> f64 {
    0.1
}

/// Resolved server startup settings.
pub struct ServerSettings {
    pub exchange_config: Config,
    pub profile: RuntimeProfile,
    pub market_engine: Option<MarketEngineConfig>,
    pub scenario: Option<ScenarioLoad>,
    /// Delay between static scenario inject events (ms).
    pub scenario_interval_ms: u64,
    /// Mount the `/test/*` lab fault routes (`PAPER_LAB_FAULTS=1`).
    pub lab_faults: bool,
}

/// `PAPER_LAB_FAULTS=1` (or `true`) exposes the `/test/*` fault-injection endpoints.
pub fn lab_faults_enabled() -> bool {
    std::env::var("PAPER_LAB_FAULTS").is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
}

fn resolve_config_path() -> PathBuf {
    if let Ok(p) = std::env::var("PAPER_EXCHANGE_CONFIG") {
        return PathBuf::from(p);
    }
    let cwd_relative = PathBuf::from("config/instruments.yaml");
    if cwd_relative.exists() {
        return cwd_relative;
    }
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../config/instruments.yaml"
    ))
}

pub fn load_settings() -> Result<ServerSettings, Box<dyn std::error::Error + Send + Sync>> {
    let config_path = resolve_config_path();

    let content = std::fs::read_to_string(&config_path)?;
    let file: ExchangeConfigFile = serde_yaml::from_str(&content)?;
    let exchange_config = Config {
        instruments: file.instruments.clone(),
        initial_balances: file.initial_balances,
        fill_model: file.fill_model,
        max_trade_history: file.max_trade_history,
        linear: file.linear,
    };
    exchange_config.validate()?;

    let scenario_spec = std::env::var("PAPER_EXCHANGE_SCENARIO")
        .ok()
        .filter(|s| !s.is_empty());

    let scenario = scenario_spec
        .as_deref()
        .map(resolve_scenario)
        .transpose()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;

    let profile = resolve_profile(&scenario);

    let env_seed = std::env::var("PAPER_EXCHANGE_SEED")
        .ok()
        .and_then(|s| s.parse().ok());

    let scenario_interval_ms = std::env::var("PAPER_EXCHANGE_SCENARIO_INTERVAL_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    let market_file = file.market.or(file.dummy_ob);
    let market_engine = build_market_engine(
        &profile,
        &scenario,
        &exchange_config.instruments,
        market_file,
        env_seed,
    )?;

    Ok(ServerSettings {
        exchange_config,
        profile,
        market_engine,
        scenario,
        scenario_interval_ms,
        lab_faults: lab_faults_enabled(),
    })
}

fn resolve_profile(scenario: &Option<ScenarioLoad>) -> RuntimeProfile {
    if matches!(scenario, Some(ScenarioLoad::Static(_))) {
        return RuntimeProfile::Lab;
    }
    if scenario.is_some() {
        return RuntimeProfile::Dev;
    }
    match std::env::var("PAPER_EXCHANGE_PROFILE")
        .unwrap_or_default()
        .to_lowercase()
        .as_str()
    {
        "lab" => RuntimeProfile::Lab,
        "dev" => RuntimeProfile::Dev,
        _ => {
            let dummy_legacy = std::env::var("PAPER_EXCHANGE_DUMMY_OB")
                .is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true"));
            if dummy_legacy {
                RuntimeProfile::Dev
            } else {
                RuntimeProfile::Lab
            }
        }
    }
}

fn build_market_engine(
    profile: &RuntimeProfile,
    scenario: &Option<ScenarioLoad>,
    instruments: &[Instrument],
    market_file: Option<MarketFile>,
    env_seed: Option<u64>,
) -> Result<Option<MarketEngineConfig>, Box<dyn std::error::Error + Send + Sync>> {
    let force_dummy = std::env::var("PAPER_EXCHANGE_DUMMY_OB")
        .is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true"));

    let overlay = matches!(scenario, Some(ScenarioLoad::Overlay(_)));
    let static_only = matches!(scenario, Some(ScenarioLoad::Static(_)));

    if static_only && !force_dummy {
        return Ok(None);
    }

    let enable =
        overlay || force_dummy || matches!(profile, RuntimeProfile::Dev) || market_file.is_some();

    if !enable {
        return Ok(None);
    }

    let mf = market_file.unwrap_or_else(|| MarketFile {
        step_ticks: None,
        spread_half_ticks: None,
        step: None,
        spread_half: None,
        levels: default_levels(),
        base_qty_per_level: default_base_qty(),
        interval_ms: default_interval_ms(),
        seed: None,
        trade_probability: default_trade_probability(),
        trade_max_qty: default_trade_max_qty(),
        instruments: vec![],
    });

    if mf.step.is_some() || mf.spread_half.is_some() {
        return Err(
            "market `step` / `spread_half` were absolute prices and are no longer \
             supported; use `step_ticks` / `spread_half_ticks` (in instrument ticks)"
                .into(),
        );
    }

    let overrides: std::collections::HashMap<&str, &MarketInstrumentFile> =
        mf.instruments.iter().map(|i| (i.id.as_str(), i)).collect();

    let specs: Vec<InstrumentMarketSpec> = instruments
        .iter()
        .map(|inst| {
            let ov = overrides.get(inst.id.0.as_str());
            let initial_mid = ov
                .and_then(|o| o.initial_mid)
                .unwrap_or_else(|| default_initial_mid(&inst.id.0));
            let (derived_step, derived_spread) = default_ticks(initial_mid, &inst.tick_size);
            InstrumentMarketSpec {
                instrument_id: inst.id.clone(),
                initial_mid,
                tick_size: inst.tick_size,
                step_ticks: ov
                    .and_then(|o| o.step_ticks)
                    .or(mf.step_ticks)
                    .unwrap_or(derived_step),
                spread_half_ticks: ov
                    .and_then(|o| o.spread_half_ticks)
                    .or(mf.spread_half_ticks)
                    .unwrap_or(derived_spread),
            }
        })
        .collect();

    Ok(Some(MarketEngineConfig {
        instruments: specs,
        levels: mf.levels,
        base_qty_per_level: mf.base_qty_per_level,
        interval_ms: mf.interval_ms,
        seed: env_seed.or(mf.seed),
        trade_probability: mf.trade_probability,
        trade_max_qty: mf.trade_max_qty,
    }))
}

pub fn log_startup(settings: &ServerSettings, bound: std::net::SocketAddr) {
    tracing::info!("paper exchange listening on {}", bound);
    let scenario_desc = settings.scenario.as_ref().map(|s| match s {
        ScenarioLoad::Overlay(t) => format!("overlay({})", t.name.as_deref().unwrap_or("unnamed")),
        ScenarioLoad::Static(p) => format!("static({})", p.display()),
    });
    tracing::info!(
        "profile={:?} scenario={:?} market_engine={} instruments={}",
        settings.profile,
        scenario_desc,
        settings.market_engine.is_some(),
        settings.exchange_config.instruments.len()
    );
    tracing::info!(
        "fill_model={:?} max_trade_history={}",
        settings.exchange_config.fill_model.kind,
        settings.exchange_config.max_trade_history
    );
    if settings.lab_faults {
        tracing::warn!("PAPER_LAB_FAULTS enabled: /test/* fault-injection routes are mounted");
    }
    tracing::info!("Paper exchange URL: ws://{}/ws/trade", bound);
    tracing::info!("REST: http://{}/balances", bound);
}
