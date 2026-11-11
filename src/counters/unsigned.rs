//! Unsigned 64-bit integer counter with sharded atomic storage.
//!
//! This module provides [`Unsigned`], a high-performance counter optimized for
//! concurrent updates from multiple threads. It uses sharding to minimize
//! contention and cache-line padding to prevent false sharing.

use crossbeam_utils::CachePadded;
use std::fmt::Debug;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::counters::{
    CounterValue, DEFAULT_SHARDS, Observable, sealed, thread_slot_index,
};

/// A high-performance unsigned 64-bit integer counter using sharded atomic storage.
///
/// `Unsigned` distributes updates across `SHARDS` (64 by default) cache-line-padded
/// slots so concurrent threads rarely contend on the same cache line.
///
/// # Memory Usage
///
/// With the default `SHARDS = 64` and 128-byte cache-line alignment, each counter
/// occupies approximately 8 KB of memory. You can customize the shard count with
/// `Unsigned::<16>::with_shards()` (any non-zero power of two).
///
/// # Examples
///
/// ```rust
/// use contatori::{Observable, Unsigned};
///
/// let counter = Unsigned::new().with_name("requests");
/// counter.incr();
/// counter.add(5);
/// assert_eq!(counter.value().as_u64(), 6);
/// ```
pub struct Unsigned<const SHARDS: usize = DEFAULT_SHARDS> {
    name: &'static str,
    components: [CachePadded<AtomicU64>; SHARDS],
}

impl Unsigned<DEFAULT_SHARDS> {
    /// Creates a new counter with the default number of shards (`64`), initialized to zero.
    #[must_use]
    pub const fn new() -> Self {
        Self::with_shards()
    }
}

impl<const SHARDS: usize> Unsigned<SHARDS> {
    /// Creates a new counter with `SHARDS` shards, initialized to zero.
    ///
    /// # Panics
    ///
    /// Fails to compile if `SHARDS` is zero or not a power of two.
    #[must_use]
    pub const fn with_shards() -> Self {
        const {
            assert!(
                SHARDS > 0 && SHARDS.is_power_of_two(),
                "SHARDS must be a non-zero power of two"
            );
        }
        Self {
            name: "",
            components: [const { CachePadded::new(AtomicU64::new(0)) }; SHARDS],
        }
    }

    /// Sets the metric name of this counter, returning `self` for method chaining.
    #[must_use]
    pub const fn with_name(self, name: &'static str) -> Self {
        Self { name, ..self }
    }

    #[inline]
    fn local_slot(&self) -> &AtomicU64 {
        &self.components[thread_slot_index::<SHARDS>()]
    }

    /// Increments the counter by `1`.
    #[inline]
    pub fn incr(&self) {
        self.add(1);
    }

    /// Decrements the counter by `1` (using wrapping arithmetic).
    #[inline]
    pub fn decr(&self) {
        self.sub(1);
    }

    /// Adds `value` to the counter.
    #[inline]
    pub fn add(&self, value: u64) {
        self.local_slot().fetch_add(value, Ordering::Relaxed);
    }

    /// Subtracts `value` from the counter using wrapping arithmetic.
    ///
    /// Even if an individual thread's shard wraps around zero, the global sum
    /// aggregates across all shards using `wrapping_add` to produce the exact net value.
    #[inline]
    pub fn sub(&self, value: u64) {
        self.local_slot().fetch_sub(value, Ordering::Relaxed);
    }

    /// Sets the value of the current thread's shard directly.
    #[inline]
    pub fn set_local_value(&self, value: u64) {
        self.local_slot().store(value, Ordering::Relaxed);
    }

    /// Returns the value of the current thread's shard.
    #[must_use]
    #[inline]
    pub fn local_value(&self) -> u64 {
        self.local_slot().load(Ordering::Relaxed)
    }

    /// Computes the total value across all shards using wrapping addition.
    #[inline]
    fn total_value(&self) -> u64 {
        self.components
            .iter()
            .map(|counter| counter.load(Ordering::Relaxed))
            .fold(0u64, u64::wrapping_add)
    }

