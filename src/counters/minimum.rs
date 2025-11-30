//! Minimum value tracker with sharded atomic storage.
//!
//! This module provides [`Minimum`], a high-performance tracker that records
//! the minimum value observed across all threads. It uses sharding to minimize
//! contention during updates.

use std::sync::atomic::{AtomicUsize, Ordering};

use crossbeam_utils::CachePadded;
use std::fmt::Debug;

use crate::counters::{CounterValue, Observable, NUM_COMPONENTS, THREAD_SLOT_INDEX};

/// A high-performance minimum value tracker using sharded atomic storage.
///
/// `Minimum` tracks the smallest value observed across all threads. Each shard
/// is initialized to `usize::MAX` so that the first observed value becomes the
/// minimum. When reading, the global minimum is computed by taking the minimum
/// across all shards.
///
/// # Use Cases
///
/// - Tracking minimum latency
/// - Recording lowest temperature readings
/// - Finding minimum queue depth over time
///
/// # Algorithm
///
/// Updates use a compare-and-swap (CAS) loop: the new value is only stored
/// if it's less than the current shard value. This ensures correctness
/// without locks while allowing concurrent updates.
///
/// # Memory Usage
///
/// Each `Minimum` tracker uses approximately 4KB of memory (64 slots × 64 bytes).
///
/// # Examples
///
/// ```rust
/// use contatori::counters::minimum::Minimum;
/// use contatori::counters::Observable;
///
/// let min_latency = Minimum::new().with_name("request_latency_min");
///
/// // Record some latencies (in microseconds)
/// min_latency.observe(150);
/// min_latency.observe(85);
/// min_latency.observe(200);
///
/// // The minimum is 85
/// assert_eq!(min_latency.value(), contatori::counters::CounterValue::Unsigned(85));
/// ```
pub struct Minimum {
    name: &'static str,
    components: [CachePadded<AtomicUsize>; NUM_COMPONENTS],
}

impl Minimum {
    /// Creates a new minimum tracker.
    ///
    /// All 64 shards are initialized to `usize::MAX`, so the first observed
    /// value will become the minimum.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use contatori::counters::minimum::Minimum;
    /// use contatori::counters::Observable;
    ///
    /// let tracker = Minimum::new();
    /// // Before any observations, value is MAX
    /// assert_eq!(tracker.value(), contatori::counters::CounterValue::Unsigned(u64::MAX));
    /// ```
    pub const fn new() -> Self {
        const MAX: CachePadded<AtomicUsize> = CachePadded::new(AtomicUsize::new(usize::MAX));
        Minimum {
            components: [MAX; NUM_COMPONENTS],
            name: "",
        }
    }

    /// Sets the name of this tracker, returning `self` for method chaining.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use contatori::counters::minimum::Minimum;
    /// use contatori::counters::Observable;
    ///
    /// let tracker = Minimum::new().with_name("min_response_time");
    /// assert_eq!(tracker.name(), "min_response_time");
    /// ```
    pub const fn with_name(self, name: &'static str) -> Self {
        Self { name, ..self }
    }

    /// Returns a reference to the current thread's shard.
    #[inline]
    fn get_component_counter(&self) -> &AtomicUsize {
        THREAD_SLOT_INDEX.with(|idx| &self.components[*idx])
    }

