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
use crate::scenario::ScenarioLoad;
use paper_exchange_scenarios::OverlayState;
use std::collections::HashMap;
use std::sync::Arc as StdArc;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tracing_subscriber::EnvFilter;

/// Main entry point: start WebSocket server and core/dummy tasks on tokio runtime.
pub fn run() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .try_init();

    let settings = match runtime::load_settings() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("failed to load exchange config: {e}");
            std::process::exit(1);
        }
    };

    let (user_command_tx, user_command_rx) = mpsc::unbounded_channel();
    let (dummy_command_tx, dummy_command_rx) = mpsc::unbounded_channel();
    let (snapshot_tx, snapshot_rx) = mpsc::unbounded_channel();
    let (rest_query_tx, rest_query_rx) = mpsc::unbounded_channel();

    let port = runtime::listen_port();
    let host = std::env::var("PAPER_EXCHANGE_HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let addr: std::net::SocketAddr = format!("{}:{}", host, port)
        .parse()
        .expect("PAPER_EXCHANGE_HOST:PAPER_EXCHANGE_PORT must be valid");

    let exchange_config = settings.exchange_config.clone();
    let market_engine = settings.market_engine.clone();
    let scenario = settings.scenario.clone();
    let scenario_interval_ms = settings.scenario_interval_ms;

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let event_tx = session::spawn_core_task(
                exchange_config,
                user_command_rx,
                dummy_command_rx,
                snapshot_rx,
                rest_query_rx,
            );

            let market_runtime = Arc::new(Mutex::new(MarketRuntime {
                instruments: HashMap::new(),
                overlay: OverlayState::default(),
            }));

            if let Some(cfg) = market_engine {
                market_engine::spawn_market_engine(
                    cfg,
                    market_runtime.clone(),
                    dummy_command_tx.clone(),
                );
            }

            match scenario {
                Some(ScenarioLoad::Overlay(timeline)) => {
                    scenario::spawn_overlay_timeline(timeline, market_runtime);
                }
                Some(ScenarioLoad::Static(path)) => {
                    scenario::spawn_static_scenario(
                        path,
                        scenario_interval_ms,
                        user_command_tx.clone(),
                        dummy_command_tx,
                    );
                }
                None => {}
            }

            tokio::task::yield_now().await;

            let state = StdArc::new(transport::AppState {
                user_command_tx,
                snapshot_tx,
                rest_query_tx,
                event_tx: event_tx.clone(),
            });
            let app = axum::Router::new()
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
                )
                .route(
                    "/test/suppress_place_ack",
                    axum::routing::post(rest::post_suppress_place_ack),
                )
                .route(
                    "/test/reject_next_place",
                    axum::routing::post(rest::post_reject_next_place),
                )
                .route(
                    "/test/duplicate_next_fill",
                    axum::routing::post(rest::post_duplicate_next_fill),
                )
                .route(
                    "/test/reset_lab_faults",
                    axum::routing::post(rest::post_reset_lab_faults),
                )
                .route(
                    "/test/stale_orderbook",
                    axum::routing::post(rest::post_stale_orderbook),
                )
                .route(
                    "/test/disconnect_clients",
                    axum::routing::post(rest::post_disconnect_clients),
                )
                .route(
                    "/test/drop_next_place",
                    axum::routing::post(rest::post_drop_next_place),
                )
                .route(
                    "/test/partial_next_fill",
                    axum::routing::post(rest::post_partial_next_fill),
                )
                .route(
                    "/test/position_before_fill",
                    axum::routing::post(rest::post_position_before_fill),
                )
                .route(
                    "/test/emit_ws_position",
                    axum::routing::post(rest::post_emit_ws_position),
                )
                .route(
                    "/test/lab_fill_open",
                    axum::routing::post(rest::post_lab_fill_open),
                )
                .route(
                    "/test/fail_open_orders",
                    axum::routing::post(rest::post_fail_open_orders),
                )
                .with_state(state);

            let listener = tokio::net::TcpListener::bind(addr).await.expect("bind");
            let bound = listener.local_addr().expect("local_addr");
            runtime::log_startup(&settings, bound);
            axum::serve(listener, app).await.expect("serve");
        });
}

async fn ws_handler_with_state(
    axum::extract::State(state): axum::extract::State<StdArc<transport::AppState>>,
    ws: axum::extract::ws::WebSocketUpgrade,
) -> axum::response::Response {
    ws.on_upgrade(move |socket| transport::handle_socket(socket, state, true))
}

async fn ws_handler_market(
    axum::extract::State(state): axum::extract::State<StdArc<transport::AppState>>,
    ws: axum::extract::ws::WebSocketUpgrade,
) -> axum::response::Response {
    ws.on_upgrade(move |socket| transport::handle_socket(socket, state, false))
}

async fn ws_handler_trade(
    axum::extract::State(state): axum::extract::State<StdArc<transport::AppState>>,
    ws: axum::extract::ws::WebSocketUpgrade,
) -> axum::response::Response {
    ws.on_upgrade(move |socket| transport::handle_socket(socket, state, true))
}
