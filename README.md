# Contatori

High-performance sharded atomic counters for Rust.

A library providing thread-safe, high-performance counters optimized for highly concurrent workloads. This library implements a **sharded counter pattern** that dramatically reduces contention compared to traditional single atomic counters.

## The Problem

In multi-threaded applications, a naive approach to counting uses a single atomic variable shared across all threads. While this is correct, it creates a severe performance bottleneck: every increment operation causes **cache line bouncing** between CPU cores, as each core must acquire exclusive access to the cache line containing the counter.

This contention grows worse with more threads and higher update frequencies, turning what should be a simple operation into a major scalability bottleneck.

## The Solution: Sharded Counters

This library solves the contention problem by **sharding** counters across multiple slots (64 by default). Each thread is assigned to a specific slot, so threads updating the counter typically operate on different memory locations, eliminating contention.

### Design Principles

1. **Per-Thread Sharding**: Each thread gets assigned a slot index via `thread_local!`, ensuring that concurrent updates from different threads don't compete for the same cache line.

2. **Cache Line Padding**: Each slot is wrapped in `CachePadded`, which adds padding to ensure each atomic value occupies its own cache line (typically 64 bytes). This prevents **false sharing** where unrelated data on the same cache line causes unnecessary invalidations.

3. **Relaxed Ordering**: All atomic operations use `Ordering::Relaxed` since counters don't need to establish happens-before relationships with other memory operations. This allows maximum optimization by the CPU.

4. **Aggregation on Read**: The global counter value is computed by summing all slots. This makes reads slightly more expensive but keeps writes extremely fast, which is the right trade-off for counters (many writes, few reads).

## Performance Benchmark

### Single Counter: Sharded vs AtomicUsize

Benchmarked on **Apple M2** (8 cores) with **8 threads**, each performing **1,000,000 increments** (8 million total operations):

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    Counter Performance Comparison                           │
│                   (8 threads × 1,000,000 iterations)                        │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  AtomicUsize (single)   ████████████████████████████████████████  162.53 ms │
│                                                                             │
│  Unsigned (sharded)     █                                           2.27 ms │
│                                                                             │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  Speedup: 71.6x faster                                                      │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

The sharded counter is **~72x faster** than a naive atomic counter under high contention. This difference grows with more threads and higher contention.

### Contatori vs OpenTelemetry

Benchmarked on **Apple M2** (8 cores) with **8 threads**, each performing **100,000 increments**:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│              Counter Performance: Contatori vs OpenTelemetry                │
│                        (8 threads × 100,000 iterations)                     │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  Simple counter (no labels):                                                │
│                                                                             │
│  OpenTelemetry Counter  ████████████████████████████████████████   25.81 ms │
│                                                                             │
│  contatori Monotone     █                                          0.33 ms  │
│                                                                             │
│  Speedup: 79x faster                                                        │
│                                                                             │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  Labeled counters (rotating GET/POST/PUT/DELETE):                           │
│                                                                             │
│  OpenTelemetry Counter  ████████████████████████████████████████  356.46 ms │
│                                                                             │
│  cont. labeled_group!   ▏                                          0.21 ms  │
│                                                                             │
│  Speedup: 1665x faster                                                      │
│                                                                             │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  High contention (all threads same label):                                  │
│                                                                             │
│  OpenTelemetry Counter  ████████████████████████████████████████  350.45 ms │
│                                                                             │
│  cont. labeled_group!   ▏                                          0.32 ms  │
│                                                                             │
│  Speedup: 1093x faster                                                      │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

Contatori is **79x to ~1600x faster** than OpenTelemetry counters depending on usage pattern. This massive difference comes from:
- **Zero runtime overhead**: Labels are resolved at compile time
- **Sharded storage**: Each sub-counter uses the same sharding strategy
- **No dynamic dispatch**: Direct field access instead of hash lookups

## Available Counter Types

