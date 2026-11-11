//! Benchmark comparing contatori counters with OpenTelemetry counters.
//!
//! This benchmark measures pure concurrent counter increment performance between:
//! 1. Contatori's sharded counters (`Monotone`, `labeled_group!`)
//! 2. OpenTelemetry's `Counter` (with and without attributes)
//!
//! Worker threads are spawned once outside the Criterion timed loop so OS thread
//! creation/teardown overhead does not pollute measurements.
//!
//! Run with:
//! ```bash
//! cargo bench --bench contatori_vs_otel
//! ```

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Barrier};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use contatori::{Monotone, Observable, labeled_group};
use criterion::{
    BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main,
};
use opentelemetry::{KeyValue, global, metrics::Counter};
use opentelemetry_sdk::metrics::{ManualReader, SdkMeterProvider};

const NUM_THREADS: usize = 8;
const OPS_PER_BATCH: u64 = 100_000;

// Define a labeled group for HTTP requests by method
labeled_group!(
    HttpRequests,
    "http_requests_total",
    "method",
    get: "GET": Monotone,
    post: "POST": Monotone,
    put: "PUT": Monotone,
    delete: "DELETE": Monotone,
);

/// Persistent barrier-synchronized worker pool that executes `f(thread_id, total_ops)`
/// across `NUM_THREADS` threads without spawning/joining inside the timed region.
struct BenchPool {
    iters: Arc<AtomicU64>,
    shutdown: Arc<AtomicBool>,
    start_barrier: Arc<Barrier>,
    end_barrier: Arc<Barrier>,
    handles: Vec<JoinHandle<()>>,
}

impl BenchPool {
    fn new<F>(num_threads: usize, f: F) -> Self
    where
        F: Fn(usize, u64) + Send + Sync + 'static,
    {
        let iters = Arc::new(AtomicU64::new(0));
        let shutdown = Arc::new(AtomicBool::new(false));
        let start_barrier = Arc::new(Barrier::new(num_threads + 1));
        let end_barrier = Arc::new(Barrier::new(num_threads + 1));
        let f = Arc::new(f);

        let mut handles = Vec::with_capacity(num_threads);
        for thread_id in 0..num_threads {
            let iters = Arc::clone(&iters);
            let shutdown = Arc::clone(&shutdown);
            let start_barrier = Arc::clone(&start_barrier);
            let end_barrier = Arc::clone(&end_barrier);
            let f = Arc::clone(&f);

            handles.push(thread::spawn(move || {
                loop {
                    start_barrier.wait();
                    if shutdown.load(Ordering::Acquire) {
                        break;
                    }
                    let total_ops = iters.load(Ordering::Relaxed) * OPS_PER_BATCH;
                    f(thread_id, total_ops);
                    end_barrier.wait();
                }
            }));
        }

        Self {
            iters,
            shutdown,
            start_barrier,
            end_barrier,
            handles,
        }
    }

    fn run_batch(&self, batch_iters: u64) -> Duration {
        self.iters.store(batch_iters, Ordering::Relaxed);
        self.start_barrier.wait();
        let start = Instant::now();
        self.end_barrier.wait();
        start.elapsed()
    }
}

impl Drop for BenchPool {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        self.start_barrier.wait();
        for handle in self.handles.drain(..) {
            handle.join().unwrap();
        }
    }
}

/// Sets up an OpenTelemetry meter provider with `ManualReader` (no background export)
/// and returns a `Counter<u64>`.
fn setup_opentelemetry() -> (SdkMeterProvider, Counter<u64>) {
    let reader = ManualReader::builder().build();
    let provider = SdkMeterProvider::builder().with_reader(reader).build();
    global::set_meter_provider(provider.clone());

    let meter = global::meter("benchmark");
    let counter = meter
        .u64_counter("http_requests_total")
        .with_description("Total HTTP requests")
        .build();
    (provider, counter)
}

