//! JSON observer for serializing counters.
//!
//! This module provides [`JsonObserver`], which serializes a collection of
//! [`Observable`] counters to JSON format using serde.
//!
//! # Feature Flag
//!
//! This module requires the `json` feature:
//!
//! ```toml
//! [dependencies]
//! contatori = { version = "0.6", features = ["json"] }
//! ```
//!
//! # Examples
//!
//! ```rust,ignore
//! use contatori::contatori::unsigned::Unsigned;
//! use contatori::contatori::Observable;
//! use contatori::observers::json::JsonObserver;
//!
//! let requests = Unsigned::new().with_name("http_requests");
//! let errors = Unsigned::new().with_name("http_errors");
//!
//! requests.add(1000);
//! errors.add(5);
//!
//! let counters: Vec<&dyn Observable> = vec![&requests, &errors];
//!
//! let observer = JsonObserver::new();
//! let json = observer.to_json(counters.into_iter()).unwrap();
//!
//! println!("{}", json);
//! // [{"name":"http_requests","value":1000},{"name":"http_errors","value":5}]
//! ```

use crate::counters::Observable;

// Re-export snapshot types for backwards compatibility
pub use crate::snapshot::{CounterSnapshot, MetricsSnapshot};

/// Configuration for the JSON observer.
#[derive(Debug, Clone, Default)]
pub struct JsonConfig {
    /// Whether to pretty-print the JSON output.
    pub pretty: bool,
    /// Whether to include a timestamp in the output.
    pub include_timestamp: bool,
    /// Whether to wrap counters in a MetricsSnapshot object.
    pub wrap_in_snapshot: bool,
}

/// An observer that serializes counters to JSON format.
///
/// # Examples
///
/// Basic usage (array of counters):
///
/// ```rust,ignore
/// use contatori::contatori::unsigned::Unsigned;
/// use contatori::contatori::Observable;
/// use contatori::observers::json::JsonObserver;
///
/// let counter = Unsigned::new().with_name("requests");
/// counter.add(42);
///
/// let counters: Vec<&dyn Observable> = vec![&counter];
/// let json = JsonObserver::new().to_json(counters.into_iter()).unwrap();
///
/// assert!(json.contains("requests"));
/// assert!(json.contains("42"));
/// ```
///
/// Pretty-printed output:
///
/// ```rust,ignore
/// use contatori::observers::json::JsonObserver;
///
/// let observer = JsonObserver::new().pretty(true);
/// ```
///
/// With timestamp wrapper:
///
/// ```rust,ignore
/// use contatori::observers::json::JsonObserver;
///
/// let observer = JsonObserver::new()
///     .wrap_in_snapshot(true)
///     .include_timestamp(true);
/// ```
#[derive(Debug, Clone, Default)]
pub struct JsonObserver {
    config: JsonConfig,
}

impl JsonObserver {
    /// Creates a new JSON observer with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a new JSON observer with the specified configuration.
    pub fn with_config(config: JsonConfig) -> Self {
        Self { config }
    }

    /// Enables or disables pretty-printing.
    pub fn pretty(mut self, enabled: bool) -> Self {
        self.config.pretty = enabled;
        self
    }

    /// Enables or disables timestamp inclusion.
    ///
    /// Only has effect when `wrap_in_snapshot` is also enabled.
    pub fn include_timestamp(mut self, enabled: bool) -> Self {
        self.config.include_timestamp = enabled;
        self
    }

    /// Enables or disables wrapping the output in a [`MetricsSnapshot`].
    pub fn wrap_in_snapshot(mut self, enabled: bool) -> Self {
        self.config.wrap_in_snapshot = enabled;
        self
    }