| Type | Description | Use Case | `MetricKind` |
|------|-------------|----------|--------------|
| `Monotone` | Monotonically increasing counter (never resets) | Prometheus counters, total requests | `Counter` |
| `Unsigned` | Unsigned integer counter | Event counts, request totals | `Gauge` |
| `Signed` | Signed integer counter | Gauges, balance tracking | `Gauge` |
| `Minimum` | Tracks minimum observed value | Latency minimums | `Gauge` |
| `Maximum` | Tracks maximum observed value | Latency maximums, peak values | `Gauge` |
| `Average` | Computes running average | Average latency, mean values | `Gauge` |
| `Rate` | Calculates rate of change (units/second) | Request rates, throughput | `Gauge` |

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
contatori = "0.7"
```

### Basic Usage

```rust
use contatori::counters::unsigned::Unsigned;
use contatori::counters::Observable;

// Create a counter (can be shared across threads via Arc)
let counter = Unsigned::new().with_name("requests");

// Increment from any thread - extremely fast!
counter.add(1);
counter.add(5);

// Read the total value (aggregates all shards)
println!("Total requests: {}", counter.value());
// value() does NOT reset the counter - it just reads
println!("Still: {}", counter.value()); // Still 6
```

### Resettable Counters

To reset a counter when reading (useful for per-period metrics), wrap it with `Resettable`:

```rust
use contatori::counters::unsigned::Unsigned;
use contatori::counters::Observable;
use contatori::adapters::Resettable;

// Create a resettable counter for per-period metrics
let requests_per_second = Resettable::new(Unsigned::new().with_name("requests_per_second"));

requests_per_second.add(100);

// value() returns the value AND resets the counter
let count = requests_per_second.value();
println!("Requests this period: {}", count); // 100
println!("After reset: {}", requests_per_second.value()); // 0
```

### Multi-threaded Usage

```rust
use contatori::counters::unsigned::Unsigned;
use contatori::counters::Observable;
use std::sync::Arc;
use std::thread;

let counter = Arc::new(Unsigned::new());
let mut handles = vec![];

for _ in 0..8 {
    let c = Arc::clone(&counter);
    handles.push(thread::spawn(move || {
        for _ in 0..1_000_000 {
            c.add(1);
        }
    }));
}

for h in handles {
    h.join().unwrap();
}

assert_eq!(counter.value(), contatori::counters::CounterValue::Unsigned(8_000_000));
```

### Tracking Statistics

```rust
use contatori::counters::minimum::Minimum;
use contatori::counters::maximum::Maximum;
use contatori::counters::average::Average;
use contatori::counters::Observable;

let min_latency = Minimum::new().with_name("latency_min");
let max_latency = Maximum::new().with_name("latency_max");
let avg_latency = Average::new().with_name("latency_avg");

// Record some latencies
for latency in [150, 85, 200, 120, 95] {
    min_latency.observe(latency);
    max_latency.observe(latency);
    avg_latency.observe(latency);
}

println!("Min: {}", min_latency.value());  // 85
println!("Max: {}", max_latency.value());  // 200
println!("Avg: {}", avg_latency.value());  // 130
```

## Thread Safety

All counter types are `Send + Sync` and can be safely shared across threads using `Arc<Counter>`. The sharding ensures that concurrent updates are efficient.

## Memory Usage

Each counter uses approximately **4KB of memory** (64 slots × 64 bytes per cache line). This is a trade-off: more memory for dramatically better performance under contention.

## Serialization & Observers

The library provides optional modules for serializing and exporting counter values in various formats. Each module is gated behind a feature flag:

| Feature | Module | Description |
|---------|--------|-------------|
| `serde` | `snapshot` | Serializable snapshot types (use with any serde format) |
| `table` | `observers::table` | Renders counters as ASCII tables |
| `json` | `observers::json` | Serializes counters to JSON (includes `serde`) |
| `opentelemetry` | `observers::opentelemetry` | Exports counters to OpenTelemetry metrics |
| `prometheus` | `observers::prometheus` | Exports in Prometheus exposition format |
| `full` | All modules | Enables all observer modules |

### Snapshot Module

The `snapshot` module provides serializable types that work with any serde-compatible format (JSON, YAML, TOML, bincode, etc.).

```toml
[dependencies]
contatori = { version = "0.7", features = ["serde"] }
```

```rust
use contatori::counters::unsigned::Unsigned;
use contatori::counters::Observable;
use contatori::snapshot::{CounterSnapshot, MetricsSnapshot};

