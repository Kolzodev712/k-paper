//! Order lifecycle: place, cancel, amend; open order index. Idempotency: reject duplicate client_order_id.

use crate::config::Config;
use crate::error::ExchangeError;
use crate::types::{AmendRequest, CancelRequest, Order, OrderRequest, OrderStatus, Side};
use chrono::Utc;
use rust_decimal::Decimal;
use std::collections::HashMap;

use crate::account::Account;
use crate::validation::validate_order_request;

pub struct OrderManager {
    config: Config,
    orders_by_id: HashMap<crate::types::OrderId, Order>,
    orders_by_client_id: HashMap<crate::types::ClientOrderId, crate::types::OrderId>,
}

impl OrderManager {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            orders_by_id: HashMap::new(),
            orders_by_client_id: HashMap::new(),
        }
    }

    fn get_instrument(
        &self,
        instrument_id: &crate::types::InstrumentId,
    ) -> Option<&crate::types::Instrument> {
        self.config
            .instruments
            .iter()
            .find(|i| i.id.0 == instrument_id.0)
    }

    pub fn place_order(
        &mut self,
        req: OrderRequest,
        account: &mut Account,
        now: chrono::DateTime<Utc>,
    ) -> Result<Order, ExchangeError> {
        if let Some(ref cid) = req.client_order_id {
            if self.orders_by_client_id.contains_key(cid) {
                return Err(ExchangeError::Validation(
                    "duplicate client_order_id".into(),
                ));
            }
        }

        let instrument = self
            .get_instrument(&req.instrument_id)
            .ok_or_else(|| ExchangeError::InvalidInstrument(req.instrument_id.0.clone()))?;

        let quote_available = account
            .get_balance(&instrument.quote_asset)
            .map(|b| b.available)
            .unwrap_or(Decimal::ZERO);
        let base_available = account
            .get_balance(&instrument.base_asset)
            .map(|b| b.available)
            .unwrap_or(Decimal::ZERO);
        let position_qty = account
            .get_position(&req.instrument_id)
            .map(|p| p.quantity)
            .unwrap_or(Decimal::ZERO);

        validate_order_request(
            &self.config,
            &req,
            quote_available,
            base_available,
            position_qty,
        )?;

        let price = req.limit_price.unwrap_or(Decimal::ZERO);
        let linear = crate::linear::instrument_is_linear(instrument);
        if linear {
            if !req.reduce_only {
                let leverage =
                    self.config
                        .linear
                        .as_ref()
                        .map(|c| c.leverage)
                        .ok_or_else(|| {
                            ExchangeError::Validation("linear contract parameters required".into())
                        })?;
                let im = crate::linear::initial_margin(req.quantity * price, leverage)?;
                account.reserve(&instrument.quote_asset, im)?;
            }
        } else {
            let reserve_amount = match req.side {
                Side::Buy => req.quantity * price,
                Side::Sell => req.quantity,
            };
            let reserve_asset = match req.side {
                Side::Buy => &instrument.quote_asset,
                Side::Sell => &instrument.base_asset,
            };
            account.reserve(reserve_asset, reserve_amount)?;
        }

        let order_id = crate::types::OrderId::new();
        let order = Order {
            order_id,
            client_order_id: req.client_order_id.clone(),
            instrument_id: req.instrument_id.clone(),
            side: req.side.clone(),
            order_type: req.order_type.clone(),
            quantity: req.quantity,
            price,
            filled_qty: Decimal::ZERO,
            status: OrderStatus::New,
            created_at: now,
            updated_at: now,
            reduce_only: req.reduce_only,
        };

        self.orders_by_id.insert(order_id, order.clone());
        if let Some(ref cid) = req.client_order_id {
            self.orders_by_client_id.insert(cid.clone(), order_id);
        }

        Ok(order)
    }

    pub fn cancel_order(
        &mut self,
        req: CancelRequest,
        account: &mut Account,
        now: chrono::DateTime<Utc>,
    ) -> Result<Order, ExchangeError> {
        let order_id = match req {
            CancelRequest::ByOrderId(id) => id,
            CancelRequest::ByClientOrderId(ref cid) => *self
                .orders_by_client_id
                .get(cid)
                .ok_or(ExchangeError::OrderNotFound)?,
        };

        let (instrument_id, side, quantity, price, filled_qty, _client_order_id, reduce_only) = {
            let order = self
                .orders_by_id
                .get(&order_id)
                .ok_or(ExchangeError::OrderNotFound)?;
            if order.status != OrderStatus::New && order.status != OrderStatus::PartiallyFilled {
                let reason = match order.status {
                    OrderStatus::Filled => "order already filled",
                    OrderStatus::Canceled => "order already cancelled",
                    OrderStatus::Rejected => "order was rejected",
                    OrderStatus::New | OrderStatus::PartiallyFilled => unreachable!(),
                };
                return Err(ExchangeError::Validation(reason.into()));
            }
            (
                order.instrument_id.clone(),
                order.side.clone(),
                order.quantity,
                order.price,
                order.filled_qty,
                order.client_order_id.clone(),
                order.reduce_only,
            )
        };

        let instrument = self
            .get_instrument(&instrument_id)
            .ok_or_else(|| ExchangeError::InvalidInstrument(instrument_id.0.clone()))?;

        let released = quantity - filled_qty;
        if crate::linear::instrument_is_linear(instrument) {
            if !reduce_only {
                let leverage = self.config.linear.as_ref().map(|c| c.leverage).unwrap_or(1);
                let im = crate::linear::initial_margin(released * price, leverage)
                    .unwrap_or(Decimal::ZERO);
                account.release(&instrument.quote_asset, im);
            }
        } else {
            let reserve_asset = match side {
                Side::Buy => &instrument.quote_asset,
                Side::Sell => &instrument.base_asset,
            };
            let release_amount = match side {
                Side::Buy => released * price,
                Side::Sell => released,
            };
            account.release(reserve_asset, release_amount);
        }

        let order = self.orders_by_id.get_mut(&order_id).unwrap();
        order.status = OrderStatus::Canceled;
        order.updated_at = now;

        let out = order.clone();
        Ok(out)
    }

    /// Cancel every open order (New/PartiallyFilled). Returns the orders that were canceled.
    ///
    /// This is best-effort: if an order becomes non-cancelable between enumeration and cancel,
    /// it will be skipped and the rest will still be canceled.
    pub fn cancel_all_orders(
        &mut self,
        account: &mut Account,
        now: chrono::DateTime<Utc>,
    ) -> Vec<Order> {
        let ids: Vec<crate::types::OrderId> =
            self.open_orders().into_iter().map(|o| o.order_id).collect();
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            if let Ok(order) = self.cancel_order(CancelRequest::ByOrderId(id), account, now) {
                out.push(order);
            }
        }
        out
    }

    pub fn amend_order(
        &mut self,
        req: AmendRequest,
        account: &mut Account,
        now: chrono::DateTime<Utc>,
    ) -> Result<Order, ExchangeError> {
        let order = self
            .orders_by_id
            .get(&req.order_id)
            .ok_or(ExchangeError::OrderNotFound)?
            .clone();

        if order.status != OrderStatus::New && order.status != OrderStatus::PartiallyFilled {
            let reason = match order.status {
                OrderStatus::Filled => "order already filled",
                OrderStatus::Canceled => "order already cancelled",
                OrderStatus::Rejected => "order was rejected",
                OrderStatus::New | OrderStatus::PartiallyFilled => unreachable!(),
            };
            return Err(ExchangeError::Validation(format!(
                "order not amendable ({})",
                reason
            )));
        }

        let new_qty = req.quantity.unwrap_or(order.quantity);
        let new_price = req.limit_price.unwrap_or(order.price);

        if new_qty < order.filled_qty {
            return Err(ExchangeError::Validation(
                "new quantity cannot be less than filled quantity".into(),
            ));
        }

        let instrument = self
            .get_instrument(&order.instrument_id)
            .ok_or_else(|| ExchangeError::InvalidInstrument(order.instrument_id.0.clone()))?;

        let old_reserve = (order.quantity - order.filled_qty) * order.price;
        let new_reserve = (new_qty - order.filled_qty) * new_price;
        let reserve_asset = match order.side {
            Side::Buy => &instrument.quote_asset,
            Side::Sell => &instrument.base_asset,
        };
        let old_reserve_qty = order.quantity - order.filled_qty;
        let new_reserve_qty = new_qty - order.filled_qty;

        if order.side == Side::Buy {
            if new_reserve > old_reserve {
                account.reserve(reserve_asset, new_reserve - old_reserve)?;
            } else {
                account.release(reserve_asset, old_reserve - new_reserve);
            }
        } else if new_reserve_qty > old_reserve_qty {
            account.reserve(reserve_asset, new_reserve_qty - old_reserve_qty)?;
        } else {
            account.release(reserve_asset, old_reserve_qty - new_reserve_qty);
        }

        let o = self.orders_by_id.get_mut(&req.order_id).unwrap();
        o.quantity = new_qty;
        o.price = new_price;
        o.updated_at = now;

        Ok(o.clone())
    }

    pub fn get_order(&self, order_id: crate::types::OrderId) -> Option<&Order> {
        self.orders_by_id.get(&order_id)
    }

    pub fn get_order_by_client_id(
        &self,
        client_order_id: &crate::types::ClientOrderId,
    ) -> Option<&Order> {
        let order_id = self.orders_by_client_id.get(client_order_id)?;
        self.orders_by_id.get(order_id)
    }

    pub fn get_order_mut(&mut self, order_id: &crate::types::OrderId) -> Option<&mut Order> {
        self.orders_by_id.get_mut(order_id)
    }

    pub fn open_orders(&self) -> Vec<&Order> {
        self.orders_by_id
            .values()
            .filter(|o| o.status == OrderStatus::New || o.status == OrderStatus::PartiallyFilled)
            .collect()
    }

    pub fn config(&self) -> &Config {
        &self.config
    }
}
