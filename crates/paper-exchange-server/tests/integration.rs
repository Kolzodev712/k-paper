//! Integration tests: one server, verify snapshot + orderbook + execution; dedicated /ws/trade returns execution.
//! Run with: cargo test -p paper-exchange-server --test integration
//! When running with other packages use: cargo test --all -- --test-threads=1 (tests use fixed ports).

use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use tokio_tungstenite::{connect_async, tungstenite::Message};

/// Trade endpoint must return execution when client sends place_order on that connection.
#[tokio::test]
async fn trade_endpoint_returns_execution_for_place_order() {
    std::env::set_var("PAPER_EXCHANGE_PORT", "19996");
    let _server = std::thread::spawn(|| paper_exchange_server::run());
    tokio::time::sleep(Duration::from_millis(500)).await;

    let url = "ws://127.0.0.1:19996/ws/trade";
    let (ws_stream, _) = connect_async(url).await.expect("connect to /ws/trade");
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
    std::env::set_var("PAPER_EXCHANGE_PORT", "19999");
    std::env::set_var("PAPER_EXCHANGE_DUMMY_OB", "1");
    let _server = std::thread::spawn(|| {
        paper_exchange_server::run();
    });

    tokio::time::sleep(Duration::from_millis(800)).await;

    let url = "ws://127.0.0.1:19999/ws";
    let (ws_stream, _) = connect_async(url).await.expect("connect");
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
