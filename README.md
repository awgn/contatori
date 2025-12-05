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

## Available Counter Types

| Type | Description | Use Case | `MetricKind` |
|------|-------------|----------|--------------|
| `Monotone` | Monotonically increasing counter (never resets) | Prometheus counters, total requests | `Counter` |
| `Unsigned` | Unsigned integer counter | Event counts, request totals | `Gauge` |
| `Signed` | Signed integer counter | Gauges, balance tracking | `Gauge` |
| `Minimum` | Tracks minimum observed value | Latency minimums | `Gauge` |
| `Maximum` | Tracks maximum observed value | Latency maximums, peak values | `Gauge` |
| `Average` | Computes running average | Average latency, mean values | `Gauge` |

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
contatori = "0.5"
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

// Read and reset atomically
let total = counter.value_and_reset();
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
| `prometheus` | `observers::prometheus` | Exports in Prometheus exposition format |
| `full` | All modules | Enables all observer modules |

### Snapshot Module

The `snapshot` module provides serializable types that work with any serde-compatible format (JSON, YAML, TOML, bincode, etc.).

```toml
[dependencies]
contatori = { version = "0.5", features = ["serde"] }
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
contatori = { version = "0.5", features = ["table"] }
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
| `render_and_reset(iter)` | Renders and atomically resets all counters |

### JsonObserver

Serializes counters to JSON format using serde.

```toml
[dependencies]
contatori = { version = "0.5", features = ["json"] }
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
| `to_json_and_reset(iter)` | Serializes and atomically resets all counters |
| `collect(iter)` | Returns a `Vec<CounterSnapshot>` for custom processing |

### PrometheusObserver

Exports counters in Prometheus exposition format using the official `prometheus` crate.

```toml
[dependencies]
contatori = { version = "0.5", features = ["prometheus"] }
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
| `render_and_reset(iter)` | Renders and atomically resets all counters |

#### Metric Types

| Prometheus Type | Description | Auto-detected from `MetricKind` |
|-----------------|-------------|--------------------------------|
| `MetricType::Counter` | Cumulative metric that only goes up | `MetricKind::Counter` (`Monotone`) |
| `MetricType::Gauge` | Value that can go up and down | `MetricKind::Gauge` (all other counters) |

## Adapters 

The library provides adapters types that add additional behavior to counters while maintaining compatibility with the `Observable` trait.

| Wrapper | Description |
|---------|-------------|
| `NonResettable` | Prevents reset on `value_and_reset()` - for monotonic counters |
| `Labeled` | Adds key-value labels/tags to a counter |

### NonResettable

Wraps a counter to prevent it from being reset when `value_and_reset()` is called. Useful for monotonic counters like Prometheus counters that should never decrease.

```rust
use contatori::counters::unsigned::Unsigned;
use contatori::counters::Observable;
use contatori::adapters::NonResettable;

let total = NonResettable::new(Unsigned::new().with_name("total_requests"));
total.add(100);

// value_and_reset() returns value but does NOT reset
assert_eq!(total.value_and_reset().as_u64(), 100);
assert_eq!(total.value().as_u64(), 100); // Still 100!

total.add(50);
assert_eq!(total.value().as_u64(), 150); // Keeps accumulating
```

### Labeled

Wraps a counter to add key-value labels (tags/dimensions). Particularly useful for Prometheus-style metrics.

```rust
use contatori::counters::unsigned::Unsigned;
use contatori::counters::Observable;
use contatori::adapters::Labeled;

let requests = Labeled::new(Unsigned::new().with_name("http_requests"))
    .with_label("method", "GET")
    .with_label("path", "/api/users")
    .with_label("status", "200");

requests.add(100);

// Access labels
for (key, value) in requests.labels() {
    println!("{}: {}", key, value);
}

// Check specific label
assert_eq!(requests.get_label("method"), Some("GET"));
```

### Combining Adapters 

Adapters can be combined for more complex behavior:

```rust
use contatori::counters::unsigned::Unsigned;
use contatori::adapters::{NonResettable, Labeled};

// A labeled, non-resettable counter
let counter = NonResettable::new(
    Labeled::new(Unsigned::new().with_name("total_bytes"))
        .with_label("direction", "ingress")
);

counter.add(1024);
```

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
