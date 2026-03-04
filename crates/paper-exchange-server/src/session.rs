//! Core loop: run PaperExchange in a dedicated thread, process commands, broadcast events.
//! v1: single account, no auth.

use crate::transport::Command;
use crate::wire::BroadcastEvent;
use paper_exchange_core::config::{Config, FillModelConfig, InitialBalance};
use paper_exchange_core::exchange::PaperExchange;
use paper_exchange_core::types::*;
use rust_decimal::Decimal;
use std::str::FromStr;
use std::sync::mpsc;
use std::thread;
use tokio::sync::broadcast;

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

pub fn run_core_loop(
    command_rx: mpsc::Receiver<Command>,
    event_tx: broadcast::Sender<BroadcastEvent>,
) {
    let config = default_config();
    let (mut exchange, market_rx, private_rx) = PaperExchange::new(config);

    loop {
        while let Ok(ev) = market_rx.try_recv() {
            let _ = event_tx.send(BroadcastEvent::Market(ev));
        }
        while let Ok(ev) = private_rx.try_recv() {
            let _ = event_tx.send(BroadcastEvent::Private(ev));
        }

        match command_rx.recv() {
            Ok(Command::PlaceOrder(req)) => {
                if let Err(e) = exchange.place_order(req) {
                    tracing::warn!("place_order error: {:?}", e);
                }
            }
            Ok(Command::Cancel(req)) => {
                if let Err(e) = exchange.cancel_order(req) {
                    tracing::warn!("cancel_order error: {:?}", e);
                }
            }
            Ok(Command::Amend(req)) => {
                if let Err(e) = exchange.amend_order(req) {
                    tracing::warn!("amend_order error: {:?}", e);
                }
            }
            Ok(Command::InjectOrderbook(snapshot)) => {
                if let Err(e) = exchange.inject_orderbook(snapshot) {
                    tracing::warn!("inject_orderbook error: {:?}", e);
                }
            }
            Ok(Command::InjectTrade(trade)) => {
                if let Err(e) = exchange.inject_trade(trade) {
                    tracing::warn!("inject_trade error: {:?}", e);
                }
            }
            Err(_) => break,
        }
    }
}

pub fn spawn_core_thread(command_rx: mpsc::Receiver<Command>) -> broadcast::Sender<BroadcastEvent> {
    let (event_tx, _) = broadcast::channel(256);
    let event_tx_clone = event_tx.clone();
    thread::spawn(move || run_core_loop(command_rx, event_tx_clone));
    event_tx
}
