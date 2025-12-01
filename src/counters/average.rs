//! Average value counter with sharded atomic storage.
//!
//! This module provides [`Average`], a high-performance counter that computes
//! the running average of observed values. It uses sharding to minimize
//! contention during updates.

use std::sync::atomic::{AtomicUsize, Ordering};

use crossbeam_utils::CachePadded;
use std::fmt::Debug;

use crate::counters::{CounterValue, Observable, NUM_COMPONENTS, THREAD_SLOT_INDEX};

/// Internal component that stores sum and count for a single shard.
///
/// By combining sum and count in a single struct wrapped in `CachePadded`,
/// we ensure both values share the same cache line, reducing memory usage
/// compared to two separate arrays.
struct SumCount {
    sum: AtomicUsize,
    count: AtomicUsize,
}

impl SumCount {
    const fn new() -> Self {
        SumCount {
            sum: AtomicUsize::new(0),
            count: AtomicUsize::new(0),
        }
    }
}

/// A high-performance average counter using sharded atomic storage.
///
/// `Average` tracks the sum and count of observed values across all threads,
/// allowing you to compute the running average. Each shard maintains its own
/// sum and count, which are aggregated when reading.
///
/// # Memory Optimization
///
/// Unlike `Unsigned` which uses a single atomic per shard, `Average` stores
/// both sum and count in each shard. By combining them in a single `CachePadded`
/// struct, we use ~4KB total instead of ~8KB that two separate arrays would require.
///
/// # Use Cases
///
/// - Computing average request latency
/// - Tracking mean values over time
/// - Calculating running averages for metrics
///
/// # Examples
///
/// Basic usage:
///
/// ```rust
/// use contatori::counters::average::Average;
/// use contatori::counters::Observable;
///
/// let avg_latency = Average::new().with_name("request_latency_avg");
///
/// // Record some latencies (in microseconds)
/// avg_latency.observe(100);
/// avg_latency.observe(150);
/// avg_latency.observe(200);
///
/// // The average is (100 + 150 + 200) / 3 = 150
/// assert_eq!(avg_latency.average(), Some(150));
/// ```
///
/// Batch observations:
///
/// ```rust
/// use contatori::counters::average::Average;
///
/// let avg = Average::new();
///
/// // Observe multiple values at once (sum=300, count=3)
/// avg.observe_many(300, 3);
///
/// assert_eq!(avg.sum(), 300);
/// assert_eq!(avg.count(), 3);
/// assert_eq!(avg.average(), Some(100));
/// ```
pub struct Average {
    name: &'static str,
    components: [CachePadded<SumCount>; NUM_COMPONENTS],
}

impl Average {
    /// Creates a new average counter initialized to zero.
    ///
    /// All 64 shards have their sum and count set to zero.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use contatori::counters::average::Average;
    ///
    /// let avg = Average::new();
    /// assert_eq!(avg.sum(), 0);
    /// assert_eq!(avg.count(), 0);
    /// assert_eq!(avg.average(), None); // No observations yet
    /// ```
    pub const fn new() -> Self {
        const ZERO: CachePadded<SumCount> = CachePadded::new(SumCount::new());
        Average {
            components: [ZERO; NUM_COMPONENTS],
            name: "",
        }
    }

    /// Sets the name of this counter, returning `self` for method chaining.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use contatori::counters::average::Average;
    /// use contatori::counters::Observable;
    ///
    /// let avg = Average::new().with_name("response_time_avg");
    /// assert_eq!(avg.name(), "response_time_avg");
    /// ```
    pub const fn with_name(self, name: &'static str) -> Self {
        Self { name, ..self }
    }

    /// Returns a reference to the current thread's shard.
    #[inline]
    fn get_local_component(&self) -> &SumCount {
        THREAD_SLOT_INDEX.with(|idx| &*self.components[*idx])
    }

    /// Observes a single value to include in the average.
    ///
    /// This increments the sum by `value` and the count by 1.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use contatori::counters::average::Average;
    ///
    /// let avg = Average::new();
    /// avg.observe(10);
    /// avg.observe(20);
    ///
    /// assert_eq!(avg.sum(), 30);
    /// assert_eq!(avg.count(), 2);
    /// assert_eq!(avg.average(), Some(15));
    /// ```
    #[inline]
    pub fn observe(&self, value: usize) {
        let component = self.get_local_component();
        component.sum.fetch_add(value, Ordering::Relaxed);
        component.count.fetch_add(1, Ordering::Relaxed);
    }

