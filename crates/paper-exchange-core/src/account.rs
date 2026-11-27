//! Account state: balances (available, locked, total). Reserve on place, release on fill/cancel.

use crate::error::ExchangeError;
use crate::linear::{initial_margin, signed_fill_qty};
use crate::types::{Asset, Balance, InstrumentId, Position, Side};
use rust_decimal::Decimal;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default)]
pub struct Account {
    balances: BTreeMap<Asset, Balance>,
    positions: BTreeMap<InstrumentId, Position>,
}

/// Parameters for a USDT-linear fill. Quantity is signed onto the position.
pub struct LinearFill<'a> {
    pub quote_asset: &'a Asset,
    pub instrument_id: &'a InstrumentId,
    pub side: Side,
    pub price: Decimal,
    pub quantity: Decimal,
    pub fee: Decimal,
    pub reducing: bool,
    pub leverage: u32,
    pub mark: Decimal,
}

impl Account {
    pub fn new(initial_balances: &[crate::config::InitialBalance]) -> Self {
        let mut balances = BTreeMap::new();
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
            positions: BTreeMap::new(),
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
    pub fn reserve(&mut self, asset: &Asset, amount: Decimal) -> Result<(), ExchangeError> {
        self.ensure_asset(asset);
        let bal = self.balances.get_mut(asset).unwrap();
        if bal.available < amount {
            return Err(ExchangeError::InsufficientBalance);
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

    /// Debit available (and total) for fees, funding, or negative realized PnL.
    pub fn debit(&mut self, asset: &Asset, amount: Decimal) -> Result<(), ExchangeError> {
        self.ensure_asset(asset);
        let bal = self.balances.get_mut(asset).unwrap();
        if bal.available < amount {
            return Err(ExchangeError::InsufficientBalance);
        }
        bal.available -= amount;
        bal.total -= amount;
        Ok(())
    }

    pub fn get_balance(&self, asset: &Asset) -> Option<&Balance> {
        self.balances.get(asset)
    }

    pub fn get_all_balances(&self) -> Vec<Balance> {
        self.balances.values().cloned().collect()
    }

    pub fn get_position(&self, instrument_id: &InstrumentId) -> Option<&Position> {
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

    /// True if `amount` can be settled out of `asset`: release up to `locked`, then debit from available.
    fn can_settle(&self, asset: &Asset, amount: Decimal) -> bool {
        let (available, locked) = self
            .balances
            .get(asset)
            .map(|b| (b.available, b.locked))
            .unwrap_or((Decimal::ZERO, Decimal::ZERO));
        available + amount.min(locked) >= amount
    }

    /// Apply a fill: consume the reserved side and credit the other. Fee reduces quote credit.
    ///
    /// Atomic: the debit is validated before any balance is touched, so on
    /// `InsufficientBalance` the account is unchanged.
    pub fn apply_fill(
        &mut self,
        side: Side,
        quote_asset: &Asset,
        base_asset: &Asset,
        price: Decimal,
        quantity: Decimal,
        fee: Decimal,
    ) -> Result<(), ExchangeError> {
        match side {
            Side::Buy => {
                let quote_amount = price * quantity + fee;
                if !self.can_settle(quote_asset, quote_amount) {
                    return Err(ExchangeError::InsufficientBalance);
                }
                self.release(quote_asset, quote_amount);
                self.debit(quote_asset, quote_amount)?;
                self.credit(base_asset, quantity);
            }
            Side::Sell => {
                let base_amount = quantity;
                let quote_amount = (price * quantity - fee).max(Decimal::ZERO);
                if !self.can_settle(base_asset, base_amount) {
                    return Err(ExchangeError::InsufficientBalance);
                }
                self.release(base_asset, base_amount);
                self.debit(base_asset, base_amount)?;
                self.credit(quote_asset, quote_amount);
            }
        }
        Ok(())
    }

    /// USDT-linear fill: IM stays in USDT; no base/quote swap. Quantity is signed on the position.
    ///
    /// Atomic: steps run against a scratch copy that replaces `self` only on success, so a
    /// failed debit (fee or realized loss) leaves balances and positions untouched.
    pub fn apply_linear_fill(&mut self, fill: LinearFill<'_>) -> Result<(), ExchangeError> {
        let mut next = self.clone();
        next.apply_linear_fill_steps(fill)?;
        *self = next;
        Ok(())
    }

    fn apply_linear_fill_steps(&mut self, fill: LinearFill<'_>) -> Result<(), ExchangeError> {
        if fill.fee > Decimal::ZERO {
            self.debit(fill.quote_asset, fill.fee)?;
        }

        let current = self
            .get_position(fill.instrument_id)
            .map(|p| (p.quantity, p.entry_price))
            .unwrap_or((Decimal::ZERO, Decimal::ZERO));
        let (cur_qty, entry) = current;
        let delta = signed_fill_qty(&fill.side, fill.quantity);
        let next_qty = cur_qty + delta;

        if fill.reducing {
            let closed = fill.quantity.min(cur_qty.abs());
            let realized = if cur_qty > Decimal::ZERO && matches!(fill.side, Side::Sell) {
                (fill.price - entry) * closed
            } else if cur_qty < Decimal::ZERO && matches!(fill.side, Side::Buy) {
                (entry - fill.price) * closed
            } else {
                Decimal::ZERO
            };
            let im = initial_margin(closed * entry, fill.leverage).unwrap_or(Decimal::ZERO);
            self.release(fill.quote_asset, im);
            if realized > Decimal::ZERO {
                self.credit(fill.quote_asset, realized);
            } else if realized < Decimal::ZERO {
                self.debit(fill.quote_asset, -realized)?;
            }
        }

        let new_entry = if !fill.reducing && next_qty.abs() > Decimal::ZERO {
            if cur_qty == Decimal::ZERO || (cur_qty.is_sign_positive() == delta.is_sign_positive())
            {
                let prev_notional = entry * cur_qty.abs();
                let add_notional = fill.price * fill.quantity;
                (prev_notional + add_notional) / next_qty.abs()
            } else {
                entry
            }
        } else if next_qty == Decimal::ZERO {
            Decimal::ZERO
        } else {
            entry
        };

        let side_out = if next_qty >= Decimal::ZERO {
            Side::Buy
        } else {
            Side::Sell
        };
        let upnl = if next_qty == Decimal::ZERO {
            Decimal::ZERO
        } else {
            (fill.mark - new_entry) * next_qty
        };
        self.update_position(Position {
            instrument_id: fill.instrument_id.clone(),
            side: side_out,
            quantity: next_qty,
            entry_price: new_entry,
            unrealized_pnl: Some(upnl),
        });
        Ok(())
    }

    pub fn apply_funding(
        &mut self,
        quote_asset: &Asset,
        amount: Decimal,
    ) -> Result<(), ExchangeError> {
        if amount > Decimal::ZERO {
            self.credit(quote_asset, amount);
            Ok(())
        } else if amount < Decimal::ZERO {
            self.debit(quote_asset, -amount)
        } else {
            Ok(())
        }
    }
}
