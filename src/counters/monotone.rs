//! Monotone 64-bit unsigned counter with sharded atomic storage.
//!
//! [`Monotone`] only supports non-decreasing operations (`add`, `incr`) and reports
//! [`MetricKind::Counter`], making it ideal for cumulative counters in Prometheus
//! and OpenTelemetry.

use crossbeam_utils::CachePadded;
use std::fmt::Debug;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::counters::{
    CounterValue, DEFAULT_SHARDS, MetricKind, Observable, sealed, thread_slot_index,
};

/// A high-performance monotonically increasing 64-bit counter.
///
/// Unlike [`Unsigned`](super::unsigned::Unsigned), `Monotone` only exposes `add` and `incr`
/// (no subtraction or reset), guaranteeing monotonicity and reporting [`MetricKind::Counter`].
///
/// # Examples
///
/// ```rust
/// use contatori::{MetricKind, Monotone, Observable};
///
/// let requests = Monotone::new().with_name("http_requests_total");
/// requests.incr();
/// requests.add(4);
///
/// assert_eq!(requests.metric_kind(), MetricKind::Counter);
/// assert_eq!(requests.value().as_u64(), 5);
/// ```
pub struct Monotone<const SHARDS: usize = DEFAULT_SHARDS> {
    name: &'static str,
    components: [CachePadded<AtomicU64>; SHARDS],
}

impl Monotone<DEFAULT_SHARDS> {
    /// Creates a new counter with the default number of shards (`64`), initialized to zero.
    #[must_use]
    pub const fn new() -> Self {
        Self::with_shards()
    }
}

impl<const SHARDS: usize> Monotone<SHARDS> {
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

    /// Adds `value` to the counter.
    #[inline]
    pub fn add(&self, value: u64) {
        self.local_slot().fetch_add(value, Ordering::Relaxed);
    }

    /// Returns the value of the current thread's shard.
    #[must_use]
    #[inline]
    pub fn local_value(&self) -> u64 {
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
    fn total_value(&self) -> u64 {
        self.components
            .iter()
            .map(|counter| counter.load(Ordering::Relaxed))
            .fold(0u64, u64::wrapping_add)
    }
}

impl<const SHARDS: usize> Observable for Monotone<SHARDS> {
    #[inline]
    fn value(&self) -> CounterValue {
        CounterValue::Unsigned(self.total_value())
    }

    #[inline]
    fn name(&self) -> &'static str {
        self.name
    }

    #[inline]
    fn metric_kind(&self) -> MetricKind {
        MetricKind::Counter
    }
}

impl<const SHARDS: usize> sealed::Resettable for Monotone<SHARDS> {
    /// Returns the total value without resetting (`Monotone` counters never reset).
    #[inline]
    fn value_and_reset(&self) -> CounterValue {
        CounterValue::Unsigned(self.total_value())
    }
}

impl<const SHARDS: usize> Default for Monotone<SHARDS> {
    fn default() -> Self {
        Self::with_shards()
    }
}

impl<const SHARDS: usize> Debug for Monotone<SHARDS> {
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
    fn test_new_and_incr() {
        let counter = Monotone::new().with_name("req");
        assert_eq!(counter.value(), CounterValue::Unsigned(0));
        assert_eq!(counter.metric_kind(), MetricKind::Counter);
        counter.incr();
        counter.add(4);
        assert_eq!(counter.value(), CounterValue::Unsigned(5));
    }

    #[test]
    fn test_custom_shards() {
        let counter = Monotone::<8>::with_shards();
        counter.incr();
        assert_eq!(counter.value(), CounterValue::Unsigned(1));
    }

    #[test]
    fn test_resettable_monotone_does_not_reset() {
        let counter = Resettable::new(Monotone::new());
        counter.add(3);
        assert_eq!(counter.value(), CounterValue::Unsigned(3));
        assert_eq!(counter.value(), CounterValue::Unsigned(3));
    }

    #[test]
    fn test_multiple_threads() {
        let counter = Arc::new(Monotone::new());
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
