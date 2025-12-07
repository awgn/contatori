//! Unsigned integer counter with sharded atomic storage.
//!
//! This module provides [`Unsigned`], a high-performance counter optimized for
//! concurrent increments from multiple threads. It uses sharding to minimize
//! contention and cache-line padding to prevent false sharing.

use std::sync::atomic::{AtomicUsize, Ordering};

use crossbeam_utils::CachePadded;
use std::fmt::Debug;

use crate::counters::{
    sealed, CounterValue, GetComponentCounter, Observable, NUM_COMPONENTS, THREAD_SLOT_INDEX,
};

/// A high-performance unsigned integer counter using sharded atomic storage.
///
/// `Unsigned` is designed for scenarios where multiple threads frequently
/// increment a shared counter. Instead of using a single atomic variable
/// (which causes severe contention), it distributes updates across 64
/// cache-line-padded slots.
///
/// # Performance
///
/// On an Apple M2 with 8 threads performing 1 million increments each:
/// - **Single AtomicUsize**: ~162 ms
/// - **Unsigned (sharded)**: ~2.3 ms
/// - **Speedup**: ~71x faster
///
/// # Memory Usage
///
/// Each `Unsigned` counter uses approximately 4KB of memory (64 slots × 64 bytes).
///
/// # Examples
///
/// Basic usage:
///
/// ```rust
/// use contatori::counters::unsigned::Unsigned;
/// use contatori::counters::Observable;
///
/// let counter = Unsigned::new();
/// counter.add(1);
/// counter.add(5);
/// assert_eq!(counter.value(), contatori::counters::CounterValue::Unsigned(6));
/// ```
///
/// Multi-threaded usage:
///
/// ```rust
/// use contatori::counters::unsigned::Unsigned;
/// use contatori::counters::Observable;
/// use std::sync::Arc;
/// use std::thread;
///
/// let counter = Arc::new(Unsigned::new());
/// let mut handles = vec![];
///
/// for _ in 0..4 {
///     let c = Arc::clone(&counter);
///     handles.push(thread::spawn(move || {
///         for _ in 0..1000 {
///             c.add(1);
///         }
///     }));
/// }
///
/// for h in handles {
///     h.join().unwrap();
/// }
///
/// assert_eq!(counter.value(), contatori::counters::CounterValue::Unsigned(4000));
/// ```
pub struct Unsigned {
    name: &'static str,
    components: [CachePadded<AtomicUsize>; NUM_COMPONENTS],
}

impl GetComponentCounter for Unsigned {
    type CounterType = AtomicUsize;

    /// Returns a reference to the current thread's shard.
    #[inline]
    fn get_component_counter(&self) -> &AtomicUsize {
        THREAD_SLOT_INDEX.with(|idx| &self.components[*idx])
    }
}

impl Unsigned {
    /// Creates a new counter initialized to zero.
    ///
    /// All 64 shards are initialized to zero. The counter has no name by default.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use contatori::counters::unsigned::Unsigned;
    /// use contatori::counters::Observable;
    ///
    /// let counter = Unsigned::new();
    /// assert_eq!(counter.value(), contatori::counters::CounterValue::Unsigned(0));
    /// ```
    pub const fn new() -> Self {
        const ZERO: CachePadded<AtomicUsize> = CachePadded::new(AtomicUsize::new(0));
        Unsigned {
            components: [ZERO; NUM_COMPONENTS],
            name: "",
        }
    }

    /// Sets the name of this counter, returning `self` for method chaining.
    ///
    /// The name is used when formatting the counter for display and can help
    /// identify counters in logs or metrics output.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use contatori::counters::unsigned::Unsigned;
    /// use contatori::counters::Observable;
    ///
    /// let counter = Unsigned::new().with_name("http_requests");
    /// assert_eq!(counter.name(), "http_requests");
    /// ```
    pub const fn with_name(self, name: &'static str) -> Self {
        Self { name, ..self }
    }

    /// Adds a value to the counter.
    ///
    /// This operation is lock-free and extremely fast due to sharding.
    /// Each thread updates its own shard, avoiding contention.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use contatori::counters::unsigned::Unsigned;
    /// use contatori::counters::Observable;
    ///
    /// let counter = Unsigned::new();
    /// counter.add(5);
    /// counter.add(3);
    /// assert_eq!(counter.value(), contatori::counters::CounterValue::Unsigned(8));
    /// ```
    #[inline]
    pub fn add(&self, value: usize) {
        self.get_component_counter()
            .fetch_add(value, Ordering::Relaxed);
    }

    /// Subtracts a value from the counter.
    ///
    /// # Warning
    ///
    /// This uses wrapping subtraction. Subtracting more than the current value
    /// will cause the counter to wrap around to a very large number.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use contatori::counters::unsigned::Unsigned;
    /// use contatori::counters::Observable;
    ///
    /// let counter = Unsigned::new();
    /// counter.add(10);
    /// counter.sub(3);
    /// assert_eq!(counter.value(), contatori::counters::CounterValue::Unsigned(7));
    /// ```
    #[inline]
    pub fn sub(&self, value: usize) {
        self.get_component_counter()
            .fetch_sub(value, Ordering::Relaxed);
    }

    /// Sets the value of the current thread's shard directly.
    ///
    /// This is useful for gauge-like behavior where you want to set an
    /// absolute value rather than increment/decrement.
    ///
    /// # Note
    ///
    /// This only sets the current thread's shard. Other threads' contributions
    /// remain unchanged, so `value()` may return a different total.
    #[inline]
    pub fn set_local_value(&self, value: usize) {
        self.get_component_counter().store(value, Ordering::Relaxed);
    }

