//! Non-resettable wrapper for monotonic counters.
//!
//! This module provides [`NonResettable`], a wrapper that prevents counters
//! from being reset when `value_and_reset()` is called. This is useful for
//! monotonic counters that should never decrease, such as Prometheus counters.
//!
//! # Example
//!
//! ```rust
//! use contatori::counters::unsigned::Unsigned;
//! use contatori::counters::Observable;
//! use contatori::adapters::NonResettable;
//!
//! let counter = NonResettable::new(Unsigned::new().with_name("total_requests"));
//! counter.add(100);
//!
//! // value_and_reset() returns the value but does NOT reset
//! assert_eq!(counter.value_and_reset().as_u64(), 100);
//! assert_eq!(counter.value().as_u64(), 100); // Still 100!
//! ```

use crate::counters::{CounterValue, MetricKind, Observable};
use std::fmt::{self, Debug};
use std::ops::Deref;

/// A wrapper that prevents a counter from being reset.
///
/// When `value_and_reset()` is called on a `NonResettable` counter, it returns
/// the current value but does not reset the underlying counter. This is useful
/// for:
///
/// - **Prometheus counters**: Counters that must be monotonically increasing
/// - **Total counts**: Metrics where you want the all-time total
/// - **Mixed reset scenarios**: When some counters in a collection should reset
///   and others should not
///
/// # Example
///
/// ```rust
/// use contatori::counters::unsigned::Unsigned;
/// use contatori::counters::Observable;
/// use contatori::adapters::NonResettable;
///
/// // Create a non-resettable counter
/// let total = NonResettable::new(Unsigned::new().with_name("total_events"));
/// total.add(50);
/// total.add(50);
///
/// // Calling value_and_reset returns the value...
/// let v = total.value_and_reset();
/// assert_eq!(v.as_u64(), 100);
///
/// // ...but the counter is NOT reset
/// assert_eq!(total.value().as_u64(), 100);
///
/// // Add more
/// total.add(25);
/// assert_eq!(total.value().as_u64(), 125);
/// ```
///
/// # Using with Observers
///
/// When using `render_and_reset()` on an observer, non-resettable counters
/// will report their values but continue accumulating:
///
/// ```rust,ignore
/// use contatori::adapters::NonResettable;
/// use contatori::counters::unsigned::Unsigned;
/// use contatori::observers::table::TableObserver;
///
/// let total = NonResettable::new(Unsigned::new().with_name("total"));
/// let period = Unsigned::new().with_name("period");
///
/// total.add(100);
/// period.add(100);
///
/// let counters: Vec<&dyn Observable> = vec![&total, &period];
/// let observer = TableObserver::new();
///
/// // First render_and_reset
/// observer.render_and_reset(counters.clone().into_iter());
/// // total: 100 (not reset), period: 100 (reset to 0)
///
/// total.add(50);
/// period.add(50);
///
/// // Second render_and_reset
/// observer.render_and_reset(counters.into_iter());
/// // total: 150 (cumulative), period: 50 (just this period)
/// ```
pub struct NonResettable<T> {
    inner: T,
}

impl<T> NonResettable<T> {
    /// Creates a new non-resettable wrapper around the given counter.
    ///
    /// # Example
    ///
    /// ```rust
    /// use contatori::counters::unsigned::Unsigned;
    /// use contatori::adapters::NonResettable;
    ///
    /// let counter = NonResettable::new(Unsigned::new().with_name("total"));
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
    /// use contatori::adapters::NonResettable;
    ///
    /// let counter = NonResettable::new(Unsigned::new().with_name("test"));
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
    /// use contatori::adapters::NonResettable;
    ///
    /// let counter = NonResettable::new(Unsigned::new());
    /// let inner: Unsigned = counter.into_inner();
    /// ```
    pub fn into_inner(self) -> T {
        self.inner
    }
}

impl<T: Observable> Observable for NonResettable<T> {
    /// Returns the name of the underlying counter.
    fn name(&self) -> &str {
        self.inner.name()
    }

    /// Returns the current value of the underlying counter.
    fn value(&self) -> CounterValue {
        self.inner.value()
    }

    /// Returns the current value WITHOUT resetting the counter.
    ///
    /// This is the key difference from the wrapped counter's behavior.
    /// The value is returned, but the counter continues accumulating.
    fn value_and_reset(&self) -> CounterValue {
        // Just return value, don't reset
        self.inner.value()
    }

    /// Returns the metric kind of the underlying counter.
    ///
    /// Delegates to the inner counter's `metric_kind()` method.
    fn metric_kind(&self) -> MetricKind {
        self.inner.metric_kind()
    }
}

impl<T: Debug> Debug for NonResettable<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NonResettable")
            .field("inner", &self.inner)
            .finish()
    }
}

/// Allows transparent access to the inner counter's methods.
impl<T> Deref for NonResettable<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

// Note: We intentionally don't implement DerefMut to prevent
// accidental mutation that could bypass the non-resettable behavior.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::counters::signed::Signed;
    use crate::counters::unsigned::Unsigned;

    #[test]
    fn test_new() {
        let counter = NonResettable::new(Unsigned::new().with_name("test"));
        assert_eq!(counter.name(), "test");
    }

    #[test]
    fn test_value() {
        let counter = NonResettable::new(Unsigned::new());
        counter.add(42);
        assert_eq!(counter.value(), CounterValue::Unsigned(42));
    }

    #[test]
    fn test_value_and_reset_does_not_reset() {
        let counter = NonResettable::new(Unsigned::new());
        counter.add(100);

        // First call
        let v1 = counter.value_and_reset();
        assert_eq!(v1, CounterValue::Unsigned(100));

        // Value should still be 100
        assert_eq!(counter.value(), CounterValue::Unsigned(100));

        // Second call should also return 100
        let v2 = counter.value_and_reset();
        assert_eq!(v2, CounterValue::Unsigned(100));
    }

    #[test]
    fn test_accumulates_after_value_and_reset() {
        let counter = NonResettable::new(Unsigned::new());
        counter.add(100);

        counter.value_and_reset();

        counter.add(50);
        assert_eq!(counter.value(), CounterValue::Unsigned(150));
    }

    #[test]
    fn test_with_signed_counter() {
        let counter = NonResettable::new(Signed::new().with_name("balance"));
        counter.add(100);
        counter.sub(30);

        assert_eq!(counter.value(), CounterValue::Signed(70));
        assert_eq!(counter.value_and_reset(), CounterValue::Signed(70));
        assert_eq!(counter.value(), CounterValue::Signed(70)); // Not reset
    }

    #[test]
    fn test_deref() {
        let counter = NonResettable::new(Unsigned::new());
        // Can call Unsigned methods directly through Deref
        counter.add(10);
        counter.add(20);
        assert_eq!(counter.value().as_u64(), 30);
    }

    #[test]
    fn test_inner() {
        let counter = NonResettable::new(Unsigned::new().with_name("inner_test"));
        assert_eq!(counter.inner().name(), "inner_test");
    }

    #[test]
    fn test_into_inner() {
        let counter = NonResettable::new(Unsigned::new().with_name("consume"));
        counter.add(42);

        let inner = counter.into_inner();
        assert_eq!(inner.name(), "consume");
        assert_eq!(inner.value(), CounterValue::Unsigned(42));
    }

    #[test]
    fn test_debug() {
        let counter = NonResettable::new(Unsigned::new().with_name("debug_test"));
        let debug_str = format!("{:?}", counter);
        assert!(debug_str.contains("NonResettable"));
    }
}
