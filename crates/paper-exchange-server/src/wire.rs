//! Wire format: client <-> server messages (JSON). Multiplexed by "type" field.

use paper_exchange_core::types::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    PlaceOrder {
        request: OrderRequest,
    },
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
