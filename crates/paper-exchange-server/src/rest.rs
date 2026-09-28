use crate::transport::{AppState, RestQuery};
use axum::extract::{Path, Query};
use axum::http::StatusCode;
use axum::Json;
use chrono::{DateTime, Utc};
use paper_exchange_core::config::Config;
use paper_exchange_core::types::{Balance, Execution, Order, OrderStatus, Side};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::oneshot;

pub async fn get_balances(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
) -> Result<Json<Vec<Balance>>, StatusCode> {
    let (tx, rx) = oneshot::channel();
    if state.rest_query_tx.send(RestQuery::Balances(tx)).is_err() {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }
    let balances = rx.await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(balances))
}

pub async fn get_positions(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
) -> Result<Json<Vec<paper_exchange_core::types::Position>>, StatusCode> {
    let (tx, rx) = oneshot::channel();
    if state.rest_query_tx.send(RestQuery::Positions(tx)).is_err() {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }
    let positions = rx.await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(positions))
}

#[derive(Deserialize)]
pub struct OpenOrdersQuery {
    #[serde(rename = "instrument_id")]
    pub instrument_id: Option<String>,
}

#[derive(Serialize)]
pub struct OpenOrderDto {
    #[serde(rename = "orderId")]
    order_id: String,
    #[serde(rename = "clientOrderId")]
    client_order_id: Option<String>,
    #[serde(rename = "instrumentId")]
    instrument_id: String,
    side: Side,
    price: Decimal,
    #[serde(rename = "amount")]
    amount: Decimal,
    #[serde(rename = "filledAmount")]
    filled_amount: Decimal,
    status: OrderStatus,
    #[serde(rename = "createdAt")]
    created_at: DateTime<Utc>,
}

impl From<Order> for OpenOrderDto {
    fn from(o: Order) -> Self {
        Self {
            order_id: o.order_id.0.to_string(),
            client_order_id: o.client_order_id.map(|c| c.0),
            instrument_id: o.instrument_id.0,
            side: o.side,
            price: o.price,
            amount: o.quantity,
            filled_amount: o.filled_qty,
            status: o.status,
            created_at: o.created_at,
        }
    }
}

pub async fn get_open_orders(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    Query(q): Query<OpenOrdersQuery>,
) -> Result<Json<Vec<OpenOrderDto>>, StatusCode> {
    let (tx, rx) = oneshot::channel();
    if state
        .rest_query_tx
        .send(RestQuery::OpenOrders {
            instrument_id: q.instrument_id,
            reply: tx,
        })
        .is_err()
    {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }
    let orders = rx.await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(orders.into_iter().map(OpenOrderDto::from).collect()))
}

#[derive(Deserialize)]
pub struct TradesQuery {
    #[serde(rename = "instrument_id")]
    pub instrument_id: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Serialize)]
pub struct TradeDto {
    #[serde(rename = "tradeId")]
    trade_id: String,
    #[serde(rename = "orderId")]
    order_id: String,
    #[serde(rename = "instrumentId")]
    instrument_id: String,
    side: Side,
    price: Decimal,
    quantity: Decimal,
    timestamp: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fee: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "feeCurrency")]
    fee_currency: Option<String>,
}

impl From<Execution> for TradeDto {
    fn from(e: Execution) -> Self {
        Self {
            trade_id: e.trade_id.0.to_string(),
            order_id: e.order_id.0.to_string(),
            instrument_id: e.instrument_id.0,
            side: e.side,
            price: e.price,
            quantity: e.quantity,
            timestamp: e.timestamp,
            fee: e.fee,
            fee_currency: None,
        }
    }
}

pub async fn get_trades(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    Query(q): Query<TradesQuery>,
) -> Result<Json<Vec<TradeDto>>, StatusCode> {
    let (tx, rx) = oneshot::channel();
    if state
        .rest_query_tx
        .send(RestQuery::Trades {
            instrument_id: q.instrument_id,
            limit: q.limit,
            reply: tx,
        })
        .is_err()
    {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }
    let trades = rx.await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(trades.into_iter().map(TradeDto::from).collect()))
}

