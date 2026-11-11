//! Average value counter with sharded atomic storage.
//!
//! [`Average`] tracks both `sum` and `count` inside each cache-line-padded shard,
//! returning floating-point averages via [`Observable::value()`] (`CounterValue::Float`)
//! and `None` via [`Observable::try_value()`] before any observations occur.

use crossbeam_utils::CachePadded;
use std::fmt::Debug;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::counters::{
    CounterValue, DEFAULT_SHARDS, Observable, sealed, thread_slot_index,
};

struct SumCount {
    sum: AtomicU64,
    count: AtomicU64,
}

impl SumCount {
    const fn new() -> Self {
        Self {
            sum: AtomicU64::new(0),
            count: AtomicU64::new(0),
        }
    }
}

/// A high-performance running average tracker using sharded atomic storage.
///
/// # Examples
///
/// ```rust
/// use contatori::{Average, CounterValue, Observable};
///
/// let avg = Average::new().with_name("request_latency_avg");
/// assert_eq!(avg.try_value(), None);
///
/// avg.observe(10);
/// avg.observe(15);
///
/// assert_eq!(avg.average_f64(), Some(12.5));
/// assert_eq!(avg.value(), CounterValue::Float(12.5));
/// ```
pub struct Average<const SHARDS: usize = DEFAULT_SHARDS> {
    name: &'static str,
    components: [CachePadded<SumCount>; SHARDS],
}

impl Average<DEFAULT_SHARDS> {
    /// Creates a new average tracker with the default number of shards (`64`).
    #[must_use]
    pub const fn new() -> Self {
        Self::with_shards()
    }
}

impl<const SHARDS: usize> Average<SHARDS> {
    /// Creates a new average tracker with `SHARDS` shards.
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
            components: [const { CachePadded::new(SumCount::new()) }; SHARDS],
        }
    }

    /// Sets the metric name of this counter, returning `self` for method chaining.
    #[must_use]
    pub const fn with_name(self, name: &'static str) -> Self {
        Self { name, ..self }
    }

    #[inline]
    fn local_slot(&self) -> &SumCount {
        &self.components[thread_slot_index::<SHARDS>()]
    }

    /// Observes a single value (`sum += value`, `count += 1`).
    #[inline]
    pub fn observe(&self, value: u64) {
        let slot = self.local_slot();
        slot.sum.fetch_add(value, Ordering::Relaxed);
        slot.count.fetch_add(1, Ordering::Relaxed);
    }

    /// Observes a pre-aggregated batch of values (`sum += sum`, `count += count`).
    #[inline]
    pub fn observe_many(&self, sum: u64, count: u64) {
        let slot = self.local_slot();
        slot.sum.fetch_add(sum, Ordering::Relaxed);
        slot.count.fetch_add(count, Ordering::Relaxed);
    }

    /// Adds `value` to the local sum without modifying the count.
    #[inline]
    pub fn add_sum(&self, value: u64) {
        self.local_slot().sum.fetch_add(value, Ordering::Relaxed);
    }

    /// Adds `value` to the local count without modifying the sum.
    #[inline]
    pub fn add_count(&self, value: u64) {
        self.local_slot().count.fetch_add(value, Ordering::Relaxed);
    }

    /// Increments the local count by `1` without modifying the sum.
    #[inline]
    pub fn incr(&self) {
        self.local_slot().count.fetch_add(1, Ordering::Relaxed);
    }

    /// Decrements the local count by `1` (wrapping) without modifying the sum.
    #[inline]
    pub fn decr(&self) {
        self.local_slot().count.fetch_sub(1, Ordering::Relaxed);
    }

    /// Creates a local batch accumulator for this average tracker.
    ///
    /// Observations on the returned [`Local`](crate::adapters::Local) proxy accumulate
    /// `(sum, count)` in non-atomic memory and flush once on
    /// [`Local::flush()`](crate::adapters::Local::flush) or when dropped.
    #[must_use]
    #[inline]
    pub const fn local(&self) -> crate::adapters::Local<'_, Self> {
        crate::adapters::Local::__from_parts(self, (0, 0))
    }

    /// Returns the total sum across all shards (using wrapping 64-bit addition).
    #[must_use]
    #[inline]
    pub fn sum(&self) -> u64 {
        self.components
            .iter()
            .map(|c| c.sum.load(Ordering::Relaxed))
            .fold(0u64, u64::wrapping_add)
    }

    /// Returns the total count across all shards (using wrapping 64-bit addition).
    #[must_use]
    #[inline]
    pub fn count(&self) -> u64 {
        self.components
            .iter()
            .map(|c| c.count.load(Ordering::Relaxed))
            .fold(0u64, u64::wrapping_add)
    }

    #[inline]
    fn totals_u128(&self) -> (u128, u64) {
        let mut sum: u128 = 0;
        let mut count: u64 = 0;
        for c in &self.components {
            sum += u128::from(c.sum.load(Ordering::Relaxed));
            count = count.wrapping_add(c.count.load(Ordering::Relaxed));
        }
        (sum, count)
    }

    #[inline]
    fn totals_u128_and_reset(&self) -> (u128, u64) {
        let mut sum: u128 = 0;
        let mut count: u64 = 0;
        for c in &self.components {
            sum += u128::from(c.sum.swap(0, Ordering::Relaxed));
            count = count.wrapping_add(c.count.swap(0, Ordering::Relaxed));
        }
        (sum, count)
    }

    /// Computes the truncated integer average, or `None` if `count == 0`.
    #[must_use]
    #[inline]
    pub fn average(&self) -> Option<u64> {
        let (sum, count) = self.totals_u128();
        if count == 0 {
            None
        } else {
            Some((sum / u128::from(count)) as u64)
        }
    }

    /// Computes the floating-point average, or `None` if `count == 0`.
    #[must_use]
    #[inline]
    pub fn average_f64(&self) -> Option<f64> {
        let (sum, count) = self.totals_u128();
        if count == 0 {
            None
        } else {
            Some(sum as f64 / count as f64)
        }
    }

    /// Returns `(sum, count)` and resets all shards to zero.
    #[inline]
    pub fn sum_count_and_reset(&self) -> (u64, u64) {
        let (sum, count) = self.totals_u128_and_reset();
        (sum as u64, count)
    }

    /// Returns the truncated integer average and resets all shards to zero.
    #[inline]
    pub fn average_and_reset(&self) -> Option<u64> {
        let (sum, count) = self.totals_u128_and_reset();
        if count == 0 {
            None
        } else {
            Some((sum / u128::from(count)) as u64)
        }
    }

    /// Returns the floating-point average and resets all shards to zero.
    #[inline]
    pub fn average_f64_and_reset(&self) -> Option<f64> {
        let (sum, count) = self.totals_u128_and_reset();
        if count == 0 {
            None
        } else {
            Some(sum as f64 / count as f64)
        }
    }
}

