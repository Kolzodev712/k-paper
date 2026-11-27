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
///
/// Channel roles and priority:
/// - `user_command_rx`: ordered FIFO for user/WS commands **and** scenario-driven events
///   (`InjectOrderbook`/`InjectTrade`/`PlaceOrder` from a static scenario share this one channel,
///   so a scenario's "book, then place" order is exactly the order the core executes them).
/// - `dummy_command_rx`: baseline market-engine traffic only (book + trade noise).
///
/// The wait is `biased`: user commands, then snapshots, REST queries, market-engine commands,
/// then exchange outputs. User/scenario traffic is therefore never starved or reordered by
/// market-engine noise; the cost is that a flood of user commands delays dummy traffic.
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
            handle_rest_query(&mut exchange, q);
        }
        while let Ok(cmd) = user_command_rx.try_recv() {
            run_user_command(&mut exchange, &event_tx, &mut private_rx, cmd);
        }

        // Then wait for one of: more commands, dummy, or exchange outputs.
        tokio::select! {
            biased;
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
                handle_rest_query(&mut exchange, q);
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
        Command::PlaceOrder(req, reply_tx) => {
            if exchange.take_drop_next_place() {
                tracing::warn!("lab drop_next_place: swallowing place without book or reply");
                drop(reply_tx);
                return;
            }
            match exchange.place_order(req) {
                Err(e) => {
                    tracing::warn!("place_order error: {:?}", e);
                    if let Some(tx) = reply_tx {
                        let _ = tx.send(Err(e.to_string()));
                    }
                }
                Ok(placed) => {
                    // Only the first Execution for *this* order answers the caller; the sender is
                    // consumed on that match, never on unrelated events. If the ACK is suppressed
                    // (lab fault) and nothing fills, the reply sender is dropped with no reply.
                    let mut reply_tx = reply_tx;
                    while let Ok(ev) = private_rx.try_recv() {
                        if reply_tx.is_some() {
                            if let PrivateEvent::Execution(exec) = &ev.payload {
                                if exec.order_id == placed.order_id {
                                    if let Some(tx) = reply_tx.take() {
                                        let _ = tx.send(Ok(exec.clone()));
                                    }
                                }
                            }
                        }
                        let _ = event_tx.send(BroadcastEvent::Private(ev));
                    }
                }
            }
        }
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
        // Scenario events share the user channel (see `run_core_loop`); inject inline, in order.
        cmd @ (Command::InjectOrderbook(_) | Command::InjectTrade(_)) => {
            run_dummy_command(exchange, cmd)
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
            unreachable!("order commands go to run_user_command")
        }
    }
}

fn handle_rest_query(exchange: &mut PaperExchange, q: RestQuery) {
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
        RestQuery::OrderByClientId {
            client_order_id,
            reply,
        } => {
            let order = exchange.get_order_by_client_id(&client_order_id);
            let _ = reply.send(order);
        }
        RestQuery::SuppressPlaceAck {
            client_order_id,
            reply,
        } => {
            match client_order_id.filter(|id| !id.is_empty()) {
                Some(id) => exchange.suppress_next_place_ack(id),
                None => exchange.suppress_next_place_ack_any(),
            }
            let _ = reply.send(());
        }
        RestQuery::RejectNextPlace { reason, reply } => {
            exchange.reject_next_place(reason);
            let _ = reply.send(());
        }
        RestQuery::DuplicateNextFill { reply } => {
            exchange.duplicate_next_fill();
            let _ = reply.send(());
        }
        RestQuery::ResetLabFaults { reply } => {
            exchange.reset_lab_faults();
            let _ = reply.send(());
        }
        RestQuery::StaleOrderbook { age_secs, reply } => {
            exchange.set_stale_book_age_secs(age_secs);
            let _ = reply.send(());
        }
        RestQuery::DropNextPlace { reply } => {
            exchange.drop_next_place();
            let _ = reply.send(());
        }
        RestQuery::PartialNextFill { qty, reply } => {
            exchange.partial_next_fill(qty);
            let _ = reply.send(());
        }
        RestQuery::PositionBeforeFill { reply } => {
            exchange.emit_position_before_fill();
            let _ = reply.send(());
        }
        RestQuery::EmitWsPosition {
            quantity,
            entry_price,
            reply,
        } => {
            exchange.emit_ws_position(quantity, entry_price);
            let _ = reply.send(());
        }
        RestQuery::LabFillOpen {
            qty,
            order_id,
            reply,
        } => {
            let result = exchange
                .lab_fill_open(qty, order_id.as_deref())
                .map_err(|e| e.to_string());
            let _ = reply.send(result);
        }
        RestQuery::EmitPlaceAck {
            order_id,
            omit_client_order_id,
            reply,
        } => {
            let result = exchange
                .emit_place_ack(&order_id, omit_client_order_id)
                .map_err(|e| e.to_string());
            let _ = reply.send(result);
        }
        RestQuery::Positions(reply) => {
            let positions = exchange.get_snapshot_data().1;
            let _ = reply.send(positions);
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

#[cfg(test)]
mod tests {
    use super::*;
    use paper_exchange_core::config::{FillModelConfig, InitialBalance};
    use rust_decimal::Decimal;
    use tokio::sync::oneshot;

    fn config() -> Config {
        Config {
            instruments: vec![Instrument {
                id: InstrumentId::new("BTC-USDT"),
                symbol: "BTC-USDT".into(),
                base_asset: Asset("BTC".into()),
                quote_asset: Asset("USDT".into()),
                tick_size: Decimal::new(1, 2),
                lot_size: Decimal::new(1, 3),
                min_notional: Decimal::TEN,
                instrument_type: None,
            }],
            initial_balances: vec![InitialBalance {
                asset: Asset("USDT".into()),
                amount: Decimal::new(100_000, 0),
            }],
            fill_model: FillModelConfig::default(),
            max_trade_history: 100,
            linear: None,
        }
    }

    /// Unrelated private events queued ahead of the place ACK must not consume the reply.
    #[test]
    fn place_reply_survives_unrelated_leading_private_events() {
        let (mut exchange, _market_rx, mut private_rx) = PaperExchange::new(config());
        exchange.emit_account_snapshot(); // Balance events now precede the ACK in `private_rx`.
        let (event_tx, _event_rx) = broadcast::channel(64);
        let (reply_tx, mut reply_rx) = oneshot::channel();

        run_user_command(
            &mut exchange,
            &event_tx,
            &mut private_rx,
            Command::PlaceOrder(
                OrderRequest {
                    instrument_id: InstrumentId::new("BTC-USDT"),
                    side: Side::Buy,
                    order_type: OrderType::Limit,
                    quantity: Decimal::new(1, 3),
                    limit_price: Some(Decimal::new(40_000, 0)),
                    client_order_id: None,
                    reduce_only: false,
                },
                Some(reply_tx),
            ),
        );

        let exec = reply_rx
            .try_recv()
            .expect("reply must not be dropped")
            .expect("place ok");
        assert_eq!(exec.quantity, Decimal::ZERO, "reply is the place ACK");
        assert!(exec.client_order_id.is_none());
    }
}