    /// Observes multiple values at once (batch optimization).
    ///
    /// This is more efficient than calling `observe()` multiple times when
    /// you have pre-aggregated data.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use contatori::counters::average::Average;
    ///
    /// let avg = Average::new();
    ///
    /// // Instead of 4 separate observe() calls
    /// avg.observe_many(100, 4); // sum=100, count=4
    ///
    /// assert_eq!(avg.average(), Some(25));
    /// ```
    #[inline]
    pub fn observe_many(&self, sum: usize, count: usize) {
        let component = self.get_local_component();
        component.sum.fetch_add(sum, Ordering::Relaxed);
        component.count.fetch_add(count, Ordering::Relaxed);
    }

    /// Adds a value to the local sum without incrementing the count.
    ///
    /// Use this when you need to manipulate sum and count separately.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use contatori::counters::average::Average;
    ///
    /// let avg = Average::new();
    /// avg.add_sum(100);
    /// avg.add_count(2);
    ///
    /// assert_eq!(avg.average(), Some(50));
    /// ```
    #[inline]
    pub fn add_sum(&self, value: usize) {
        self.get_local_component()
            .sum
            .fetch_add(value, Ordering::Relaxed);
    }

    /// Adds a value to the local count without modifying the sum.
    ///
    /// Use this when you need to manipulate sum and count separately.
    #[inline]
    pub fn add_count(&self, value: usize) {
        self.get_local_component()
            .count
            .fetch_add(value, Ordering::Relaxed);
    }

    /// Increments the local count by 1 without modifying the sum.
    ///
    /// Useful for counting events without associated values.
    #[inline]
    pub fn incr(&self) {
        self.get_local_component()
            .count
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Decrements the local count by 1 without modifying the sum.
    ///
    /// # Warning
    ///
    /// This can cause underflow if count goes below zero.
    #[inline]
    pub fn decr(&self) {
        self.get_local_component()
            .count
            .fetch_sub(1, Ordering::Relaxed);
    }

    /// Returns the total sum of all observed values across all shards.
    #[inline]
    pub fn sum(&self) -> usize {
        self.components
            .iter()
            .map(|c| c.sum.load(Ordering::Relaxed))
            .sum()
    }

    /// Returns the total count of observations across all shards.
    #[inline]
    pub fn count(&self) -> usize {
        self.components
            .iter()
            .map(|c| c.count.load(Ordering::Relaxed))
            .sum()
    }

    /// Computes the average as an integer (truncated).
    ///
    /// Returns `None` if no values have been observed (count is zero).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use contatori::counters::average::Average;
    ///
    /// let avg = Average::new();
    /// assert_eq!(avg.average(), None);
    ///
    /// avg.observe(10);
    /// avg.observe(20);
    /// assert_eq!(avg.average(), Some(15));
    /// ```
    #[inline]
    pub fn average(&self) -> Option<usize> {
        let total_sum = self.sum();
        let total_count = self.count();
        if total_count == 0 {
            None
        } else {
            Some(total_sum / total_count)
        }
    }

    /// Computes the average as a floating-point number for higher precision.
    ///
    /// Returns `None` if no values have been observed (count is zero).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use contatori::counters::average::Average;
    ///
    /// let avg = Average::new();
    /// avg.observe(1);
    /// avg.observe(2);
    ///
    /// assert_eq!(avg.average(), Some(1));      // Truncated
    /// assert_eq!(avg.average_f64(), Some(1.5)); // Precise
    /// ```
    #[inline]
    pub fn average_f64(&self) -> Option<f64> {
        let total_sum = self.sum();
        let total_count = self.count();
        if total_count == 0 {
            None
        } else {
            Some(total_sum as f64 / total_count as f64)
        }
    }

    /// Computes sum and count, then resets all shards to zero.
    #[inline]
    fn raw_value_and_reset(&self) -> (usize, usize) {
        let mut total_sum = 0;
        let mut total_count = 0;
        for component in self.components.iter() {
            total_sum += component.sum.swap(0, Ordering::Relaxed);
            total_count += component.count.swap(0, Ordering::Relaxed);
        }
        (total_sum, total_count)
    }

    /// Returns sum and count, then resets the counter.
    ///
    /// Useful for periodic metric collection where you want to compute
    /// the average for a time window and start fresh.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use contatori::counters::average::Average;
    ///
    /// let avg = Average::new();
    /// avg.observe(100);
    /// avg.observe(200);
    ///
    /// let (sum, count) = avg.sum_count_and_reset();
    /// assert_eq!(sum, 300);
    /// assert_eq!(count, 2);
    ///
    /// // After reset
    /// assert_eq!(avg.sum(), 0);
    /// assert_eq!(avg.count(), 0);
    /// ```
    #[inline]
    pub fn sum_count_and_reset(&self) -> (usize, usize) {
        self.raw_value_and_reset()
    }

    /// Returns the average and resets the counter.
    ///
    /// Combines `average()` and reset in a single operation.
    /// Returns `None` if no values were observed during this period.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use contatori::counters::average::Average;
    ///
    /// let avg = Average::new();
    /// avg.observe(100);
    /// avg.observe(200);
    ///
    /// let result = avg.average_and_reset();
    /// assert_eq!(result, Some(150));
    ///
    /// // After reset
    /// assert_eq!(avg.average(), None);
    /// ```
    #[inline]
    pub fn average_and_reset(&self) -> Option<usize> {
        let (sum, count) = self.raw_value_and_reset();
        if count == 0 {
            None
        } else {
            Some(sum / count)
        }
    }
}

impl Observable for Average {
    /// Returns the average as a `CounterValue`.
    ///
    /// If no values have been observed, returns `0`.
    #[inline]
    fn value(&self) -> CounterValue {
        CounterValue::Unsigned(self.average().unwrap_or(0) as u64)
    }

