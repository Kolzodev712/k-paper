# Invariants

- Fills are atomic: account mutation must succeed before order status becomes Filled/PartiallyFilled; failures reject without half-applied state.
- Open-order iteration and cancel-all ordering are stable (placement sequence), not HashMap iteration order.
- Scenario-driven events preserve file order on a single command channel.
- Lagged private/market streams surface an explicit lag signal (or close); clients must not silently continue without gap detection.
- Unsupported fill-model config is rejected at load time.
- Lab fault HTTP routes are off unless PAPER_LAB_FAULTS is enabled.

<!-- doctrine-setup:invariants -->
