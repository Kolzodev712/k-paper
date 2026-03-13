//! WebSocket server: accept connections, route commands to core, broadcast events to clients.

use crate::wire::{BroadcastEvent, ClientMessage, ServerMessage};
use axum::extract::ws::{Message, WebSocket};
use axum::extract::WebSocketUpgrade;
use axum::response::Response;
use futures_util::stream::StreamExt;
use futures_util::SinkExt;
use paper_exchange_core::config::Config;
use paper_exchange_core::types::{Balance, Execution, Order, Position};
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tracing::info;

pub type SnapshotData = (Vec<Balance>, Vec<Position>);
/// Optional reply for CancelAllOrders: core returns the canceled order_ids (UUID strings).
pub type CancelAllReply = oneshot::Sender<Vec<String>>;

/// Snapshot request: broadcast to all (on connect) or reply on a oneshot (get_account_snapshot).
pub enum SnapshotRequest {
    Broadcast,
    Reply(oneshot::Sender<SnapshotData>),
}

/// REST query channel: HTTP handlers ask core for snapshots of current state.
pub enum RestQuery {
    Balances(oneshot::Sender<Vec<Balance>>),
    OpenOrders {
        instrument_id: Option<String>,
        reply: oneshot::Sender<Vec<Order>>,
    },
    Trades {
        instrument_id: Option<String>,
        limit: Option<usize>,
        reply: oneshot::Sender<Vec<Execution>>,
    },
    MarketInfo(oneshot::Sender<Config>),
}

pub async fn ws_handler(ws: WebSocketUpgrade, state: Arc<AppState>) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, state, true))
}

pub struct AppState {
    pub user_command_tx: mpsc::UnboundedSender<Command>,
    pub snapshot_tx: mpsc::UnboundedSender<SnapshotRequest>,
    pub rest_query_tx: mpsc::UnboundedSender<RestQuery>,
    pub event_tx: broadcast::Sender<BroadcastEvent>,
}

/// Optional reply for PlaceOrder: Ok(execution) or Err(error message) so the connection can send Execution or Error.
pub type PlaceOrderReply = oneshot::Sender<Result<Execution, String>>;
/// Optional reply for Cancel/Amend: Ok(order_id) or Err(error message) so the connection can send ack or Error.
pub type OrderAckReply = oneshot::Sender<Result<String, String>>;

