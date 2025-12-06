//! Labeled wrapper for adding tags/dimensions to counters.
//!
//! This module provides [`Labeled`], a wrapper that adds key-value labels
//! (also known as tags or dimensions) to a counter. This is particularly
//! useful for Prometheus-style metrics where labels are used to distinguish
//! between different instances of the same metric.
//!
//! # Example
//!
//! ```rust
//! use contatori::counters::unsigned::Unsigned;
//! use contatori::counters::Observable;
//! use contatori::adapters::Labeled;
//!
//! let counter = Labeled::new(Unsigned::new().with_name("http_requests"))
//!     .with_label("method", "GET")
//!     .with_label("path", "/api/users")
//!     .with_label("status", "200");
//!
//! counter.add(100);
//!
//! // Access labels via Observable trait
//! for (key, value) in counter.labels() {
//!     println!("{}: {}", key, value);
//! }
//! ```

use crate::counters::{sealed, CounterValue, MetricKind, Observable};
use std::fmt::{self, Debug};
use std::ops::Deref;

/// A wrapper that adds labels (key-value tags) to a counter.
///
/// Labels are useful for:
///
/// - **Prometheus metrics**: Labels are exported as metric dimensions
/// - **Filtering and grouping**: Query metrics by label values
/// - **Multi-dimensional metrics**: Same metric name with different label combinations
///
/// # Example
///
/// ```rust
/// use contatori::counters::unsigned::Unsigned;
/// use contatori::counters::Observable;
/// use contatori::adapters::Labeled;
///
/// // Create a labeled counter
/// let requests = Labeled::new(Unsigned::new().with_name("http_requests"))
///     .with_label("method", "POST")
///     .with_label("endpoint", "/api/submit");
///
/// requests.add(1);
///
/// // Check labels
/// assert_eq!(requests.get_label("method"), Some("POST"));
/// assert_eq!(requests.get_label("endpoint"), Some("/api/submit"));
/// ```
///
/// # Use with Prometheus Observer
///
/// ```rust,ignore
/// use contatori::adapters::Labeled;
/// use contatori::counters::unsigned::Unsigned;
/// use contatori::observers::prometheus::PrometheusObserver;
///
/// let get_requests = Labeled::new(Unsigned::new().with_name("http_requests"))
///     .with_label("method", "GET");
///
/// let post_requests = Labeled::new(Unsigned::new().with_name("http_requests"))
///     .with_label("method", "POST");
///
/// get_requests.add(100);
/// post_requests.add(50);
///
/// // Prometheus output will show:
/// // http_requests{method="GET"} 100
/// // http_requests{method="POST"} 50
/// ```
pub struct Labeled<T> {
    inner: T,
    labels: Vec<(String, String)>,
}

