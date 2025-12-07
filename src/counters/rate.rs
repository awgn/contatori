//! Rate counter with sharded atomic storage.
//!
//! This module provides [`Rate`], a high-performance counter that calculates
//! the rate of change (units per second) over time. It uses sharding to minimize
//! contention and cache-line padding to prevent false sharing.
//!
//! # Design
//!
//! The `Rate` counter uses:
//! - Sharded atomic storage for the counter value (like other counters)
//! - `AtomicU64` for the last observed value
//! - `AtomicOptionInstant` for the last timestamp (from `atomic-time` crate)
//!
//! This allows the counter to be initialized in a `const` context.
//!
//! # Examples
//!
//! ```rust
//! use contatori::counters::rate::Rate;
//! use contatori::counters::Observable;
//! use std::thread;
//! use std::time::Duration;
//!
//! let counter = Rate::new().with_name("requests_per_sec");
//!
//! // First call returns 0.0 (no previous measurement)
//! let rate1 = counter.rate();
//! assert_eq!(rate1, 0.0);
//!
//! // Add some values
//! counter.add(100);
//!
//! // Wait a bit
//! thread::sleep(Duration::from_millis(100));
//!
//! // Now rate() returns the rate of change
//! let rate2 = counter.rate();
//! // rate2 ≈ 100 / 0.1 = ~1000 per second
//! ```

use atomic_time::AtomicOptionInstant;
use crossbeam_utils::CachePadded;
use std::fmt::Debug;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

use crate::counters::{
    sealed, CounterValue, GetComponentCounter, MetricKind, Observable, ObservableEntry,
    NUM_COMPONENTS, THREAD_SLOT_INDEX,
};

/// A high-performance rate counter using sharded atomic storage.
///
/// `Rate` tracks the rate of change (units per second) of increments over time.
/// It combines sharded counter storage with rate calculation state.
///
/// # Const Initialization
///
/// The counter can be initialized in a `const` context, making it suitable
/// for `static` variables:
///
/// ```rust
/// use contatori::counters::rate::Rate;
///
/// static REQUESTS_RATE: Rate = Rate::new();
/// ```
///
/// # Rate Calculation
///
/// The `rate()` method returns the rate of change since the last call:
///
/// ```text
/// rate = (current_value - last_value) / elapsed_seconds
/// ```
///
/// On the first call, `rate()` returns `0.0` and establishes a baseline.
///
/// # Memory Usage
///
/// Each `Rate` counter uses approximately 4KB of memory (64 slots × 64 bytes)
/// plus a small overhead for rate calculation state.
///
/// # Examples
///
/// Basic usage:
///
/// ```rust
/// use contatori::counters::rate::Rate;
/// use contatori::counters::Observable;
///
/// let counter = Rate::new();
/// counter.add(1);
/// counter.add(5);
///
/// // total_value() returns the absolute count
/// assert_eq!(counter.total_value(), 6);
/// ```
///
/// Multi-threaded usage:
///
/// ```rust
/// use contatori::counters::rate::Rate;
/// use contatori::counters::Observable;
/// use std::sync::Arc;
/// use std::thread;
///
/// let counter = Arc::new(Rate::new());
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
/// assert_eq!(counter.total_value(), 4000);
/// ```
pub struct Rate {
    name: &'static str,
    components: [CachePadded<AtomicUsize>; NUM_COMPONENTS],
    /// Last observed value for rate calculation
    last_value: AtomicU64,
    /// Last timestamp when rate was calculated (None = never called)
    last_instant: AtomicOptionInstant,
}

impl GetComponentCounter for Rate {
    type CounterType = AtomicUsize;

    /// Returns a reference to the current thread's shard.
    #[inline]
    fn get_component_counter(&self) -> &AtomicUsize {
        THREAD_SLOT_INDEX.with(|idx| &self.components[*idx])
    }
}

