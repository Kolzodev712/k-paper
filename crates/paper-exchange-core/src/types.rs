//! Exchange-native data types (instruments, orders, trades, executions, account).

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

// ---------- IDs ----------

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct InstrumentId(pub String);

impl InstrumentId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

impl fmt::Display for InstrumentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OrderId(#[serde(with = "crate::types::uuid_str")] pub Uuid);

impl Default for OrderId {
    fn default() -> Self {
        Self::new()
    }
}

impl OrderId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl fmt::Display for OrderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TradeId(#[serde(with = "crate::types::uuid_str")] pub Uuid);

impl Default for TradeId {
    fn default() -> Self {
        Self::new()
    }
}

impl TradeId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn nil() -> Self {
        Self(Uuid::nil())
    }
}

impl fmt::Display for TradeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ClientOrderId(pub String);

impl fmt::Display for ClientOrderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AccountId(#[serde(with = "crate::types::uuid_str")] pub Uuid);

impl fmt::Display for AccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

pub mod uuid_str {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use uuid::Uuid;
    pub fn serialize<S: Serializer>(u: &Uuid, s: S) -> Result<S::Ok, S::Error> {
        u.to_string().serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Uuid, D::Error> {
        let s = String::deserialize(d)?;
        Uuid::parse_str(&s).map_err(serde::de::Error::custom)
    }
}

// ---------- Asset ----------

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Asset(pub String);

impl fmt::Display for Asset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ---------- Instrument ----------

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum InstrumentType {
    Spot,
    Futures,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Instrument {
    pub id: InstrumentId,
    pub symbol: String,
    pub base_asset: Asset,
    pub quote_asset: Asset,
    pub tick_size: Decimal,
    pub lot_size: Decimal,
    pub min_notional: Decimal,
    pub instrument_type: Option<InstrumentType>,
}

// ---------- Orderbook ----------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BookLevel {
    pub price: Decimal,
    pub quantity: Decimal,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrderbookSnapshot {
    pub instrument_id: InstrumentId,
    pub bids: Vec<BookLevel>,
    pub asks: Vec<BookLevel>,
    pub sequence: u64,
    pub timestamp: DateTime<Utc>,
}

// ---------- Order input ----------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Side {
    Buy,
    Sell,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderType {
    Limit,
    Market,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrderRequest {
    pub instrument_id: InstrumentId,
    pub side: Side,
    pub order_type: OrderType,
    pub quantity: Decimal,
    pub limit_price: Option<Decimal>,
    pub client_order_id: Option<ClientOrderId>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum CancelRequest {
    ByOrderId(OrderId),
    ByClientOrderId(ClientOrderId),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AmendRequest {
    pub order_id: OrderId,
    pub quantity: Option<Decimal>,
    pub limit_price: Option<Decimal>,
}

// ---------- Order state ----------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderStatus {
    New,
    PartiallyFilled,
    Filled,
    Canceled,
    Rejected,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Order {
    pub order_id: OrderId,
    pub client_order_id: Option<ClientOrderId>,
    pub instrument_id: InstrumentId,
    pub side: Side,
    pub order_type: OrderType,
    pub quantity: Decimal,
    pub price: Decimal,
    pub filled_qty: Decimal,
    pub status: OrderStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ---------- Trades ----------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PublicTrade {
    pub instrument_id: InstrumentId,
    pub price: Decimal,
    pub quantity: Decimal,
    pub side: Side,
    pub timestamp: DateTime<Utc>,
    pub trade_id: TradeId,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Execution {
    pub order_id: OrderId,
    pub trade_id: TradeId,
    pub instrument_id: InstrumentId,
    pub side: Side,
    pub price: Decimal,
    pub quantity: Decimal,
    pub fee: Option<Decimal>,
    pub timestamp: DateTime<Utc>,
    pub reject_reason: Option<String>,
}

// ---------- Account ----------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Balance {
    pub asset: Asset,
    pub available: Decimal,
    pub locked: Decimal,
    pub total: Decimal,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Position {
    pub instrument_id: InstrumentId,
    pub side: Side,
    pub quantity: Decimal,
    pub entry_price: Decimal,
    pub unrealized_pnl: Option<Decimal>,
}

// ---------- Stream events ----------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum MarketDataEvent {
    Orderbook(OrderbookSnapshot),
    PublicTrade(PublicTrade),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum PrivateEvent {
    Execution(Execution),
    Balance(Balance),
    Position(Position),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StreamEvent<T> {
    pub sequence: u64,
    pub payload: T,
}
