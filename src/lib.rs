//! # Contatori - High-Performance Sharded Atomic Counters
//!
//! A Rust library providing thread-safe, high-performance counters optimized for
//! highly concurrent workloads. This library implements a **sharded counter pattern**
//! that dramatically reduces contention compared to traditional single atomic counters.
//!
//! ## The Problem
//!
//! In multi-threaded applications, a naive approach to counting uses a single atomic
//! variable shared across all threads. While this is correct, it creates a severe
//! performance bottleneck: every increment operation causes **cache line bouncing**
//! between CPU cores, as each core must acquire exclusive access to the cache line
//! containing the counter.
//!
//! This contention grows worse with more threads and higher update frequencies,
//! turning what should be a simple operation into a major scalability bottleneck.
//!
//! ## The Solution: Sharded Counters
//!
//! This library solves the contention problem by **sharding** counters across multiple
//! slots (64 by default). Each thread is assigned to a specific slot, so threads
//! updating the counter typically operate on different memory locations, eliminating
//! contention.
//!
//! ### Design Principles
//!
//! 1. **Per-Thread Sharding**: Each thread gets assigned a slot index via `thread_local!`,
//!    ensuring that concurrent updates from different threads don't compete for the
//!    same cache line.
//!
//! 2. **Cache Line Padding**: Each slot is wrapped in [`crossbeam_utils::CachePadded`],
//!    which adds padding to ensure each atomic value occupies its own cache line
//!    (typically 64 bytes). This prevents **false sharing** where unrelated data
//!    on the same cache line causes unnecessary invalidations.
//!
//! 3. **Relaxed Ordering**: All atomic operations use `Ordering::Relaxed` since
//!    counters don't need to establish happens-before relationships with other
//!    memory operations. This allows maximum optimization by the CPU.
//!
//! 4. **Aggregation on Read**: The global counter value is computed by summing all
//!    slots. This makes reads slightly more expensive but keeps writes extremely fast,
//!    which is the right trade-off for counters (many writes, few reads).
//!
//! ## Performance Benchmark
//!
//! Benchmarked on **Apple M2** (8 cores) with **8 threads**, each performing
//! **1,000,000 increments** (8 million total operations):
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────────┐
//! │                    Counter Performance Comparison                           │
//! │                   (8 threads × 1,000,000 iterations)                        │
//! ├─────────────────────────────────────────────────────────────────────────────┤
//! │                                                                             │
//! │  AtomicUsize (single)   ████████████████████████████████████████  162.53 ms │
//! │                                                                             │
//! │  Unsigned (sharded)     █                                           2.27 ms │
//! │                                                                             │
//! ├─────────────────────────────────────────────────────────────────────────────┤
//! │                                                                             │
//! │  Speedup: 71.6x faster                                                      │
//! │                                                                             │
//! └─────────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! The sharded counter is **~72x faster** than a naive atomic counter under high
//! contention. This difference grows with more threads and higher contention.
//!
//! ## Available Counter Types
//!
//! | Type | Description | Use Case |
//! |------|-------------|----------|
//! | [`Unsigned`](counters::unsigned::Unsigned) | Unsigned integer counter | Event counts, request totals |
//! | [`Signed`](counters::signed::Signed) | Signed integer counter | Gauges, balance tracking |
//! | [`Minimum`](counters::minimum::Minimum) | Tracks minimum observed value | Latency minimums |
//! | [`Maximum`](counters::maximum::Maximum) | Tracks maximum observed value | Latency maximums, peak values |
//! | [`Average`](counters::average::Average) | Computes running average | Average latency, mean values |
//!
//! ## Quick Start
//!
//! ```rust
//! use contatori::counters::unsigned::Unsigned;
//! use contatori::counters::Observable;
//!
//! // Create a counter (can be shared across threads via Arc)
//! let counter = Unsigned::new().with_name("requests");
//!
//! // Increment from any thread - extremely fast!
//! counter.add(1);
//! counter.add(5);
//!
//! // Read the total value (aggregates all shards)
//! println!("Total requests: {}", counter.value());
//!
//! // Read and reset atomically
//! let total = counter.value_and_reset();
//! ```
//!
//! ## Thread Safety
//!
//! All counter types are `Send + Sync` and can be safely shared across threads
//! using `Arc<Counter>`. The sharding ensures that concurrent updates are efficient.
//!
//! ## Memory Usage
//!
//! Each counter uses approximately **4KB of memory** (64 slots × 64 bytes per cache line).
//! This is a trade-off: more memory for dramatically better performance under contention.
//!
//! ## When to Use
//!
//! Use these counters when:
//! - Multiple threads frequently update the same counter
//! - Write performance is more important than read performance
//! - You're tracking metrics, statistics, or telemetry data
//!
//! For single-threaded scenarios or rarely-updated counters, a simple `AtomicUsize`
//! may be more appropriate due to lower memory overhead.
//!
//! ## Observers
//!
//! The library provides optional observer modules for exporting counter values
//! in various formats. Each observer is gated behind a feature flag:
//!
//! | Feature | Module | Description |
//! |---------|--------|-------------|
//! | `table` | [`observers::table`] | Pretty-print counters as ASCII tables |
//! | `json` | [`observers::json`] | Serialize counters to JSON |
//! | `prometheus` | [`observers::prometheus`] | Export in Prometheus exposition format |
//! | `full` | All observers | Enables all observer modules |
//!
//! ### Example: Table Output
//!
//! ```toml
//! [dependencies]
//! contatori = { version = "0.3", features = ["table"] }
//! ```
//!
//! ```rust,ignore
//! use contatori::counters::unsigned::Unsigned;
//! use contatori::counters::Observable;
//! use contatori::observers::table::TableObserver;
//!
//! let requests = Unsigned::new().with_name("http_requests");
//! requests.add(1000);
//!
//! let counters: Vec<&dyn Observable> = vec![&requests];
//! println!("{}", TableObserver::new().render(counters.into_iter()));
//! ```
//!
//! ### Example: JSON Output
//!
//! ```toml
//! [dependencies]
//! contatori = { version = "0.3", features = ["serde_json"] }
//! ```
//!
//! ```rust,ignore
//! use contatori::observers::json::JsonObserver;
//!
//! let json = JsonObserver::new()
//!     .pretty(true)
//!     .to_json(counters.into_iter())?;
//! ```
//!
//! ### Example: Prometheus Output
//!
//! ```toml
//! [dependencies]
//! contatori = { version = "0.3", features = ["prometheus"] }
//! ```
//!
//! ```rust,ignore
//! use contatori::observers::prometheus::PrometheusObserver;
//!
//! let output = PrometheusObserver::new()
//!     .with_prefix("myapp")
//!     .with_global_label("instance", "server-1")
//!     .render(counters.into_iter());
//! ```

pub mod counters;
pub mod observers;
pub mod adapters;

#[cfg(feature = "serde")]
pub mod snapshot;
