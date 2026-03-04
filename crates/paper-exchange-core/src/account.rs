//! Account state: balances (available, locked, total). Reserve on place, release on fill/cancel.

use crate::types::{Asset, Balance, Position, Side};
use rust_decimal::Decimal;
use std::collections::HashMap;

#[derive(Clone, Debug, Default)]
pub struct Account {
    balances: HashMap<Asset, Balance>,
    positions: HashMap<crate::types::InstrumentId, Position>,
}

impl Account {
    pub fn new(initial_balances: &[crate::config::InitialBalance]) -> Self {
        let mut balances = HashMap::new();
        for ib in initial_balances {
            balances.insert(
                ib.asset.clone(),
                Balance {
                    asset: ib.asset.clone(),
                    available: ib.amount,
                    locked: Decimal::ZERO,
                    total: ib.amount,
                },
            );
        }
        Self {
            balances,
            positions: HashMap::new(),
        }
    }

    fn ensure_asset(&mut self, asset: &Asset) {
        if !self.balances.contains_key(asset) {
            self.balances.insert(
                asset.clone(),
                Balance {
                    asset: asset.clone(),
                    available: Decimal::ZERO,
                    locked: Decimal::ZERO,
                    total: Decimal::ZERO,
                },
            );
        }
    }

    /// Reserve (lock) amount of asset for an order. Call on place order.
    pub fn reserve(
        &mut self,
        asset: &Asset,
        amount: Decimal,
    ) -> Result<(), crate::error::ExchangeError> {
        self.ensure_asset(asset);
        let bal = self.balances.get_mut(asset).unwrap();
        if bal.available < amount {
            return Err(crate::error::ExchangeError::InsufficientBalance);
        }
        bal.available -= amount;
        bal.locked += amount;
        Ok(())
    }

    /// Release (unlock) amount. Call on cancel or when reducing filled quantity.
    pub fn release(&mut self, asset: &Asset, amount: Decimal) {
        self.ensure_asset(asset);
        let bal = self.balances.get_mut(asset).unwrap();
        let release = amount.min(bal.locked);
        bal.locked -= release;
        bal.available += release;
    }

    /// Credit (add) to available. Call when receiving base/quote from a fill.
    pub fn credit(&mut self, asset: &Asset, amount: Decimal) {
        self.ensure_asset(asset);
        let bal = self.balances.get_mut(asset).unwrap();
        bal.available += amount;
        bal.total += amount;
    }

    pub fn get_balance(&self, asset: &Asset) -> Option<&Balance> {
        self.balances.get(asset)
    }

    pub fn get_all_balances(&self) -> Vec<Balance> {
        self.balances.values().cloned().collect()
    }

    pub fn get_position(&self, instrument_id: &crate::types::InstrumentId) -> Option<&Position> {
        self.positions.get(instrument_id)
    }

    pub fn get_all_positions(&self) -> Vec<Position> {
        self.positions.values().cloned().collect()
    }

    pub fn update_position(&mut self, position: Position) {
        if position.quantity == Decimal::ZERO {
            self.positions.remove(&position.instrument_id);
        } else {
            self.positions
                .insert(position.instrument_id.clone(), position);
        }
    }

    /// Apply a fill: release reserved side, credit the other side. Fee reduces credit.
    pub fn apply_fill(
        &mut self,
        side: Side,
        quote_asset: &Asset,
        base_asset: &Asset,
        price: Decimal,
        quantity: Decimal,
        fee: Decimal,
    ) {
        match side {
            Side::Buy => {
                let quote_amount = price * quantity + fee;
                self.release(quote_asset, quote_amount);
                self.credit(base_asset, quantity);
            }
            Side::Sell => {
                let base_amount = quantity;
                let quote_amount = price * quantity - fee;
                self.release(base_asset, base_amount);
                self.credit(quote_asset, quote_amount);
            }
        }
    }
}
