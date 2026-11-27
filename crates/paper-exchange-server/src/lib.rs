//! # paper-exchange-server
//!
//! Server that wraps paper-exchange-core: WebSocket, session handling, JSON wire format.
//! Fully async with tokio multi-thread runtime; no blocking std threads.

pub mod dummy_ob;
pub mod market_engine;
pub mod market_runtime;
pub mod rest;
pub mod runtime;
pub mod scenario;
pub mod session;
pub mod transport;
pub mod wire;

use crate::market_runtime::MarketRuntime;
use crate::runtime::ServerSettings;
use crate::scenario::ScenarioLoad;
use axum::Router;
use paper_exchange_scenarios::OverlayState;
use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tracing_subscriber::EnvFilter;

/// Error type for startup and serving: config, bind, and I/O failures.
pub type ServerError = Box<dyn std::error::Error + Send + Sync>;

/// Build the HTTP/WS router for an already-wired [`transport::AppState`].
///
/// The `/test/*` lab fault routes are mounted only when `state.lab_faults` is true
/// (`PAPER_LAB_FAULTS=1` for the binary); otherwise they return 404.
pub fn build_router(state: Arc<transport::AppState>) -> Router {
    let lab_faults = state.lab_faults;
    let mut app = Router::new()
        .route("/ws", axum::routing::get(ws_handler_with_state))
        .route("/ws/market", axum::routing::get(ws_handler_market))
        .route("/ws/trade", axum::routing::get(ws_handler_trade))
        .route("/balances", axum::routing::get(rest::get_balances))
        .route("/open_orders", axum::routing::get(rest::get_open_orders))
        .route("/positions", axum::routing::get(rest::get_positions))
        .route("/trades", axum::routing::get(rest::get_trades))
        .route(
            "/exchange_info",
            axum::routing::get(rest::get_exchange_info),
        )
        .route(
            "/orders/by_client_id/:client_order_id",
            axum::routing::get(rest::get_order_by_client_id),
        );
    if lab_faults {
        app = app.merge(lab_fault_routes());
    }
    app.with_state(state)
}

/// Lab/test fault-injection routes. Never mounted unless explicitly enabled.
fn lab_fault_routes() -> Router<Arc<transport::AppState>> {
    use axum::routing::post;
    Router::new()
        .route(
            "/test/suppress_place_ack",
            post(rest::post_suppress_place_ack),
        )
        .route(
            "/test/reject_next_place",
            post(rest::post_reject_next_place),
        )
        .route(
            "/test/duplicate_next_fill",
            post(rest::post_duplicate_next_fill),
        )
        .route("/test/reset_lab_faults", post(rest::post_reset_lab_faults))
        .route("/test/stale_orderbook", post(rest::post_stale_orderbook))
        .route(
            "/test/disconnect_clients",
            post(rest::post_disconnect_clients),
        )
        .route("/test/drop_next_place", post(rest::post_drop_next_place))
        .route(
            "/test/partial_next_fill",
            post(rest::post_partial_next_fill),
        )
        .route(
            "/test/position_before_fill",
            post(rest::post_position_before_fill),
        )
        .route("/test/emit_ws_position", post(rest::post_emit_ws_position))
        .route("/test/lab_fill_open", post(rest::post_lab_fill_open))
        .route("/test/emit_place_ack", post(rest::post_emit_place_ack))
        .route("/test/fail_open_orders", post(rest::post_fail_open_orders))
}

/// Wire the core loop, market engine and scenario for `settings`, then serve HTTP/WS on
/// `listener` until `shutdown` resolves.
///
/// Returns `Err` for invalid exchange config or an I/O failure while serving. Background tasks
/// started here (market engine, core loop) are stopped before returning.
pub async fn serve<F>(
    listener: TcpListener,
    settings: ServerSettings,
    shutdown: F,
) -> Result<(), ServerError>
where
    F: Future<Output = ()> + Send + 'static,
{
    settings.exchange_config.validate()?;
    let bound = listener.local_addr()?;

    let (user_command_tx, user_command_rx) = mpsc::unbounded_channel();
    let (dummy_command_tx, dummy_command_rx) = mpsc::unbounded_channel();
    let (snapshot_tx, snapshot_rx) = mpsc::unbounded_channel();
    let (rest_query_tx, rest_query_rx) = mpsc::unbounded_channel();

    let event_tx = session::spawn_core_task(
        settings.exchange_config.clone(),
        user_command_rx,
        dummy_command_rx,
        snapshot_rx,
        rest_query_rx,
    );

    let market_runtime = Arc::new(Mutex::new(MarketRuntime {
        instruments: HashMap::new(),
        overlay: OverlayState::default(),
    }));

    let market_task = settings.market_engine.clone().map(|cfg| {
        market_engine::spawn_market_engine(cfg, market_runtime.clone(), dummy_command_tx.clone())
    });
    // Only the engine task should keep the dummy channel open, so aborting it stops the core.
    drop(dummy_command_tx);

    match settings.scenario.clone() {
        Some(ScenarioLoad::Overlay(timeline)) => {
            scenario::spawn_overlay_timeline(timeline, market_runtime);
        }
        Some(ScenarioLoad::Static(path)) => {
            // One ordered channel for the whole scenario (book/trade/place in file order).
            scenario::spawn_static_scenario(
                path,
                settings.scenario_interval_ms,
                user_command_tx.clone(),
            );
        }
        None => {}
    }

    tokio::task::yield_now().await;

    let state = Arc::new(transport::AppState::new(
        user_command_tx,
        snapshot_tx,
        rest_query_tx,
        event_tx,
        settings.lab_faults,
    ));
    let app = build_router(state);

    runtime::log_startup(&settings, bound);
    let result = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await;

    if let Some(task) = market_task {
        task.abort();
    }
    result.map_err(Into::into)
}

/// Main entry point: load settings from the environment, bind, and serve forever.
///
/// Thin wrapper over [`serve`] for the binaries; returns `Err` instead of exiting so the caller
/// decides the exit code.
pub fn run() -> Result<(), ServerError> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse()?))
        .try_init();

    let settings =
        runtime::load_settings().map_err(|e| format!("failed to load exchange config: {e}"))?;

    let port = runtime::listen_port();
    let host = std::env::var("PAPER_EXCHANGE_HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let addr: std::net::SocketAddr = format!("{host}:{port}")
        .parse()
        .map_err(|e| format!("PAPER_EXCHANGE_HOST:PAPER_EXCHANGE_PORT must be valid: {e}"))?;

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(async {
            let listener = TcpListener::bind(addr).await?;
            serve(listener, settings, std::future::pending()).await
        })
}

async fn ws_handler_with_state(
    axum::extract::State(state): axum::extract::State<Arc<transport::AppState>>,
    ws: axum::extract::ws::WebSocketUpgrade,
) -> axum::response::Response {
    ws.on_upgrade(move |socket| transport::handle_socket(socket, state, true))
}

async fn ws_handler_market(
    axum::extract::State(state): axum::extract::State<Arc<transport::AppState>>,
    ws: axum::extract::ws::WebSocketUpgrade,
) -> axum::response::Response {
    ws.on_upgrade(move |socket| transport::handle_socket(socket, state, false))
}

async fn ws_handler_trade(
    axum::extract::State(state): axum::extract::State<Arc<transport::AppState>>,
    ws: axum::extract::ws::WebSocketUpgrade,
) -> axum::response::Response {
    ws.on_upgrade(move |socket| transport::handle_socket(socket, state, true))
}
