//! Core loop: run PaperExchange in an async task, process commands via tokio channels, broadcast events.
//! Fully async so no blocking across std/tokio boundary.

use crate::transport::{Command, SnapshotRequest};
use crate::wire::BroadcastEvent;
use paper_exchange_core::config::{Config, FillModelConfig, InitialBalance};
use paper_exchange_core::exchange::PaperExchange;
use paper_exchange_core::types::PrivateEvent;
use paper_exchange_core::types::*;
use rust_decimal::Decimal;
use std::str::FromStr;
use tokio::sync::broadcast;
use tokio::sync::mpsc as tokio_mpsc;

fn decimal(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap_or(Decimal::ZERO)
}

fn default_config() -> Config {
    Config {
        instruments: vec![Instrument {
            id: InstrumentId::new("BTC-USDT"),
            symbol: "BTC-USDT".to_string(),
            base_asset: Asset("BTC".to_string()),
            quote_asset: Asset("USDT".to_string()),
            tick_size: decimal("0.01"),
            lot_size: decimal("0.001"),
            min_notional: decimal("10"),
            instrument_type: None,
        }],
        initial_balances: vec![
            InitialBalance {
                asset: Asset("USDT".to_string()),
                amount: decimal("1000000"),
            },
            InitialBalance {
                asset: Asset("BTC".to_string()),
                amount: decimal("100"),
            },
        ],
        fill_model: FillModelConfig::default(),
    }
}

/// Async core loop: one select! for commands and for exchange outputs (market + private).
pub async fn run_core_loop(
    mut user_command_rx: tokio_mpsc::UnboundedReceiver<Command>,
    mut dummy_command_rx: tokio_mpsc::UnboundedReceiver<Command>,
    mut snapshot_rx: tokio_mpsc::UnboundedReceiver<SnapshotRequest>,
    event_tx: broadcast::Sender<BroadcastEvent>,
) {
    let config = default_config();
    let (mut exchange, mut market_rx, mut private_rx) = PaperExchange::new(config);

    loop {
        // Same order as original: drain exchange outputs first so broadcasts go out immediately.
        while let Ok(ev) = market_rx.try_recv() {
            let _ = event_tx.send(BroadcastEvent::Market(ev));
        }
        while let Ok(ev) = private_rx.try_recv() {
            let _ = event_tx.send(BroadcastEvent::Private(ev));
        }

        // Then drain snapshot and user commands so they're never starved by dummy/market traffic.
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
        Command::PlaceOrder(req, mut reply_tx) => {
            if let Err(e) = exchange.place_order(req) {
                tracing::warn!("place_order error: {:?}", e);
            } else {
                while let Ok(ev) = private_rx.try_recv() {
                    if let Some(tx) = reply_tx.take() {
                        if let PrivateEvent::Execution(exec) = ev.payload.clone() {
                            let _ = tx.send(exec);
                        }
                    }
                    let _ = event_tx.send(BroadcastEvent::Private(ev));
                }
            }
        }
        Command::Cancel(req, reply_tx) => match exchange.cancel_order(req) {
            Ok(order) => {
                if let Some(tx) = reply_tx {
                    let _ = tx.send(order.order_id.0.to_string());
                }
            }
            Err(e) => tracing::warn!("cancel_order error: {:?}", e),
        },
        Command::Amend(req, reply_tx) => match exchange.amend_order(req) {
            Ok(order) => {
                if let Some(tx) = reply_tx {
                    let _ = tx.send(order.order_id.0.to_string());
                }
            }
            Err(e) => tracing::warn!("amend_order error: {:?}", e),
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
        Command::PlaceOrder(_, _) | Command::Cancel(_, _) | Command::Amend(_, _) => {
            unreachable!("user commands go to run_user_command")
        }
    }
}

/// Spawn the core loop as a tokio task; returns the broadcast sender for events.
pub fn spawn_core_task(
    user_command_rx: tokio_mpsc::UnboundedReceiver<Command>,
    dummy_command_rx: tokio_mpsc::UnboundedReceiver<Command>,
    snapshot_rx: tokio_mpsc::UnboundedReceiver<SnapshotRequest>,
) -> broadcast::Sender<BroadcastEvent> {
    let (event_tx, _) = broadcast::channel(256);
    let event_tx_clone = event_tx.clone();
    tokio::spawn(async move {
        run_core_loop(
            user_command_rx,
            dummy_command_rx,
            snapshot_rx,
            event_tx_clone,
        )
        .await
    });
    event_tx
}
