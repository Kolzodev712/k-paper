//! WebSocket server: accept connections, route commands to core, broadcast events to clients.

use crate::wire::{BroadcastEvent, ClientMessage, ServerMessage};
use axum::extract::ws::{Message, WebSocket};
use axum::extract::WebSocketUpgrade;
use axum::response::Response;
use futures_util::stream::StreamExt;
use futures_util::SinkExt;
use std::str::FromStr;
use std::sync::mpsc;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::info;

pub async fn ws_handler(ws: WebSocketUpgrade, state: Arc<AppState>) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

pub struct AppState {
    pub command_tx: mpsc::Sender<Command>,
    pub event_tx: broadcast::Sender<BroadcastEvent>,
}

#[derive(Debug)]
pub enum Command {
    PlaceOrder(paper_exchange_core::types::OrderRequest),
    Cancel(paper_exchange_core::types::CancelRequest),
    Amend(paper_exchange_core::types::AmendRequest),
    InjectOrderbook(paper_exchange_core::types::OrderbookSnapshot),
    InjectTrade(paper_exchange_core::types::PublicTrade),
}

pub async fn handle_socket(socket: WebSocket, state: Arc<AppState>) {
    let (mut sender, mut receiver) = socket.split();
    let mut event_rx = state.event_tx.subscribe();
    let command_tx = state.command_tx.clone();

    let send_task = tokio::spawn(async move {
        while let Ok(ev) = event_rx.recv().await {
            let msg: ServerMessage = match ev {
                BroadcastEvent::Market(m) => m.into(),
                BroadcastEvent::Private(p) => p.into(),
            };
            if let Ok(serialized) = serde_json::to_string(&msg) {
                if sender.send(Message::Text(serialized)).await.is_err() {
                    break;
                }
            }
        }
    });

    while let Some(Ok(msg)) = receiver.next().await {
        if let Message::Text(text) = msg {
            if let Ok(client_msg) = serde_json::from_str::<ClientMessage>(&text) {
                match client_msg {
                    ClientMessage::PlaceOrder { request } => {
                        let _ = command_tx.send(Command::PlaceOrder(request));
                    }
                    ClientMessage::CancelOrder { order_id } => {
                        if let Ok(uuid) = uuid::Uuid::parse_str(&order_id) {
                            let _ = command_tx.send(Command::Cancel(
                                paper_exchange_core::types::CancelRequest::ByOrderId(
                                    paper_exchange_core::types::OrderId(uuid),
                                ),
                            ));
                        }
                    }
                    ClientMessage::CancelByClientOrderId { client_order_id } => {
                        let _ = command_tx.send(Command::Cancel(
                            paper_exchange_core::types::CancelRequest::ByClientOrderId(
                                paper_exchange_core::types::ClientOrderId(client_order_id),
                            ),
                        ));
                    }
                    ClientMessage::AmendOrder {
                        order_id,
                        quantity,
                        limit_price,
                    } => {
                        if let Ok(uuid) = uuid::Uuid::parse_str(&order_id) {
                            let req = paper_exchange_core::types::AmendRequest {
                                order_id: paper_exchange_core::types::OrderId(uuid),
                                quantity: quantity
                                    .and_then(|s| rust_decimal::Decimal::from_str(&s).ok()),
                                limit_price: limit_price
                                    .and_then(|s| rust_decimal::Decimal::from_str(&s).ok()),
                            };
                            let _ = command_tx.send(Command::Amend(req));
                        }
                    }
                }
            }
        }
    }

    send_task.abort();
    info!("WebSocket client disconnected");
}
