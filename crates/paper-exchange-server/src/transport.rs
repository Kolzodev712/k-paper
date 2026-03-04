//! WebSocket server: accept connections, route commands to core, broadcast events to clients.

use crate::wire::{BroadcastEvent, ClientMessage, ServerMessage};
use axum::extract::ws::{Message, WebSocket};
use axum::extract::WebSocketUpgrade;
use axum::response::Response;
use futures_util::stream::StreamExt;
use futures_util::SinkExt;
use paper_exchange_core::types::{Balance, Execution, Position};
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tracing::info;

pub type SnapshotData = (Vec<Balance>, Vec<Position>);

/// Snapshot request: broadcast to all (on connect) or reply on a oneshot (get_account_snapshot).
pub enum SnapshotRequest {
    Broadcast,
    Reply(oneshot::Sender<SnapshotData>),
}

pub async fn ws_handler(ws: WebSocketUpgrade, state: Arc<AppState>) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

pub struct AppState {
    pub user_command_tx: mpsc::UnboundedSender<Command>,
    pub snapshot_tx: mpsc::UnboundedSender<SnapshotRequest>,
    pub event_tx: broadcast::Sender<BroadcastEvent>,
}

/// Optional reply for PlaceOrder: core sends the initial Execution (ack) on this oneshot so the requesting connection gets it directly.
pub type PlaceOrderReply = oneshot::Sender<Execution>;
/// Optional reply for Cancel/Amend: core sends the order_id (UUID string) so the requesting connection gets an ack directly.
pub type OrderAckReply = oneshot::Sender<String>;

#[derive(Debug)]
pub enum Command {
    PlaceOrder(
        paper_exchange_core::types::OrderRequest,
        Option<PlaceOrderReply>,
    ),
    Cancel(
        paper_exchange_core::types::CancelRequest,
        Option<OrderAckReply>,
    ),
    Amend(
        paper_exchange_core::types::AmendRequest,
        Option<OrderAckReply>,
    ),
    InjectOrderbook(paper_exchange_core::types::OrderbookSnapshot),
    InjectTrade(paper_exchange_core::types::PublicTrade),
}

pub async fn handle_socket(socket: WebSocket, state: Arc<AppState>) {
    let (mut sender, mut receiver) = socket.split();
    let mut event_rx = state.event_tx.subscribe();
    let user_command_tx = state.user_command_tx.clone();
    let snapshot_tx = state.snapshot_tx.clone();

    // Channel to push messages directly to this connection (e.g. snapshot reply for get_account_snapshot).
    let (direct_tx, mut direct_rx) = tokio::sync::mpsc::channel::<Vec<ServerMessage>>(4);

    // Send "connected" immediately so the client gets at least one message (proves WebSocket path works).
    let _ = direct_tx.send(vec![ServerMessage::Connected {}]).await;

    // Spawn send_task first so it is already waiting on event_rx before we trigger snapshot (avoids missing early broadcasts).
    let send_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                result = event_rx.recv() => {
                    let ev = match result {
                        Ok(ev) => ev,
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            tracing::warn!("broadcast lagged, skipped {} messages", n);
                            continue;
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                    };
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
                Some(msgs) = direct_rx.recv() => {
                    for msg in msgs {
                        if let Ok(serialized) = serde_json::to_string(&msg) {
                            if sender.send(Message::Text(serialized)).await.is_err() {
                                return;
                            }
                        }
                    }
                }
                else => break,
            }
        }
    });

    // Now request broadcast snapshot so core sends; send_task is already waiting on event_rx.
    if snapshot_tx.send(SnapshotRequest::Broadcast).is_ok() {
        tracing::info!("new connection: account snapshot requested (broadcast)");
    }
    tokio::task::yield_now().await;

    while let Some(Ok(msg)) = receiver.next().await {
        if let Message::Text(text) = msg {
            if let Ok(client_msg) = serde_json::from_str::<ClientMessage>(&text) {
                match client_msg {
                    ClientMessage::GetAccountSnapshot => {
                        let (reply_tx, reply_rx) = oneshot::channel();
                        if snapshot_tx.send(SnapshotRequest::Reply(reply_tx)).is_ok() {
                            let direct_tx = direct_tx.clone();
                            tokio::spawn(async move {
                                if let Ok((balances, positions)) = reply_rx.await {
                                    let msgs: Vec<ServerMessage> = balances
                                        .into_iter()
                                        .map(ServerMessage::Balance)
                                        .chain(positions.into_iter().map(ServerMessage::Position))
                                        .collect();
                                    let _ = direct_tx.send(msgs).await;
                                }
                            });
                        }
                    }
                    ClientMessage::PlaceOrder { request } => {
                        let (reply_tx, reply_rx) = oneshot::channel();
                        if user_command_tx
                            .send(Command::PlaceOrder(request, Some(reply_tx)))
                            .is_ok()
                        {
                            let direct_tx = direct_tx.clone();
                            tokio::spawn(async move {
                                if let Ok(exec) = reply_rx.await {
                                    let msg = ServerMessage::Execution(exec);
                                    if let Ok(serialized) = serde_json::to_string(&msg) {
                                        tracing::info!(
                                            "k-paper: place_order ack sent (same connection): {}",
                                            serialized
                                        );
                                    }
                                    let _ = direct_tx.send(vec![msg]).await;
                                }
                            });
                        }
                    }
                    ClientMessage::CancelOrder { order_id } => {
                        if let Ok(uuid) = uuid::Uuid::parse_str(&order_id) {
                            let (reply_tx, reply_rx) = oneshot::channel();
                            if user_command_tx
                                .send(Command::Cancel(
                                    paper_exchange_core::types::CancelRequest::ByOrderId(
                                        paper_exchange_core::types::OrderId(uuid),
                                    ),
                                    Some(reply_tx),
                                ))
                                .is_ok()
                            {
                                let direct_tx = direct_tx.clone();
                                tokio::spawn(async move {
                                    if let Ok(oid) = reply_rx.await {
                                        let _ = direct_tx
                                            .send(vec![ServerMessage::CancelAck { order_id: oid }])
                                            .await;
                                    }
                                });
                            }
                        }
                    }
                    ClientMessage::CancelByClientOrderId { client_order_id } => {
                        let (reply_tx, reply_rx) = oneshot::channel();
                        if user_command_tx
                            .send(Command::Cancel(
                                paper_exchange_core::types::CancelRequest::ByClientOrderId(
                                    paper_exchange_core::types::ClientOrderId(client_order_id),
                                ),
                                Some(reply_tx),
                            ))
                            .is_ok()
                        {
                            let direct_tx = direct_tx.clone();
                            tokio::spawn(async move {
                                if let Ok(oid) = reply_rx.await {
                                    let _ = direct_tx
                                        .send(vec![ServerMessage::CancelAck { order_id: oid }])
                                        .await;
                                }
                            });
                        }
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
                            let (reply_tx, reply_rx) = oneshot::channel();
                            if user_command_tx
                                .send(Command::Amend(req, Some(reply_tx)))
                                .is_ok()
                            {
                                let direct_tx = direct_tx.clone();
                                tokio::spawn(async move {
                                    if let Ok(oid) = reply_rx.await {
                                        let _ = direct_tx
                                            .send(vec![ServerMessage::AmendAck { order_id: oid }])
                                            .await;
                                    }
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    send_task.abort();
    info!("WebSocket client disconnected");
}
