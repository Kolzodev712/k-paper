//! Order lifecycle: place, cancel, amend; open order index.
//!
//! TODO: place_order(OrderRequest) -> Result<OrderId, Error>.
//! TODO: cancel_order(id), amend_order(id, AmendRequest).
//! TODO: Index by order_id and client_order_id; idempotency for client_order_id.
