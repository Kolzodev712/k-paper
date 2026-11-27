# Workload and platform facts

## Performance importance

Performance materially important: only certain components

## Workload facts

Deployment CPU architecture: x86_64
Production CPU family: UNKNOWN
Operating system: Linux
Rust/toolchain version constraints: rust-toolchain.toml pin, edition 2021
Typical event/request rate: lab/dev rates (single process)
Peak event/request rate: UNKNOWN
Typical active cardinality: ~5 instruments
Maximum expected cardinality: tens
Read/write balance: single-writer core; REST reads queued through core task
Burst behavior: scenario dumps at interval_ms=0
Latency target: UNKNOWN
Tail-latency target: UNKNOWN
Memory constraint: docker-compose ~256MiB — unbounded order history is a risk
Threading/runtime model: one core-loop task + market engine + WS/REST tasks
Known allocation constraints: avoid scanning all historic orders; VecDeque trade history; shared JSON fan-out preferred
Deployment topology: local docker / binary; ephemeral-port serve for tests

<!-- doctrine-setup:workload -->
