//! Maximum value tracker with sharded atomic storage.
//!
//! This module provides [`Maximum`], a high-performance tracker that records
//! the maximum value observed across all threads. It uses sharding to minimize
//! contention during updates.

use std::sync::atomic::{AtomicUsize, Ordering};

use crossbeam_utils::CachePadded;
use std::fmt::Debug;

use crate::counters::{sealed, CounterValue, Observable, NUM_COMPONENTS, THREAD_SLOT_INDEX};

/// A high-performance maximum value tracker using sharded atomic storage.
///
/// `Maximum` tracks the largest value observed across all threads. Each shard
/// is initialized to `usize::MIN` (0) so that the first observed value becomes
/// the maximum. When reading, the global maximum is computed by taking the
/// maximum across all shards.
///
/// # Use Cases
///
/// - Tracking maximum latency (worst case response time)
/// - Recording peak memory usage
/// - Finding maximum queue depth over time
///
/// # Algorithm
///
/// Updates use a compare-and-swap (CAS) loop: the new value is only stored
/// if it's greater than the current shard value. This ensures correctness
/// without locks while allowing concurrent updates.
///
/// # Memory Usage
///
/// Each `Maximum` tracker uses approximately 4KB of memory (64 slots × 64 bytes).
///
/// # Examples
///
/// ```rust
/// use contatori::counters::maximum::Maximum;
/// use contatori::counters::Observable;
///
/// let max_latency = Maximum::new().with_name("request_latency_max");
///
/// // Record some latencies (in microseconds)
/// max_latency.observe(150);
/// max_latency.observe(85);
/// max_latency.observe(200);
///
/// // The maximum is 200
/// assert_eq!(max_latency.value(), contatori::counters::CounterValue::Unsigned(200));
/// ```
pub struct Maximum {
    name: &'static str,
    components: [CachePadded<AtomicUsize>; NUM_COMPONENTS],
}

impl Maximum {
    /// Creates a new maximum tracker.
    ///
    /// All 64 shards are initialized to `usize::MIN` (0), so the first observed
    /// value will become the maximum.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use contatori::counters::maximum::Maximum;
    /// use contatori::counters::Observable;
    ///
    /// let tracker = Maximum::new();
    /// // Before any observations, value is 0
    /// assert_eq!(tracker.value(), contatori::counters::CounterValue::Unsigned(0));
    /// ```
    pub const fn new() -> Self {
        const MIN: CachePadded<AtomicUsize> = CachePadded::new(AtomicUsize::new(usize::MIN));
        Maximum {
            components: [MIN; NUM_COMPONENTS],
            name: "",
        }
    }

    /// Sets the name of this tracker, returning `self` for method chaining.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use contatori::counters::maximum::Maximum;
    /// use contatori::counters::Observable;
    ///
    /// let tracker = Maximum::new().with_name("max_response_time");
    /// assert_eq!(tracker.name(), "max_response_time");
    /// ```
    pub const fn with_name(self, name: &'static str) -> Self {
        Self { name, ..self }
    }

    /// Observes a value and updates the local maximum if necessary.
    ///
    /// This method uses a compare-and-swap loop to atomically update the
    /// shard value only if the new value is greater than the current one.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use contatori::counters::maximum::Maximum;
    /// use contatori::counters::Observable;
    ///
    /// let tracker = Maximum::new();
    /// tracker.observe(100);
    /// tracker.observe(150);  // New maximum
    /// tracker.observe(75);   // Ignored (not greater)
    ///
    /// assert_eq!(tracker.value(), contatori::counters::CounterValue::Unsigned(150));
    /// ```
    #[inline]
    pub fn observe(&self, value: usize) {
        THREAD_SLOT_INDEX.with(|idx| {
            let counter = &self.components[*idx];
            let mut current = counter.load(Ordering::Relaxed);
            while value > current {
                match counter.compare_exchange_weak(
                    current,
                    value,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => break,
                    Err(actual) => current = actual,
                }
            }
        });
    }

    /// Computes the global maximum by finding the largest value across all shards.
    ///
    /// Returns `None` if no values have been observed (all shards are at `usize::MIN`).
    #[inline]
    fn raw_value(&self) -> Option<usize> {
        let max = self
            .components
            .iter()
            .map(|counter| counter.load(Ordering::Relaxed))
            .max()
            .unwrap_or(usize::MIN);

        if max == usize::MIN {
            None
        } else {
            Some(max)
        }
    }

    /// Computes the global maximum and resets all shards to `usize::MIN`.
    ///
    /// This is useful for periodic metric collection where you want to
    /// capture the maximum since the last collection.
    ///
    /// Returns `None` if no values were observed during this period.
    #[inline]
    fn raw_value_and_reset(&self) -> Option<usize> {
        let mut max = usize::MIN;
        for counter in self.components.iter() {
            let val = counter.swap(usize::MIN, Ordering::Relaxed);
            if val > max {
                max = val;
            }
        }

        if max == usize::MIN {
            None
        } else {
            Some(max)
        }
    }
}