    /// Observes a value and updates the local minimum if necessary.
    ///
    /// This method uses a compare-and-swap loop to atomically update the
    /// shard value only if the new value is smaller than the current one.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use contatori::counters::minimum::Minimum;
    /// use contatori::counters::Observable;
    ///
    /// let tracker = Minimum::new();
    /// tracker.observe(100);
    /// tracker.observe(50);   // New minimum
    /// tracker.observe(75);   // Ignored (not smaller)
    ///
    /// assert_eq!(tracker.value(), contatori::counters::CounterValue::Unsigned(50));
    /// ```
    #[inline]
    pub fn observe(&self, value: usize) {
        let counter = self.get_component_counter();
        let mut current = counter.load(Ordering::Relaxed);
        while value < current {
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
    }

    /// Sets the value of the current thread's shard directly.
    ///
    /// Use with caution: this bypasses the minimum logic and sets the
    /// shard to an arbitrary value.
    #[inline]
    pub fn set_local_value(&self, value: usize) {
        self.get_component_counter().store(value, Ordering::Relaxed);
    }

    /// Returns the value of the current thread's shard.
    #[inline]
    pub fn local_value(&self) -> usize {
        self.get_component_counter().load(Ordering::Relaxed)
    }

    /// Computes the global minimum by finding the smallest value across all shards.
    #[inline]
    fn raw_value(&self) -> usize {
        self.components
            .iter()
            .map(|counter| counter.load(Ordering::Relaxed))
            .min()
            .unwrap_or(usize::MAX)
    }

    /// Computes the global minimum and resets all shards to `usize::MAX`.
    ///
    /// This is useful for periodic metric collection where you want to
    /// capture the minimum since the last collection.
    #[inline]
    fn raw_value_and_reset(&self) -> usize {
        let mut min = usize::MAX;
        for counter in self.components.iter() {
            let val = counter.swap(usize::MAX, Ordering::Relaxed);
            if val < min {
                min = val;
            }
        }
        min
    }
}

impl Observable for Minimum {
    /// Returns the global minimum across all shards.
    ///
    /// If no values have been observed, returns `u64::MAX`.
    #[inline]
    fn value(&self) -> CounterValue {
        CounterValue::Unsigned(self.raw_value() as u64)
    }

    /// Returns the global minimum and resets all shards to `MAX`.
    ///
    /// After reset, the next observed value will become the new minimum.
    #[inline]
    fn value_and_reset(&self) -> CounterValue {
        CounterValue::Unsigned(self.raw_value_and_reset() as u64)
    }

