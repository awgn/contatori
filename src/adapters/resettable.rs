//! Resettable wrapper for counters that reset on observation.
//!
//! This module provides [`Resettable`], a wrapper that resets counters
//! when `value()` is called. This is useful for evaluating metrics
//! over observation periods (e.g., requests per second, average latency
//! since last check).
//!
//! # Example
//!
//! ```rust
//! use contatori::counters::unsigned::Unsigned;
//! use contatori::counters::Observable;
//! use contatori::adapters::Resettable;
//!
//! let counter = Resettable::new(Unsigned::new().with_name("requests_per_period"));
//! counter.add(100);
//!
//! // value() returns the value AND resets the counter
//! assert_eq!(counter.value().as_u64(), 100);
//! assert_eq!(counter.value().as_u64(), 0); // Reset to 0!
//! ```

use crate::counters::{sealed, CounterValue, MetricKind, Observable, ObservableEntry};
use std::fmt::{self, Debug};
use std::ops::Deref;

/// A wrapper that resets a counter when `value()` is called.
///
/// When `value()` is called on a `Resettable` counter, it returns
/// the current value and resets the underlying counter. This is useful
/// for:
///
/// - **Per-period metrics**: Measuring requests/errors/etc. per observation period
/// - **Delta tracking**: Getting the change since last observation
/// - **Periodic reporting**: Collecting and resetting metrics atomically
///
/// # Example
///
/// ```rust
/// use contatori::counters::unsigned::Unsigned;
/// use contatori::counters::Observable;
/// use contatori::adapters::Resettable;
///
/// // Create a resettable counter
/// let requests = Resettable::new(Unsigned::new().with_name("requests"));
/// requests.add(50);
/// requests.add(50);
///
/// // Calling value() returns the value AND resets
/// let v = requests.value();
/// assert_eq!(v.as_u64(), 100);
///
/// // Counter is now reset to 0
/// assert_eq!(requests.value().as_u64(), 0);
///
/// // Add more
/// requests.add(25);
/// assert_eq!(requests.value().as_u64(), 25);
/// ```
///
/// # Using with Observers
///
/// When using observers, `Resettable` counters will be reset after each
/// `render()` call:
///
/// ```rust,ignore
/// use contatori::adapters::Resettable;
/// use contatori::counters::unsigned::Unsigned;
/// use contatori::observers::table::TableObserver;
///
/// let period_requests = Resettable::new(Unsigned::new().with_name("period_requests"));
/// let total_requests = Unsigned::new().with_name("total_requests");
///
/// period_requests.add(100);
/// total_requests.add(100);
///
/// let counters: Vec<&dyn Observable> = vec![&period_requests, &total_requests];
/// let observer = TableObserver::new();
///
/// // First render
/// observer.render(counters.clone().into_iter());
/// // period_requests: 100 (then reset to 0), total_requests: 100
///
/// period_requests.add(50);
/// total_requests.add(50);
///
/// // Second render
/// observer.render(counters.into_iter());
/// // period_requests: 50 (just this period), total_requests: 150 (cumulative)
/// ```
pub struct Resettable<T> {
    inner: T,
}

impl<T> Resettable<T> {
    /// Creates a new resettable wrapper around the given counter.
    ///
    /// # Example
    ///
    /// ```rust
    /// use contatori::counters::unsigned::Unsigned;
    /// use contatori::adapters::Resettable;
    ///
    /// let counter = Resettable::new(Unsigned::new().with_name("periodic"));
    /// ```
    pub const fn new(inner: T) -> Self {
        Self { inner }
    }

    /// Returns a reference to the inner counter.
    ///
    /// # Example
    ///
    /// ```rust
    /// use contatori::counters::unsigned::Unsigned;
    /// use contatori::adapters::Resettable;
    ///
    /// let counter = Resettable::new(Unsigned::new().with_name("test"));
    /// let inner: &Unsigned = counter.inner();
    /// ```
    pub fn inner(&self) -> &T {
        &self.inner
    }

    /// Returns a mutable reference to the inner counter.
    pub fn inner_mut(&mut self) -> &mut T {
        &mut self.inner
    }

    /// Consumes the wrapper and returns the inner counter.
    ///
    /// # Example
    ///
    /// ```rust
    /// use contatori::counters::unsigned::Unsigned;
    /// use contatori::adapters::Resettable;
    ///
    /// let counter = Resettable::new(Unsigned::new());
    /// let inner: Unsigned = counter.into_inner();
    /// ```
    pub fn into_inner(self) -> T {
        self.inner
    }
}