let requests = Unsigned::new().with_name("requests");
let errors = Unsigned::new().with_name("errors");

requests.add(1000);
errors.add(5);

let counters: Vec<&dyn Observable> = vec![&requests, &errors];

// Collect snapshots
let snapshot = MetricsSnapshot::collect(counters.into_iter());

// Serialize with any serde-compatible format
let json = serde_json::to_string(&snapshot).unwrap();
let yaml = serde_yaml::to_string(&snapshot).unwrap();
let bytes = bincode::serialize(&snapshot).unwrap();
```

### TableObserver

Renders counters as formatted ASCII tables using the `tabled` crate.

```toml
[dependencies]
contatori = { version = "0.7", features = ["table"] }
```

```rust
use contatori::counters::unsigned::Unsigned;
use contatori::counters::Observable;
use contatori::observers::table::{TableObserver, TableStyle};

let requests = Unsigned::new().with_name("requests");
let errors = Unsigned::new().with_name("errors");

requests.add(1000);
errors.add(5);

let counters: Vec<&dyn Observable> = vec![&requests, &errors];

// Standard format (vertical list)
let observer = TableObserver::new().with_style(TableStyle::Rounded);
println!("{}", observer.render(counters.into_iter()));
// ╭──────────┬───────╮
// │ Name     │ Value │
// ├──────────┼───────┤
// │ requests │ 1000  │
// │ errors   │ 5     │
// ╰──────────┴───────╯

// Compact format (multiple columns)
let observer = TableObserver::new()
    .compact(true)
    .columns(3);
println!("{}", observer.render(counters.into_iter()));
// ╭────────────────┬───────────┬──────────────╮
// │ requests: 1000 │ errors: 5 │ latency: 120 │
// ╰────────────────┴───────────┴──────────────╯
```

**Available styles:** `Ascii`, `Rounded`, `Sharp`, `Modern`, `Extended`, `Markdown`, `ReStructuredText`, `Dots`, `Blank`, `Double`

**Compact separators:** `Colon` (`:`), `Equals` (`=`), `Arrow` (`→`), `Pipe` (`|`), `Space`

#### TableObserver Configuration

| Method | Description |
|--------|-------------|
| `with_style(TableStyle)` | Sets the table border style |
| `with_header(bool)` | Shows or hides the header row |
| `with_title(String)` | Adds a title above the table |
| `compact(bool)` | Enables compact horizontal layout |
| `columns(usize)` | Number of columns in compact mode |
| `separator(CompactSeparator)` | Separator between name and value in compact mode |
| `render(iter)` | Renders the counters to a string |

**Note:** To reset counters when rendering, wrap them with `Resettable`.

### JsonObserver

Serializes counters to JSON format using serde.

```toml
[dependencies]
contatori = { version = "0.7", features = ["json"] }
```

```rust
use contatori::counters::unsigned::Unsigned;
use contatori::counters::Observable;
use contatori::observers::json::JsonObserver;

let requests = Unsigned::new().with_name("http_requests");
let errors = Unsigned::new().with_name("http_errors");

requests.add(1000);
errors.add(5);

let counters: Vec<&dyn Observable> = vec![&requests, &errors];

// Simple array output
let json = JsonObserver::new()
    .to_json(counters.into_iter())
    .unwrap();

// Pretty-printed output with timestamp wrapper
let json = JsonObserver::new()
    .pretty(true)
    .wrap_in_snapshot(true)
    .include_timestamp(true)
    .to_json(counters.into_iter())
    .unwrap();
