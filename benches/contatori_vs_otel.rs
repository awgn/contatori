//! Benchmark comparing contatori counters with OpenTelemetry counters.
//!
//! This benchmark measures the performance difference between:
//! 1. Contatori's sharded counters (Monotone, labeled_group!)
//! 2. OpenTelemetry's Counter (with and without attributes)
//!
//! Run with:
//! ```bash
//! cargo bench --bench labeled_group_benchmark
//! ```

use std::sync::Arc;
use std::thread;

use contatori::counters::monotone::Monotone;
use contatori::counters::Observable;
use contatori::labeled_group;
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use opentelemetry::{global, metrics::Counter, KeyValue};
use opentelemetry_sdk::metrics::{ManualReader, SdkMeterProvider};

const NUM_THREADS: usize = 8;
const ITERATIONS_PER_THREAD: usize = 100_000;

// Define a labeled group for HTTP requests by method
labeled_group!(
    HttpRequests,
    "http_requests_total",
    "method",
    value: Monotone,
    get: "GET": Monotone,
    post: "POST": Monotone,
    put: "PUT": Monotone,
    delete: "DELETE": Monotone,
);

/// Sets up the OpenTelemetry meter provider and returns a Counter.
/// Uses ManualReader to avoid any export overhead during benchmarking.
fn setup_opentelemetry() -> Counter<u64> {
    let reader = ManualReader::builder().build();
    let provider = SdkMeterProvider::builder().with_reader(reader).build();
    global::set_meter_provider(provider);

    let meter = global::meter("benchmark");
    meter
        .u64_counter("http_requests_total")
        .with_description("Total HTTP requests")
        .build()
}

/// Benchmark comparing labeled counters with uniform label distribution.
/// Each thread rotates through GET/POST/PUT/DELETE methods.
fn bench_labeled_group(c: &mut Criterion) {
    let mut group = c.benchmark_group("labeled_counters");

    // Benchmark contatori labeled_group! macro
    group.bench_function(
        BenchmarkId::new(
            "contatori labeled_group!",
            format!("{}threads x {}iter", NUM_THREADS, ITERATIONS_PER_THREAD),
        ),
        |b| {
            b.iter(|| {
                let requests = Arc::new(HttpRequests::new());
                let mut handles = vec![];

                for thread_id in 0..NUM_THREADS {
                    let req = Arc::clone(&requests);
                    let handle = thread::spawn(move || {
                        for i in 0..ITERATIONS_PER_THREAD {
                            // Rotate through methods to simulate realistic workload
                            match (thread_id + i) % 4 {
                                0 => req.get.add(1),
                                1 => req.post.add(1),
                                2 => req.put.add(1),
                                _ => req.delete.add(1),
                            }
                        }
                    });
                    handles.push(handle);
                }

                for handle in handles {
                    handle.join().unwrap();
                }

                // Read all counter values to ensure fair comparison
                black_box(requests.get.value());
                black_box(requests.post.value());
                black_box(requests.put.value());
                black_box(requests.delete.value());
            })
        },
    );

    // Benchmark OpenTelemetry counter with attributes
    group.bench_function(
        BenchmarkId::new(
            "OpenTelemetry Counter",
            format!("{}threads x {}iter", NUM_THREADS, ITERATIONS_PER_THREAD),
        ),
        |b| {
            let counter = setup_opentelemetry();

            b.iter(|| {
                let counter = counter.clone();
                let mut handles = vec![];

                for thread_id in 0..NUM_THREADS {
                    let counter = counter.clone();
                    let handle = thread::spawn(move || {
                        // Pre-create KeyValue attributes (recommended OTel usage pattern)
                        let get_attr = [KeyValue::new("method", "GET")];
                        let post_attr = [KeyValue::new("method", "POST")];
                        let put_attr = [KeyValue::new("method", "PUT")];
                        let delete_attr = [KeyValue::new("method", "DELETE")];

                        for i in 0..ITERATIONS_PER_THREAD {
                            // Rotate through methods to simulate realistic workload
                            match (thread_id + i) % 4 {
                                0 => counter.add(1, &get_attr),
                                1 => counter.add(1, &post_attr),
                                2 => counter.add(1, &put_attr),
                                _ => counter.add(1, &delete_attr),
                            }
                        }
                    });
                    handles.push(handle);
                }

                for handle in handles {
                    handle.join().unwrap();
                }

                black_box(())
            })
        },
    );

    group.finish();
}