    /// Computes the total value and resets all shards to zero.
    #[inline]
    fn total_value_and_reset(&self) -> u64 {
        self.components
            .iter()
            .map(|counter| counter.swap(0, Ordering::Relaxed))
            .fold(0u64, u64::wrapping_add)
    }
}

impl<const SHARDS: usize> Observable for Unsigned<SHARDS> {
    #[inline]
    fn value(&self) -> CounterValue {
        CounterValue::Unsigned(self.total_value())
    }

    #[inline]
    fn name(&self) -> &'static str {
        self.name
    }
}

impl<const SHARDS: usize> sealed::Resettable for Unsigned<SHARDS> {
    #[inline]
    fn value_and_reset(&self) -> CounterValue {
        CounterValue::Unsigned(self.total_value_and_reset())
    }
}

impl<const SHARDS: usize> Default for Unsigned<SHARDS> {
    fn default() -> Self {
        Self::with_shards()
    }
}

impl<const SHARDS: usize> Debug for Unsigned<SHARDS> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}{{", self.name)?;
        for (i, counter) in self.components.iter().enumerate() {
            let val = counter.load(Ordering::Relaxed);
            if val != 0 {
                write!(f, " [{i}]:{val}")?;
            }
        }
        write!(f, " }}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::Resettable;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn test_new_and_incr_decr() {
        let counter = Unsigned::new();
        assert_eq!(counter.value(), CounterValue::Unsigned(0));
        counter.incr();
        counter.add(4);
        counter.decr();
        assert_eq!(counter.value(), CounterValue::Unsigned(4));
    }

    #[test]
    fn test_custom_shards() {
        static SMALL: Unsigned<16> = Unsigned::with_shards().with_name("small");
        SMALL.add(10);
        assert_eq!(SMALL.name(), "small");
        assert_eq!(SMALL.value(), CounterValue::Unsigned(10));
    }

    #[test]
    fn test_cross_thread_add_and_sub_wrapping() {
        // Thread A increments by 100 on its shard, Thread B decrements by 30 on a different shard
        // (causing Thread B's shard to wrap below 0). Aggregation must yield 70 without panicking.
        let counter = Arc::new(Unsigned::new());

        let c1 = Arc::clone(&counter);
        let t1 = thread::spawn(move || {
            c1.add(100);
        });

        let c2 = Arc::clone(&counter);
        let t2 = thread::spawn(move || {
            c2.sub(30);
        });

        t1.join().unwrap();
        t2.join().unwrap();

        assert_eq!(counter.value(), CounterValue::Unsigned(70));
    }

    #[test]
    fn test_cross_thread_resettable_with_sub() {
        let counter = Arc::new(Resettable::new(Unsigned::new()));

        let c1 = Arc::clone(&counter);
        let t1 = thread::spawn(move || c1.add(50));
        let c2 = Arc::clone(&counter);
        let t2 = thread::spawn(move || c2.sub(20));

        t1.join().unwrap();
        t2.join().unwrap();

        assert_eq!(counter.value(), CounterValue::Unsigned(30));
        assert_eq!(counter.value(), CounterValue::Unsigned(0));
    }

    #[test]
    fn test_local_value_and_debug() {
        let counter = Unsigned::new().with_name("my_counter");
        counter.set_local_value(42);
        assert_eq!(counter.local_value(), 42);
        assert_eq!(format!("{}", &counter as &dyn Observable), "my_counter:42");
        let dbg = format!("{counter:?}");
        assert!(dbg.starts_with("my_counter{"));
        assert!(dbg.contains("42"));
    }

    #[test]
    fn test_multiple_threads() {
        let counter = Arc::new(Unsigned::new());
        let mut handles = vec![];

        for _ in 0..4 {
            let c = Arc::clone(&counter);
            handles.push(thread::spawn(move || {
                for _ in 0..100 {
                    c.incr();
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(counter.value(), CounterValue::Unsigned(400));
    }
}