```

#### JsonObserver Configuration

| Method | Description |
|--------|-------------|
| `pretty(bool)` | Enables pretty-printed JSON output |
| `wrap_in_snapshot(bool)` | Wraps output in a `MetricsSnapshot` object |
| `include_timestamp(bool)` | Includes timestamp in the snapshot (requires `wrap_in_snapshot`) |
| `to_json(iter)` | Serializes counters to a JSON string |
| `collect(iter)` | Returns a `Vec<CounterSnapshot>` for custom processing |

**Note:** To reset counters when serializing, wrap them with `Resettable`.

### PrometheusObserver

Exports counters in Prometheus exposition format using the official `prometheus` crate.

```toml
[dependencies]
contatori = { version = "0.7", features = ["prometheus"] }
```

#### Automatic Metric Type Detection

The observer automatically determines the correct Prometheus metric type based on the counter's `metric_kind()` method:

| Counter Type | `MetricKind` | Prometheus Type |
|--------------|--------------|-----------------|
| `Monotone` | `Counter` | Counter |
| `Unsigned` | `Gauge` | Gauge |
| `Signed` | `Gauge` | Gauge |
| `Minimum` | `Gauge` | Gauge |
| `Maximum` | `Gauge` | Gauge |
| `Average` | `Gauge` | Gauge |

This means you don't need to manually specify types for most use cases:

```rust
use contatori::counters::monotone::Monotone;
use contatori::counters::signed::Signed;
use contatori::counters::{Observable, MetricKind};
use contatori::observers::prometheus::PrometheusObserver;

// Monotone returns MetricKind::Counter, auto-detected as Prometheus Counter
let requests = Monotone::new().with_name("http_requests_total");
assert_eq!(requests.metric_kind(), MetricKind::Counter);
requests.add(1000);

// Signed returns MetricKind::Gauge, auto-detected as Prometheus Gauge
let connections = Signed::new().with_name("active_connections");
assert_eq!(connections.metric_kind(), MetricKind::Gauge);
connections.add(42);

let counters: Vec<&dyn Observable> = vec![&requests, &connections];

let observer = PrometheusObserver::new()
    .with_namespace("myapp")
    .with_help("http_requests_total", "Total number of HTTP requests")
    .with_help("active_connections", "Current number of active connections");

let output = observer.render(counters.into_iter()).unwrap();
// Output will have:
// # TYPE myapp_http_requests_total counter
// # TYPE myapp_active_connections gauge
```

#### Manual Type Override

You can override the auto-detected type using `with_type()`:

```rust
use contatori::counters::unsigned::Unsigned;
use contatori::counters::Observable;
use contatori::observers::prometheus::{PrometheusObserver, MetricType};

let requests = Unsigned::new().with_name("http_requests_total");
requests.add(1000);

let counters: Vec<&dyn Observable> = vec![&requests];

// Force Unsigned to be exported as Counter instead of Gauge
let observer = PrometheusObserver::new()
    .with_type("http_requests_total", MetricType::Counter);

let output = observer.render(counters.into_iter()).unwrap();
```

#### PrometheusObserver Configuration

| Method | Description |
|--------|-------------|
| `with_namespace(str)` | Sets a prefix for all metric names (e.g., `myapp_`) |
| `with_subsystem(str)` | Sets a subsystem between namespace and metric name |
| `with_const_label(name, value)` | Adds a constant label to all metrics |
| `with_type(name, MetricType)` | Overrides auto-detected metric type (`Counter` or `Gauge`) |
| `with_help(name, text)` | Sets the help text for a specific metric |
| `render(iter)` | Renders counters to Prometheus exposition format |

**Note:** To reset counters when rendering, wrap them with `Resettable`.

#### Metric Types

| Prometheus Type | Description | Auto-detected from `MetricKind` |
|-----------------|-------------|--------------------------------|
| `MetricType::Counter` | Cumulative metric that only goes up | `MetricKind::Counter` (`Monotone`) |
| `MetricType::Gauge` | Value that can go up and down | `MetricKind::Gauge` (all other counters) |

### OpenTelemetryObserver

Exports counters to OpenTelemetry using observable instruments (callbacks). When OpenTelemetry collects metrics, it calls the registered callbacks which read values directly from contatori counters.

```toml
[dependencies]
contatori = { version = "0.7", features = ["opentelemetry"] }
opentelemetry = "0.27"
opentelemetry_sdk = { version = "0.27", features = ["rt-tokio"] }
opentelemetry-stdout = { version = "0.27", features = ["metrics"] }
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

