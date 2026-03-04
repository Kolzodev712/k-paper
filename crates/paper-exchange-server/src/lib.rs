//! # paper-exchange-server
//!
//! Server that wraps paper-exchange-core: WebSocket, session handling, JSON wire format.

pub mod dummy_ob;
pub mod session;
pub mod transport;
pub mod wire;

use std::sync::mpsc;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

/// Main entry point: start WebSocket server and core thread.
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .init();

    let (command_tx, command_rx) = mpsc::channel();
    let event_tx = session::spawn_core_thread(command_rx);

    if std::env::var("PAPER_EXCHANGE_DUMMY_OB")
        .is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
    {
        let config = dummy_ob::DummyObConfig::default();
        dummy_ob::spawn_dummy_ob(config, command_tx.clone());
        tracing::info!("dummy orderbook generator enabled (BTC-USDT)");
    }

    let state = Arc::new(transport::AppState {
        command_tx,
        event_tx: event_tx.clone(),
    });

    let app = axum::Router::new()
        .route("/ws", axum::routing::get(ws_handler_with_state))
        .with_state(state);

    let port = std::env::var("PAPER_EXCHANGE_PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(0);
    let host = std::env::var("PAPER_EXCHANGE_HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let addr: std::net::SocketAddr = format!("{}:{}", host, port)
        .parse()
        .expect("PAPER_EXCHANGE_HOST:PAPER_EXCHANGE_PORT must be valid");
    tracing::info!("paper exchange WebSocket on ws://{}/ws", addr);

    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let listener = tokio::net::TcpListener::bind(addr).await.expect("bind");
            axum::serve(listener, app).await.expect("serve");
        });
}

async fn ws_handler_with_state(
    axum::extract::State(state): axum::extract::State<Arc<transport::AppState>>,
    ws: axum::extract::ws::WebSocketUpgrade,
) -> axum::response::Response {
    ws.on_upgrade(move |socket| transport::handle_socket(socket, state))
}