impl Rate {
    /// Creates a new counter initialized to zero.
    ///
    /// All 64 shards are initialized to zero. The counter has no name by default.
    /// The rate calculation state is initialized to "never called".
    ///
    /// # Examples
    ///
    /// ```rust
    /// use contatori::counters::rate::Rate;
    /// use contatori::counters::Observable;
    ///
    /// let counter = Rate::new();
    /// assert_eq!(counter.total_value(), 0);
    /// ```
    pub const fn new() -> Self {
        const ZERO: CachePadded<AtomicUsize> = CachePadded::new(AtomicUsize::new(0));
        Rate {
            name: "",
            components: [ZERO; NUM_COMPONENTS],
            last_value: AtomicU64::new(0),
            last_instant: AtomicOptionInstant::none(),
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
    /// use contatori::counters::rate::Rate;
    /// use contatori::counters::Observable;
    ///
    /// let counter = Rate::new().with_name("http_requests_rate");
    /// assert_eq!(counter.name(), "http_requests_rate");
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
    /// use contatori::counters::rate::Rate;
    ///
    /// let counter = Rate::new();
    /// counter.add(5);
    /// counter.add(3);
    /// assert_eq!(counter.total_value(), 8);
    /// ```
    #[inline]
    pub fn add(&self, value: usize) {
        self.get_component_counter()
            .fetch_add(value, Ordering::Relaxed);
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
    ///
    /// This returns the absolute counter value, not the rate.
    #[inline]
    pub fn total_value(&self) -> usize {
        self.components
            .iter()
            .map(|counter| counter.load(Ordering::Relaxed))
            .sum()
    }

    /// Calculates and returns the rate of change (units per second).
    ///
    /// On the first call, this returns `0.0` and records the current value
    /// and timestamp. On subsequent calls, it returns the rate of change
    /// since the last call.
    ///
    /// # Rate Calculation
    ///
    /// ```text
    /// rate = (current_value - last_value) / elapsed_seconds
    /// ```
    ///
    /// # Examples
    ///
    /// ```rust
    /// use contatori::counters::rate::Rate;
    /// use std::thread;
    /// use std::time::Duration;
    ///
    /// let counter = Rate::new();
    ///
    /// // First call: returns 0.0, records baseline
    /// assert_eq!(counter.rate(), 0.0);
    ///
    /// // Add values and wait
    /// counter.add(1000);
    /// thread::sleep(Duration::from_millis(100));
    ///
    /// // Second call: returns rate ≈ 1000 / 0.1 = ~10000/sec
    /// let rate = counter.rate();
    /// assert!(rate > 0.0);
    /// ```
    pub fn rate(&self) -> f64 {
        let now = Instant::now();
        let current_value = self.total_value() as u64;

        match self.last_instant.load(Ordering::Relaxed) {
            Some(last_time) => {
                // Calculate elapsed time
                let elapsed = now.duration_since(last_time);
                let elapsed_secs = elapsed.as_secs_f64();

                // Get the last value and update it atomically
                let last_val = self.last_value.swap(current_value, Ordering::Relaxed);

                // Update the timestamp
                self.last_instant.store(Some(now), Ordering::Relaxed);

                // Calculate rate (handle zero elapsed time)
                if elapsed_secs > 0.0 {
                    let delta = current_value.saturating_sub(last_val);
                    delta as f64 / elapsed_secs
                } else {
                    0.0
                }
            }
            None => {
                // First call: record baseline and return 0.0
                self.last_value.store(current_value, Ordering::Relaxed);
                self.last_instant.store(Some(now), Ordering::Relaxed);
                0.0
            }
        }
    }
}

impl Observable for Rate {
    /// Returns the current rate as a float value.
    ///
    /// Note: Each call to `value()` updates the rate calculation state.
    #[inline]
    fn value(&self) -> CounterValue {
        CounterValue::Float(self.rate())
    }

    /// Returns the name of this counter.
    #[inline]
    fn name(&self) -> &'static str {
        self.name
    }

    /// Returns [`MetricKind::Gauge`] because rates can increase or decrease.
    #[inline]
    fn metric_kind(&self) -> MetricKind {
        MetricKind::Gauge
    }

    /// Expands this rate counter into observable entries.
    fn expand(&self) -> Vec<ObservableEntry> {
        vec![ObservableEntry {
            name: self.name(),
            label: None,
            value: self.value(),
            metric_kind: self.metric_kind(),
        }]
    }
}

impl sealed::Resettable for Rate {
    /// Returns the current rate. Rate counters maintain their state.
    #[inline]
    fn value_and_reset(&self) -> CounterValue {
        CounterValue::Float(self.rate())
    }
}

impl Default for Rate {
    /// Creates a new counter initialized to zero with no name.
    fn default() -> Self {
        Self::new()
    }
}

impl Debug for Rate {
    /// Formats the counter showing non-zero shards and rate state.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}{{", self.name)?;
        for (i, counter) in self.components.iter().enumerate() {
            let val = counter.load(Ordering::Relaxed);
            if val != 0 {
                write!(f, " [{i}]:{val}")?;
            }
        }
        write!(
            f,
            " | last_value:{} }}",
            self.last_value.load(Ordering::Relaxed)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_new() {
        let counter = Rate::new();
        assert_eq!(counter.total_value(), 0);
    }

    #[test]
    fn test_const_new() {
        static COUNTER: Rate = Rate::new();
        COUNTER.add(1);
        assert!(COUNTER.total_value() >= 1);
    }

    #[test]
    fn test_with_name() {
        let counter = Rate::new().with_name("my_rate");
        assert_eq!(counter.name(), "my_rate");
    }

    #[test]
    fn test_add() {
        let counter = Rate::new();
        counter.add(1);
        assert_eq!(counter.total_value(), 1);
        counter.add(5);
        assert_eq!(counter.total_value(), 6);
    }

    #[test]
    fn test_local_value() {
        let counter = Rate::new();
        assert_eq!(counter.local_value(), 0);
        counter.add(1);
        assert_eq!(counter.local_value(), 1);
    }

    #[test]
    fn test_first_rate_call_returns_zero() {
        let counter = Rate::new();
        counter.add(100);
        // First call should return 0.0 regardless of current value
        assert_eq!(counter.rate(), 0.0);
    }

    #[test]
    fn test_rate_calculation() {
        let counter = Rate::new();

        // First call: baseline
        let _ = counter.rate();

        // Add values and wait
        counter.add(1000);
        thread::sleep(Duration::from_millis(50));

        // Second call: should have positive rate
        let rate = counter.rate();
        assert!(rate > 0.0, "Rate should be positive, got {}", rate);

        // Rate should be approximately 1000 / 0.05 = 20000/sec
        // Allow for timing variations
        assert!(
            rate > 5000.0 && rate < 50000.0,
            "Rate {} is outside expected range",
            rate
        );
    }

    #[test]
    fn test_rate_with_no_change() {
        let counter = Rate::new();
        counter.add(100);

        // First call: baseline
        let _ = counter.rate();

        // Wait without adding more values
        thread::sleep(Duration::from_millis(10));

        // Rate should be 0 (no change)
        let rate = counter.rate();
        assert_eq!(rate, 0.0);
    }

    #[test]
    fn test_observable_impl() {
        let counter = Rate::new().with_name("requests_rate");

        assert_eq!(counter.name(), "requests_rate");
        assert_eq!(counter.metric_kind(), MetricKind::Gauge);
    }

    #[test]
    fn test_multiple_threads() {
        let counter = Arc::new(Rate::new());
        let mut handles = vec![];

        for _ in 0..4 {
            let c = Arc::clone(&counter);
            handles.push(thread::spawn(move || {
                for _ in 0..100 {
                    c.add(1);
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(counter.total_value(), 400);
    }

    #[test]
    fn test_debug() {
        let counter = Rate::new().with_name("test");
        counter.add(42);
        let debug_str = format!("{:?}", counter);
        assert!(debug_str.contains("test"));
        assert!(debug_str.contains("42"));
    }

    #[test]
    fn test_default() {
        let counter = Rate::default();
        assert_eq!(counter.total_value(), 0);
        assert_eq!(counter.name(), "");
    }

    #[test]
    fn test_expand() {
        let counter = Rate::new().with_name("test_rate");
        let entries = counter.expand();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "test_rate");
        assert!(entries[0].label.is_none());
        assert_eq!(entries[0].metric_kind, MetricKind::Gauge);
    }

    #[test]
    fn test_dyn_format() {
        let counter = Rate::new().with_name("test_counter");
        // Initialize rate state
        let _ = counter.rate();
        counter.add(1);
        thread::sleep(Duration::from_millis(10));

        let formatted = format!("{}", &counter as &dyn Observable);
        assert!(formatted.starts_with("test_counter:"));
    }
}
