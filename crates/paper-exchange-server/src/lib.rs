//! # paper-exchange-server
//!
//! Server that wraps paper-exchange-core: WebSocket, session handling, JSON wire format.
//! Fully async with tokio multi-thread runtime; no blocking std threads.

pub mod dummy_ob;
pub mod rest;
pub mod runtime;
pub mod scenario;
pub mod session;
pub mod transport;
pub mod wire;

use std::sync::Arc;
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

    let port = std::env::var("PAPER_EXCHANGE_PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(0);
    let host = std::env::var("PAPER_EXCHANGE_HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let addr: std::net::SocketAddr = format!("{}:{}", host, port)
        .parse()
        .expect("PAPER_EXCHANGE_HOST:PAPER_EXCHANGE_PORT must be valid");

    let exchange_config = settings.exchange_config.clone();
    let dummy_ob = settings.dummy_ob.clone();
    let scenario_path = settings.scenario_path.clone();
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

            if let Some(cfg) = dummy_ob {
                dummy_ob::spawn_dummy_ob(cfg, dummy_command_tx.clone());
            }

            if let Some(path) = scenario_path {
                scenario::spawn_scenario(
                    path,
                    scenario_interval_ms,
                    user_command_tx.clone(),
                    dummy_command_tx,
                );
            }

            tokio::task::yield_now().await;

            let state = Arc::new(transport::AppState {
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
                .route("/trades", axum::routing::get(rest::get_trades))
                .route(
                    "/exchange_info",
                    axum::routing::get(rest::get_exchange_info),
                )
                .with_state(state);

            let listener = tokio::net::TcpListener::bind(addr).await.expect("bind");
            let bound = listener.local_addr().expect("local_addr");
            runtime::log_startup(&settings, bound);
            axum::serve(listener, app).await.expect("serve");
        });
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