/// Benchmark comparing labeled counters with uniform label distribution.
/// Each thread rotates through GET/POST/PUT/DELETE methods.
fn bench_labeled_group(c: &mut Criterion) {
    let mut group = c.benchmark_group("labeled_counters");
    group.throughput(Throughput::Elements(NUM_THREADS as u64 * OPS_PER_BATCH));

    group.bench_function(
        BenchmarkId::new(
            "contatori labeled_group!",
            format!("{NUM_THREADS}threads x {OPS_PER_BATCH}ops"),
        ),
        |b| {
            let requests = Arc::new(HttpRequests::new());
            let worker_req = Arc::clone(&requests);
            let pool = BenchPool::new(NUM_THREADS, move |thread_id, total_ops| {
                let offset = thread_id as u64;
                for i in 0..total_ops {
                    match (offset + i) % 4 {
                        0 => worker_req.get.add(black_box(1)),
                        1 => worker_req.post.add(black_box(1)),
                        2 => worker_req.put.add(black_box(1)),
                        _ => worker_req.delete.add(black_box(1)),
                    }
                }
            });

            b.iter_custom(|iters| {
                let elapsed = pool.run_batch(iters);
                black_box(requests.total());
                elapsed
            });
        },
    );

    group.bench_function(
        BenchmarkId::new(
            "OpenTelemetry Counter",
            format!("{NUM_THREADS}threads x {OPS_PER_BATCH}ops"),
        ),
        |b| {
            let (_provider, counter) = setup_opentelemetry();
            let pool = BenchPool::new(NUM_THREADS, move |thread_id, total_ops| {
                let get_attr = [KeyValue::new("method", "GET")];
                let post_attr = [KeyValue::new("method", "POST")];
                let put_attr = [KeyValue::new("method", "PUT")];
                let delete_attr = [KeyValue::new("method", "DELETE")];
                let offset = thread_id as u64;

                for i in 0..total_ops {
                    match (offset + i) % 4 {
                        0 => counter.add(black_box(1), &get_attr),
                        1 => counter.add(black_box(1), &post_attr),
                        2 => counter.add(black_box(1), &put_attr),
                        _ => counter.add(black_box(1), &delete_attr),
                    }
                }
            });

            b.iter_custom(|iters| pool.run_batch(iters));
        },
    );

    group.finish();
}

/// Benchmark with high contention: all threads increment the same label.
fn bench_single_label_high_contention(c: &mut Criterion) {
    let mut group = c.benchmark_group("single_label_high_contention");
    group.throughput(Throughput::Elements(NUM_THREADS as u64 * OPS_PER_BATCH));

    group.bench_function(
        BenchmarkId::new(
            "contatori (same label)",
            format!("{NUM_THREADS}threads x {OPS_PER_BATCH}ops"),
        ),
        |b| {
            let requests = Arc::new(HttpRequests::new());
            let worker_req = Arc::clone(&requests);
            let pool = BenchPool::new(NUM_THREADS, move |_, total_ops| {
                for _ in 0..total_ops {
                    worker_req.get.add(black_box(1));
                }
            });

            b.iter_custom(|iters| {
                let elapsed = pool.run_batch(iters);
                black_box(requests.get.value());
                elapsed
            });
        },
    );

    group.bench_function(
        BenchmarkId::new(
            "OpenTelemetry (same label)",
            format!("{NUM_THREADS}threads x {OPS_PER_BATCH}ops"),
        ),
        |b| {
            let (_provider, counter) = setup_opentelemetry();
            let pool = BenchPool::new(NUM_THREADS, move |_, total_ops| {
                let get_attr = [KeyValue::new("method", "GET")];
                for _ in 0..total_ops {
                    counter.add(black_box(1), &get_attr);
                }
            });

            b.iter_custom(|iters| pool.run_batch(iters));
        },
    );

    group.finish();
}

/// Benchmark comparing simple counters without labels.
fn bench_simple_counter(c: &mut Criterion) {
    let mut group = c.benchmark_group("simple_counter");
    group.throughput(Throughput::Elements(NUM_THREADS as u64 * OPS_PER_BATCH));

    group.bench_function(
        BenchmarkId::new(
            "contatori Monotone",
            format!("{NUM_THREADS}threads x {OPS_PER_BATCH}ops"),
        ),
        |b| {
            let counter = Arc::new(Monotone::new());
            let worker_counter = Arc::clone(&counter);
            let pool = BenchPool::new(NUM_THREADS, move |_, total_ops| {
                for _ in 0..total_ops {
                    worker_counter.add(black_box(1));
                }
            });

            b.iter_custom(|iters| {
                let elapsed = pool.run_batch(iters);
                black_box(counter.value());
                elapsed
            });
        },
    );

    group.bench_function(
        BenchmarkId::new(
            "OpenTelemetry Counter",
            format!("{NUM_THREADS}threads x {OPS_PER_BATCH}ops"),
        ),
        |b| {
            let (_provider, counter) = setup_opentelemetry();
            let pool = BenchPool::new(NUM_THREADS, move |_, total_ops| {
                for _ in 0..total_ops {
                    counter.add(black_box(1), &[]);
                }
            });

            b.iter_custom(|iters| pool.run_batch(iters));
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
