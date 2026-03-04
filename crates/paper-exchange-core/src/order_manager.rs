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

        validate_order_request(&self.config, &req, quote_available, base_available)?;

        let price = req.limit_price.unwrap_or(Decimal::ZERO);

        let reserve_amount = match req.side {
            Side::Buy => req.quantity * price,
            Side::Sell => req.quantity,
        };
        let reserve_asset = match req.side {
            Side::Buy => &instrument.quote_asset,
            Side::Sell => &instrument.base_asset,
        };
        account.reserve(reserve_asset, reserve_amount)?;

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

        let (instrument_id, side, quantity, price, filled_qty, client_order_id) = {
            let order = self
                .orders_by_id
                .get(&order_id)
                .ok_or(ExchangeError::OrderNotFound)?;
            if order.status != OrderStatus::New && order.status != OrderStatus::PartiallyFilled {
                return Err(ExchangeError::Validation("order not cancelable".into()));
            }
            (
                order.instrument_id.clone(),
                order.side.clone(),
                order.quantity,
                order.price,
                order.filled_qty,
                order.client_order_id.clone(),
            )
        };

        let instrument = self
            .get_instrument(&instrument_id)
            .ok_or_else(|| ExchangeError::InvalidInstrument(instrument_id.0.clone()))?;

        let released = quantity - filled_qty;
        let reserve_asset = match side {
            Side::Buy => &instrument.quote_asset,
            Side::Sell => &instrument.base_asset,
        };
        let release_amount = match side {
            Side::Buy => released * price,
            Side::Sell => released,
        };
        account.release(reserve_asset, release_amount);

        let order = self.orders_by_id.get_mut(&order_id).unwrap();
        order.status = OrderStatus::Canceled;
        order.updated_at = now;

        let out = order.clone();
        if let Some(ref cid) = client_order_id {
            self.orders_by_client_id.remove(cid);
        }
        Ok(out)
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
            return Err(ExchangeError::Validation("order not amendable".into()));
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