    /// Collects counters into a vector of [`CounterSnapshot`].
    ///
    /// This is useful when you need the intermediate representation
    /// before serialization.
    ///
    /// Uses `expand()` on each counter, so labeled groups will produce
    /// multiple snapshots.
    pub fn collect<'a>(
        &self,
        counters: impl Iterator<Item = &'a dyn Observable>,
    ) -> Vec<CounterSnapshot> {
        counters
            .flat_map(CounterSnapshot::from_observable)
            .collect()
    }

    /// Serializes counters to a JSON string.
    ///
    /// # Arguments
    ///
    /// * `counters` - An iterator over references to [`Observable`] trait objects
    ///
    /// # Returns
    ///
    /// A `Result` containing the JSON string or a serialization error.
    pub fn to_json<'a>(
        &self,
        counters: impl Iterator<Item = &'a dyn Observable>,
    ) -> Result<String, serde_json::Error> {
        let snapshots = self.collect(counters);

        if self.config.wrap_in_snapshot {
            let snapshot = if self.config.include_timestamp {
                MetricsSnapshot::with_timestamp(snapshots, current_timestamp_ms())
            } else {
                MetricsSnapshot::new(snapshots)
            };

            if self.config.pretty {
                serde_json::to_string_pretty(&snapshot)
            } else {
                serde_json::to_string(&snapshot)
            }
        } else if self.config.pretty {
            serde_json::to_string_pretty(&snapshots)
        } else {
            serde_json::to_string(&snapshots)
        }
    }

    /// Serializes counters to a JSON byte vector.
    pub fn to_json_bytes<'a>(
        &self,
        counters: impl Iterator<Item = &'a dyn Observable>,
    ) -> Result<Vec<u8>, serde_json::Error> {
        let snapshots = self.collect(counters);

        if self.config.wrap_in_snapshot {
            let snapshot = if self.config.include_timestamp {
                MetricsSnapshot::with_timestamp(snapshots, current_timestamp_ms())
            } else {
                MetricsSnapshot::new(snapshots)
            };
            serde_json::to_vec(&snapshot)
        } else {
            serde_json::to_vec(&snapshots)
        }
    }
}

