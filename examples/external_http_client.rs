//! Minimal external client against a running k-paper HTTP/WS server.
//! No trading_system dependency.
//!
//! ```text
//! cargo run --example external_http_client
//! PAPER_HOST=127.0.0.1 PAPER_PORT=19999 cargo run --example external_http_client
//! ```

use futures_util::{SinkExt, StreamExt};
use std::env;
use std::process::ExitCode;
use std::time::Duration;
use tokio::time::timeout;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

#[tokio::main]
async fn main() -> ExitCode {
    let host = env::var("PAPER_HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port = env::var("PAPER_PORT").unwrap_or_else(|_| "19999".into());
    let base = format!("http://{host}:{port}");
    let ws_url = format!("ws://{host}:{port}/ws/trade");

    match run(&base, &ws_url).await {
        Ok(()) => {
            eprintln!("OK: external client finished");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("external client failed: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn run(base: &str, ws_url: &str) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;

    let info = client
        .get(format!("{base}/exchange_info"))
        .send()
        .await
        .map_err(|e| format!("exchange_info: {e}"))?
        .error_for_status()
        .map_err(|e| format!("exchange_info status: {e}"))?
        .text()
        .await
        .map_err(|e| e.to_string())?;
    if !info.contains("BTC-USDT") && !info.contains("BTC/USDT") {
        return Err(format!("exchange_info missing BTC-USDT: {info}"));
    }
    println!("== exchange_info ok ==");

    let balances_before = client
        .get(format!("{base}/balances"))
        .send()
        .await
        .map_err(|e| format!("balances: {e}"))?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .text()
        .await
        .map_err(|e| e.to_string())?;
    println!("== balances (before) ==\n{balances_before}");

    let (mut ws, _) = connect_async(ws_url)
        .await
        .map_err(|e| format!("ws connect {ws_url}: {e}"))?;

    let place = serde_json::json!({
        "type": "place_order",
        "request": {
            "instrument_id": "BTC-USDT",
            "side": "Buy",
            "order_type": "Limit",
            "quantity": "0.001",
            "limit_price": "50000",
            "client_order_id": "ext-demo-1"
        }
    });
    ws.send(Message::Text(place.to_string()))
        .await
        .map_err(|e| format!("ws send: {e}"))?;
    println!("== place_order sent ==");

    let mut saw_execution_or_error = false;
    let deadline = timeout(Duration::from_secs(10), async {
        while let Some(msg) = ws.next().await {
            let msg = msg.map_err(|e| format!("ws read: {e}"))?;
            let text = match msg {
                Message::Text(t) => t,
                Message::Binary(b) => String::from_utf8_lossy(&b).into_owned(),
                Message::Close(_) => break,
                _ => continue,
            };
            println!("ws: {text}");
            if text.contains("\"execution\"")
                || text.contains("\"type\":\"error\"")
                || text.contains("\"type\": \"error\"")
                || text.contains("PlaceAck")
                || text.contains("place_ack")
                || text.contains("\"ack\"")
            {
                saw_execution_or_error = true;
                break;
            }
        }
        Ok::<(), String>(())
    })
    .await
    .map_err(|_| "timed out waiting for place response".to_string())??;

    let _ = deadline;
    if !saw_execution_or_error {
        return Err("no execution/ack/error observed on trade socket".into());
    }

    let orders = client
        .get(format!("{base}/open_orders?instrument_id=BTC-USDT"))
        .send()
        .await
        .map_err(|e| format!("open_orders: {e}"))?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .text()
        .await
        .map_err(|e| e.to_string())?;
    println!("== open_orders ==\n{orders}");

    let balances_after = client
        .get(format!("{base}/balances"))
        .send()
        .await
        .map_err(|e| format!("balances after: {e}"))?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .text()
        .await
        .map_err(|e| e.to_string())?;
    println!("== balances (after) ==\n{balances_after}");
    Ok(())
}
