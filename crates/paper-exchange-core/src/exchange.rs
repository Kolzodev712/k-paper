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
use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
use tokio::sync::mpsc;

pub struct PaperExchange {
    config: Arc<Config>,
    orderbook: Orderbook,
    order_manager: OrderManager,
    account: Account,
    /// In-memory trade history derived from executions (fills only, no acks). Used by REST /trades.
    trades: VecDeque<Execution>,
    market_tx: mpsc::UnboundedSender<StreamEvent<MarketDataEvent>>,
    private_tx: mpsc::UnboundedSender<StreamEvent<PrivateEvent>>,
    market_sequence: u64,
    private_sequence: u64,
    /// One-shot lab fault: accept the order but do not emit the place ACK execution.
    suppress_place_ack: HashSet<String>,
    /// One-shot lab fault: suppress the ACK of the next place regardless of client id.
    suppress_next_any: bool,
    /// One-shot lab fault: reject the next place without keeping an order.
    reject_next: Option<String>,
    /// One-shot lab fault: emit the next fill execution twice.
    duplicate_next_fill: bool,
    /// When set, injected orderbooks are stamped `now - age` so the runner sees stale MD.
    stale_book_age_secs: Option<i64>,
    /// One-shot lab fault: swallow the next place with no book and no WS reply.
    drop_next_place: bool,
    /// One-shot lab fault: cap the next fill quantity (partial fill).
    partial_next_qty: Option<Decimal>,
    /// One-shot lab fault: emit Position before Execution on the next fill.
    position_before_fill: bool,
    /// Injectable clock: when set, `now()` returns this instant instead of `Utc::now()`.
    now_override: Option<chrono::DateTime<Utc>>,
    /// When set, trade ids are derived from `next_trade_seq` (order ids via the order manager).
    deterministic_ids: bool,
    next_trade_seq: u64,
}

impl PaperExchange {
    /// Build an exchange. Accepts a `Config` or an already shared `Arc<Config>`.
    ///
    /// Iteration order is deterministic (open orders by placement sequence, balances and
    /// positions by key). `OrderId`/`TradeId` stay random UUIDs and timestamps use the wall
    /// clock unless [`set_deterministic_ids`](Self::set_deterministic_ids) /
    /// [`set_now_override`](Self::set_now_override) are used.
    pub fn new(
        config: impl Into<Arc<Config>>,
    ) -> (
        Self,
        mpsc::UnboundedReceiver<StreamEvent<MarketDataEvent>>,
        mpsc::UnboundedReceiver<StreamEvent<PrivateEvent>>,
    ) {
        let config: Arc<Config> = config.into();
        let (market_tx, market_rx) = mpsc::unbounded_channel();
        let (private_tx, private_rx) = mpsc::unbounded_channel();
        // orderbook entries added below
        let account = Account::new(&config.initial_balances);
        let order_manager = OrderManager::new(Arc::clone(&config));
        let mut orderbook = Orderbook::new();
        for inst in &config.instruments {
            orderbook.ensure_instrument(inst.id.clone());
        }
        let exchange = Self {
            config,
            orderbook,
            order_manager,
            account,
            trades: VecDeque::new(),
            market_tx,
            private_tx,
            market_sequence: 0,
            private_sequence: 0,
            suppress_place_ack: HashSet::new(),
            suppress_next_any: false,
            reject_next: None,
            duplicate_next_fill: false,
            stale_book_age_secs: None,
            drop_next_place: false,
            partial_next_qty: None,
            position_before_fill: false,
            now_override: None,
            deterministic_ids: false,
            next_trade_seq: 0,
        };
        (exchange, market_rx, private_rx)
    }

    fn now(&self) -> chrono::DateTime<Utc> {
        self.now_override.unwrap_or_else(Utc::now)
    }

    /// Pin (or with `None` release) the exchange clock. Order/execution/book timestamps use it.
    pub fn set_now_override(&mut self, now: Option<chrono::DateTime<Utc>>) {
        self.now_override = now;
    }

    /// Derive `OrderId`/`TradeId` from monotonically increasing sequences instead of random UUIDs.
    pub fn set_deterministic_ids(&mut self, on: bool) {
        self.deterministic_ids = on;
        self.order_manager.set_deterministic_ids(on);
    }