```rust
use contatori::counters::monotone::Monotone;
use contatori::counters::unsigned::Unsigned;
use contatori::counters::Observable;
use contatori::observers::opentelemetry::OtelObserver;

use opentelemetry_sdk::metrics::{PeriodicReader, SdkMeterProvider};
use opentelemetry_sdk::runtime;
use std::time::Duration;

// Define static counters
static HTTP_REQUESTS: Monotone = Monotone::new().with_name("http_requests_total");
static ACTIVE_CONNECTIONS: Unsigned = Unsigned::new().with_name("active_connections");

#[tokio::main]
async fn main() {
    // Setup OpenTelemetry with stdout exporter
    let exporter = opentelemetry_stdout::MetricExporter::default();
    let reader = PeriodicReader::builder(exporter, runtime::Tokio)
        .with_interval(Duration::from_secs(60))
        .build();
    let provider = SdkMeterProvider::builder().with_reader(reader).build();
    opentelemetry::global::set_meter_provider(provider.clone());

    // Register contatori metrics with OpenTelemetry
    let observer = OtelObserver::new("my_service");
    let counters: &[&'static (dyn Observable + Send + Sync)] = &[
        &HTTP_REQUESTS,
        &ACTIVE_CONNECTIONS,
    ];
    observer.register(counters).unwrap();

    // Update counters
    HTTP_REQUESTS.add(100);
    ACTIVE_CONNECTIONS.add(5);

    // Flush metrics (they will be printed to stdout)
    provider.force_flush().unwrap();
    provider.shutdown().unwrap();
}
```

#### Automatic Metric Type Detection

| Counter Type | `MetricKind` | OpenTelemetry Type |
|--------------|--------------|-------------------|
| `Monotone` | `Counter` | ObservableCounter (u64) |
| `Unsigned` | `Gauge` | ObservableGauge (f64) |
| `Signed` | `Gauge` | ObservableGauge (f64) |
| `Minimum` | `Gauge` | ObservableGauge (f64) |
| `Maximum` | `Gauge` | ObservableGauge (f64) |
| `Average` | `Gauge` | ObservableGauge (f64) |

#### OtelObserver Configuration

| Method | Description |
|--------|-------------|
| `new(scope_name)` | Creates observer with the given instrumentation scope name |
| `with_description_prefix(str)` | Adds a prefix to metric descriptions |
| `register(&[...])` | Registers static counters with OpenTelemetry |

#### Labeled Groups Support

Labeled groups are automatically exported with OpenTelemetry attributes:

```rust
use contatori::labeled_group;
use contatori::counters::unsigned::Unsigned;

labeled_group!(
    HttpByMethod,
    "http_requests_by_method",
    "method",
    get: "GET": Unsigned,
    post: "POST": Unsigned,
);

static HTTP_METHODS: HttpByMethod = HttpByMethod::new();

// Each counter becomes a data point with the "method" attribute
HTTP_METHODS.get.add(100);  // method="GET"
HTTP_METHODS.post.add(50);  // method="POST"
```

**Note:** Counters must be `'static` and implement `Send + Sync` to be registered with OpenTelemetry, as the callbacks are invoked asynchronously.

## Adapters 

The library provides adapter types that add additional behavior to counters while maintaining compatibility with the `Observable` trait.

| Wrapper/Macro | Description |
|---------------|-------------|
| `Resettable` | Resets counter when `value()` is called - for periodic metrics |
| `labeled_group!` | Creates a struct of counters with shared metric name and different labels |

### Resettable

Wraps a counter to reset it when `value()` is called. Useful for evaluating metrics over observation periods (e.g., requests per second, errors per minute).

```rust
use contatori::counters::unsigned::Unsigned;
use contatori::counters::Observable;
use contatori::adapters::Resettable;

let requests = Resettable::new(Unsigned::new().with_name("requests_per_period"));
requests.add(100);

// value() returns the value AND resets the counter
assert_eq!(requests.value().as_u64(), 100);
assert_eq!(requests.value().as_u64(), 0); // Reset to 0!

requests.add(50);
assert_eq!(requests.value().as_u64(), 50); // Just this period
```

