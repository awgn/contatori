//! Maximum value tracker with sharded atomic storage.
//!
//! [`Maximum`] records the largest `u64` value observed across all threads.
//! Internally, values are stored offset by `+1` (`0` = unobserved), allowing
//! `observe(0)` to be distinguished accurately from "no observations yet" in a
//! single lock-free `AtomicU64`.

use crossbeam_utils::CachePadded;
use std::fmt::Debug;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::counters::{
    CounterValue, DEFAULT_SHARDS, Observable, sealed, thread_slot_index,
};

const UNOBSERVED: u64 = 0;

/// A high-performance maximum value tracker using sharded atomic storage.
///
/// # Examples
///
/// ```rust
/// use contatori::{Maximum, Observable};
///
/// let max_latency = Maximum::new().with_name("request_latency_max");
/// assert_eq!(max_latency.get(), None);
///
/// max_latency.observe(0);
/// assert_eq!(max_latency.get(), Some(0));
///
/// max_latency.observe(200);
/// max_latency.observe(120);
/// assert_eq!(max_latency.get(), Some(200));
/// ```
pub struct Maximum<const SHARDS: usize = DEFAULT_SHARDS> {
    name: &'static str,
    components: [CachePadded<AtomicU64>; SHARDS],
}

impl Maximum<DEFAULT_SHARDS> {
    /// Creates a new maximum tracker with the default number of shards (`64`).
    #[must_use]
    pub const fn new() -> Self {
        Self::with_shards()
    }
}

impl<const SHARDS: usize> Maximum<SHARDS> {
    /// Creates a new maximum tracker with `SHARDS` shards.
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
            components: [const { CachePadded::new(AtomicU64::new(UNOBSERVED)) }; SHARDS],
        }
    }

    /// Sets the metric name of this tracker, returning `self` for method chaining.
    #[must_use]
    pub const fn with_name(self, name: &'static str) -> Self {
        Self { name, ..self }
    }

    #[inline]
    const fn encode(value: u64) -> u64 {
        value.saturating_add(1)
    }

    #[inline]
    const fn decode(raw: u64) -> Option<u64> {
        if raw == UNOBSERVED {
            None
        } else {
            Some(raw - 1)
        }
    }

    #[inline]
    fn local_slot(&self) -> &AtomicU64 {
        &self.components[thread_slot_index::<SHARDS>()]
    }

    /// Observes a value and updates the current thread's shard maximum if greater.
    #[inline]
    pub fn observe(&self, value: u64) {
        let encoded = Self::encode(value);
        let counter = self.local_slot();
        let mut current = counter.load(Ordering::Relaxed);
        while encoded > current {
            match counter.compare_exchange_weak(
                current,
                encoded,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => current = actual,
            }
        }
    }

    /// Sets the value of the current thread's shard directly.
    #[inline]
    pub fn set_local_value(&self, value: u64) {
        self.local_slot()
            .store(Self::encode(value), Ordering::Relaxed);
    }

    /// Returns the observed maximum in the current thread's shard, or `None` if unobserved.
    #[must_use]
    #[inline]
    pub fn local_value(&self) -> Option<u64> {
        Self::decode(self.local_slot().load(Ordering::Relaxed))
    }

    /// Returns the global maximum across all shards, or `None` if no values were observed.
    #[must_use]
    #[inline]
    pub fn get(&self) -> Option<u64> {
        let raw = self
            .components
            .iter()
            .map(|counter| counter.load(Ordering::Relaxed))
            .max()
            .unwrap_or(UNOBSERVED);
        Self::decode(raw)
    }

    /// Returns the global maximum across all shards and resets every shard to unobserved.
    #[inline]
    pub fn get_and_reset(&self) -> Option<u64> {
        let mut max = UNOBSERVED;
        for counter in &self.components {
            let val = counter.swap(UNOBSERVED, Ordering::Relaxed);
            if val > max {
                max = val;
            }
        }
        Self::decode(max)
    }
}

impl<const SHARDS: usize> Observable for Maximum<SHARDS> {
    /// Returns the global maximum, or `0` if no values have been observed yet.
    ///
    /// Use [`Maximum::get()`] or [`Observable::try_value()`] to distinguish `0` from unobserved.
    #[inline]
    fn value(&self) -> CounterValue {
        CounterValue::Unsigned(self.get().unwrap_or(0))
    }

    #[inline]
    fn try_value(&self) -> Option<CounterValue> {
        self.get().map(CounterValue::Unsigned)
    }

    #[inline]
    fn name(&self) -> &'static str {
        self.name
    }
}

impl<const SHARDS: usize> sealed::Resettable for Maximum<SHARDS> {
    #[inline]
    fn value_and_reset(&self) -> CounterValue {
        CounterValue::Unsigned(self.get_and_reset().unwrap_or(0))
    }

    #[inline]
    fn try_value_and_reset(&self) -> Option<CounterValue> {
        self.get_and_reset().map(CounterValue::Unsigned)
    }
}

impl<const SHARDS: usize> Default for Maximum<SHARDS> {
    fn default() -> Self {
        Self::with_shards()
    }
}

impl<const SHARDS: usize> Debug for Maximum<SHARDS> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}{{", self.name)?;
        for (i, counter) in self.components.iter().enumerate() {
            if let Some(val) = Self::decode(counter.load(Ordering::Relaxed)) {
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
    fn test_empty_vs_zero_observed() {
        let counter = Maximum::new();
        assert_eq!(counter.get(), None);
        assert_eq!(counter.try_value(), None);
        assert_eq!(counter.value(), CounterValue::Unsigned(0));

        counter.observe(0);
        assert_eq!(counter.get(), Some(0));
        assert_eq!(counter.try_value(), Some(CounterValue::Unsigned(0)));
        assert!(format!("{counter:?}").contains(":0"));
    }

    #[test]
    fn test_observe_mixed() {
        let counter = Maximum::new();
        counter.observe(15);
        counter.observe(42);
        counter.observe(8);
        counter.observe(100);
        counter.observe(3);
        assert_eq!(counter.get(), Some(100));
    }

    #[test]
    fn test_resettable() {
        let counter = Resettable::new(Maximum::new());
        counter.observe(50);
        counter.observe(100);
        assert_eq!(counter.try_value(), Some(CounterValue::Unsigned(100)));
        assert_eq!(counter.try_value(), None);
        counter.observe(25);
        assert_eq!(counter.value(), CounterValue::Unsigned(25));
    }

    #[test]
    fn test_multiple_threads() {
        let counter = Arc::new(Maximum::new());
        let mut handles = vec![];
        for i in 0..4u64 {
            let c = Arc::clone(&counter);
            handles.push(thread::spawn(move || {
                for j in 0..100u64 {
                    c.observe(i * 100 + j);
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(counter.get(), Some(399));
    }
}
