//! Benchmark comparing `contatori::Unsigned` (sharded `AtomicU64`) against a single `AtomicU64`.
//!
//! Worker threads are spawned once outside the Criterion measurement window and synchronized
//! with barriers so that OS thread creation/teardown overhead is completely excluded.
//!
//! Run with:
//! ```bash
//! cargo bench --bench contatori_vs_atomic
//! ```

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Barrier};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use contatori::{Observable, Unsigned};
use criterion::{
    BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main,
};

const NUM_THREADS: usize = 8;
const OPS_PER_BATCH: u64 = 100_000;

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

fn bench_unsigned_counter(c: &mut Criterion) {
    let mut group = c.benchmark_group("counter_increment");
    group.throughput(Throughput::Elements(NUM_THREADS as u64 * OPS_PER_BATCH));

    group.bench_function(
        BenchmarkId::new(
            "Local<Unsigned> (batch=256)",
            format!("{NUM_THREADS}threads x {OPS_PER_BATCH}ops"),
        ),
        |b| {
            let counter = Arc::new(Unsigned::new());
            let worker_counter = Arc::clone(&counter);
            let pool = BenchPool::new(NUM_THREADS, move |_, total_ops| {
                let mut local = worker_counter.local();
                let mut remaining = total_ops;
                while remaining >= 256 {
                    for _ in 0..256 {
                        local.add(black_box(1));
                    }
                    local.flush();
                    remaining -= 256;
                }
                for _ in 0..remaining {
                    local.add(black_box(1));
                }
                local.flush();
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
            "Unsigned (sharded AtomicU64)",
            format!("{NUM_THREADS}threads x {OPS_PER_BATCH}ops"),
        ),
        |b| {
            let counter = Arc::new(Unsigned::new());
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
            "AtomicU64 (single contended)",
            format!("{NUM_THREADS}threads x {OPS_PER_BATCH}ops"),
        ),
        |b| {
            let counter = Arc::new(AtomicU64::new(0));
            let worker_counter = Arc::clone(&counter);
            let pool = BenchPool::new(NUM_THREADS, move |_, total_ops| {
                for _ in 0..total_ops {
                    worker_counter.fetch_add(black_box(1), Ordering::Relaxed);
                }
            });

            b.iter_custom(|iters| {
                let elapsed = pool.run_batch(iters);
                black_box(counter.load(Ordering::Relaxed));
                elapsed
            });
        },
    );

    group.finish();
}

criterion_group!(benches, bench_unsigned_counter);
criterion_main!(benches);