Regular counters (without `Resettable`) keep their value across reads:

```rust
use contatori::counters::unsigned::Unsigned;
use contatori::counters::Observable;

let total = Unsigned::new().with_name("total_requests");
total.add(100);

// value() just reads, does NOT reset
assert_eq!(total.value().as_u64(), 100);
assert_eq!(total.value().as_u64(), 100); // Still 100!
```

### Rate Counter

The `Rate` counter calculates the rate of change (units per second) over time. It's useful for tracking throughput, request rates, or any metric where you need to know "how fast" something is happening.

```rust
use contatori::counters::rate::Rate;
use contatori::counters::Observable;
use std::thread;
use std::time::Duration;

// Can be used as a static
static REQUESTS: Rate = Rate::new().with_name("requests_per_sec");

// Increment like a normal counter
REQUESTS.add(1);
REQUESTS.add(5);

// Get the absolute count
println!("Total: {}", REQUESTS.total_value()); // 6

// Get the rate (units per second)
// First call returns 0.0 and establishes baseline
let rate1 = REQUESTS.rate(); // 0.0

// Add more and wait
REQUESTS.add(1000);
thread::sleep(Duration::from_secs(1));

// Now rate() returns actual rate
let rate2 = REQUESTS.rate(); // ~1000.0 per second
```

The `Rate` counter:
- Uses sharded storage like all other counters (high performance)
- Can be initialized in `const` context (`static RATE: Rate = Rate::new()`)
- Returns `MetricKind::Gauge` (rates can go up or down)
- Exports as float values in Prometheus

### Labeled Group

The `labeled_group!` macro creates a struct containing multiple counters that share a metric name but have different label values. This is the recommended way to track metrics with labels (e.g., HTTP requests by method).

```rust
use contatori::labeled_group;
use contatori::counters::unsigned::Unsigned;
use contatori::counters::Observable;

// Define a labeled group
labeled_group!(
    HttpRequests,
    "http_requests",    // metric name
    "method",           // label key
    total: Unsigned,              // no label (aggregate)
    get: "GET": Unsigned,         // method="GET"
    post: "POST": Unsigned,       // method="POST"
    put: "PUT": Unsigned,         // method="PUT"
    delete: "DELETE": Unsigned,   // method="DELETE"
);

// Can be used as a static
static HTTP: HttpRequests = HttpRequests::new();

// Direct field access for incrementing
HTTP.total.add(1);
HTTP.get.add(1);

// Observers automatically expand the group:
// http_requests 1           (no label - the total)
// http_requests{method="GET"} 1
// http_requests{method="POST"} 0
// http_requests{method="PUT"} 0
// http_requests{method="DELETE"} 0
```

The `expand()` method on `Observable` returns all sub-counters with their labels, which observers use automatically.

## When to Use Sharded Counters

Sharded counters are ideal when:
- Multiple threads frequently update the same counter
- Write performance is more important than read performance
- You're tracking metrics, statistics, or telemetry data

For single-threaded scenarios or rarely-updated counters, a simple `AtomicUsize` may be more appropriate due to lower memory overhead.

## Running Benchmarks

```bash
cargo bench
```

## Running Tests

```bash
cargo test
```

## License

MIT

## Architecture Diagram

```
                         ┌─────────────────────────────────────┐
                         │         Counter Structure           │
                         ├─────────────────────────────────────┤
  Thread 0 ──writes──►   │ [Slot 0] ████████ (CachePadded)     │
  Thread 1 ──writes──►   │ [Slot 1] ████████ (CachePadded)     │
  Thread 2 ──writes──►   │ [Slot 2] ████████ (CachePadded)     │
       ...               │    ...                              │
  Thread 63 ─writes──►   │ [Slot 63] ███████ (CachePadded)     │
                         └─────────────────────────────────────┘
                                         │
                                         ▼
                                  value() aggregates
                                  all slots on read
```