/// Benchmark with high contention: all threads increment the same label.
/// This is the worst-case scenario for cache-line contention.
fn bench_single_label_high_contention(c: &mut Criterion) {
    let mut group = c.benchmark_group("single_label_high_contention");

    // Benchmark contatori - all threads hitting the same counter (high contention)
    group.bench_function(
        BenchmarkId::new(
            "contatori (same label)",
            format!("{}threads x {}iter", NUM_THREADS, ITERATIONS_PER_THREAD),
        ),
        |b| {
            b.iter(|| {
                let requests = Arc::new(HttpRequests::new());
                let mut handles = vec![];

                for _ in 0..NUM_THREADS {
                    let req = Arc::clone(&requests);
                    let handle = thread::spawn(move || {
                        for _ in 0..ITERATIONS_PER_THREAD {
                            // All threads increment the same counter (GET) - high contention
                            req.get.add(1);
                        }
                    });
                    handles.push(handle);
                }

                for handle in handles {
                    handle.join().unwrap();
                }

                black_box(requests.get.value())
            })
        },
    );

    // Benchmark OpenTelemetry - all threads incrementing the same label (high contention)
    group.bench_function(
        BenchmarkId::new(
            "OpenTelemetry (same label)",
            format!("{}threads x {}iter", NUM_THREADS, ITERATIONS_PER_THREAD),
        ),
        |b| {
            let counter = setup_opentelemetry();

            b.iter(|| {
                let counter = counter.clone();
                let mut handles = vec![];

                for _ in 0..NUM_THREADS {
                    let counter = counter.clone();
                    let handle = thread::spawn(move || {
                        let get_attr = [KeyValue::new("method", "GET")];

                        for _ in 0..ITERATIONS_PER_THREAD {
                            // All threads increment the same label - high contention
                            counter.add(1, &get_attr);
                        }
                    });
                    handles.push(handle);
                }

                for handle in handles {
                    handle.join().unwrap();
                }

                black_box(())
            })
        },
    );

    group.finish();
}

/// Benchmark comparing simple counters without labels.
/// Direct comparison between contatori::Monotone and OpenTelemetry Counter.
fn bench_simple_counter(c: &mut Criterion) {
    let mut group = c.benchmark_group("simple_counter");

    // Benchmark contatori Monotone counter (no labels)
    group.bench_function(
        BenchmarkId::new(
            "contatori Monotone",
            format!("{}threads x {}iter", NUM_THREADS, ITERATIONS_PER_THREAD),
        ),
        |b| {
            b.iter(|| {
                let counter = Arc::new(Monotone::new());
                let mut handles = vec![];

                for _ in 0..NUM_THREADS {
                    let c = Arc::clone(&counter);
                    let handle = thread::spawn(move || {
                        for _ in 0..ITERATIONS_PER_THREAD {
                            c.add(1);
                        }
                    });
                    handles.push(handle);
                }

                for handle in handles {
                    handle.join().unwrap();
                }

                black_box(counter.value())
            })
        },
    );

    // Benchmark OpenTelemetry counter without attributes
    group.bench_function(
        BenchmarkId::new(
            "OpenTelemetry Counter",
            format!("{}threads x {}iter", NUM_THREADS, ITERATIONS_PER_THREAD),
        ),
        |b| {
            let counter = setup_opentelemetry();

            b.iter(|| {
                let counter = counter.clone();
                let mut handles = vec![];

                for _ in 0..NUM_THREADS {
                    let counter = counter.clone();
                    let handle = thread::spawn(move || {
                        // No attributes - simplest possible usage
                        for _ in 0..ITERATIONS_PER_THREAD {
                            counter.add(1, &[]);
                        }
                    });
                    handles.push(handle);
                }

                for handle in handles {
                    handle.join().unwrap();
                }

                black_box(())
            })
        },
    );

    group.finish();
}

criterion_group!(
    benches,
    bench_simple_counter,
    bench_labeled_group,
    bench_single_label_high_contention
);
criterion_main!(benches);
