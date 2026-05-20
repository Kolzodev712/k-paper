//! Core loop: run PaperExchange in an async task, process commands via tokio channels, broadcast events.
//! Fully async so no blocking across std/tokio boundary.

use crate::transport::{Command, RestQuery, SnapshotRequest};
use crate::wire::BroadcastEvent;
use paper_exchange_core::config::Config;
use paper_exchange_core::exchange::PaperExchange;
use paper_exchange_core::types::PrivateEvent;
use paper_exchange_core::types::*;
use tokio::sync::broadcast;
use tokio::sync::mpsc as tokio_mpsc;

/// Async core loop: one select! for commands and for exchange outputs (market + private).
pub async fn run_core_loop(
    config: Config,
    mut user_command_rx: tokio_mpsc::UnboundedReceiver<Command>,
    mut dummy_command_rx: tokio_mpsc::UnboundedReceiver<Command>,
    mut snapshot_rx: tokio_mpsc::UnboundedReceiver<SnapshotRequest>,
    mut rest_query_rx: tokio_mpsc::UnboundedReceiver<RestQuery>,
    event_tx: broadcast::Sender<BroadcastEvent>,
) {
    let (mut exchange, mut market_rx, mut private_rx) = PaperExchange::new(config);

    loop {
        // Same order as original: drain exchange outputs first so broadcasts go out immediately.
        while let Ok(ev) = market_rx.try_recv() {
            let _ = event_tx.send(BroadcastEvent::Market(ev));
        }
        while let Ok(ev) = private_rx.try_recv() {
            let _ = event_tx.send(BroadcastEvent::Private(ev));
        }

        // Then drain snapshot, REST queries, and user commands so they're never starved by dummy/market traffic.
        while let Ok(req) = snapshot_rx.try_recv() {
            match req {
                SnapshotRequest::Broadcast => {
                    exchange.emit_account_snapshot();
                    tracing::info!("account snapshot emitted (broadcast to all connections)");
                    while let Ok(ev) = private_rx.try_recv() {
                        let _ = event_tx.send(BroadcastEvent::Private(ev));
                    }
                }
                SnapshotRequest::Reply(reply_tx) => {
                    let data = exchange.get_snapshot_data();
                    let _ = reply_tx.send(data);
                    tracing::info!("account snapshot sent (direct reply to connection)");
                }
            }
        }
        while let Ok(q) = rest_query_rx.try_recv() {
            handle_rest_query(&exchange, q);
        }
        while let Ok(cmd) = user_command_rx.try_recv() {
            run_user_command(&mut exchange, &event_tx, &mut private_rx, cmd);
        }

        // Then wait for one of: more commands, dummy, or exchange outputs.
        tokio::select! {
            Some(cmd) = user_command_rx.recv() => {
                run_user_command(&mut exchange, &event_tx, &mut private_rx, cmd);
            }
            Some(cmd) = dummy_command_rx.recv() => {
                run_dummy_command(&mut exchange, cmd);
            }
            Some(req) = snapshot_rx.recv() => {
                match req {
                    SnapshotRequest::Broadcast => {
                        exchange.emit_account_snapshot();
                        tracing::info!("account snapshot emitted (broadcast to all connections)");
                        while let Ok(ev) = private_rx.try_recv() {
                            let _ = event_tx.send(BroadcastEvent::Private(ev));
                        }
                    }
                    SnapshotRequest::Reply(reply_tx) => {
                        let data = exchange.get_snapshot_data();
                        let _ = reply_tx.send(data);
                        tracing::info!("account snapshot sent (direct reply to connection)");
                    }
                }
            }
            Some(q) = rest_query_rx.recv() => {
                handle_rest_query(&exchange, q);
            }
            Some(ev) = market_rx.recv() => {
                let _ = event_tx.send(BroadcastEvent::Market(ev));
            }
            Some(ev) = private_rx.recv() => {
                let _ = event_tx.send(BroadcastEvent::Private(ev));
            }
            else => break,
        }
    }
}