    /// Returns the value of the current thread's shard.
    ///
    /// This is useful for debugging or when you need to know this thread's
    /// contribution to the total.
    #[inline]
    pub fn local_value(&self) -> usize {
        self.get_component_counter().load(Ordering::Relaxed)
    }

    /// Computes the total value by summing all shards.
    #[inline]
    fn total_value(&self) -> usize {
        self.components
            .iter()
            .map(|counter| counter.load(Ordering::Relaxed))
            .sum()
    }

    /// Computes the total value and resets all shards to zero.
    #[inline]
    fn total_value_and_reset(&self) -> usize {
        let mut total = 0;
        for counter in self.components.iter() {
            total += counter.swap(0, Ordering::Relaxed);
        }
        total
    }
}

impl Observable for Unsigned {
    /// Returns the total counter value by summing all shards.
    ///
    /// This iterates over all 64 shards and sums their values.
    #[inline]
    fn value(&self) -> CounterValue {
        CounterValue::Unsigned(self.total_value() as u64)
    }

    /// Returns the name of this counter.
    #[inline]
    fn name(&self) -> &'static str {
        self.name
    }
}

impl sealed::Resettable for Unsigned {
    /// Returns the total value and resets all shards to zero.
    ///
    /// Useful for periodic metric collection.
    #[inline]
    fn value_and_reset(&self) -> CounterValue {
        CounterValue::Unsigned(self.total_value_and_reset() as u64)
    }
}

impl Default for Unsigned {
    /// Creates a new counter initialized to zero with no name.
    fn default() -> Self {
        Self::new()
    }
}

impl Debug for Unsigned {
    /// Formats the counter showing non-zero shards.
    ///
    /// Output format: `name{ [slot]:value [slot]:value ... }`
    ///
    /// Only shards with non-zero values are shown.
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
    use crate::counters::Observable;

    #[test]
    fn test_new() {
        let counter = Unsigned::new();
        assert_eq!(counter.value(), CounterValue::Unsigned(0));
    }

    #[test]
    fn test_add() {
        let counter = Unsigned::new();
        counter.add(1);
        assert_eq!(counter.value(), CounterValue::Unsigned(1));
        counter.add(1);
        counter.add(1);
        assert_eq!(counter.value(), CounterValue::Unsigned(3));
    }

    #[test]
    fn test_sub() {
        let counter = Unsigned::new();
        counter.set_local_value(10);
        counter.sub(1);
        assert_eq!(counter.value(), CounterValue::Unsigned(9));
        counter.sub(1);
        counter.sub(1);
        assert_eq!(counter.value(), CounterValue::Unsigned(7));
    }

    #[test]
    fn test_set_local_value() {
        let counter = Unsigned::new();
        counter.set_local_value(42);
        assert_eq!(counter.local_value(), 42);
        assert_eq!(counter.value(), CounterValue::Unsigned(42));
    }

    #[test]
    fn test_local_value() {
        let counter = Unsigned::new();
        assert_eq!(counter.local_value(), 0);
        counter.add(1);
        assert_eq!(counter.local_value(), 1);
    }

    #[test]
    fn test_resettable() {
        use crate::adapters::Resettable;
        let counter = Resettable::new(Unsigned::new());
        counter.add(1);
        counter.add(1);
        counter.add(1);
        assert_eq!(counter.value(), CounterValue::Unsigned(3));
        // After value() the counter should be reset
        assert_eq!(counter.value(), CounterValue::Unsigned(0));
    }

    #[test]
    fn test_debug() {
        let counter = Unsigned::new();
        counter.set_local_value(5);
        let debug_str = format!("{:?}", counter);
        assert!(debug_str.starts_with("{"));
        assert!(debug_str.contains("5"));
        assert!(debug_str.ends_with("}"));
    }

    #[test]
    fn test_dyn_format() {
        let counter = Unsigned::new().with_name("test_counter");
        counter.add(1);
        let formatted = format!("{}", &counter as &dyn Observable);
        assert_eq!(formatted, "test_counter:1");
    }

    #[test]
    fn test_dyn_debug() {
        let counter = Unsigned::new().with_name("test_counter");
        counter.add(1);
        counter.add(1);
        let debug_str = format!("{:?}", &counter as &dyn Observable);
        assert!(debug_str.starts_with("test_counter{"));
        assert!(debug_str.contains("2"));
        assert!(debug_str.ends_with("}"));
    }

    #[test]
    fn test_multiple_threads() {
        use std::sync::Arc;
        use std::thread;

        let counter = Arc::new(Unsigned::new());
        let mut handles = vec![];

        for _ in 0..4 {
            let counter_clone = Arc::clone(&counter);
            let handle = thread::spawn(move || {
                for _ in 0..100 {
                    counter_clone.add(1);
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }

        assert_eq!(counter.value(), CounterValue::Unsigned(400));
    }

    #[test]
    fn test_name_default() {
        let counter = Unsigned::new();
        assert_eq!(counter.name(), "");
    }

    #[test]
    fn test_with_name() {
        let counter = Unsigned::new().with_name("my_counter");
        assert_eq!(counter.name(), "my_counter");
    }

    #[test]
    fn test_with_name_preserves_value() {
        let counter = Unsigned::new().with_name("test");
        counter.add(1);
        counter.add(1);
        assert_eq!(counter.name(), "test");
        assert_eq!(counter.value(), CounterValue::Unsigned(2));
    }

    #[test]
    fn test_default() {
        let counter = Unsigned::default();
        assert_eq!(counter.value(), CounterValue::Unsigned(0));
        assert_eq!(counter.name(), "");
    }
}
