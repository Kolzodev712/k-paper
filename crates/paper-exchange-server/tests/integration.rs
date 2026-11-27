//! Integration tests: one server, verify snapshot + orderbook + execution; dedicated /ws/trade returns execution.
//! Run with: cargo test -p paper-exchange-server --test integration
//! Servers bind ephemeral ports via `serve()`, so these tests are safe to run in parallel.

use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use tokio_tungstenite::{connect_async, tungstenite::Message};

/// Start a dev-profile server (random-walk book) on an ephemeral port inside this runtime.
/// Ephemeral ports + `serve()` keep the tests independent, so they can run in parallel.
async fn start_dev_server() -> std::net::SocketAddr {
    std::env::set_var("PAPER_EXCHANGE_DUMMY_OB", "1");
    let settings = paper_exchange_server::runtime::load_settings().expect("load settings");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(paper_exchange_server::serve(
        listener,
        settings,
        std::future::pending(),
    ));
    addr
}

/// Trade endpoint must return execution when client sends place_order on that connection.
#[tokio::test]
async fn trade_endpoint_returns_execution_for_place_order() {
    let addr = start_dev_server().await;
    tokio::time::sleep(Duration::from_millis(500)).await;

    let url = format!("ws://{addr}/ws/trade");
    let (ws_stream, _) = connect_async(&url).await.expect("connect to /ws/trade");
    let (mut write, mut read) = ws_stream.split();

    let place_order = r#"{"type":"place_order","request":{"instrument_id":"BTC-USDT","side":"Buy","order_type":"Limit","quantity":"0.1","limit_price":"50000","client_order_id":null}}"#;
    write
        .send(Message::Text(place_order.into()))
        .await
        .expect("send");

    let mut saw_execution = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    while tokio::time::Instant::now() < deadline {
        let timeout = tokio::time::timeout(Duration::from_millis(200), read.next());
        if let Ok(Some(Ok(Message::Text(t)))) = timeout.await {
            if t.contains("execution") || t.contains("Execution") {
                saw_execution = true;
                break;
            }
        }
    }
    assert!(
        saw_execution,
        "expected execution report on /ws/trade after place_order"
    );
}

/// Single test: start server with dummy OB, connect, verify balance + orderbook, then place_order and verify execution,
/// then cancel_all_orders and verify CancelAllAck.
#[tokio::test]
async fn client_receives_snapshot_orderbook_and_execution() {
    let addr = start_dev_server().await;
    tokio::time::sleep(Duration::from_millis(800)).await;

    let url = format!("ws://{addr}/ws");
    let (ws_stream, _) = connect_async(&url).await.expect("connect");
    let (mut write, mut read) = ws_stream.split();

    // 1) Collect messages until we have balance and orderbook (snapshot on connect + dummy OB)
    let mut saw_balance = false;
    let mut saw_orderbook = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while tokio::time::Instant::now() < deadline {
        let timeout = tokio::time::timeout(Duration::from_millis(200), read.next());
        match timeout.await {
            Ok(Some(Ok(Message::Text(t)))) => {
                if t.contains("\"type\":\"balance\"") || t.contains("\"type\":\"Balance\"") {
                    saw_balance = true;
                }
                if t.contains("\"type\":\"orderbook\"") || t.contains("\"type\":\"Orderbook\"") {
                    saw_orderbook = true;
                }
                if saw_balance && saw_orderbook {
                    break;
                }
            }
            Ok(Some(Ok(_))) => {}
            Ok(Some(Err(_))) => break,
            Ok(None) => break,
            Err(_) => {}
        }
    }

    assert!(
        saw_balance,
        "expected at least one balance message (snapshot on connect)"
    );
    assert!(
        saw_orderbook,
        "expected at least one orderbook message (dummy OB)"
    );

    // 2) Place order and expect execution
    let place_order = r#"{"type":"place_order","request":{"instrument_id":"BTC-USDT","side":"Buy","order_type":"Limit","quantity":"0.1","limit_price":"50000","client_order_id":null}}"#;
    write
        .send(Message::Text(place_order.into()))
        .await
        .expect("send");

    let mut saw_execution = false;
    let deadline2 = tokio::time::Instant::now() + Duration::from_secs(2);
    while tokio::time::Instant::now() < deadline2 {
        let timeout = tokio::time::timeout(Duration::from_millis(200), read.next());
        if let Ok(Some(Ok(Message::Text(t)))) = timeout.await {
            if t.contains("execution") || t.contains("Execution") {
                saw_execution = true;
                break;
            }
        }
    }
    assert!(
        saw_execution,
        "expected to receive execution report after place_order"
    );

    // 3) Cancel all orders and expect CancelAllAck
    let cancel_all = r#"{"type":"cancel_all_orders"}"#;
    write
        .send(Message::Text(cancel_all.into()))
        .await
        .expect("send");

    let mut saw_cancel_all_ack = false;
    let deadline3 = tokio::time::Instant::now() + Duration::from_secs(2);
    while tokio::time::Instant::now() < deadline3 {
        let timeout = tokio::time::timeout(Duration::from_millis(200), read.next());
        if let Ok(Some(Ok(Message::Text(t)))) = timeout.await {
            if t.contains("CancelAllAck") || t.contains("\"type\":\"cancel_all_ack\"") {
                saw_cancel_all_ack = true;
                break;
            }
        }
    }
    assert!(
        saw_cancel_all_ack,
        "expected CancelAllAck after cancel_all_orders"
    );
}