fn run_user_command(
    exchange: &mut PaperExchange,
    event_tx: &broadcast::Sender<BroadcastEvent>,
    private_rx: &mut tokio_mpsc::UnboundedReceiver<
        paper_exchange_core::types::StreamEvent<PrivateEvent>,
    >,
    cmd: Command,
) {
    match cmd {
        Command::PlaceOrder(req, mut reply_tx) => match exchange.place_order(req) {
            Err(e) => {
                tracing::warn!("place_order error: {:?}", e);
                if let Some(tx) = reply_tx.take() {
                    let _ = tx.send(Err(e.to_string()));
                }
            }
            Ok(_) => {
                while let Ok(ev) = private_rx.try_recv() {
                    if let Some(tx) = reply_tx.take() {
                        if let PrivateEvent::Execution(exec) = ev.payload.clone() {
                            let _ = tx.send(Ok(exec));
                        }
                    }
                    let _ = event_tx.send(BroadcastEvent::Private(ev));
                }
            }
        },
        Command::CancelAll(reply_tx) => {
            let cancelled = exchange.cancel_all_orders();
            if let Some(tx) = reply_tx {
                let order_ids: Vec<String> = cancelled
                    .into_iter()
                    .map(|o| o.order_id.0.to_string())
                    .collect();
                let _ = tx.send(order_ids);
            }
        }
        Command::Cancel(req, reply_tx) => match exchange.cancel_order(req) {
            Ok(order) => {
                if let Some(tx) = reply_tx {
                    let _ = tx.send(Ok(order.order_id.0.to_string()));
                }
            }
            Err(e) => {
                tracing::warn!("cancel_order error: {:?}", e);
                if let Some(tx) = reply_tx {
                    let _ = tx.send(Err(e.to_string()));
                }
            }
        },
        Command::Amend(req, reply_tx) => match exchange.amend_order(req) {
            Ok(order) => {
                if let Some(tx) = reply_tx {
                    let _ = tx.send(Ok(order.order_id.0.to_string()));
                }
            }
            Err(e) => {
                tracing::warn!("amend_order error: {:?}", e);
                if let Some(tx) = reply_tx {
                    let _ = tx.send(Err(e.to_string()));
                }
            }
        },
        Command::InjectOrderbook(_) | Command::InjectTrade(_) => {
            unreachable!("dummy commands go to run_dummy_command")
        }
    }
}

fn run_dummy_command(exchange: &mut PaperExchange, cmd: Command) {
    match cmd {
        Command::InjectOrderbook(snapshot) => {
            if let Err(e) = exchange.inject_orderbook(snapshot) {
                tracing::warn!("inject_orderbook error: {:?}", e);
            }
        }
        Command::InjectTrade(trade) => {
            if let Err(e) = exchange.inject_trade(trade) {
                tracing::warn!("inject_trade error: {:?}", e);
            }
        }
        Command::PlaceOrder(_, _)
        | Command::CancelAll(_)
        | Command::Cancel(_, _)
        | Command::Amend(_, _) => {
            unreachable!("user commands go to run_user_command")
        }
    }
}

fn handle_rest_query(exchange: &PaperExchange, q: RestQuery) {
    match q {
        RestQuery::Balances(reply) => {
            let balances = exchange.get_snapshot_data().0;
            let _ = reply.send(balances);
        }
        RestQuery::OpenOrders {
            instrument_id,
            reply,
        } => {
            let iid = instrument_id.as_deref().map(InstrumentId::new);
            let orders = exchange.get_open_orders(iid.as_ref());
            let _ = reply.send(orders);
        }
        RestQuery::Trades {
            instrument_id,
            limit,
            reply,
        } => {
            let iid = instrument_id.as_deref().map(InstrumentId::new);
            let trades = exchange.get_trades(iid.as_ref(), limit);
            let _ = reply.send(trades);
        }
        RestQuery::MarketInfo(reply) => {
            let cfg = exchange.get_config();
            let _ = reply.send(cfg);
        }
    }
}

/// Spawn the core loop as a tokio task; returns the broadcast sender for events.
pub fn spawn_core_task(
    config: Config,
    user_command_rx: tokio_mpsc::UnboundedReceiver<Command>,
    dummy_command_rx: tokio_mpsc::UnboundedReceiver<Command>,
    snapshot_rx: tokio_mpsc::UnboundedReceiver<SnapshotRequest>,
    rest_query_rx: tokio_mpsc::UnboundedReceiver<RestQuery>,
) -> broadcast::Sender<BroadcastEvent> {
    let (event_tx, _) = broadcast::channel(256);
    let event_tx_clone = event_tx.clone();
    tokio::spawn(async move {
        run_core_loop(
            config,
            user_command_rx,
            dummy_command_rx,
            snapshot_rx,
            rest_query_rx,
            event_tx_clone,
        )
        .await
    });
    event_tx
}
