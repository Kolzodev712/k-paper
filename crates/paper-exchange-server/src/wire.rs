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
    #[serde(default)]
    pub reduce_only: bool,
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
            reduce_only: w.reduce_only,
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
    /// Market/private stream events carry the core stream `seq` (per stream: market and private
    /// are numbered independently, starting at 1). A gap means events were missed. Direct replies
    /// (account snapshot, place reply) are not stream events and omit `seq`.
    Orderbook {
        #[serde(flatten)]
        snapshot: OrderbookSnapshot,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        seq: Option<u64>,
    },
    PublicTrade {
        #[serde(flatten)]
        trade: PublicTrade,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        seq: Option<u64>,
    },
    Execution {
        #[serde(flatten)]
        execution: Execution,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        seq: Option<u64>,
    },
    Balance {
        #[serde(flatten)]
        balance: Balance,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        seq: Option<u64>,
    },
    Position {
        #[serde(flatten)]
        position: Position,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        seq: Option<u64>,
    },
    /// This connection fell behind the broadcast and `skipped` events were dropped. Sent once,
    /// immediately before the server closes the socket: reconnect to get a fresh account
    /// snapshot and resync (the stream cannot be resumed).
    Lagged {
        skipped: u64,
    },
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
    /// Lab fault: every connected WS client should close.
    LabDisconnect,
}

impl From<paper_exchange_core::types::StreamEvent<paper_exchange_core::types::MarketDataEvent>>
    for ServerMessage
{
    fn from(
        ev: paper_exchange_core::types::StreamEvent<paper_exchange_core::types::MarketDataEvent>,
    ) -> Self {
        let seq = Some(ev.sequence);
        match ev.payload {
            paper_exchange_core::types::MarketDataEvent::Orderbook(snapshot) => {
                ServerMessage::Orderbook { snapshot, seq }
            }
            paper_exchange_core::types::MarketDataEvent::PublicTrade(trade) => {
                ServerMessage::PublicTrade { trade, seq }
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
        let seq = Some(ev.sequence);
        match ev.payload {
            paper_exchange_core::types::PrivateEvent::Execution(execution) => {
                ServerMessage::Execution { execution, seq }
            }
            paper_exchange_core::types::PrivateEvent::Balance(balance) => {
                ServerMessage::Balance { balance, seq }
            }
            paper_exchange_core::types::PrivateEvent::Position(position) => {
                ServerMessage::Position { position, seq }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use rust_decimal::Decimal;

    #[test]
    fn execution_ack_wire_includes_client_order_id() {
        let exec = Execution {
            order_id: OrderId::new(),
            trade_id: TradeId::nil(),
            instrument_id: InstrumentId::new("BTC-USDT"),
            side: Side::Buy,
            price: Decimal::new(1000, 0),
            quantity: Decimal::ZERO,
            fee: None,
            timestamp: Utc::now(),
            reject_reason: None,
            client_order_id: Some(ClientOrderId("cid-1".into())),
        };
        let json = serde_json::to_string(&ServerMessage::Execution {
            execution: exec,
            seq: None,
        })
        .unwrap();
        assert!(json.contains("cid-1"), "{json}");
        assert!(json.contains("client_order_id"), "{json}");
        assert!(
            !json.contains("\"seq\""),
            "direct replies carry no seq: {json}"
        );
    }

    #[test]
    fn stream_events_carry_seq_on_the_wire() {
        use paper_exchange_core::types::StreamEvent;
        let ev = StreamEvent {
            sequence: 42,
            payload: PrivateEvent::Balance(Balance {
                asset: Asset("USDT".into()),
                available: Decimal::ONE,
                locked: Decimal::ZERO,
                total: Decimal::ONE,
            }),
        };
        let json = serde_json::to_string(&ServerMessage::from(ev)).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["type"], "balance");
        assert_eq!(v["seq"], 42);
        assert_eq!(v["asset"], "USDT");
        let back: ServerMessage = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, ServerMessage::Balance { seq: Some(42), .. }));
    }

    #[test]
    fn lagged_message_serializes_skipped_count() {
        let json = serde_json::to_string(&ServerMessage::Lagged { skipped: 7 }).unwrap();
        assert_eq!(json, r#"{"type":"lagged","skipped":7}"#);
    }

    #[test]
    fn config_json_defaults_max_trade_history() {
        use paper_exchange_core::config::Config;
        let cfg: Config = serde_json::from_str(
            r#"{"instruments":[],"initial_balances":[],"fill_model":{"kind":"immediate_cross","slippage_bps":0,"fee_bps":0,"partial_fills":false}}"#,
        )
        .unwrap();
        assert_eq!(cfg.max_trade_history, 10_000);
    }
}