impl<T> Labeled<T> {
    /// Creates a new labeled wrapper around the given counter.
    ///
    /// # Example
    ///
    /// ```rust
    /// use contatori::counters::unsigned::Unsigned;
    /// use contatori::adapters::Labeled;
    ///
    /// let counter = Labeled::new(Unsigned::new().with_name("requests"));
    /// ```
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            labels: Vec::new(),
        }
    }

    /// Creates a new labeled wrapper with pre-defined labels.
    ///
    /// # Example
    ///
    /// ```rust
    /// use contatori::counters::unsigned::Unsigned;
    /// use contatori::adapters::Labeled;
    ///
    /// let labels = vec![
    ///     ("env".to_string(), "production".to_string()),
    ///     ("region".to_string(), "us-east".to_string()),
    /// ];
    ///
    /// let counter = Labeled::with_labels(Unsigned::new(), labels);
    /// ```
    pub fn with_labels(inner: T, labels: Vec<(String, String)>) -> Self {
        Self { inner, labels }
    }

    /// Adds a label to the counter.
    ///
    /// If the label already exists, its value is updated.
    ///
    /// # Example
    ///
    /// ```rust
    /// use contatori::counters::unsigned::Unsigned;
    /// use contatori::adapters::Labeled;
    ///
    /// let counter = Labeled::new(Unsigned::new())
    ///     .with_label("region", "us-east-1")
    ///     .with_label("instance", "i-1234");
    /// ```
    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        let key = key.into();
        let value = value.into();

        // Update existing label or add new one
        if let Some(pos) = self.labels.iter().position(|(k, _)| k == &key) {
            self.labels[pos].1 = value;
        } else {
            self.labels.push((key, value));
        }
        self
    }

    /// Adds a label to an existing counter (non-builder pattern).
    ///
    /// # Example
    ///
    /// ```rust
    /// use contatori::counters::unsigned::Unsigned;
    /// use contatori::adapters::Labeled;
    ///
    /// let mut counter = Labeled::new(Unsigned::new());
    /// counter.add_label("key", "value");
    /// ```
    pub fn add_label(&mut self, key: impl Into<String>, value: impl Into<String>) {
        let key = key.into();
        let value = value.into();

        if let Some(pos) = self.labels.iter().position(|(k, _)| k == &key) {
            self.labels[pos].1 = value;
        } else {
            self.labels.push((key, value));
        }
    }

    /// Removes a label from the counter.
    ///
    /// Returns the previous value if the label existed.
    ///
    /// # Example
    ///
    /// ```rust
    /// use contatori::counters::unsigned::Unsigned;
    /// use contatori::adapters::Labeled;
    ///
    /// let mut counter = Labeled::new(Unsigned::new())
    ///     .with_label("temp", "value");
    ///
    /// counter.remove_label("temp");
    /// assert!(counter.get_label("temp").is_none());
    /// ```
    pub fn remove_label(&mut self, key: &str) -> Option<String> {
        if let Some(pos) = self.labels.iter().position(|(k, _)| k == key) {
            Some(self.labels.remove(pos).1)
        } else {
            None
        }
    }

    /// Returns the value of a label, if it exists.
    ///
    /// # Example
    ///
    /// ```rust
    /// use contatori::counters::unsigned::Unsigned;
    /// use contatori::adapters::Labeled;
    ///
    /// let counter = Labeled::new(Unsigned::new())
    ///     .with_label("env", "prod");
    ///
    /// assert_eq!(counter.get_label("env"), Some("prod"));
    /// assert_eq!(counter.get_label("missing"), None);
    /// ```
    pub fn get_label(&self, key: &str) -> Option<&str> {
        self.labels
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// Returns an iterator over all labels as (&str, &str) tuples.
    ///
    /// # Example
    ///
    /// ```rust
    /// use contatori::counters::unsigned::Unsigned;
    /// use contatori::adapters::Labeled;
    ///
    /// let counter = Labeled::new(Unsigned::new())
    ///     .with_label("a", "1")
    ///     .with_label("b", "2");
    ///
    /// for (key, value) in counter.labels_iter() {
    ///     println!("{} = {}", key, value);
    /// }
    /// ```
    pub fn labels_iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.labels.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// Returns the number of labels.
    pub fn label_count(&self) -> usize {
        self.labels.len()
    }

    /// Returns true if the counter has any labels.
    pub fn has_labels(&self) -> bool {
        !self.labels.is_empty()
    }

    /// Returns a reference to the labels as a slice.
    pub fn labels_slice(&self) -> &[(String, String)] {
        &self.labels
    }

    /// Returns a reference to the inner counter.
    pub fn inner(&self) -> &T {
        &self.inner
    }

    /// Returns a mutable reference to the inner counter.
    pub fn inner_mut(&mut self) -> &mut T {
        &mut self.inner
    }

    /// Consumes the wrapper and returns the inner counter.
    pub fn into_inner(self) -> T {
        self.inner
    }

    /// Consumes the wrapper and returns both the inner counter and labels.
    pub fn into_parts(self) -> (T, Vec<(String, String)>) {
        (self.inner, self.labels)
    }
}

impl<T: Observable> Observable for Labeled<T> {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn value(&self) -> CounterValue {
        self.inner.value()
    }

    fn labels(&self) -> &[(String, String)] {
        &self.labels
    }

    /// Returns the metric kind of the underlying counter.
    ///
    /// Delegates to the inner counter's `metric_kind()` method.
    fn metric_kind(&self) -> MetricKind {
        self.inner.metric_kind()
    }
}

impl<T: sealed::Resettable> sealed::Resettable for Labeled<T> {
    fn value_and_reset(&self) -> CounterValue {
        self.inner.value_and_reset()
    }
}

impl<T: Debug> Debug for Labeled<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Labeled")
            .field("inner", &self.inner)
            .field("labels", &self.labels)
            .finish()
    }
}