#[derive(Serialize)]
pub struct RateLimitDto {
    #[serde(rename = "rateLimitType")]
    rate_limit_type: String,
    interval: String,
    limit: u64,
}

#[derive(Serialize)]
pub struct SymbolDto {
    symbol: String,
    #[serde(rename = "baseAsset")]
    base_asset: String,
    #[serde(rename = "quoteAsset")]
    quote_asset: String,
    status: String,
    #[serde(rename = "tradingAllowed")]
    trading_allowed: bool,
    #[serde(skip_serializing_if = "Option::is_none", rename = "minOrderSize")]
    min_order_size: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "maxOrderSize")]
    max_order_size: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "stepSize")]
    step_size: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "minPrice")]
    min_price: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "maxPrice")]
    max_price: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "tickSize")]
    tick_size: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "minNotional")]
    min_notional: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "pricePrecision")]
    price_precision: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "quantityPrecision")]
    quantity_precision: Option<u32>,
}

#[derive(Serialize)]
pub struct ExchangeInfoDto {
    #[serde(rename = "serverTime")]
    server_time: i64,
    #[serde(rename = "rateLimits")]
    rate_limits: Vec<RateLimitDto>,
    symbols: Vec<SymbolDto>,
}

pub async fn get_exchange_info(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
) -> Result<Json<ExchangeInfoDto>, StatusCode> {
    let (tx, rx) = oneshot::channel();
    if state.rest_query_tx.send(RestQuery::MarketInfo(tx)).is_err() {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }
    let cfg: Config = rx.await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let now = Utc::now().timestamp_millis();
    let symbols: Vec<SymbolDto> = cfg
        .instruments
        .into_iter()
        .map(|inst| SymbolDto {
            symbol: inst.symbol.clone(),
            base_asset: inst.base_asset.0,
            quote_asset: inst.quote_asset.0,
            status: "TRADING".to_string(),
            trading_allowed: true,
            min_order_size: Some(inst.lot_size),
            max_order_size: None,
            step_size: Some(inst.lot_size),
            min_price: None,
            max_price: None,
            tick_size: Some(inst.tick_size),
            min_notional: Some(inst.min_notional),
            price_precision: None,
            quantity_precision: None,
        })
        .collect();

    Ok(Json(ExchangeInfoDto {
        server_time: now,
        rate_limits: Vec::new(),
        symbols,
    }))
}

pub async fn get_order_by_client_id(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    Path(client_order_id): Path<String>,
) -> Result<Json<OpenOrderDto>, StatusCode> {
    let (tx, rx) = oneshot::channel();
    if state
        .rest_query_tx
        .send(RestQuery::OrderByClientId {
            client_order_id,
            reply: tx,
        })
        .is_err()
    {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }
    match rx.await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)? {
        Some(order) => Ok(Json(OpenOrderDto::from(order))),
        None => Err(StatusCode::NOT_FOUND),
    }
}

#[derive(Deserialize)]
pub struct SuppressPlaceAckBody {
    #[serde(default, rename = "clientOrderId")]
    pub client_order_id: Option<String>,
}

