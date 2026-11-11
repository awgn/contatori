# Contatori

High-performance sharded atomic counters for Rust.

`contatori` provides thread-safe, ultra-fast counters and statistical trackers optimized for highly concurrent workloads. It implements a **sharded counter pattern** that eliminates cache-line contention compared to traditional single atomic counters.

## The Problem

In multi-threaded applications, updating a single shared atomic variable from many cores causes severe **cache-line bouncing**: every `fetch_add` forces exclusive ownership of that cache line across CPU cores, turning a simple increment into a scalability bottleneck.

## The Solution: Sharded Counters

`contatori` distributes each counter across multiple cache-line-padded shards (`64` by default, configurable via const generics `SHARDS`):

1. **Per-Thread Sharding**: Each thread caches a monotonically assigned ID in a `const`-initialized `thread_local!` `Cell` and indexes its shard via `id & (SHARDS - 1)`.
2. **Cache-Line Padding**: Every shard is wrapped in [`crossbeam_utils::CachePadded`] (128-byte aligned on `x86_64` and `aarch64`) to prevent **false sharing**.
3. **Relaxed Atomic Ordering**: Hot-path updates use `Ordering::Relaxed` (single atomic instruction, zero locks, zero syscalls).
4. **Aggregation on Read**: Global values are computed only when observed, trading slightly more work on cold reads for massive throughput on hot writes.

## Performance Benchmark

### Single Counter: Sharded vs `AtomicU64`

Benchmarked with 8 persistent worker threads (`8 threads × 100,000 increments` per batch):

```text
┌─────────────────────────────────────────────────────────────────────────────┐
│                    Counter Performance Comparison                           │
│                   (8 threads × 100,000 increments)                          │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  AtomicU64 (single)     ████████████████████████████████████████  30.74 ms  │
│                         (26.03 Melem/s)                                     │
│                                                                             │
│  Unsigned (sharded)     ▏                                         0.224 ms  │
│                         (3.58 Gelem/s)                                      │
│                                                                             │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  Speedup: ~137x faster                                                      │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Contatori vs OpenTelemetry Counters

Benchmarked with 8 persistent worker threads (`8 threads × 100,000 increments` per batch):

```text
┌─────────────────────────────────────────────────────────────────────────────┐
│              Counter Performance: Contatori vs OpenTelemetry                │
│                        (8 threads × 100,000 increments)                     │
├─────────────────────────────────────────────────────────────────────────────┤
│  Simple counter (no labels):                                                │
│  OpenTelemetry Counter  ████████████████████████████████████████   30.33 ms │
│  contatori Monotone     ▏                                         0.227 ms  │
│  Speedup: ~134x faster  (3.53 Gelem/s vs 26.38 Melem/s)                     │
├─────────────────────────────────────────────────────────────────────────────┤
│  Labeled counters (rotating GET/POST/PUT/DELETE):                           │
│  OpenTelemetry Counter  ████████████████████████████████████████  280.64 ms │
│  cont. labeled_group!   ▏                                         0.166 ms  │
│  Speedup: ~1,691x faster (4.82 Gelem/s vs 2.85 Melem/s)                     │
├─────────────────────────────────────────────────────────────────────────────┤
│  High contention (all threads same label):                                  │
│  OpenTelemetry Counter  ████████████████████████████████████████  281.48 ms │
│  cont. labeled_group!   ▏                                         0.226 ms  │
│  Speedup: ~1,245x faster (3.54 Gelem/s vs 2.84 Melem/s)                     │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Available Counter Types

| Type | Description | `Observable::value()` | `MetricKind` |
|------|-------------|-----------------------|--------------|
| [`Monotone`] | Monotonically increasing `u64` counter (never resets) | `CounterValue::Unsigned` | `Counter` |
| [`Unsigned`] | Unsigned `u64` counter (supports `add`/`sub` & reset) | `CounterValue::Unsigned` | `Gauge` |
| [`Signed`] | Signed `i64` counter (gauges, balances, queue depth) | `CounterValue::Signed` | `Gauge` |
| [`Minimum`] | Tracks minimum observed `u64` (`None` until first observation) | `CounterValue::Unsigned` | `Gauge` |
| [`Maximum`] | Tracks maximum observed `u64` (`None` until first observation) | `CounterValue::Unsigned` | `Gauge` |
| [`Average`] | Tracks running floating-point mean (`None` until first observation) | `CounterValue::Float` | `Gauge` |
| [`Rate`] | Smooth rate of change (units/sec) via continuous-time EWMA | `CounterValue::Float` | `Gauge` |

