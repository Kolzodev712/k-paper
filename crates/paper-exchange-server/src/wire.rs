//! Wire format: client <-> server messages (JSON). Multiplexed by "type" field.

use paper_exchange_core::types::*;
use rust_decimal::Decimal;
use serde::de::Error;
use serde::{Deserialize, Deserializer, Serialize};
use std::str::FromStr;

/// Deserialize Decimal from JSON number or string (clients may send quantity/limit_price as 0.002 or "0.002").
fn decimal_from_number_or_string<'de, D>(d: D) -> Result<Decimal, D::Error>
where
    D: Deserializer<'de>,
{
    let v = serde_json::Value::deserialize(d)?;
    match v {
        serde_json::Value::Number(n) => {
            let s = n.to_string();
            Decimal::from_str(&s).map_err(Error::custom)
        }
        serde_json::Value::String(s) => Decimal::from_str(&s).map_err(Error::custom),
        _ => Err(Error::custom(
            "quantity/limit_price: expected number or string",
        )),
    }
}

fn opt_decimal_from_number_or_string<'de, D>(d: D) -> Result<Option<Decimal>, D::Error>
where
    D: Deserializer<'de>,
{
    let v = Option::<serde_json::Value>::deserialize(d)?;
    match v {
        None => Ok(None),
        Some(serde_json::Value::Number(n)) => {
            let s = n.to_string();
            Decimal::from_str(&s).map(Some).map_err(Error::custom)
        }
        Some(serde_json::Value::String(s)) => {
            Decimal::from_str(&s).map(Some).map_err(Error::custom)
        }
        Some(_) => Err(Error::custom("limit_price: expected number or string")),
    }
}

/// Deserialize Option<ClientOrderId> from null, string, or [string] (some clients send a one-element array).
fn opt_client_order_id_from_string_or_array<'de, D>(d: D) -> Result<Option<ClientOrderId>, D::Error>
where
    D: Deserializer<'de>,
{
    let v = serde_json::Value::deserialize(d)?;
    match v {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::String(s) => Ok(Some(ClientOrderId(s))),
        serde_json::Value::Array(arr) => {
            let s = arr
                .into_iter()
                .next()
                .and_then(|v| v.as_str().map(String::from));
            Ok(s.map(ClientOrderId))
        }
        _ => Err(Error::custom(
            "client_order_id: expected null, string, or [string]",
        )),
    }
}

/// Wire-only order request: accepts quantity/limit_price as JSON number or string; client_order_id as string or [string].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireOrderRequest {
    pub instrument_id: InstrumentId,
    pub side: Side,
    pub order_type: OrderType,
    #[serde(deserialize_with = "decimal_from_number_or_string")]
    pub quantity: Decimal,
    #[serde(default, deserialize_with = "opt_decimal_from_number_or_string")]
    pub limit_price: Option<Decimal>,
    #[serde(default, deserialize_with = "opt_client_order_id_from_string_or_array")]
    pub client_order_id: Option<ClientOrderId>,
}

impl From<WireOrderRequest> for OrderRequest {
    fn from(w: WireOrderRequest) -> Self {
        Self {
            instrument_id: w.instrument_id,
            side: w.side,
            order_type: w.order_type,
            quantity: w.quantity,
            limit_price: w.limit_price,
            client_order_id: w.client_order_id,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    PlaceOrder {
        request: WireOrderRequest,
    },
    /// Cancel all open orders.
    CancelAllOrders,
    CancelOrder {
        order_id: String,
    },
    CancelByClientOrderId {
        client_order_id: String,
    },
    AmendOrder {
        order_id: String,
        quantity: Option<String>,
        limit_price: Option<String>,
    },
    /// Request current balances and positions (same as sent on connect). Server responds with balance/position messages.
    GetAccountSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    /// Sent once on connect so the client can verify the connection and server version.
    Connected {},
    Orderbook(OrderbookSnapshot),
    PublicTrade(PublicTrade),
    Execution(Execution),
    Balance(Balance),
    Position(Position),
    #[serde(rename = "CancelAck")]
    CancelAck {
        order_id: String,
    },
    #[serde(rename = "CancelAllAck")]
    CancelAllAck {
        order_ids: Vec<String>,
    },
    #[serde(rename = "AmendAck")]
    AmendAck {
        order_id: String,
    },
    Error {
        message: String,
    },
}

/// Internal event for broadcast (wraps core stream events).
#[derive(Clone, Debug)]
pub enum BroadcastEvent {
    Market(paper_exchange_core::types::StreamEvent<paper_exchange_core::types::MarketDataEvent>),
    Private(paper_exchange_core::types::StreamEvent<paper_exchange_core::types::PrivateEvent>),
}

impl From<paper_exchange_core::types::StreamEvent<paper_exchange_core::types::MarketDataEvent>>
    for ServerMessage
{
    fn from(
        ev: paper_exchange_core::types::StreamEvent<paper_exchange_core::types::MarketDataEvent>,
    ) -> Self {
        match ev.payload {
            paper_exchange_core::types::MarketDataEvent::Orderbook(s) => {
                ServerMessage::Orderbook(s)
            }
            paper_exchange_core::types::MarketDataEvent::PublicTrade(t) => {
                ServerMessage::PublicTrade(t)
            }
        }
    }
}

impl From<paper_exchange_core::types::StreamEvent<paper_exchange_core::types::PrivateEvent>>
    for ServerMessage
{
    fn from(
        ev: paper_exchange_core::types::StreamEvent<paper_exchange_core::types::PrivateEvent>,
    ) -> Self {
        match ev.payload {
            paper_exchange_core::types::PrivateEvent::Execution(e) => ServerMessage::Execution(e),
            paper_exchange_core::types::PrivateEvent::Balance(b) => ServerMessage::Balance(b),
            paper_exchange_core::types::PrivateEvent::Position(p) => ServerMessage::Position(p),
        }
    }
}
