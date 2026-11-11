//! Signed 64-bit integer counter with sharded atomic storage.
//!
//! [`Signed`] supports both positive and negative updates across threads,
//! making it ideal for gauges such as active connections, queue depth, or balance tracking.

use crossbeam_utils::CachePadded;
use std::fmt::Debug;
use std::sync::atomic::{AtomicI64, Ordering};

use crate::counters::{
    CounterValue, DEFAULT_SHARDS, Observable, sealed, thread_slot_index,
};

/// A high-performance signed 64-bit integer counter using sharded atomic storage.
///
/// # Examples
///
/// ```rust
/// use contatori::{Observable, Signed};
///
/// let connections = Signed::new().with_name("active_connections");
/// connections.incr();
/// connections.incr();
/// connections.decr();
/// assert_eq!(connections.value().as_i64(), 1);
/// ```
pub struct Signed<const SHARDS: usize = DEFAULT_SHARDS> {
    name: &'static str,
    components: [CachePadded<AtomicI64>; SHARDS],
}

impl Signed<DEFAULT_SHARDS> {
    /// Creates a new counter with the default number of shards (`64`), initialized to zero.
    #[must_use]
    pub const fn new() -> Self {
        Self::with_shards()
    }
}

impl<const SHARDS: usize> Signed<SHARDS> {
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
            components: [const { CachePadded::new(AtomicI64::new(0)) }; SHARDS],
        }
    }

    /// Sets the metric name of this counter, returning `self` for method chaining.
    #[must_use]
    pub const fn with_name(self, name: &'static str) -> Self {
        Self { name, ..self }
    }

    #[inline]
    fn local_slot(&self) -> &AtomicI64 {
        &self.components[thread_slot_index::<SHARDS>()]
    }

    /// Increments the counter by `1`.
    #[inline]
    pub fn incr(&self) {
        self.add(1);
    }

    /// Decrements the counter by `1`.
    #[inline]
    pub fn decr(&self) {
        self.sub(1);
    }

    /// Adds `value` (positive or negative) to the counter.
    #[inline]
    pub fn add(&self, value: i64) {
        self.local_slot().fetch_add(value, Ordering::Relaxed);
    }

    /// Subtracts `value` from the counter.
    #[inline]
    pub fn sub(&self, value: i64) {
        self.local_slot().fetch_sub(value, Ordering::Relaxed);
    }

    /// Sets the value of the current thread's shard directly.
    #[inline]
    pub fn set_local_value(&self, value: i64) {
        self.local_slot().store(value, Ordering::Relaxed);
    }

    /// Returns the value of the current thread's shard.
    #[must_use]
    #[inline]
    pub fn local_value(&self) -> i64 {
        self.local_slot().load(Ordering::Relaxed)
    }

    /// Creates a local batch accumulator for this counter.
    ///
    /// Updates on the returned [`Local`](crate::adapters::Local) proxy are buffered
    /// in non-atomic memory and flushed to this counter on [`Local::flush()`](crate::adapters::Local::flush)
    /// or when dropped.
    #[must_use]
    #[inline]
    pub const fn local(&self) -> crate::adapters::Local<'_, Self> {
        crate::adapters::Local::__from_parts(self, 0)
    }

    #[inline]
    fn total_value(&self) -> i64 {
        self.components
            .iter()
            .map(|counter| counter.load(Ordering::Relaxed))
            .fold(0i64, i64::wrapping_add)
    }

    #[inline]
    fn total_value_and_reset(&self) -> i64 {
        self.components
            .iter()
            .map(|counter| counter.swap(0, Ordering::Relaxed))
            .fold(0i64, i64::wrapping_add)
    }
}

impl<const SHARDS: usize> Observable for Signed<SHARDS> {
    #[inline]
    fn value(&self) -> CounterValue {
        CounterValue::Signed(self.total_value())
    }

    #[inline]
    fn name(&self) -> &'static str {
        self.name
    }
}

impl<const SHARDS: usize> sealed::Resettable for Signed<SHARDS> {
    #[inline]
    fn value_and_reset(&self) -> CounterValue {
        CounterValue::Signed(self.total_value_and_reset())
    }
}

impl<const SHARDS: usize> Default for Signed<SHARDS> {
    fn default() -> Self {
        Self::with_shards()
    }
}

impl<const SHARDS: usize> Debug for Signed<SHARDS> {
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
    fn test_signed_operations() {
        let counter = Signed::new().with_name("balance");
        assert_eq!(counter.value(), CounterValue::Signed(0));
        counter.add(10);
        counter.sub(15);
        counter.incr();
        counter.decr();
        assert_eq!(counter.value(), CounterValue::Signed(-5));
    }

    #[test]
    fn test_resettable_signed() {
        let counter = Resettable::new(Signed::new());
        counter.add(5);
        counter.sub(8);
        assert_eq!(counter.value(), CounterValue::Signed(-3));
        assert_eq!(counter.value(), CounterValue::Signed(0));
    }

    #[test]
    fn test_multiple_threads() {
        let counter = Arc::new(Signed::new());
        let mut handles = vec![];
        for i in 0..4 {
            let c = Arc::clone(&counter);
            handles.push(thread::spawn(move || {
                for _ in 0..100 {
                    if i % 2 == 0 {
                        c.incr();
                    } else {
                        c.decr();
                    }
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(counter.value(), CounterValue::Signed(0));
    }
}