## Quick Start

```toml
[dependencies]
contatori = "0.8"
```

### Basic Usage

```rust
use contatori::{Monotone, Observable, Unsigned};

static TOTAL_REQUESTS: Monotone = Monotone::new().with_name("http_requests_total");

TOTAL_REQUESTS.incr();
TOTAL_REQUESTS.add(5);

assert_eq!(TOTAL_REQUESTS.value().as_u64(), 6);

// Adjustable gauge-style counter
let inflight = Unsigned::new().with_name("inflight_requests");
inflight.add(10);
inflight.sub(3);
assert_eq!(inflight.value().as_u64(), 7);
```

### Customizing Shard Count (Memory vs Contention)

By default each counter uses `64` shards (~8 KB on 64-bit platforms with 128-byte `CachePadded` slots). You can tune the shard count to any power of two via const generics:

```rust
use contatori::{Observable, Unsigned};

// Compact 16-shard counter (~2 KB)
static COMPACT: Unsigned<16> = Unsigned::with_shards().with_name("compact_requests");

// High-core-count 128-shard counter (~16 KB)
let wide = Unsigned::<128>::with_shards();

COMPACT.incr();
wide.add(41);
assert_eq!(COMPACT.value().as_u64() + wide.value().as_u64(), 42);
```

### Statistical Trackers (`Minimum`, `Maximum`, `Average`)

Statistical trackers distinguish "no observations yet" (`None` via `.get()` / `.try_value()`, omitted automatically by observers) from an observed `0`:

```rust
use contatori::{Average, Maximum, Minimum, Observable};

let min_latency = Minimum::new().with_name("latency_min_us");
let max_latency = Maximum::new().with_name("latency_max_us");
let avg_latency = Average::new().with_name("latency_avg_us");

assert_eq!(min_latency.get(), None);
assert_eq!(min_latency.try_value(), None);

for latency in [150, 85, 200, 120, 95] {
    min_latency.observe(latency);
    max_latency.observe(latency);
    avg_latency.observe(latency);
}

assert_eq!(min_latency.get(), Some(85));
assert_eq!(max_latency.get(), Some(200));
assert_eq!(avg_latency.average_f64(), Some(130.0));
```

### Labeled Counter Groups (`labeled_group!`)

`labeled_group!` generates a struct of compile-time labeled counters with direct field access and zero runtime hashing. Both single-label and multi-label dimensions are supported:

```rust
use contatori::{Monotone, Observable, labeled_group};

// Single label key
labeled_group!(
    HttpByMethod,
    "http_requests_total",
    "method",
    get: "GET": Monotone,
    post: "POST": Monotone,
);

// Multiple label keys
labeled_group!(
    HttpByMethodStatus,
    "http_responses_total",
    ["method", "status"],
    get_200: ["GET", "200"]: Monotone,
    get_404: ["GET", "404"]: Monotone,
    post_201: ["POST", "201"]: Monotone,
);

static REQ: HttpByMethod = HttpByMethod::new();
static RESP: HttpByMethodStatus = HttpByMethodStatus::new();

REQ.get.add(10);
REQ.post.add(5);
assert_eq!(REQ.total().as_u64(), 15);

RESP.get_200.add(9);
RESP.get_404.incr();
assert_eq!(RESP.expand()[0].labels, &[("method", "GET"), ("status", "200")]);
```

### Resettable Counters & Groups

Wrap any counter or `labeled_group!` in [`Resettable`] to atomically read-and-reset shards on each observation period:

```rust
use contatori::{Observable, Resettable, Unsigned, labeled_group};

let per_period = Resettable::new(Unsigned::new().with_name("requests_per_period"));
per_period.add(100);

assert_eq!(per_period.value().as_u64(), 100);
assert_eq!(per_period.value().as_u64(), 0); // Reset after read!

labeled_group!(
    WindowErrors,
    "window_errors",
    "kind",
    timeout: "timeout": Unsigned,
    refused: "refused": Unsigned,
);

let errors = Resettable::new(WindowErrors::new());
errors.timeout.add(3);
assert_eq!(errors.value().as_u64(), 3);
assert_eq!(errors.value().as_u64(), 0);
```

### Rate Counter (Multi-Observer EWMA)

[`Rate`] computes units/second using a continuous-time Exponentially Weighted Moving Average (default 5s window, configurable via `with_window`). Reads are idempotent across concurrent observers:

```rust
use contatori::{Observable, Rate};
use std::time::Duration;

static RPS: Rate = Rate::new()
    .with_name("requests_per_sec")
    .with_window(Duration::from_secs(5));

let _ = RPS.rate(); // Establish baseline
RPS.add(500);
assert_eq!(RPS.total_value(), 500);
```

## Serialization & Observers

Each exporter is gated behind an optional Cargo feature:

| Feature | Module | Description |
|---------|--------|-------------|
| `serde` | `snapshot` | Serializable `CounterSnapshot` and `MetricsSnapshot` types |
| `table` | `observers::table` | Pretty ASCII / Unicode tables (`TableObserver`) |
| `json` | `observers::json` | JSON serialization (`JsonObserver`, implies `serde`) |
| `prometheus` | `observers::prometheus` | Native `prometheus::core::Collector` & exposition renderer |
| `opentelemetry` | `observers::opentelemetry` | OpenTelemetry observable instrument callbacks |
| `full` | All modules | Enables `table`, `json`, `prometheus`, and `opentelemetry` |

### Prometheus (`PrometheusObserver` & `Collector`)

Register `'static` counters once into a `prometheus::Registry` (recommended for `/metrics` endpoints), or render ad-hoc counters on the fly:

```rust ignore
use contatori::{Monotone, Observable, Signed};
use contatori::observers::prometheus::PrometheusObserver;

static REQUESTS: Monotone = Monotone::new().with_name("http_requests_total");
static CONNECTIONS: Signed = Signed::new().with_name("active_connections");

let observer = PrometheusObserver::new()
    .with_namespace("myapp")
    .with_help("http_requests_total", "Total HTTP requests");

// Register once via prometheus::core::Collector:
observer.register(&[&REQUESTS, &CONNECTIONS])?;

REQUESTS.add(1000);
CONNECTIONS.add(42);

let exposition = observer.render_registered()?;
```

### JSON (`JsonObserver`)

```rust ignore
use contatori::{Observable, Unsigned};
use contatori::observers::json::JsonObserver;

let requests = Unsigned::new().with_name("http_requests");
requests.add(1000);

let counters: Vec<&dyn Observable> = vec![&requests];
let json = JsonObserver::new()
    .pretty(true)
    .wrap_in_snapshot(true)
    .include_timestamp(true)
    .render(counters.into_iter())?;
```

### Table (`TableObserver`)

```rust ignore
use contatori::{Observable, Unsigned};
use contatori::observers::table::{TableObserver, TableStyle};

let requests = Unsigned::new().with_name("requests");
requests.add(1000);

let counters: Vec<&dyn Observable> = vec![&requests];
println!("{}", TableObserver::new().with_style(TableStyle::Rounded).render(counters.into_iter()));
```

### OpenTelemetry (`OtelObserver`)

```rust ignore
use contatori::{Monotone, Observable, Unsigned};
use contatori::observers::opentelemetry::OtelObserver;

static HTTP_REQUESTS: Monotone = Monotone::new().with_name("http_requests_total");
static ACTIVE_CONNECTIONS: Unsigned = Unsigned::new().with_name("active_connections");

let observer = OtelObserver::new("my_service");
observer.register(&[&HTTP_REQUESTS, &ACTIVE_CONNECTIONS])?;
```

## Benchmarks & Tests

```bash
cargo test --all-features
cargo bench
```

## License

MIT
