//! Main facade: inject events, place/cancel/amend, output streams.

use crate::account::Account;
use crate::config::Config;
use crate::error::ExchangeError;
use crate::matching::{try_fill, Fill};
use crate::order_manager::OrderManager;
use crate::orderbook::Orderbook;
use crate::types::*;
use chrono::Utc;
use rust_decimal::Decimal;
use tokio::sync::mpsc;

pub struct PaperExchange {
    config: Config,
    orderbook: Orderbook,
    order_manager: OrderManager,
    account: Account,
    /// In-memory trade history derived from executions (fills only, no acks). Used by REST /trades.
    trades: Vec<Execution>,
    market_tx: mpsc::UnboundedSender<StreamEvent<MarketDataEvent>>,
    private_tx: mpsc::UnboundedSender<StreamEvent<PrivateEvent>>,
    market_sequence: u64,
    private_sequence: u64,
}

impl PaperExchange {
    pub fn new(
        config: Config,
    ) -> (
        Self,
        mpsc::UnboundedReceiver<StreamEvent<MarketDataEvent>>,
        mpsc::UnboundedReceiver<StreamEvent<PrivateEvent>>,
    ) {
        let (market_tx, market_rx) = mpsc::unbounded_channel();
        let (private_tx, private_rx) = mpsc::unbounded_channel();
        // orderbook entries added below
        let account = Account::new(&config.initial_balances);
        let order_manager = OrderManager::new(config.clone());
        let mut orderbook = Orderbook::new();
        for inst in &config.instruments {
            orderbook.ensure_instrument(inst.id.clone());
        }
        let exchange = Self {
            config: config.clone(),
            orderbook,
            order_manager,
            account,
            trades: Vec::new(),
            market_tx,
            private_tx,
            market_sequence: 0,
            private_sequence: 0,
        };
        (exchange, market_rx, private_rx)
    }

    fn now(&self) -> chrono::DateTime<Utc> {
        Utc::now()
    }

    fn send_market(&mut self, event: MarketDataEvent) {
        self.market_sequence += 1;
        let _ = self.market_tx.send(StreamEvent {
            sequence: self.market_sequence,
            payload: event,
        });
    }

    fn send_private(&mut self, event: PrivateEvent) {
        self.private_sequence += 1;
        let _ = self.private_tx.send(StreamEvent {
            sequence: self.private_sequence,
            payload: event,
        });
    }

    pub fn inject_orderbook(&mut self, snapshot: OrderbookSnapshot) -> Result<(), ExchangeError> {
        let out = self.orderbook.apply_snapshot(snapshot);
        self.send_market(MarketDataEvent::Orderbook(out));
        self.run_matching()?;
        Ok(())
    }

    pub fn inject_trade(&mut self, trade: PublicTrade) -> Result<(), ExchangeError> {
        self.send_market(MarketDataEvent::PublicTrade(trade.clone()));
        self.run_matching()?;
        Ok(())
    }

    fn run_matching(&mut self) -> Result<(), ExchangeError> {
        let open: Vec<Order> = self
            .order_manager
            .open_orders()
            .into_iter()
            .cloned()
            .collect();
        for order in open {
            let book = self.orderbook.get(&order.instrument_id);
            if let Some(fill) = try_fill(&order, book, &self.config.fill_model) {
                self.apply_fill(&order, &fill)?;
            }
        }
        Ok(())
    }

    fn apply_fill(&mut self, order: &Order, fill: &Fill) -> Result<(), ExchangeError> {
        let instrument = self
            .config
            .instruments
            .iter()
            .find(|i| i.id.0 == order.instrument_id.0)
            .ok_or_else(|| ExchangeError::InvalidInstrument(order.instrument_id.0.clone()))?;

        let trade_id = TradeId::new();
        let now = self.now();

        self.account.apply_fill(
            order.side.clone(),
            &instrument.quote_asset,
            &instrument.base_asset,
            fill.price,
            fill.quantity,
            fill.fee,
        );

        let o = self.order_manager.get_order_mut(&order.order_id).unwrap();
        o.filled_qty += fill.quantity;
        o.updated_at = now;
        o.status = if o.filled_qty >= o.quantity {
            OrderStatus::Filled
        } else {
            OrderStatus::PartiallyFilled
        };

        self.account.apply_fill(
            order.side.clone(),
            &instrument.quote_asset,
            &instrument.base_asset,
            fill.price,
            fill.quantity,
            fill.fee,
        );

        let exec = Execution {
            order_id: order.order_id,
            trade_id,
            instrument_id: order.instrument_id.clone(),
            side: order.side.clone(),
            price: fill.price,
            quantity: fill.quantity,
            fee: Some(fill.fee),
            timestamp: now,
            reject_reason: None,
        };
        // Persist fill in in-memory trade history for REST /trades.
        self.trades.push(exec.clone());
        self.send_private(PrivateEvent::Execution(exec));

        let pt = PublicTrade {
            instrument_id: order.instrument_id.clone(),
            price: fill.price,
            quantity: fill.quantity,
            side: order.side.clone(),
            timestamp: now,
            trade_id,
        };
        self.send_market(MarketDataEvent::PublicTrade(pt));

        for bal in self.account.get_all_balances() {
            self.send_private(PrivateEvent::Balance(bal));
        }

        Ok(())
    }