#[derive(Debug)]
pub enum Command {
    PlaceOrder(
        paper_exchange_core::types::OrderRequest,
        Option<PlaceOrderReply>,
    ),
    CancelAll(Option<CancelAllReply>),
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

/// If `trading` is true: full behavior (snapshot, commands, market + private). If false: market-only (connected + orderbook/trades, no snapshot, commands ignored).
pub async fn handle_socket(socket: WebSocket, state: Arc<AppState>, trading: bool) {
    let endpoint = if trading { "trading" } else { "market" };
    info!("{} endpoint: client connected", endpoint);

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
                    // Market-only connections only get market events (orderbook, public_trade); skip private (executions, etc.).
                    if !trading {
                        if let BroadcastEvent::Private(_) = ev {
                            continue;
                        }
                    }
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

    // Trading connections get account snapshot on connect; market-only do not.
    if trading {
        if snapshot_tx.send(SnapshotRequest::Broadcast).is_ok() {
            tracing::info!("new trading connection: account snapshot requested (broadcast)");
        }
        tokio::task::yield_now().await;
    }

    while let Some(Ok(msg)) = receiver.next().await {
        let Message::Text(text) = msg else { continue };
        if !trading {
            // Market-only: drain messages, do not process commands.
            continue;
        }
        // Log every incoming text frame on trading endpoint for debugging (length + prefix).
        let len = text.len();
        let prefix: String = text.chars().take(200).collect();
        if len > 200 {
            info!("trading: incoming frame len={} prefix={}...", len, prefix);
        } else {
            info!("trading: incoming frame len={} body={}", len, prefix);
        }
        match serde_json::from_str::<ClientMessage>(&text) {
            Ok(client_msg) => match &client_msg {
                ClientMessage::GetAccountSnapshot => {
                    info!("trading: command received: get_account_snapshot");
                    let (reply_tx, reply_rx) = oneshot::channel();
                    if snapshot_tx.send(SnapshotRequest::Reply(reply_tx)).is_ok() {
                        let direct_tx = direct_tx.clone();
                        tokio::spawn(async move {
                            if let Ok((balances, positions)) = reply_rx.await {
                                let n_balances = balances.len();
                                let n_positions = positions.len();
                                info!(
                                    "trading: sending account snapshot ({} balances, {} positions)",
                                    n_balances, n_positions
                                );
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
                    info!(
                        "trading: command received: place_order instrument_id={} side={:?} quantity={}",
                        request.instrument_id, request.side, request.quantity
                    );
                    let (reply_tx, reply_rx) = oneshot::channel();
                    if user_command_tx
                        .send(Command::PlaceOrder(
                            paper_exchange_core::types::OrderRequest::from(request.clone()),
                            Some(reply_tx),
                        ))
                        .is_ok()
                    {
                        let direct_tx = direct_tx.clone();
                        tokio::spawn(async move {
                            match reply_rx.await {
                                Ok(Ok(exec)) => {
                                    info!(
                                        "trading: sending execution report order_id={} instrument_id={}",
                                        exec.order_id, exec.instrument_id
                                    );
                                    let _ =
                                        direct_tx.send(vec![ServerMessage::Execution(exec)]).await;
                                }
                                Ok(Err(msg)) => {
                                    info!("trading: sending place_order error: {}", msg);
                                    let _ = direct_tx
                                        .send(vec![ServerMessage::Error { message: msg }])
                                        .await;
                                }
                                Err(_) => {}
                            }
                        });
                    }
                }
                ClientMessage::CancelAllOrders => {
                    info!("trading: command received: cancel_all_orders");
                    let (reply_tx, reply_rx) = oneshot::channel();
                    if user_command_tx
                        .send(Command::CancelAll(Some(reply_tx)))
                        .is_ok()
                    {
                        let direct_tx = direct_tx.clone();
                        tokio::spawn(async move {
                            if let Ok(order_ids) = reply_rx.await {
                                info!(
                                    "trading: sending CancelAllAck order_count={}",
                                    order_ids.len()
                                );
                                let _ = direct_tx
                                    .send(vec![ServerMessage::CancelAllAck { order_ids }])
                                    .await;
                            }
                        });
                    }
                }
                ClientMessage::CancelOrder { order_id } => {
                    info!(
                        "trading: command received: cancel_order order_id={}",
                        order_id
                    );
                    if let Ok(uuid) = uuid::Uuid::parse_str(order_id) {
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
                                match reply_rx.await {
                                    Ok(Ok(oid)) => {
                                        info!("trading: sending CancelAck order_id={}", oid);
                                        let _ = direct_tx
                                            .send(vec![ServerMessage::CancelAck { order_id: oid }])
                                            .await;
                                    }
                                    Ok(Err(msg)) => {
                                        info!("trading: sending cancel_order error: {}", msg);
                                        let _ = direct_tx
                                            .send(vec![ServerMessage::Error { message: msg }])
                                            .await;
                                    }
                                    Err(_) => {}
                                }
                            });
                        }
                    }
                }
                ClientMessage::CancelByClientOrderId { client_order_id } => {
                    info!(
                        "trading: command received: cancel_by_client_order_id client_order_id={}",
                        client_order_id
                    );
                    let (reply_tx, reply_rx) = oneshot::channel();
                    if user_command_tx
                        .send(Command::Cancel(
                            paper_exchange_core::types::CancelRequest::ByClientOrderId(
                                paper_exchange_core::types::ClientOrderId(client_order_id.clone()),
                            ),
                            Some(reply_tx),
                        ))
                        .is_ok()
                    {
                        let direct_tx = direct_tx.clone();
                        tokio::spawn(async move {
                            match reply_rx.await {
                                Ok(Ok(oid)) => {
                                    info!("trading: sending CancelAck order_id={}", oid);
                                    let _ = direct_tx
                                        .send(vec![ServerMessage::CancelAck { order_id: oid }])
                                        .await;
                                }
                                Ok(Err(msg)) => {
                                    info!(
                                        "trading: sending cancel_by_client_order_id error: {}",
                                        msg
                                    );
                                    let _ = direct_tx
                                        .send(vec![ServerMessage::Error { message: msg }])
                                        .await;
                                }
                                Err(_) => {}
                            }
                        });
                    }
                }
                ClientMessage::AmendOrder {
                    order_id,
                    quantity,
                    limit_price,
                } => {
                    info!(
                        "trading: command received: amend_order order_id={} quantity={:?} limit_price={:?}",
                        order_id, quantity, limit_price
                    );
                    if let Ok(uuid) = uuid::Uuid::parse_str(order_id) {
                        let req = paper_exchange_core::types::AmendRequest {
                            order_id: paper_exchange_core::types::OrderId(uuid),
                            quantity: quantity
                                .as_ref()
                                .and_then(|s| rust_decimal::Decimal::from_str(s).ok()),
                            limit_price: limit_price
                                .as_ref()
                                .and_then(|s| rust_decimal::Decimal::from_str(s).ok()),
                        };
                        let (reply_tx, reply_rx) = oneshot::channel();
                        if user_command_tx
                            .send(Command::Amend(req, Some(reply_tx)))
                            .is_ok()
                        {
                            let direct_tx = direct_tx.clone();
                            tokio::spawn(async move {
                                match reply_rx.await {
                                    Ok(Ok(oid)) => {
                                        info!("trading: sending AmendAck order_id={}", oid);
                                        let _ = direct_tx
                                            .send(vec![ServerMessage::AmendAck { order_id: oid }])
                                            .await;
                                    }
                                    Ok(Err(msg)) => {
                                        info!("trading: sending amend_order error: {}", msg);
                                        let _ = direct_tx
                                            .send(vec![ServerMessage::Error { message: msg }])
                                            .await;
                                    }
                                    Err(_) => {}
                                }
                            });
                        }
                    }
                }
            },
            Err(e) => {
                info!(
                    "trading: parse_failed error={} len={} prefix={}",
                    e,
                    text.len(),
                    text.chars().take(200).collect::<String>()
                );
            }
        }
    }

    send_task.abort();
    info!("WebSocket client disconnected");
}