impl Observable for Maximum {
    /// Returns the global maximum across all shards.
    ///
    /// If no values have been observed, returns `0`.
    #[inline]
    fn value(&self) -> CounterValue {
        CounterValue::Unsigned(self.raw_value().unwrap_or(0) as u64)
    }

    /// Returns the name of this tracker.
    #[inline]
    fn name(&self) -> &'static str {
        self.name
    }
}

impl sealed::Resettable for Maximum {
    /// Returns the global maximum and resets all shards to `MIN`.
    ///
    /// After reset, the next observed value will become the new maximum.
    /// Returns `0` if no values were observed.
    #[inline]
    fn value_and_reset(&self) -> CounterValue {
        CounterValue::Unsigned(self.raw_value_and_reset().unwrap_or(0) as u64)
    }
}

impl Debug for Maximum {
    /// Formats the tracker showing shards that have observed values.
    ///
    /// Shards still at `usize::MIN` (no observations) are not shown.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}{{", self.name)?;
        for (i, counter) in self.components.iter().enumerate() {
            let val = counter.load(Ordering::Relaxed);
            if val != usize::MIN {
                write!(f, " [{i}]:{val}")?;
            }
        }
        write!(f, " }}")
    }
}

impl Default for Maximum {
    /// Creates a new tracker with all shards set to `MIN`.
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let counter = Maximum::new();
        assert_eq!(counter.value(), CounterValue::Unsigned(0));
    }

    #[test]
    fn test_observe_single() {
        let counter = Maximum::new();
        counter.observe(42);
        assert_eq!(counter.value(), CounterValue::Unsigned(42));
    }

    #[test]
    fn test_observe_increasing() {
        let counter = Maximum::new();
        counter.observe(10);
        counter.observe(20);
        counter.observe(30);
        assert_eq!(counter.value(), CounterValue::Unsigned(30));
    }

    #[test]
    fn test_observe_decreasing() {
        let counter = Maximum::new();
        counter.observe(30);
        counter.observe(20);
        counter.observe(10);
        assert_eq!(counter.value(), CounterValue::Unsigned(30));
    }

    #[test]
    fn test_observe_mixed() {
        let counter = Maximum::new();
        counter.observe(15);
        counter.observe(42);
        counter.observe(8);
        counter.observe(100);
        counter.observe(3);
        assert_eq!(counter.value(), CounterValue::Unsigned(100));
    }

    #[test]
    fn test_resettable() {
        use crate::adapters::Resettable;
        let counter = Resettable::new(Maximum::new());
        counter.observe(50);
        counter.observe(100);
        assert_eq!(counter.value(), CounterValue::Unsigned(100));
        // After value() the counter should be reset
        assert_eq!(counter.value(), CounterValue::Unsigned(0));
    }

    #[test]
    fn test_resettable_then_observe() {
        use crate::adapters::Resettable;
        let counter = Resettable::new(Maximum::new());
        counter.observe(100);
        let _ = counter.value(); // reset
        counter.observe(25);
        assert_eq!(counter.value(), CounterValue::Unsigned(25));
    }

    #[test]
    fn test_debug() {
        let counter = Maximum::new();
        counter.observe(42);
        let debug_str = format!("{:?}", counter);
        assert!(debug_str.starts_with("{"));
        assert!(debug_str.contains("42"));
        assert!(debug_str.ends_with("}"));
    }

    #[test]
    fn test_dyn_format() {
        let counter = Maximum::new().with_name("max_counter");
        counter.observe(99);
        let formatted = format!("{}", &counter as &dyn Observable);
        assert_eq!(formatted, "max_counter:99");
    }

    #[test]
    fn test_dyn_debug() {
        let counter = Maximum::new().with_name("max_counter");
        counter.observe(77);
        let debug_str = format!("{:?}", &counter as &dyn Observable);
        assert!(debug_str.starts_with("max_counter{"));
        assert!(debug_str.contains("77"));
        assert!(debug_str.ends_with("}"));
    }

    #[test]
    fn test_multiple_threads() {
        use std::sync::Arc;
        use std::thread;

        let counter = Arc::new(Maximum::new());
        let mut handles = vec![];

        for i in 0..4 {
            let counter_clone = Arc::clone(&counter);
            let handle = thread::spawn(move || {
                for j in 0..100 {
                    counter_clone.observe(i * 100 + j);
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }

        // Thread 3 observes values 300-399, so max should be 399
        assert_eq!(counter.value(), CounterValue::Unsigned(399));
    }

    #[test]
    fn test_name_default() {
        let counter = Maximum::new();
        assert_eq!(counter.name(), "");
    }

    #[test]
    fn test_with_name() {
        let counter = Maximum::new().with_name("my_max");
        assert_eq!(counter.name(), "my_max");
    }

    #[test]
    fn test_default() {
        let counter = Maximum::default();
        assert_eq!(counter.value(), CounterValue::Unsigned(0));
        assert_eq!(counter.name(), "");
    }

    #[test]
    fn test_observe_zero() {
        let counter = Maximum::new();
        counter.observe(0);
        // 0 is a valid observation, should be returned
        assert_eq!(counter.value(), CounterValue::Unsigned(0));
    }
}