impl<T: sealed::Resettable> Observable for Resettable<T> {
    /// Returns the name of the underlying counter.
    fn name(&self) -> &'static str {
        self.inner.name()
    }

    /// Returns the current value AND resets the counter.
    ///
    /// This is the key behavior of `Resettable`: the counter is reset
    /// to its initial state after the value is read.
    fn value(&self) -> CounterValue {
        self.inner.value_and_reset()
    }

    /// Returns the metric kind of the underlying counter.
    ///
    /// Delegates to the inner counter's `metric_kind()` method.
    fn metric_kind(&self) -> MetricKind {
        self.inner.metric_kind()
    }

    /// Expands this observable into entries, using reset values.
    ///
    /// For resettable counters, each entry's value is read-and-reset.
    fn expand(&self) -> Vec<ObservableEntry> {
        // For a simple resettable counter, return one entry with the reset value
        vec![ObservableEntry {
            name: self.inner.name(),
            label: None,
            value: self.inner.value_and_reset(),
            metric_kind: self.inner.metric_kind(),
        }]
    }
}

impl<T: Debug> Debug for Resettable<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Resettable")
            .field("inner", &self.inner)
            .finish()
    }
}

/// Allows transparent access to the inner counter's methods.
impl<T> Deref for Resettable<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

// Note: We intentionally don't implement DerefMut to prevent
// accidental mutation that could bypass the resettable behavior.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::counters::signed::Signed;
    use crate::counters::unsigned::Unsigned;

    #[test]
    fn test_new() {
        let counter = Resettable::new(Unsigned::new().with_name("test"));
        assert_eq!(counter.name(), "test");
    }

    #[test]
    fn test_value_resets() {
        let counter = Resettable::new(Unsigned::new());
        counter.add(42);

        // First read returns 42 and resets
        assert_eq!(counter.value(), CounterValue::Unsigned(42));

        // Second read returns 0 (counter was reset)
        assert_eq!(counter.value(), CounterValue::Unsigned(0));
    }

    #[test]
    fn test_multiple_adds_then_reset() {
        let counter = Resettable::new(Unsigned::new());
        counter.add(10);
        counter.add(20);
        counter.add(30);

        // Should get total, then reset
        assert_eq!(counter.value(), CounterValue::Unsigned(60));
        assert_eq!(counter.value(), CounterValue::Unsigned(0));
    }

    #[test]
    fn test_accumulates_after_reset() {
        let counter = Resettable::new(Unsigned::new());
        counter.add(100);

        // Reset by reading
        let _ = counter.value();

        // Now add more
        counter.add(50);
        assert_eq!(counter.value(), CounterValue::Unsigned(50));
    }

    #[test]
    fn test_with_signed_counter() {
        let counter = Resettable::new(Signed::new().with_name("balance"));
        counter.add(100);
        counter.sub(30);

        assert_eq!(counter.value(), CounterValue::Signed(70));
        // After reset
        assert_eq!(counter.value(), CounterValue::Signed(0));
    }

    #[test]
    fn test_deref() {
        let counter = Resettable::new(Unsigned::new());
        // Can call Unsigned methods directly through Deref
        counter.add(10);
        counter.add(20);
        // Note: using inner's value method directly won't reset
        // We need to use Resettable's value()
        assert_eq!(counter.value().as_u64(), 30);
    }

    #[test]
    fn test_inner() {
        let counter = Resettable::new(Unsigned::new().with_name("inner_test"));
        assert_eq!(counter.inner().name(), "inner_test");
    }

    #[test]
    fn test_into_inner() {
        let counter = Resettable::new(Unsigned::new().with_name("consume"));
        counter.add(42);

        // Read and reset first
        let _ = counter.value();

        // Add more
        let counter = Resettable::new(Unsigned::new().with_name("consume"));
        counter.add(100);

        let inner = counter.into_inner();
        assert_eq!(inner.name(), "consume");
        // Inner still has the value (wasn't reset because we didn't call value())
        assert_eq!(inner.value(), CounterValue::Unsigned(100));
    }

    #[test]
    fn test_debug() {
        let counter = Resettable::new(Unsigned::new().with_name("debug_test"));
        let debug_str = format!("{:?}", counter);
        assert!(debug_str.contains("Resettable"));
    }
}