    pub fn place_order(&mut self, req: OrderRequest) -> Result<Order, ExchangeError> {
        let now = self.now();
        let order = self
            .order_manager
            .place_order(req, &mut self.account, now)?;

        self.send_private(PrivateEvent::Execution(Execution {
            order_id: order.order_id,
            trade_id: TradeId::nil(),
            instrument_id: order.instrument_id.clone(),
            side: order.side.clone(),
            price: order.price,
            quantity: Decimal::ZERO,
            fee: None,
            timestamp: now,
            reject_reason: None,
        }));

        self.run_matching()?;

        Ok(self
            .order_manager
            .get_order(order.order_id)
            .cloned()
            .unwrap_or(order))
    }

    /// Return current balances and positions without sending to the stream (for direct reply to a connection).
    pub fn get_snapshot_data(&self) -> (Vec<Balance>, Vec<Position>) {
        (
            self.account.get_all_balances(),
            self.account.get_all_positions(),
        )
    }

    /// Emit current balances and positions to the private stream (e.g. for new clients).
    pub fn emit_account_snapshot(&mut self) {
        for bal in self.account.get_all_balances() {
            self.send_private(PrivateEvent::Balance(bal));
        }
        for pos in self.account.get_all_positions() {
            self.send_private(PrivateEvent::Position(pos));
        }
    }

    /// Return all open orders (New/PartiallyFilled), optionally filtered by instrument_id.
    pub fn get_open_orders(&self, instrument_id: Option<&InstrumentId>) -> Vec<Order> {
        let mut out: Vec<Order> = self
            .order_manager
            .open_orders()
            .into_iter()
            .filter(|o| {
                instrument_id
                    .map(|iid| &o.instrument_id == iid)
                    .unwrap_or(true)
            })
            .cloned()
            .collect();
        // Sort by created_at ascending for deterministic output.
        out.sort_by_key(|o| o.created_at);
        out
    }

    /// Return trade executions (fills only), optionally filtered by instrument_id and limited to the last N.
    pub fn get_trades(
        &self,
        instrument_id: Option<&InstrumentId>,
        limit: Option<usize>,
    ) -> Vec<Execution> {
        let mut trades: Vec<Execution> = self
            .trades
            .iter()
            .filter(|e| {
                instrument_id
                    .map(|iid| &e.instrument_id == iid)
                    .unwrap_or(true)
            })
            .cloned()
            .collect();
        // Trades are stored in insertion order; keep as-is and apply limit from the end.
        if let Some(n) = limit {
            if trades.len() > n {
                trades = trades.split_off(trades.len() - n);
            }
        }
        trades
    }

    /// Return a copy of the static config (used to derive /exchange_info).
    pub fn get_config(&self) -> Config {
        self.config.clone()
    }

    pub fn cancel_order(&mut self, req: CancelRequest) -> Result<Order, ExchangeError> {
        let now = self.now();
        self.order_manager.cancel_order(req, &mut self.account, now)
    }

    /// Cancel all open orders (New/PartiallyFilled). Returns the orders that were canceled.
    pub fn cancel_all_orders(&mut self) -> Vec<Order> {
        let now = self.now();
        self.order_manager.cancel_all_orders(&mut self.account, now)
    }

    pub fn amend_order(&mut self, req: AmendRequest) -> Result<Order, ExchangeError> {
        let now = self.now();
        self.order_manager.amend_order(req, &mut self.account, now)
    }
}