    /// Returns the name of this tracker.
    #[inline]
    fn name(&self) -> &str {
        self.name
    }
}

impl Default for Minimum {
    /// Creates a new tracker with all shards set to `MAX`.
    fn default() -> Self {
        Self::new()
    }
}

impl Debug for Minimum {
    /// Formats the tracker showing shards that have observed values.
    ///
    /// Shards still at `usize::MAX` (no observations) are not shown.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}{{", self.name)?;
        for (i, counter) in self.components.iter().enumerate() {
            let val = counter.load(Ordering::Relaxed);
            if val != usize::MAX {
                write!(f, " [{i}]:{val}")?;
            }
        }
        write!(f, " }}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::counters::Observable;

    #[test]
    fn test_new() {
        let counter = Minimum::new();
        // Inizialmente è MAX perché nessun valore è stato osservato
        assert_eq!(counter.value(), CounterValue::Unsigned(u64::MAX));
    }

    #[test]
    fn test_observe_single() {
        let counter = Minimum::new();
        counter.observe(42);
        assert_eq!(counter.value(), CounterValue::Unsigned(42));
    }

    #[test]
    fn test_observe_multiple_decreasing() {
        let counter = Minimum::new();
        counter.observe(100);
        assert_eq!(counter.value(), CounterValue::Unsigned(100));
        counter.observe(50);
        assert_eq!(counter.value(), CounterValue::Unsigned(50));
        counter.observe(10);
        assert_eq!(counter.value(), CounterValue::Unsigned(10));
    }

    #[test]
    fn test_observe_multiple_increasing() {
        let counter = Minimum::new();
        counter.observe(10);
        assert_eq!(counter.value(), CounterValue::Unsigned(10));
        counter.observe(50);
        assert_eq!(counter.value(), CounterValue::Unsigned(10)); // Rimane 10
        counter.observe(100);
        assert_eq!(counter.value(), CounterValue::Unsigned(10)); // Rimane 10
    }

    #[test]
    fn test_observe_mixed() {
        let counter = Minimum::new();
        counter.observe(50);
        counter.observe(30);
        counter.observe(70);
        counter.observe(20);
        counter.observe(60);
        assert_eq!(counter.value(), CounterValue::Unsigned(20));
    }

    #[test]
    fn test_observe_zero() {
        let counter = Minimum::new();
        counter.observe(100);
        counter.observe(0);
        assert_eq!(counter.value(), CounterValue::Unsigned(0));
    }

    #[test]
    fn test_set_local_value() {
        let counter = Minimum::new();
        counter.set_local_value(42);
        assert_eq!(counter.local_value(), 42);
        assert_eq!(counter.value(), CounterValue::Unsigned(42));
    }

    #[test]
    fn test_local_value() {
        let counter = Minimum::new();
        assert_eq!(counter.local_value(), usize::MAX);
        counter.observe(100);
        assert_eq!(counter.local_value(), 100);
    }

    #[test]
    fn test_value_and_reset() {
        let counter = Minimum::new();
        counter.observe(50);
        counter.observe(30);
        assert_eq!(counter.value(), CounterValue::Unsigned(30));
        let min = counter.value_and_reset();
        assert_eq!(min, CounterValue::Unsigned(30));
        // Dopo il reset torna a MAX
        assert_eq!(counter.value(), CounterValue::Unsigned(u64::MAX));
    }

    #[test]
    fn test_value_and_reset_then_observe() {
        let counter = Minimum::new();
        counter.observe(30);
        counter.value_and_reset();
        counter.observe(100);
        assert_eq!(counter.value(), CounterValue::Unsigned(100));
    }

    #[test]
    fn test_debug() {
        let counter = Minimum::new();
        counter.set_local_value(5);
        let debug_str = format!("{:?}", counter);
        assert!(debug_str.starts_with("{"));
        assert!(debug_str.contains("5"));
        assert!(debug_str.ends_with("}"));
    }

    #[test]
    fn test_dyn_format() {
        let counter = Minimum::new().with_name("test_counter");
        counter.observe(42);
        let formatted = format!("{}", &counter as &dyn Observable);
        assert_eq!(formatted, "test_counter:42");
    }

    #[test]
    fn test_dyn_debug() {
        let counter = Minimum::new().with_name("test_counter");
        counter.observe(42);
        let debug_str = format!("{:?}", &counter as &dyn Observable);
        assert!(debug_str.starts_with("test_counter{"));
        assert!(debug_str.contains("42"));
        assert!(debug_str.ends_with("}"));
    }

    #[test]
    fn test_multiple_threads() {
        use std::sync::Arc;
        use std::thread;

        let counter = Arc::new(Minimum::new());
        let mut handles = vec![];

        // Ogni thread osserva valori diversi
        for i in 0..4 {
            let counter_clone = Arc::clone(&counter);
            let handle = thread::spawn(move || {
                for j in 0..100 {
                    counter_clone.observe((i + 1) * 1000 + j);
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }

        // Il minimo dovrebbe essere 1000 (thread 0, j=0 -> (0+1)*1000 + 0 = 1000)
        assert_eq!(counter.value(), CounterValue::Unsigned(1000));
    }

    #[test]
    fn test_name_default() {
        let counter = Minimum::new();
        assert_eq!(counter.name(), "");
    }

    #[test]
    fn test_with_name() {
        let counter = Minimum::new().with_name("my_counter");
        assert_eq!(counter.name(), "my_counter");
    }

    #[test]
    fn test_with_name_preserves_value() {
        let counter = Minimum::new().with_name("test");
        counter.observe(42);
        assert_eq!(counter.name(), "test");
        assert_eq!(counter.value(), CounterValue::Unsigned(42));
    }

    #[test]
    fn test_default() {
        let counter = Minimum::default();
        assert_eq!(counter.value(), CounterValue::Unsigned(u64::MAX));
        assert_eq!(counter.name(), "");
    }
}