impl<const SHARDS: usize> Observable for Average<SHARDS> {
    /// Returns the floating-point average (`CounterValue::Float`), or `0.0` if empty.
    #[inline]
    fn value(&self) -> CounterValue {
        CounterValue::Float(self.average_f64().unwrap_or(0.0))
    }

    #[inline]
    fn try_value(&self) -> Option<CounterValue> {
        self.average_f64().map(CounterValue::Float)
    }

    #[inline]
    fn name(&self) -> &'static str {
        self.name
    }
}

impl<const SHARDS: usize> sealed::Resettable for Average<SHARDS> {
    #[inline]
    fn value_and_reset(&self) -> CounterValue {
        CounterValue::Float(self.average_f64_and_reset().unwrap_or(0.0))
    }

    #[inline]
    fn try_value_and_reset(&self) -> Option<CounterValue> {
        self.average_f64_and_reset().map(CounterValue::Float)
    }
}

impl<const SHARDS: usize> Default for Average<SHARDS> {
    fn default() -> Self {
        Self::with_shards()
    }
}

impl<const SHARDS: usize> Debug for Average<SHARDS> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}{{", self.name)?;
        for (i, component) in self.components.iter().enumerate() {
            let sum = component.sum.load(Ordering::Relaxed);
            let count = component.count.load(Ordering::Relaxed);
            if count != 0 || sum != 0 {
                write!(f, " [{i}]:sum={sum},count={count}")?;
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
    fn test_empty_and_float_precision() {
        let counter = Average::new().with_name("avg");
        assert_eq!(counter.average(), None);
        assert_eq!(counter.average_f64(), None);
        assert_eq!(counter.try_value(), None);
        assert_eq!(counter.value(), CounterValue::Float(0.0));

        counter.observe(1);
        counter.observe(2);
        assert_eq!(counter.average(), Some(1));
        assert_eq!(counter.average_f64(), Some(1.5));
        assert_eq!(counter.value(), CounterValue::Float(1.5));
        assert_eq!(counter.try_value(), Some(CounterValue::Float(1.5)));
    }

    #[test]
    fn test_cross_thread_incr_decr_wrapping() {
        let counter = Arc::new(Average::new());
        let c1 = Arc::clone(&counter);
        let t1 = thread::spawn(move || c1.add_count(10));
        let c2 = Arc::clone(&counter);
        let t2 = thread::spawn(move || c2.decr());
        t1.join().unwrap();
        t2.join().unwrap();
        assert_eq!(counter.count(), 9);
    }

    #[test]
    fn test_resettable_float_average() {
        let counter = Resettable::new(Average::new());
        counter.observe(10);
        counter.observe(20);
        assert_eq!(counter.try_value(), Some(CounterValue::Float(15.0)));
        assert_eq!(counter.try_value(), None);

        counter.observe(1);
        counter.observe(2);
        assert_eq!(counter.value(), CounterValue::Float(1.5));
    }

    #[test]
    fn test_multiple_threads() {
        let counter = Arc::new(Average::new());
        let mut handles = vec![];
        for _ in 0..4 {
            let c = Arc::clone(&counter);
            handles.push(thread::spawn(move || {
                for j in 1..=100u64 {
                    c.observe(j);
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(counter.count(), 400);
        assert_eq!(counter.sum(), 20200);
        assert_eq!(counter.average_f64(), Some(50.5));
    }
}
