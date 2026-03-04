//! Integration test: connect to server, send place_order, receive execution.

use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use tokio_tungstenite::{connect_async, tungstenite::Message};

#[tokio::test]
async fn client_connects_and_places_order() {
    std::env::set_var("PAPER_EXCHANGE_PORT", "19999");
    let _server = std::thread::spawn(|| {
        paper_exchange_server::run();
    });

    tokio::time::sleep(Duration::from_millis(500)).await;

    let url = "ws://127.0.0.1:19999/ws";
    let (ws_stream, _) = connect_async(url).await.expect("connect");
    let (mut write, mut read) = ws_stream.split();

    let place_order = r#"{"type":"place_order","request":{"instrument_id":"BTC-USDT","side":"Buy","order_type":"Limit","quantity":"0.1","limit_price":"50000","client_order_id":null}}"#;
    write
        .send(Message::Text(place_order.into()))
        .await
        .expect("send");

    let mut saw_execution = false;
    for _ in 0..10 {
        if let Some(Ok(msg)) = read.next().await {
            if let Message::Text(t) = msg {
                if t.contains("execution") || t.contains("Execution") {
                    saw_execution = true;
                    break;
                }
            }
        }
    }

    assert!(saw_execution, "expected to receive execution report");
}