/// Lab fault hook: next matching place (or the next place if id omitted) is accepted but the ACK is dropped.
pub async fn post_suppress_place_ack(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    Json(body): Json<SuppressPlaceAckBody>,
) -> Result<StatusCode, StatusCode> {
    let (tx, rx) = oneshot::channel();
    if state
        .rest_query_tx
        .send(RestQuery::SuppressPlaceAck {
            client_order_id: body.client_order_id,
            reply: tx,
        })
        .is_err()
    {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }
    rx.await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct RejectNextPlaceBody {
    #[serde(default)]
    pub reason: Option<String>,
}

pub async fn post_reject_next_place(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    Json(body): Json<RejectNextPlaceBody>,
) -> Result<StatusCode, StatusCode> {
    let (tx, rx) = oneshot::channel();
    if state
        .rest_query_tx
        .send(RestQuery::RejectNextPlace {
            reason: body
                .reason
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "lab_reject".into()),
            reply: tx,
        })
        .is_err()
    {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }
    rx.await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn post_duplicate_next_fill(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
) -> Result<StatusCode, StatusCode> {
    let (tx, rx) = oneshot::channel();
    if state
        .rest_query_tx
        .send(RestQuery::DuplicateNextFill { reply: tx })
        .is_err()
    {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }
    rx.await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn post_reset_lab_faults(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
) -> Result<StatusCode, StatusCode> {
    let (tx, rx) = oneshot::channel();
    if state
        .rest_query_tx
        .send(RestQuery::ResetLabFaults { reply: tx })
        .is_err()
    {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }
    rx.await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct StaleOrderbookBody {
    #[serde(default)]
    pub age_secs: Option<i64>,
}

pub async fn post_stale_orderbook(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    Json(body): Json<StaleOrderbookBody>,
) -> Result<StatusCode, StatusCode> {
    let (tx, rx) = oneshot::channel();
    if state
        .rest_query_tx
        .send(RestQuery::StaleOrderbook {
            age_secs: body.age_secs.unwrap_or(3600),
            reply: tx,
        })
        .is_err()
    {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }
    rx.await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn post_disconnect_clients(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
) -> Result<StatusCode, StatusCode> {
    let _ = state
        .event_tx
        .send(crate::wire::BroadcastEvent::LabDisconnect);
    Ok(StatusCode::NO_CONTENT)
}

pub async fn post_drop_next_place(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
) -> Result<StatusCode, StatusCode> {
    let (tx, rx) = oneshot::channel();
    if state
        .rest_query_tx
        .send(RestQuery::DropNextPlace { reply: tx })
        .is_err()
    {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }
    rx.await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct QtyBody {
    #[serde(default)]
    pub qty: Option<Decimal>,
}

pub async fn post_partial_next_fill(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    Json(body): Json<QtyBody>,
) -> Result<StatusCode, StatusCode> {
    let (tx, rx) = oneshot::channel();
    if state
        .rest_query_tx
        .send(RestQuery::PartialNextFill {
            qty: body.qty.unwrap_or(Decimal::new(1, 3)),
            reply: tx,
        })
        .is_err()
    {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }
    rx.await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn post_position_before_fill(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
) -> Result<StatusCode, StatusCode> {
    let (tx, rx) = oneshot::channel();
    if state
        .rest_query_tx
        .send(RestQuery::PositionBeforeFill { reply: tx })
        .is_err()
    {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }
    rx.await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct EmitWsPositionBody {
    #[serde(default)]
    pub quantity: Option<Decimal>,
    #[serde(default)]
    pub entry_price: Option<Decimal>,
}

pub async fn post_emit_ws_position(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    Json(body): Json<EmitWsPositionBody>,
) -> Result<StatusCode, StatusCode> {
    let (tx, rx) = oneshot::channel();
    if state
        .rest_query_tx
        .send(RestQuery::EmitWsPosition {
            quantity: body.quantity.unwrap_or(Decimal::ONE),
            entry_price: body.entry_price.unwrap_or(Decimal::new(50000, 0)),
            reply: tx,
        })
        .is_err()
    {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }
    rx.await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn post_lab_fill_open(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    Json(body): Json<QtyBody>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let (tx, rx) = oneshot::channel();
    if state
        .rest_query_tx
        .send(RestQuery::LabFillOpen {
            qty: body.qty.unwrap_or(Decimal::ZERO),
            reply: tx,
        })
        .is_err()
    {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }
    match rx.await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)? {
        Ok(order) => Ok(Json(serde_json::json!({
            "order_id": order.order_id.0.to_string(),
            "status": format!("{:?}", order.status),
            "filled_qty": order.filled_qty.to_string(),
            "quantity": order.quantity.to_string(),
        }))),
        Err(_) => Err(StatusCode::CONFLICT),
    }
}
