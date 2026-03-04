//! # paper-exchange-server
//!
//! Server that wraps paper-exchange-core: WebSocket, session handling, JSON wire format.
//! Fully async with tokio multi-thread runtime; no blocking std threads.

pub mod dummy_ob;
pub mod session;
pub mod transport;
pub mod wire;

use std::sync::Arc;
use tokio::sync::mpsc;
use tracing_subscriber::EnvFilter;

/// Main entry point: start WebSocket server and core/dummy tasks on tokio runtime.
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .init();

    let (user_command_tx, user_command_rx) = mpsc::unbounded_channel();
    let (dummy_command_tx, dummy_command_rx) = mpsc::unbounded_channel();
    let (snapshot_tx, snapshot_rx) = mpsc::unbounded_channel();

    let port = std::env::var("PAPER_EXCHANGE_PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(0);
    let host = std::env::var("PAPER_EXCHANGE_HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let addr: std::net::SocketAddr = format!("{}:{}", host, port)
        .parse()
        .expect("PAPER_EXCHANGE_HOST:PAPER_EXCHANGE_PORT must be valid");
    let dummy_ob_enabled = std::env::var("PAPER_EXCHANGE_DUMMY_OB")
        .is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true"));

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            // Spawn core and dummy tasks only after the runtime is running.
            let event_tx = session::spawn_core_task(user_command_rx, dummy_command_rx, snapshot_rx);
            if dummy_ob_enabled {
                dummy_ob::spawn_dummy_ob(dummy_ob::DummyObConfig::default(), dummy_command_tx);
                tracing::info!("dummy orderbook generator enabled (BTC-USDT)");
            }
            // Yield so core (and dummy) tasks run and are waiting before we accept connections.
            tokio::task::yield_now().await;

            let state = Arc::new(transport::AppState {
                user_command_tx,
                snapshot_tx,
                event_tx: event_tx.clone(),
            });
            let app = axum::Router::new()
                .route("/ws", axum::routing::get(ws_handler_with_state))
                .with_state(state);

            let listener = tokio::net::TcpListener::bind(addr).await.expect("bind");
            let bound = listener.local_addr().expect("local_addr");
            let paper_url = format!("ws://{}/ws", bound);
            tracing::info!("paper exchange listening on {}", bound);
            tracing::info!("Paper exchange URL (use in client config): {}", paper_url);
            axum::serve(listener, app).await.expect("serve");
        });
}

async fn ws_handler_with_state(
    axum::extract::State(state): axum::extract::State<Arc<transport::AppState>>,
    ws: axum::extract::ws::WebSocketUpgrade,
) -> axum::response::Response {
    ws.on_upgrade(move |socket| transport::handle_socket(socket, state))
}
