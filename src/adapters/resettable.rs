//! Resettable wrapper for counters and labeled groups that reset on observation.
//!
//! When wrapped in [`Resettable`], calling [`Observable::value()`],
//! [`Observable::try_value()`], or [`Observable::expand_into()`] reads the current
//! value(s) and resets the underlying shards for the next observation period.

use crate::counters::{CounterValue, MetricKind, Observable, ObservableEntry, sealed};
use std::fmt::{self, Debug};
use std::ops::Deref;

/// A wrapper that resets an underlying counter (or [`labeled_group!`](crate::labeled_group))
/// when observed.
///
/// # Examples
///
/// ```rust
/// use contatori::{Observable, Resettable, Unsigned};
///
/// let requests = Resettable::new(Unsigned::new().with_name("requests_per_period"));
/// requests.add(50);
/// requests.add(50);
///
/// assert_eq!(requests.value().as_u64(), 100);
/// assert_eq!(requests.value().as_u64(), 0); // Reset!
/// ```
pub struct Resettable<T> {
    inner: T,
}

impl<T> Resettable<T> {
    /// Creates a new resettable wrapper around `inner`.
    #[must_use]
    pub const fn new(inner: T) -> Self {
        Self { inner }
    }

    /// Returns a shared reference to the inner counter or group.
    #[must_use]
    #[inline]
    pub const fn inner(&self) -> &T {
        &self.inner
    }

    /// Returns a mutable reference to the inner counter or group.
    #[inline]
    pub fn inner_mut(&mut self) -> &mut T {
        &mut self.inner
    }

    /// Consumes the wrapper and returns the inner counter or group.
    #[must_use]
    #[inline]
    pub fn into_inner(self) -> T {
        self.inner
    }
}

impl<T: Default> Default for Resettable<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T: sealed::Resettable> Observable for Resettable<T> {
    #[inline]
    fn name(&self) -> &'static str {
        self.inner.name()
    }

    #[inline]
    fn metric_kind(&self) -> MetricKind {
        self.inner.metric_kind()
    }

    #[inline]
    fn value(&self) -> CounterValue {
        self.inner.value_and_reset()
    }

    #[inline]
    fn try_value(&self) -> Option<CounterValue> {
        self.inner.try_value_and_reset()
    }

    #[inline]
    fn expand_into(&self, out: &mut Vec<ObservableEntry>) {
        self.inner.expand_and_reset_into(out);
    }
}

impl<T: sealed::Resettable> sealed::Resettable for Resettable<T> {
    #[inline]
    fn value_and_reset(&self) -> CounterValue {
        self.inner.value_and_reset()
    }

    #[inline]
    fn try_value_and_reset(&self) -> Option<CounterValue> {
        self.inner.try_value_and_reset()
    }

    #[inline]
    fn expand_and_reset_into(&self, out: &mut Vec<ObservableEntry>) {
        self.inner.expand_and_reset_into(out);
    }
}

impl<T: Debug> Debug for Resettable<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Resettable")
            .field("inner", &self.inner)
            .finish()
    }
}

impl<T> Deref for Resettable<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::counters::minimum::Minimum;
    use crate::counters::signed::Signed;
    use crate::counters::unsigned::Unsigned;

    #[test]
    fn test_value_resets() {
        let counter = Resettable::new(Unsigned::new().with_name("test"));
        assert_eq!(counter.name(), "test");
        counter.add(42);
        assert_eq!(counter.value(), CounterValue::Unsigned(42));
        assert_eq!(counter.value(), CounterValue::Unsigned(0));
    }

    #[test]
    fn test_try_value_resets_tracker() {
        let tracker = Resettable::new(Minimum::new().with_name("lat_min"));
        assert_eq!(tracker.try_value(), None);
        tracker.observe(25);
        assert_eq!(tracker.try_value(), Some(CounterValue::Unsigned(25)));
        assert_eq!(tracker.try_value(), None);
    }

    #[test]
    fn test_with_signed_counter() {
        let counter = Resettable::new(Signed::new().with_name("balance"));
        counter.add(100);
        counter.sub(30);
        assert_eq!(counter.value(), CounterValue::Signed(70));
        assert_eq!(counter.value(), CounterValue::Signed(0));
    }

    #[test]
    fn test_into_inner_and_debug() {
        let counter = Resettable::new(Unsigned::new().with_name("consume"));
        counter.add(100);
        assert!(format!("{counter:?}").contains("Resettable"));
        let inner = counter.into_inner();
        assert_eq!(inner.value(), CounterValue::Unsigned(100));
    }
}
