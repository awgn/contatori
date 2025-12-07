use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;

use contatori::counters::unsigned::Unsigned;
use contatori::counters::Observable;
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};

const NUM_THREADS: usize = 8;
const ITERATIONS_PER_THREAD: usize = 1_000_000;

fn bench_unsigned_counter(c: &mut Criterion) {
    let mut group = c.benchmark_group("counter_increment");

    group.bench_function(
        BenchmarkId::new(
            "Unsigned (sharded)",
            format!("{}threads x {}iter", NUM_THREADS, ITERATIONS_PER_THREAD),
        ),
        |b| {
            b.iter(|| {
                let counter = Arc::new(Unsigned::new());
                let mut handles = vec![];

                for _ in 0..NUM_THREADS {
                    let counter_clone = Arc::clone(&counter);
                    let handle = thread::spawn(move || {
                        for _ in 0..ITERATIONS_PER_THREAD {
                            counter_clone.add(1);
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

    group.bench_function(
        BenchmarkId::new(
            "AtomicUsize (single)",
            format!("{}threads x {}iter", NUM_THREADS, ITERATIONS_PER_THREAD),
        ),
        |b| {
            b.iter(|| {
                let counter = Arc::new(AtomicUsize::new(0));
                let mut handles = vec![];

                for _ in 0..NUM_THREADS {
                    let counter_clone = Arc::clone(&counter);
                    let handle = thread::spawn(move || {
                        for _ in 0..ITERATIONS_PER_THREAD {
                            counter_clone.fetch_add(1, Ordering::Relaxed);
                        }
                    });
                    handles.push(handle);
                }

                for handle in handles {
                    handle.join().unwrap();
                }

                black_box(counter.load(Ordering::Relaxed))
            })
        },
    );

    group.finish();
}

criterion_group!(benches, bench_unsigned_counter);
criterion_main!(benches);