impl<T> Deref for Labeled<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::counters::unsigned::Unsigned;

    #[test]
    fn test_new() {
        let counter = Labeled::new(Unsigned::new().with_name("test"));
        assert_eq!(counter.name(), "test");
        assert!(!counter.has_labels());
    }

    #[test]
    fn test_with_label() {
        let counter = Labeled::new(Unsigned::new())
            .with_label("method", "GET")
            .with_label("path", "/api");

        assert_eq!(counter.get_label("method"), Some("GET"));
        assert_eq!(counter.get_label("path"), Some("/api"));
        assert_eq!(counter.label_count(), 2);
    }

    #[test]
    fn test_add_label() {
        let mut counter = Labeled::new(Unsigned::new());
        counter.add_label("key", "value");

        assert_eq!(counter.get_label("key"), Some("value"));
    }

    #[test]
    fn test_remove_label() {
        let mut counter = Labeled::new(Unsigned::new()).with_label("key", "value");

        let removed = counter.remove_label("key");
        assert_eq!(removed, Some("value".to_string()));
        assert!(counter.get_label("key").is_none());
    }

    #[test]
    fn test_labels_iter() {
        let counter = Labeled::new(Unsigned::new())
            .with_label("a", "1")
            .with_label("b", "2");

        let labels: Vec<_> = counter.labels_iter().collect();
        assert_eq!(labels.len(), 2);
        assert!(labels.contains(&("a", "1")));
        assert!(labels.contains(&("b", "2")));
    }

    #[test]
    fn test_observable_labels() {
        let counter = Labeled::new(Unsigned::new())
            .with_label("a", "1")
            .with_label("b", "2");

        let labels = counter.labels();
        assert_eq!(labels.len(), 2);
    }

    #[test]
    fn test_with_labels() {
        let labels = vec![
            ("env".to_string(), "prod".to_string()),
            ("region".to_string(), "us-east".to_string()),
        ];

        let counter = Labeled::with_labels(Unsigned::new(), labels);
        assert_eq!(counter.label_count(), 2);
    }

    #[test]
    fn test_value() {
        let counter = Labeled::new(Unsigned::new()).with_label("test", "value");

        counter.add(42);
        assert_eq!(counter.value(), CounterValue::Unsigned(42));
    }

    #[test]
    fn test_resettable() {
        use crate::adapters::Resettable;
        let counter =
            Resettable::new(Labeled::new(Unsigned::new()).with_label("test", "value"));

        counter.add(100);
        assert_eq!(counter.value(), CounterValue::Unsigned(100));
        // After value() the counter should be reset
        assert_eq!(counter.value(), CounterValue::Unsigned(0));
    }

    #[test]
    fn test_deref() {
        let counter = Labeled::new(Unsigned::new());
        counter.add(10);
        counter.add(20);
        assert_eq!(counter.value().as_u64(), 30);
    }

    #[test]
    fn test_inner() {
        let counter = Labeled::new(Unsigned::new().with_name("inner_test"));
        assert_eq!(counter.inner().name(), "inner_test");
    }

    #[test]
    fn test_into_inner() {
        let counter = Labeled::new(Unsigned::new().with_name("consume")).with_label("key", "value");
        counter.add(42);

        let inner = counter.into_inner();
        assert_eq!(inner.name(), "consume");
        assert_eq!(inner.value(), CounterValue::Unsigned(42));
    }

    #[test]
    fn test_into_parts() {
        let counter = Labeled::new(Unsigned::new()).with_label("key", "value");
        counter.add(42);

        let (inner, labels) = counter.into_parts();
        assert_eq!(inner.value(), CounterValue::Unsigned(42));
        assert_eq!(labels.len(), 1);
        assert_eq!(labels[0], ("key".to_string(), "value".to_string()));
    }

    #[test]
    fn test_debug() {
        let counter =
            Labeled::new(Unsigned::new().with_name("debug_test")).with_label("key", "value");

        let debug_str = format!("{:?}", counter);
        assert!(debug_str.contains("Labeled"));
        assert!(debug_str.contains("key"));
    }

    #[test]
    fn test_label_update() {
        let counter = Labeled::new(Unsigned::new())
            .with_label("key", "old")
            .with_label("key", "new");

        assert_eq!(counter.get_label("key"), Some("new"));
        assert_eq!(counter.label_count(), 1);
    }

    #[test]
    fn test_has_labels() {
        let counter1 = Labeled::new(Unsigned::new());
        assert!(!counter1.has_labels());

        let counter2 = Labeled::new(Unsigned::new()).with_label("key", "value");
        assert!(counter2.has_labels());
    }

    #[test]
    fn test_labels_order_preserved() {
        let counter = Labeled::new(Unsigned::new())
            .with_label("c", "3")
            .with_label("a", "1")
            .with_label("b", "2");

        let labels: Vec<_> = counter.labels_iter().collect();
        assert_eq!(labels[0], ("c", "3"));
        assert_eq!(labels[1], ("a", "1"));
        assert_eq!(labels[2], ("b", "2"));
    }
}