// ---------- serve() / build_router() ----------

mod serve_api {
    use paper_exchange_core::config::{Config, FillModelKind, InitialBalance};
    use paper_exchange_core::types::{Asset, Instrument, InstrumentId};
    use paper_exchange_server::runtime::{RuntimeProfile, ServerSettings};
    use rust_decimal::Decimal;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    fn settings(lab_faults: bool) -> ServerSettings {
        ServerSettings {
            exchange_config: Config {
                instruments: vec![Instrument {
                    id: InstrumentId::new("BTC-USDT"),
                    symbol: "BTC-USDT".into(),
                    base_asset: Asset("BTC".into()),
                    quote_asset: Asset("USDT".into()),
                    tick_size: Decimal::new(1, 2),
                    lot_size: Decimal::new(1, 3),
                    min_notional: Decimal::new(10, 0),
                    instrument_type: None,
                }],
                initial_balances: vec![InitialBalance {
                    asset: Asset("USDT".into()),
                    amount: Decimal::new(1000, 0),
                }],
                ..Config::default()
            },
            profile: RuntimeProfile::Lab,
            market_engine: None,
            scenario: None,
            scenario_interval_ms: 0,
            lab_faults,
        }
    }

    /// Minimal HTTP/1.1 request; returns the numeric status code.
    async fn http_status(addr: std::net::SocketAddr, method: &str, path: &str) -> u16 {
        let mut s = TcpStream::connect(addr).await.expect("connect");
        let req = format!(
            "{method} {path} HTTP/1.1\r\nhost: x\r\ncontent-type: application/json\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{{}}"
        );
        s.write_all(req.as_bytes()).await.expect("write");
        let mut buf = String::new();
        s.read_to_string(&mut buf).await.expect("read");
        buf.split_whitespace()
            .nth(1)
            .and_then(|c| c.parse().ok())
            .expect("status line")
    }

    async fn start(
        settings: ServerSettings,
    ) -> (
        std::net::SocketAddr,
        tokio::sync::oneshot::Sender<()>,
        tokio::task::JoinHandle<Result<(), paper_exchange_server::ServerError>>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
        let task = tokio::spawn(paper_exchange_server::serve(listener, settings, async {
            let _ = stop_rx.await;
        }));
        (addr, stop_tx, task)
    }

    #[tokio::test]
    async fn lab_routes_are_not_mounted_by_default() {
        let (addr, stop, task) = start(settings(false)).await;
        assert_eq!(http_status(addr, "GET", "/balances").await, 200);
        assert_eq!(
            http_status(addr, "POST", "/test/fail_open_orders").await,
            404
        );
        assert_eq!(http_status(addr, "GET", "/open_orders").await, 200);
        stop.send(()).expect("shutdown");
        task.await.expect("join").expect("serve ok");
    }

    #[tokio::test]
    async fn lab_routes_work_when_enabled_and_shutdown_is_graceful() {
        let (addr, stop, task) = start(settings(true)).await;
        assert_eq!(http_status(addr, "GET", "/open_orders").await, 200);
        assert_eq!(
            http_status(addr, "POST", "/test/fail_open_orders").await,
            204
        );
        assert_eq!(http_status(addr, "GET", "/open_orders").await, 503);
        assert_eq!(
            http_status(addr, "POST", "/test/reset_lab_faults").await,
            204
        );
        assert_eq!(http_status(addr, "GET", "/open_orders").await, 200);
        stop.send(()).expect("shutdown");
        task.await.expect("join").expect("serve ok");
    }

    #[tokio::test]
    async fn serve_rejects_unsupported_fill_model() {
        let mut bad = settings(false);
        bad.exchange_config.fill_model.kind = FillModelKind::TradeThrough;
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let err = paper_exchange_server::serve(listener, bad, std::future::pending())
            .await
            .expect_err("must reject trade_through");
        assert!(err.to_string().contains("trade_through"), "{err}");

        let mut bad = settings(false);
        bad.exchange_config.fill_model.partial_fills = true;
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let err = paper_exchange_server::serve(listener, bad, std::future::pending())
            .await
            .expect_err("must reject partial_fills");
        assert!(err.to_string().contains("partial_fills"), "{err}");
    }
}
