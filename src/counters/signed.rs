//! Signed integer counter with sharded atomic storage.
//!
//! This module provides [`Signed`], a high-performance counter that supports
//! both positive and negative values. It uses the same sharding strategy as
//! [`Unsigned`](super::unsigned::Unsigned) to minimize contention.

use std::sync::atomic::{AtomicIsize, Ordering};

use crossbeam_utils::CachePadded;
use std::fmt::Debug;

use crate::counters::{
    sealed, CounterValue, GetComponentCounter, Observable, NUM_COMPONENTS, THREAD_SLOT_INDEX,
};

/// A high-performance signed integer counter using sharded atomic storage.
///
/// `Signed` is similar to [`Unsigned`](super::unsigned::Unsigned) but uses
/// `AtomicIsize` to support negative values. This makes it suitable for
/// gauges, balance tracking, or any metric that can increase or decrease.
///
/// # Performance
///
/// Uses the same sharding strategy as `Unsigned`, providing similar
/// performance benefits (~71x faster than a single atomic under high contention).
///
/// # Memory Usage
///
/// Each `Signed` counter uses approximately 4KB of memory (64 slots × 64 bytes).
///
/// # Examples
///
/// Basic usage:
///
/// ```rust
/// use contatori::counters::signed::Signed;
/// use contatori::counters::Observable;
///
/// let gauge = Signed::new().with_name("active_connections");
///
/// // Connections open
/// gauge.add(1);
/// gauge.add(1);
///
/// // Connection closes
/// gauge.sub(1);
///
/// assert_eq!(gauge.value(), contatori::counters::CounterValue::Signed(1));
/// ```
///
/// Tracking balance:
///
/// ```rust
/// use contatori::counters::signed::Signed;
/// use contatori::counters::Observable;
///
/// let balance = Signed::new();
/// balance.add(100);  // Deposit
/// balance.sub(150);  // Withdrawal (overdraft!)
///
/// assert_eq!(balance.value(), contatori::counters::CounterValue::Signed(-50));
/// ```
pub struct Signed {
    name: &'static str,
    components: [CachePadded<AtomicIsize>; NUM_COMPONENTS],
}

impl GetComponentCounter for Signed {
    type CounterType = AtomicIsize;

    /// Returns a reference to the current thread's shard.
    #[inline]
    fn get_component_counter(&self) -> &AtomicIsize {
        THREAD_SLOT_INDEX.with(|idx| &self.components[*idx])
    }
}

impl Signed {
    /// Creates a new counter initialized to zero.
    ///
    /// All 64 shards are initialized to zero. The counter has no name by default.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use contatori::counters::signed::Signed;
    /// use contatori::counters::Observable;
    ///
    /// let counter = Signed::new();
    /// assert_eq!(counter.value(), contatori::counters::CounterValue::Signed(0));
    /// ```
    pub const fn new() -> Self {
        const ZERO: CachePadded<AtomicIsize> = CachePadded::new(AtomicIsize::new(0));
        Signed {
            components: [ZERO; NUM_COMPONENTS],
            name: "",
        }
    }

    /// Sets the name of this counter, returning `self` for method chaining.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use contatori::counters::signed::Signed;
    /// use contatori::counters::Observable;
    ///
    /// let counter = Signed::new().with_name("temperature_delta");
    /// assert_eq!(counter.name(), "temperature_delta");
    /// ```
    pub const fn with_name(self, name: &'static str) -> Self {
        Self { name, ..self }
    }

    /// Adds a value to the counter (can be negative).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use contatori::counters::signed::Signed;
    /// use contatori::counters::Observable;
    ///
    /// let counter = Signed::new();
    /// counter.add(10);
    /// counter.add(-15);
    /// assert_eq!(counter.value(), contatori::counters::CounterValue::Signed(-5));
    /// ```
    #[inline]
    pub fn add(&self, value: isize) {
        self.get_component_counter()
            .fetch_add(value, Ordering::Relaxed);
    }

    /// Subtracts a value from the counter.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use contatori::counters::signed::Signed;
    /// use contatori::counters::Observable;
    ///
    /// let counter = Signed::new();
    /// counter.sub(5);
    /// assert_eq!(counter.value(), contatori::counters::CounterValue::Signed(-5));
    /// ```
    #[inline]
    pub fn sub(&self, value: isize) {
        self.get_component_counter()
            .fetch_sub(value, Ordering::Relaxed);
    }

    /// Sets the value of the current thread's shard directly.
    ///
    /// This only affects the current thread's shard; other shards remain unchanged.
    #[inline]
    pub fn set_local_value(&self, value: isize) {
        self.get_component_counter().store(value, Ordering::Relaxed);
    }

    /// Returns the value of the current thread's shard.
    #[inline]
    pub fn local_value(&self) -> isize {
        self.get_component_counter().load(Ordering::Relaxed)
    }

    /// Computes the total value by summing all shards.
    #[inline]
    fn total_value(&self) -> isize {
        self.components
            .iter()
            .map(|counter| counter.load(Ordering::Relaxed))
            .sum()
    }

