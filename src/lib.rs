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
//! | [`Unsigned`](contatori::unsigned::Unsigned) | Unsigned integer counter | Event counts, request totals |
//! | [`Signed`](contatori::signed::Signed) | Signed integer counter | Gauges, balance tracking |
//! | [`Minimum`](contatori::minimum::Minimum) | Tracks minimum observed value | Latency minimums |
//! | [`Maximum`](contatori::maximum::Maximum) | Tracks maximum observed value | Latency maximums, peak values |
//! | [`Average`](contatori::average::Average) | Computes running average | Average latency, mean values |
//!
//! ## Quick Start
//!
//! ```rust
//! use contatori::contatori::unsigned::Unsigned;
//! use contatori::contatori::Observable;
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

pub mod contatori;