    /// Returns the average and resets the counter.
    ///
    /// If no values were observed, returns `0`.
    #[inline]
    fn value_and_reset(&self) -> CounterValue {
        CounterValue::Unsigned(self.average_and_reset().unwrap_or(0) as u64)
    }

    /// Returns the name of this counter.
    #[inline]
    fn name(&self) -> &str {
        self.name
    }
}

impl Default for Average {
    /// Creates a new average counter initialized to zero.
    fn default() -> Self {
        Self::new()
    }
}

impl Debug for Average {
    /// Formats the counter showing non-zero shards.
    ///
    /// Output format: `name{ [slot]:sum=X,count=Y ... }`
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}{{", self.name)?;
        for (i, component) in self.components.iter().enumerate() {
            let sum = component.sum.load(Ordering::Relaxed);
            let count = component.count.load(Ordering::Relaxed);
            if count != 0 {
                write!(f, " [{i}]:sum={sum},count={count}")?;
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
        let counter = Average::new();
        assert_eq!(counter.sum(), 0);
        assert_eq!(counter.count(), 0);
        assert_eq!(counter.average(), None);
    }

    #[test]
    fn test_observe_single() {
        let counter = Average::new();
        counter.observe(42);
        assert_eq!(counter.sum(), 42);
        assert_eq!(counter.count(), 1);
        assert_eq!(counter.average(), Some(42));
    }

    #[test]
    fn test_observe_multiple() {
        let counter = Average::new();
        counter.observe(10);
        counter.observe(20);
        counter.observe(30);
        assert_eq!(counter.sum(), 60);
        assert_eq!(counter.count(), 3);
        assert_eq!(counter.average(), Some(20));
    }

    #[test]
    fn test_observe_many() {
        let counter = Average::new();
        counter.observe_many(100, 4);
        assert_eq!(counter.sum(), 100);
        assert_eq!(counter.count(), 4);
        assert_eq!(counter.average(), Some(25));
    }

    #[test]
    fn test_average_f64() {
        let counter = Average::new();
        counter.observe(10);
        counter.observe(20);
        counter.observe(30);
        assert_eq!(counter.average_f64(), Some(20.0));

        let counter2 = Average::new();
        counter2.observe(1);
        counter2.observe(2);
        assert_eq!(counter2.average(), Some(1));
        assert_eq!(counter2.average_f64(), Some(1.5));
    }

    #[test]
    fn test_average_empty() {
        let counter = Average::new();
        assert_eq!(counter.average(), None);
        assert_eq!(counter.average_f64(), None);
    }

    #[test]
    fn test_sum_count_and_reset() {
        let counter = Average::new();
        counter.observe(10);
        counter.observe(20);
        counter.observe(30);

        let (sum, count) = counter.sum_count_and_reset();
        assert_eq!(sum, 60);
        assert_eq!(count, 3);

        assert_eq!(counter.sum(), 0);
        assert_eq!(counter.count(), 0);
        assert_eq!(counter.average(), None);
    }

    #[test]
    fn test_average_and_reset() {
        let counter = Average::new();
        counter.observe(10);
        counter.observe(20);
        counter.observe(30);

        let avg = counter.average_and_reset();
        assert_eq!(avg, Some(20));

        assert_eq!(counter.average(), None);
    }

    #[test]
    fn test_value_and_reset_then_observe() {
        let counter = Average::new();
        counter.observe(100);
        counter.value_and_reset();
        counter.observe(50);
        assert_eq!(counter.average(), Some(50));
    }

    #[test]
    fn test_debug() {
        let counter = Average::new();
        counter.observe(10);
        counter.observe(20);
        let debug_str = format!("{:?}", counter);
        assert!(debug_str.starts_with("{"));
        assert!(debug_str.contains("sum=30"));
        assert!(debug_str.contains("count=2"));
        assert!(debug_str.ends_with("}"));
    }

    #[test]
    fn test_dyn_format() {
        let counter = Average::new().with_name("avg_counter");
        counter.observe(10);
        counter.observe(20);
        counter.observe(30);
        let formatted = format!("{}", &counter as &dyn Observable);
        assert_eq!(formatted, "avg_counter:20");
    }

    #[test]
    fn test_dyn_debug() {
        let counter = Average::new().with_name("avg_counter");
        counter.observe(50);
        let debug_str = format!("{:?}", &counter as &dyn Observable);
        assert!(debug_str.starts_with("avg_counter{"));
        assert!(debug_str.ends_with("}"));
    }

    #[test]
    fn test_multiple_threads() {
        use std::sync::Arc;
        use std::thread;

        let counter = Arc::new(Average::new());
        let mut handles = vec![];

        for _ in 0..4 {
            let counter_clone = Arc::clone(&counter);
            let handle = thread::spawn(move || {
                for j in 1..=100 {
                    counter_clone.observe(j);
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }

        assert_eq!(counter.count(), 400);
        assert_eq!(counter.sum(), 20200);
        assert_eq!(counter.average(), Some(50));
    }

    #[test]
    fn test_name_default() {
        let counter = Average::new();
        assert_eq!(counter.name(), "");
    }

    #[test]
    fn test_with_name() {
        let counter = Average::new().with_name("my_avg");
        assert_eq!(counter.name(), "my_avg");
    }

    #[test]
    fn test_default() {
        let counter = Average::default();
        assert_eq!(counter.average(), None);
        assert_eq!(counter.name(), "");
    }

    #[test]
    fn test_observable_value_empty() {
        let counter = Average::new();
        assert_eq!(counter.value(), CounterValue::Unsigned(0));
    }

    #[test]
    fn test_observable_value() {
        let counter = Average::new();
        counter.observe(10);
        counter.observe(30);
        assert_eq!(counter.value(), CounterValue::Unsigned(20));
    }

    #[test]
    fn test_add_sum() {
        let counter = Average::new();
        counter.add_sum(100);
        assert_eq!(counter.sum(), 100);
        assert_eq!(counter.count(), 0);
        assert_eq!(counter.average(), None);
    }

    #[test]
    fn test_add_count() {
        let counter = Average::new();
        counter.add_count(5);
        assert_eq!(counter.sum(), 0);
        assert_eq!(counter.count(), 5);
        assert_eq!(counter.average(), Some(0));
    }

    #[test]
    fn test_add_sum_and_count_separately() {
        let counter = Average::new();
        counter.add_sum(100);
        counter.add_count(4);
        assert_eq!(counter.sum(), 100);
        assert_eq!(counter.count(), 4);
        assert_eq!(counter.average(), Some(25));
    }

    #[test]
    fn test_incr() {
        let counter = Average::new();
        counter.incr();
        counter.incr();
        counter.incr();
        assert_eq!(counter.count(), 3);
        assert_eq!(counter.sum(), 0);
    }

    #[test]
    fn test_decr() {
        let counter = Average::new();
        counter.add_count(10);
        counter.decr();
        counter.decr();
        assert_eq!(counter.count(), 8);
    }

    #[test]
    fn test_combined_operations() {
        let counter = Average::new();
        counter.observe(10);
        counter.observe(20);
        counter.add_sum(30);
        counter.incr();
        assert_eq!(counter.sum(), 60);
        assert_eq!(counter.count(), 3);
        assert_eq!(counter.average(), Some(20));
    }
}
