//! Server runtime: load YAML config, resolve lab/dev profile from environment.

use crate::dummy_ob::DummyObConfig;
use paper_exchange_core::config::{Config, FillModelConfig, InitialBalance};
use paper_exchange_core::types::{Instrument, InstrumentId};
use rust_decimal::Decimal;
use serde::Deserialize;
use std::path::PathBuf;
use std::str::FromStr;

fn decimal(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap_or(Decimal::ZERO)
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
    dummy_ob: Option<DummyObFile>,
}

fn default_max_trade_history() -> usize {
    10_000
}

#[derive(Debug, Deserialize)]
struct DummyObFile {
    #[serde(default)]
    instrument_id: Option<String>,
    #[serde(default = "default_initial_mid")]
    initial_mid: f64,
    #[serde(default = "default_step")]
    step: f64,
    #[serde(default = "default_spread_half")]
    spread_half: f64,
    #[serde(default = "default_levels")]
    levels: usize,
    #[serde(default = "default_tick_size")]
    tick_size: String,
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
}

fn default_initial_mid() -> f64 {
    50_000.0
}
fn default_step() -> f64 {
    100.0
}
fn default_spread_half() -> f64 {
    50.0
}
fn default_levels() -> usize {
    5
}
fn default_tick_size() -> String {
    "0.01".into()
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

impl DummyObFile {
    fn into_config(self, env_seed: Option<u64>) -> DummyObConfig {
        let instrument_id =
            InstrumentId::new(self.instrument_id.unwrap_or_else(|| "BTC-USDT".to_string()));
        DummyObConfig {
            instrument_id,
            initial_mid: self.initial_mid,
            step: self.step,
            spread_half: self.spread_half,
            levels: self.levels,
            tick_size: decimal(&self.tick_size),
            base_qty_per_level: self.base_qty_per_level,
            interval_ms: self.interval_ms,
            seed: env_seed.or(self.seed),
            trade_probability: self.trade_probability,
            trade_max_qty: self.trade_max_qty,
        }
    }
}

/// Resolved server startup settings.
pub struct ServerSettings {
    pub exchange_config: Config,
    pub profile: RuntimeProfile,
    pub dummy_ob: Option<DummyObConfig>,
    pub scenario_path: Option<PathBuf>,
    /// Delay between scripted scenario events (ms); 0 = run back-to-back at startup.
    pub scenario_interval_ms: u64,
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
        instruments: file.instruments,
        initial_balances: file.initial_balances,
        fill_model: file.fill_model,
        max_trade_history: file.max_trade_history,
    };

    let scenario_path = std::env::var("PAPER_EXCHANGE_SCENARIO")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from);

    let profile = resolve_profile(&scenario_path);

    let env_seed = std::env::var("PAPER_EXCHANGE_SEED")
        .ok()
        .and_then(|s| s.parse().ok());

    let scenario_interval_ms = std::env::var("PAPER_EXCHANGE_SCENARIO_INTERVAL_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    let dummy_ob = resolve_dummy_ob(&profile, &scenario_path, file.dummy_ob, env_seed);

    Ok(ServerSettings {
        exchange_config,
        profile,
        dummy_ob,
        scenario_path,
        scenario_interval_ms,
    })
}

fn resolve_profile(scenario_path: &Option<PathBuf>) -> RuntimeProfile {
    if scenario_path.is_some() {
        return RuntimeProfile::Lab;
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

fn resolve_dummy_ob(
    profile: &RuntimeProfile,
    scenario_path: &Option<PathBuf>,
    file: Option<DummyObFile>,
    env_seed: Option<u64>,
) -> Option<DummyObConfig> {
    let force_dummy = std::env::var("PAPER_EXCHANGE_DUMMY_OB")
        .is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true"));

    if scenario_path.is_some() && !force_dummy {
        return None;
    }

    match profile {
        RuntimeProfile::Lab if !force_dummy => None,
        RuntimeProfile::Lab | RuntimeProfile::Dev => {
            file.map(|f| f.into_config(env_seed)).or_else(|| {
                if matches!(profile, RuntimeProfile::Dev) || force_dummy {
                    Some(DummyObConfig::with_seed(env_seed))
                } else {
                    None
                }
            })
        }
    }
}

pub fn log_startup(settings: &ServerSettings, bound: std::net::SocketAddr) {
    tracing::info!("paper exchange listening on {}", bound);
    tracing::info!(
        "profile={:?} scenario={:?} dummy_ob={}",
        settings.profile,
        settings
            .scenario_path
            .as_ref()
            .map(|p| p.display().to_string()),
        settings.dummy_ob.is_some()
    );
    tracing::info!(
        "fill_model={:?} max_trade_history={}",
        settings.exchange_config.fill_model.kind,
        settings.exchange_config.max_trade_history
    );
    tracing::info!("Paper exchange URL: ws://{}/ws/trade", bound);
    tracing::info!("REST: http://{}/balances", bound);
}