    fn next_trade_id(&mut self) -> TradeId {
        if self.deterministic_ids {
            self.next_trade_seq += 1;
            TradeId::from_seq(self.next_trade_seq)
        } else {
            TradeId::new()
        }
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

    pub fn inject_orderbook(
        &mut self,
        mut snapshot: OrderbookSnapshot,
    ) -> Result<(), ExchangeError> {
        if let Some(age) = self.stale_book_age_secs {
            snapshot.timestamp = self.now() - chrono::Duration::seconds(age.max(0));
        }
        let aged = snapshot.timestamp;
        let mut out = self.orderbook.apply_snapshot(snapshot);
        if self.stale_book_age_secs.is_some() {
            out.timestamp = aged;
        }
        self.send_market(MarketDataEvent::Orderbook(out));
        self.run_matching()?;
        Ok(())
    }

    pub fn inject_trade(&mut self, trade: PublicTrade) -> Result<(), ExchangeError> {
        self.send_market(MarketDataEvent::PublicTrade(trade.clone()));
        self.run_matching()?;
        Ok(())
    }

    /// Match every open order, oldest placement first, against the current book.
    ///
    /// A fill that cannot be settled (`InsufficientBalance`, e.g. a market buy priced above the
    /// available quote) leaves the account and the order's fill state untouched and rejects the
    /// order, so one unfunded order neither corrupts state nor blocks the rest of the queue.
    fn run_matching(&mut self) -> Result<(), ExchangeError> {
        for order_id in self.order_manager.open_order_ids() {
            let Some(order) = self.order_manager.get_order(order_id) else {
                continue;
            };
            let Some(mut fill) = try_fill(
                order,
                self.orderbook.get(&order.instrument_id),
                &self.config.fill_model,
            ) else {
                continue;
            };
            if let Some(cap) = self.partial_next_qty.take() {
                if cap > Decimal::ZERO && cap < fill.quantity {
                    fill.quantity = cap;
                }
            }
            let order = order.clone();
            match self.apply_fill(&order, &fill) {
                Ok(()) => {}
                Err(ExchangeError::InsufficientBalance) => self.reject_unfunded(&order)?,
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    /// Reject an open order whose fill could not be funded; emits a rejecting execution + balances.
    fn reject_unfunded(&mut self, order: &Order) -> Result<(), ExchangeError> {
        let now = self.now();
        let rejected = self
            .order_manager
            .reject_order(order.order_id, &mut self.account, now)?;
        self.send_private(PrivateEvent::Execution(Execution {
            order_id: rejected.order_id,
            trade_id: TradeId::nil(),
            instrument_id: rejected.instrument_id.clone(),
            side: rejected.side,
            price: rejected.price,
            quantity: Decimal::ZERO,
            fee: None,
            timestamp: now,
            reject_reason: Some("insufficient balance to settle fill".into()),
            client_order_id: rejected.client_order_id.clone(),
        }));
        for bal in self.account.get_all_balances() {
            self.send_private(PrivateEvent::Balance(bal));
        }
        Ok(())
    }

    /// Settle a fill: account first (atomic), then order state, so an order is never marked
    /// filled when its account debit fails.
    fn apply_fill(&mut self, order: &Order, fill: &Fill) -> Result<(), ExchangeError> {
        if self.order_manager.get_order(order.order_id).is_none() {
            return Err(ExchangeError::OrderNotFound);
        }
        let instrument = self
            .config
            .instruments
            .iter()
            .find(|i| i.id.0 == order.instrument_id.0)
            .ok_or_else(|| ExchangeError::InvalidInstrument(order.instrument_id.0.clone()))?;

        let now = self.now();
        let reducing = order.reduce_only;

        let linear = crate::linear::instrument_is_linear(instrument);
        let fee = if linear {
            self.config
                .linear
                .as_ref()
                .map(|c| crate::linear::taker_fee(fill.price * fill.quantity, c.taker_fee_bps))
                .unwrap_or(fill.fee)
        } else {
            fill.fee
        };

        if linear {
            let leverage = self.config.linear.as_ref().map(|c| c.leverage).unwrap_or(1);
            self.account.apply_linear_fill(crate::account::LinearFill {
                quote_asset: &instrument.quote_asset,
                instrument_id: &order.instrument_id,
                side: order.side,
                price: fill.price,
                quantity: fill.quantity,
                fee,
                reducing,
                leverage,
                mark: fill.price,
            })?;
        } else {
            self.account.apply_fill(
                order.side,
                &instrument.quote_asset,
                &instrument.base_asset,
                fill.price,
                fill.quantity,
                fee,
            )?;
        }

        // Account is settled; the order must exist (checked above), so this cannot fail.
        self.order_manager
            .record_fill(order.order_id, fill.quantity, now)?;
        let trade_id = self.next_trade_id();

        let exec = Execution {
            order_id: order.order_id,
            trade_id,
            instrument_id: order.instrument_id.clone(),
            side: order.side,
            price: fill.price,
            quantity: fill.quantity,
            fee: Some(fee),
            timestamp: now,
            reject_reason: None,
            client_order_id: order.client_order_id.clone(),
        };
        // Persist fill in in-memory trade history for REST /trades (bounded).
        self.trades.push_back(exec.clone());
        let cap = self.config.max_trade_history;
        if cap > 0 {
            while self.trades.len() > cap {
                self.trades.pop_front();
            }
        }
        let duplicate = if self.duplicate_next_fill {
            self.duplicate_next_fill = false;
            true
        } else {
            false
        };
        let position_first = if self.position_before_fill {
            self.position_before_fill = false;
            true
        } else {
            false
        };

        let pt = PublicTrade {
            instrument_id: order.instrument_id.clone(),
            price: fill.price,
            quantity: fill.quantity,
            side: order.side,
            timestamp: now,
            trade_id,
        };
        let balances: Vec<Balance> = self.account.get_all_balances();
        let mut positions: Vec<Position> = self.account.get_all_positions();
        if linear && self.account.get_position(&order.instrument_id).is_none() {
            // Account drops flat rows; still emit qty=0 so the runner can clear TSM.
            positions.push(Position {
                instrument_id: order.instrument_id.clone(),
                side: order.side,
                quantity: Decimal::ZERO,
                entry_price: Decimal::ZERO,
                unrealized_pnl: Some(Decimal::ZERO),
            });
        }

        let emit_private_account =
            |exchange: &mut Self, balances: &[Balance], positions: &[Position]| {
                for bal in balances {
                    exchange.send_private(PrivateEvent::Balance(bal.clone()));
                }
                for pos in positions {
                    exchange.send_private(PrivateEvent::Position(pos.clone()));
                }
            };

        if position_first {
            emit_private_account(self, &balances, &positions);
            self.send_private(PrivateEvent::Execution(exec.clone()));
            if duplicate {
                self.send_private(PrivateEvent::Execution(exec));
            }
        } else {
            self.send_private(PrivateEvent::Execution(exec.clone()));
            if duplicate {
                self.send_private(PrivateEvent::Execution(exec));
            }
            emit_private_account(self, &balances, &positions);
        }
        self.send_market(MarketDataEvent::PublicTrade(pt));

        Ok(())
    }

    pub fn place_order(&mut self, req: OrderRequest) -> Result<Order, ExchangeError> {
        let now = self.now();
        if let Some(reason) = self.reject_next.take() {
            return Err(ExchangeError::Validation(reason));
        }
        let mut suppress = false;
        if self.suppress_next_any {
            self.suppress_next_any = false;
            suppress = true;
        }
        if let Some(cid) = req.client_order_id.as_ref() {
            if self.suppress_place_ack.remove(&cid.0) {
                suppress = true;
            }
        }
        let order = self
            .order_manager
            .place_order(req, &mut self.account, now)?;

        if !suppress {
            self.send_private(PrivateEvent::Execution(Self::place_ack_execution(
                &order, now, false,
            )));
        }

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
    /// Ordered by placement sequence (oldest first); only open orders are scanned.
    pub fn get_open_orders(&self, instrument_id: Option<&InstrumentId>) -> Vec<Order> {
        self.order_manager
            .open_orders()
            .into_iter()
            .filter(|o| {
                instrument_id
                    .map(|iid| &o.instrument_id == iid)
                    .unwrap_or(true)
            })
            .cloned()
            .collect()
    }

    /// Look up an order by client id, including filled/canceled/rejected.
    pub fn get_order_by_client_id(&self, client_order_id: &str) -> Option<Order> {
        self.order_manager
            .get_order_by_client_id(&ClientOrderId(client_order_id.to_string()))
            .cloned()
    }

    /// Next matching `client_order_id` is accepted on the book but the place ACK is dropped.
    pub fn suppress_next_place_ack(&mut self, client_order_id: String) {
        self.suppress_place_ack.insert(client_order_id);
    }

    /// Next place, regardless of client id, is accepted but the place ACK is dropped.
    pub fn suppress_next_place_ack_any(&mut self) {
        self.suppress_next_any = true;
    }

    /// Next place is rejected and not booked.
    pub fn reject_next_place(&mut self, reason: String) {
        self.reject_next = Some(reason);
    }

    /// Next fill execution is emitted twice (idempotency lab).
    pub fn duplicate_next_fill(&mut self) {
        self.duplicate_next_fill = true;
    }

    /// Stamp subsequent injected books with an aged timestamp (stale-MD lab).
    pub fn set_stale_book_age_secs(&mut self, age_secs: i64) {
        self.stale_book_age_secs = Some(age_secs.max(0));
    }

    /// Next place is dropped: not booked, no ACK, no reject.
    pub fn drop_next_place(&mut self) {
        self.drop_next_place = true;
    }

    /// Consume the drop-next-place flag if armed.
    pub fn take_drop_next_place(&mut self) -> bool {
        let armed = self.drop_next_place;
        self.drop_next_place = false;
        armed
    }

    /// Cap the next fill quantity so the order remains working.
    pub fn partial_next_fill(&mut self, qty: Decimal) {
        self.partial_next_qty = Some(qty);
    }

    /// Emit Position before Execution on the next fill.
    pub fn emit_position_before_fill(&mut self) {
        self.position_before_fill = true;
    }

    /// Broadcast a Position on the private stream without changing REST account state.
    pub fn emit_ws_position(&mut self, quantity: Decimal, entry_price: Decimal) {
        let instrument_id = self
            .config
            .instruments
            .first()
            .map(|i| i.id.clone())
            .unwrap_or_else(|| InstrumentId::new("BTC-USDT"));
        self.send_private(PrivateEvent::Position(Position {
            instrument_id,
            side: if quantity < Decimal::ZERO {
                Side::Sell
            } else {
                Side::Buy
            },
            quantity,
            entry_price,
            unrealized_pnl: Some(Decimal::ZERO),
        }));
    }

    fn place_ack_execution(
        order: &Order,
        now: chrono::DateTime<Utc>,
        omit_client_order_id: bool,
    ) -> Execution {
        Execution {
            order_id: order.order_id,
            trade_id: TradeId::nil(),
            instrument_id: order.instrument_id.clone(),
            side: order.side,
            price: order.price,
            quantity: Decimal::ZERO,
            fee: None,
            timestamp: now,
            reject_reason: None,
            client_order_id: if omit_client_order_id {
                None
            } else {
                order.client_order_id.clone()
            },
        }
    }

    /// Emit a delayed place ACK for `order_id`. Used to inject a wrong ACK while
    /// another place is in flight. `omit_client_order_id` reproduces the legacy
    /// uncorrelated ACK.
    pub fn emit_place_ack(
        &mut self,
        order_id: &str,
        omit_client_order_id: bool,
    ) -> Result<(), ExchangeError> {
        let uuid = uuid::Uuid::parse_str(order_id.trim()).map_err(|_| {
            ExchangeError::Validation(format!("invalid emit_place_ack order id {order_id}"))
        })?;
        let order = self
            .order_manager
            .get_order(OrderId(uuid))
            .cloned()
            .ok_or_else(|| ExchangeError::Validation(format!("order {order_id} not found")))?;
        self.send_private(PrivateEvent::Execution(Self::place_ack_execution(
            &order,
            self.now(),
            omit_client_order_id,
        )));
        Ok(())
    }

    /// Fill `qty` of a named open order, or the oldest open order if `order_id` is omitted.
    /// "Oldest" is placement order; two-level labs should still name the target explicitly.
    pub fn lab_fill_open(
        &mut self,
        qty: Decimal,
        order_id: Option<&str>,
    ) -> Result<Order, ExchangeError> {
        let order = if let Some(raw) = order_id.map(str::trim).filter(|s| !s.is_empty()) {
            let uuid = uuid::Uuid::parse_str(raw).map_err(|_| {
                ExchangeError::Validation(format!("invalid lab fill order id {raw}"))
            })?;
            self.order_manager
                .get_order(OrderId(uuid))
                .cloned()
                .ok_or_else(|| ExchangeError::Validation(format!("order {raw} not found")))?
        } else {
            self.order_manager
                .open_orders()
                .into_iter()
                .next()
                .cloned()
                .ok_or_else(|| ExchangeError::Validation("no open order to fill".into()))?
        };
        let remaining = order.quantity - order.filled_qty;
        if remaining <= Decimal::ZERO {
            return Err(ExchangeError::Validation(
                "open order has no remaining qty".into(),
            ));
        }
        let fill_qty = if qty > Decimal::ZERO {
            qty.min(remaining)
        } else {
            remaining
        };
        let book = self.orderbook.get(&order.instrument_id);
        let price = match order.side {
            Side::Buy => book.and_then(|b| b.best_ask()).unwrap_or(order.price),
            Side::Sell => book.and_then(|b| b.best_bid()).unwrap_or(order.price),
        };
        let fill = Fill {
            price,
            quantity: fill_qty,
            fee: Decimal::ZERO,
        };
        self.apply_fill(&order, &fill)?;
        self.order_manager
            .get_order(order.order_id)
            .cloned()
            .ok_or_else(|| ExchangeError::Validation("filled order missing".into()))
    }

    /// Clear one-shot lab faults so a later fixture cannot inherit them.
    pub fn reset_lab_faults(&mut self) {
        self.suppress_place_ack.clear();
        self.suppress_next_any = false;
        self.reject_next = None;
        self.duplicate_next_fill = false;
        self.stale_book_age_secs = None;
        self.drop_next_place = false;
        self.partial_next_qty = None;
        self.position_before_fill = false;
    }

    /// Return trade executions (fills only), optionally filtered by instrument_id and limited to the last N.
    pub fn get_trades(
        &self,
        instrument_id: Option<&InstrumentId>,
        limit: Option<usize>,
    ) -> Vec<Execution> {
        let matching = self.trades.iter().filter(|e| {
            instrument_id
                .map(|iid| &e.instrument_id == iid)
                .unwrap_or(true)
        });
        // Trades are stored in insertion order; apply the limit from the end, keep order.
        let mut trades: Vec<Execution> = match limit {
            Some(n) => matching.rev().take(n).cloned().collect(),
            None => return matching.cloned().collect(),
        };
        trades.reverse();
        trades
    }

    /// Return the shared static config (used to derive /exchange_info).
    pub fn get_config(&self) -> Arc<Config> {
        Arc::clone(&self.config)
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

    /// Apply one controlled funding settlement: `rate * signed_qty * mark` in quote (USDT).
    /// Rate comes from the explicit linear fixture (`funding_rate`); this is not an 8h Bybit clock.
    pub fn apply_funding(
        &mut self,
        instrument_id: &InstrumentId,
        mark: Decimal,
    ) -> Result<(), ExchangeError> {
        let instrument = self
            .config
            .instruments
            .iter()
            .find(|i| &i.id == instrument_id)
            .ok_or_else(|| ExchangeError::InvalidInstrument(instrument_id.0.clone()))?;
        let linear = self.config.linear.as_ref().ok_or_else(|| {
            ExchangeError::Validation("funding requires explicit linear contract parameters".into())
        })?;
        let Some(pos) = self.account.get_position(instrument_id).cloned() else {
            return Ok(());
        };
        let amount = linear.funding_rate * pos.quantity * mark;
        self.account
            .apply_funding(&instrument.quote_asset, -amount)?;
        for bal in self.account.get_all_balances() {
            self.send_private(PrivateEvent::Balance(bal));
        }
        Ok(())
    }
}