    /// Computes the total value and resets all shards to zero.
    #[inline]
    fn total_value_and_reset(&self) -> isize {
        let mut total = 0;
        for counter in self.components.iter() {
            total += counter.swap(0, Ordering::Relaxed);
        }
        total
    }
}

impl Observable for Signed {
    /// Returns the total counter value by summing all shards.
    #[inline]
    fn value(&self) -> CounterValue {
        CounterValue::Signed(self.total_value() as i64)
    }

    /// Returns the name of this counter.
    #[inline]
    fn name(&self) -> &'static str {
        self.name
    }
}

impl sealed::Resettable for Signed {
    /// Returns the total value and resets all shards to zero.
    #[inline]
    fn value_and_reset(&self) -> CounterValue {
        CounterValue::Signed(self.total_value_and_reset() as i64)
    }
}

impl Default for Signed {
    /// Creates a new counter initialized to zero with no name.
    fn default() -> Self {
        Self::new()
    }
}

impl Debug for Signed {
    /// Formats the counter showing non-zero shards.
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
        let counter = Signed::new();
        assert_eq!(counter.value(), CounterValue::Signed(0));
    }

    #[test]
    fn test_incr() {
        let counter = Signed::new();
        counter.add(1);
        assert_eq!(counter.value(), CounterValue::Signed(1));
        counter.add(1);
        counter.add(1);
        assert_eq!(counter.value(), CounterValue::Signed(3));
    }

    #[test]
    fn test_decr() {
        let counter = Signed::new();
        counter.sub(1);
        assert_eq!(counter.value(), CounterValue::Signed(-1));
        counter.sub(1);
        counter.sub(1);
        assert_eq!(counter.value(), CounterValue::Signed(-3));
    }

    #[test]
    fn test_add() {
        let counter = Signed::new();
        counter.add(10);
        assert_eq!(counter.value(), CounterValue::Signed(10));
        counter.add(-15);
        assert_eq!(counter.value(), CounterValue::Signed(-5));
    }

    #[test]
    fn test_sub() {
        let counter = Signed::new();
        counter.sub(5);
        assert_eq!(counter.value(), CounterValue::Signed(-5));
        counter.sub(-10);
        assert_eq!(counter.value(), CounterValue::Signed(5));
    }

    #[test]
    fn test_set_local_value() {
        let counter = Signed::new();
        counter.set_local_value(-42);
        assert_eq!(counter.local_value(), -42);
        assert_eq!(counter.value(), CounterValue::Signed(-42));
    }

    #[test]
    fn test_local_value() {
        let counter = Signed::new();
        assert_eq!(counter.local_value(), 0);
        counter.sub(1);
        assert_eq!(counter.local_value(), -1);
    }

    #[test]
    fn test_resettable() {
        use crate::adapters::Resettable;
        let counter = Resettable::new(Signed::new());
        counter.add(5);
        counter.sub(8);
        assert_eq!(counter.value(), CounterValue::Signed(-3));
        // After value() the counter should be reset
        assert_eq!(counter.value(), CounterValue::Signed(0));
    }

    #[test]
    fn test_debug() {
        let counter = Signed::new();
        counter.set_local_value(-5);
        let debug_str = format!("{:?}", counter);
        assert!(debug_str.starts_with("{"));
        assert!(debug_str.contains("-5"));
        assert!(debug_str.ends_with("}"));
    }

    #[test]
    fn test_dyn_format() {
        let counter = Signed::new().with_name("test_counter");
        counter.sub(1);
        let formatted = format!("{}", &counter as &dyn Observable);
        assert_eq!(formatted, "test_counter:-1");
    }

    #[test]
    fn test_dyn_debug() {
        let counter = Signed::new().with_name("test_counter");
        counter.add(-2);
        let debug_str = format!("{:?}", &counter as &dyn Observable);
        assert!(debug_str.starts_with("test_counter{"));
        assert!(debug_str.contains("-2"));
        assert!(debug_str.ends_with("}"));
    }

    #[test]
    fn test_multiple_threads() {
        use std::sync::Arc;
        use std::thread;

        let counter = Arc::new(Signed::new());
        let mut handles = vec![];

        // Half threads increment, half decrement
        for i in 0..4 {
            let counter_clone = Arc::clone(&counter);
            let handle = thread::spawn(move || {
                for _ in 0..100 {
                    if i % 2 == 0 {
                        counter_clone.add(1);
                    } else {
                        counter_clone.sub(1);
                    }
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }

        assert_eq!(counter.value(), CounterValue::Signed(0));
    }

    #[test]
    fn test_name_default() {
        let counter = Signed::new();
        assert_eq!(counter.name(), "");
    }

    #[test]
    fn test_with_name() {
        let counter = Signed::new().with_name("my_counter");
        assert_eq!(counter.name(), "my_counter");
    }

    #[test]
    fn test_with_name_preserves_value() {
        let counter = Signed::new().with_name("test");
        counter.sub(1);
        counter.sub(1);
        assert_eq!(counter.name(), "test");
        assert_eq!(counter.value(), CounterValue::Signed(-2));
    }

    #[test]
    fn test_default() {
        let counter = Signed::default();
        assert_eq!(counter.value(), CounterValue::Signed(0));
        assert_eq!(counter.name(), "");
    }
}