/// Returns the current timestamp in milliseconds since Unix epoch.
fn current_timestamp_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::counters::average::Average;
    use crate::counters::maximum::Maximum;
    use crate::counters::minimum::Minimum;
    use crate::counters::signed::Signed;
    use crate::counters::unsigned::Unsigned;
    use crate::counters::CounterValue;

    #[test]
    fn test_to_json_empty() {
        let observer = JsonObserver::new();
        let counters: Vec<&dyn Observable> = vec![];
        let json = observer.to_json(counters.into_iter()).unwrap();
        assert_eq!(json, "[]");
    }

    #[test]
    fn test_to_json_single_counter() {
        let counter = Unsigned::new().with_name("test_counter");
        counter.add(42);

        let observer = JsonObserver::new();
        let counters: Vec<&dyn Observable> = vec![&counter];
        let json = observer.to_json(counters.into_iter()).unwrap();

        assert!(json.contains("test_counter"));
        assert!(json.contains("42"));
    }

    #[test]
    fn test_to_json_multiple_counters() {
        let requests = Unsigned::new().with_name("requests");
        let errors = Unsigned::new().with_name("errors");

        requests.add(1000);
        errors.add(5);

        let observer = JsonObserver::new();
        let counters: Vec<&dyn Observable> = vec![&requests, &errors];
        let json = observer.to_json(counters.into_iter()).unwrap();

        assert!(json.contains("requests"));
        assert!(json.contains("1000"));
        assert!(json.contains("errors"));
        assert!(json.contains("5"));
    }

    #[test]
    fn test_to_json_signed_counter() {
        let balance = Signed::new().with_name("balance");
        balance.sub(100);

        let observer = JsonObserver::new();
        let counters: Vec<&dyn Observable> = vec![&balance];
        let json = observer.to_json(counters.into_iter()).unwrap();

        assert!(json.contains("balance"));
        assert!(json.contains("-100"));
    }

    #[test]
    fn test_to_json_pretty() {
        let counter = Unsigned::new().with_name("test");
        counter.add(1);

        let observer = JsonObserver::new().pretty(true);
        let counters: Vec<&dyn Observable> = vec![&counter];
        let json = observer.to_json(counters.into_iter()).unwrap();

        // Pretty JSON contains newlines
        assert!(json.contains('\n'));
    }

    #[test]
    fn test_to_json_with_snapshot() {
        let counter = Unsigned::new().with_name("metric");
        counter.add(100);

        let observer = JsonObserver::new().wrap_in_snapshot(true);
        let counters: Vec<&dyn Observable> = vec![&counter];
        let json = observer.to_json(counters.into_iter()).unwrap();

        assert!(json.contains("counters"));
        assert!(json.contains("metric"));
        assert!(json.contains("100"));
    }

    #[test]
    fn test_to_json_with_timestamp() {
        let counter = Unsigned::new().with_name("metric");
        counter.add(50);

        let observer = JsonObserver::new()
            .wrap_in_snapshot(true)
            .include_timestamp(true);

        let counters: Vec<&dyn Observable> = vec![&counter];
        let json = observer.to_json(counters.into_iter()).unwrap();

        assert!(json.contains("timestamp_ms"));
        assert!(json.contains("counters"));
    }

    #[test]
    fn test_collect() {
        let counter = Unsigned::new().with_name("collected");
        counter.add(25);

        let observer = JsonObserver::new();
        let counters: Vec<&dyn Observable> = vec![&counter];
        let snapshots = observer.collect(counters.into_iter());

        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].name, "collected");
        assert_eq!(snapshots[0].value, CounterValue::Unsigned(25));
    }

    #[test]
    fn test_unnamed_counter() {
        let counter = Unsigned::new(); // No name
        counter.add(99);

        let observer = JsonObserver::new();
        let counters: Vec<&dyn Observable> = vec![&counter];
        let json = observer.to_json(counters.into_iter()).unwrap();

        assert!(json.contains("(unnamed)"));
    }

    #[test]
    fn test_all_counter_types() {
        let unsigned = Unsigned::new().with_name("unsigned");
        let signed = Signed::new().with_name("signed");
        let minimum = Minimum::new().with_name("minimum");
        let maximum = Maximum::new().with_name("maximum");
        let average = Average::new().with_name("average");

        unsigned.add(100);
        signed.sub(50);
        minimum.observe(25);
        maximum.observe(200);
        average.observe(100);
        average.observe(200);

        let counters: Vec<&dyn Observable> = vec![&unsigned, &signed, &minimum, &maximum, &average];

        let observer = JsonObserver::new();
        let json = observer.to_json(counters.into_iter()).unwrap();

        assert!(json.contains("unsigned"));
        assert!(json.contains("signed"));
        assert!(json.contains("minimum"));
        assert!(json.contains("maximum"));
        assert!(json.contains("average"));
    }

    #[test]
    fn test_deserialize_snapshot() {
        let json = r#"{"name":"test","value":42}"#;
        let snapshot: CounterSnapshot = serde_json::from_str(json).unwrap();

        assert_eq!(snapshot.name, "test");
        assert_eq!(snapshot.value, CounterValue::Unsigned(42));
    }

    #[test]
    fn test_deserialize_metrics_snapshot() {
        let json = r#"{"timestamp_ms":1234567890,"counters":[{"name":"a","value":1}]}"#;
        let snapshot: MetricsSnapshot = serde_json::from_str(json).unwrap();

        assert_eq!(snapshot.timestamp_ms, Some(1234567890));
        assert_eq!(snapshot.counters.len(), 1);
        assert_eq!(snapshot.counters[0].name, "a");
    }

    #[test]
    fn test_metrics_snapshot_get() {
        let snapshot = MetricsSnapshot::new(vec![
            CounterSnapshot {
                name: "foo".to_string(),
                label: None,
                value: CounterValue::Unsigned(1),
            },
            CounterSnapshot {
                name: "bar".to_string(),
                label: None,
                value: CounterValue::Unsigned(2),
            },
        ]);

        assert!(snapshot.get("foo").is_some());
        assert!(snapshot.get("bar").is_some());
        assert!(snapshot.get("baz").is_none());
    }

    #[test]
    fn test_counter_value_conversions() {
        let unsigned = CounterValue::Unsigned(100);
        assert_eq!(unsigned.as_i64(), 100);
        assert_eq!(unsigned.as_u64(), 100);
        assert_eq!(unsigned.as_f64(), 100.0);

        let signed = CounterValue::Signed(-50);
        assert_eq!(signed.as_i64(), -50);
        assert_eq!(signed.as_f64(), -50.0);
    }

    #[test]
    fn test_to_json_bytes() {
        let counter = Unsigned::new().with_name("bytes_test");
        counter.add(123);

        let observer = JsonObserver::new();
        let counters: Vec<&dyn Observable> = vec![&counter];
        let bytes = observer.to_json_bytes(counters.into_iter()).unwrap();

        let json = String::from_utf8(bytes).unwrap();
        assert!(json.contains("bytes_test"));
        assert!(json.contains("123"));
    }
}
