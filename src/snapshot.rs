//! Snapshot types for serializing counter state.
//!
//! This module provides serializable snapshot types that can be used
//! to capture and export counter values in various formats.
//!
//! # Feature Flag
//!
//! This module requires the `serde` feature:
//!
//! ```toml
//! [dependencies]
//! contatori = { version = "0.3", features = ["serde"] }
//! ```
//!
//! # Examples
//!
//! ```rust,ignore
//! use contatori::counters::{CounterValue, Observable};
//! use contatori::counters::unsigned::Unsigned;
//! use contatori::snapshot::CounterSnapshot;
//!
//! let counter = Unsigned::new().with_name("requests");
//! counter.add(42);
//!
//! // Create a snapshot
//! let snapshot = CounterSnapshot::new(counter.name(), counter.value());
//!
//! // Serialize with any serde-compatible format
//! let json = serde_json::to_string(&snapshot).unwrap();
//! let yaml = serde_yaml::to_string(&snapshot).unwrap();
//! let bytes = bincode::serialize(&snapshot).unwrap();
//! ```

use crate::counters::{CounterValue, Observable};
use serde::{Deserialize, Serialize};

/// A snapshot of a single counter's state.
///
/// This struct is serializable and can be used for:
/// - Storing counter values to files
/// - Sending metrics over HTTP APIs
/// - Inter-process communication
///
/// # Examples
///
/// ```rust,ignore
/// use contatori::counters::CounterValue;
/// use contatori::snapshot::CounterSnapshot;
///
/// let snapshot = CounterSnapshot {
///     name: "requests".to_string(),
///     value: CounterValue::Unsigned(42),
/// };
///
/// let json = serde_json::to_string(&snapshot).unwrap();
/// assert_eq!(json, r#"{"name":"requests","value":42}"#);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CounterSnapshot {
    /// The name of the counter.
    pub name: String,
    /// The value of the counter.
    pub value: CounterValue,
}

impl CounterSnapshot {
    /// Creates a new counter snapshot.
    pub fn new(name: impl Into<String>, value: CounterValue) -> Self {
        Self {
            name: name.into(),
            value,
        }
    }

    /// Creates a snapshot from an observable counter.
    pub fn from_observable(counter: &dyn Observable) -> Self {
        Self {
            name: if counter.name().is_empty() {
                "(unnamed)".to_string()
            } else {
                counter.name().to_string()
            },
            value: counter.value(),
        }
    }

    /// Creates a snapshot from an observable counter and resets it atomically.
    pub fn from_observable_and_reset(counter: &dyn Observable) -> Self {
        Self {
            name: if counter.name().is_empty() {
                "(unnamed)".to_string()
            } else {
                counter.name().to_string()
            },
            value: counter.value_and_reset(),
        }
    }
}

/// A collection of counter snapshots, typically representing a point-in-time
/// capture of all metrics.
///
/// # Examples
///
/// ```rust,ignore
/// use contatori::snapshot::{CounterSnapshot, MetricsSnapshot};
/// use contatori::counters::CounterValue;
///
/// let snapshot = MetricsSnapshot::new(vec![
///     CounterSnapshot::new("requests", CounterValue::Unsigned(1000)),
///     CounterSnapshot::new("errors", CounterValue::Unsigned(5)),
/// ]);
///
/// // Add timestamp
/// let snapshot = MetricsSnapshot::with_timestamp(counters, current_time_ms());
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MetricsSnapshot {
    /// Optional timestamp in milliseconds since Unix epoch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp_ms: Option<u64>,
    /// The counter snapshots.
    pub counters: Vec<CounterSnapshot>,
}

impl MetricsSnapshot {
    /// Creates a new metrics snapshot with the given counters.
    pub fn new(counters: Vec<CounterSnapshot>) -> Self {
        Self {
            timestamp_ms: None,
            counters,
        }
    }

    /// Creates a new metrics snapshot with counters and a timestamp.
    pub fn with_timestamp(counters: Vec<CounterSnapshot>, timestamp_ms: u64) -> Self {
        Self {
            timestamp_ms: Some(timestamp_ms),
            counters,
        }
    }

    /// Finds a counter by name.
    pub fn get(&self, name: &str) -> Option<&CounterSnapshot> {
        self.counters.iter().find(|c| c.name == name)
    }

    /// Collects snapshots from an iterator of observable counters.
    pub fn collect<'a>(counters: impl Iterator<Item = &'a dyn Observable>) -> Self {
        Self::new(counters.map(CounterSnapshot::from_observable).collect())
    }

    /// Collects snapshots from an iterator of observable counters and resets them.
    pub fn collect_and_reset<'a>(counters: impl Iterator<Item = &'a dyn Observable>) -> Self {
        Self::new(
            counters
                .map(CounterSnapshot::from_observable_and_reset)
                .collect(),
        )
    }

    /// Collects snapshots with a timestamp.
    pub fn collect_with_timestamp<'a>(
        counters: impl Iterator<Item = &'a dyn Observable>,
        timestamp_ms: u64,
    ) -> Self {
        Self::with_timestamp(
            counters.map(CounterSnapshot::from_observable).collect(),
            timestamp_ms,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::counters::unsigned::Unsigned;

    #[test]
    fn test_counter_snapshot_new() {
        let snapshot = CounterSnapshot::new("test", CounterValue::Unsigned(42));
        assert_eq!(snapshot.name, "test");
        assert_eq!(snapshot.value, CounterValue::Unsigned(42));
    }

    #[test]
    fn test_counter_snapshot_from_observable() {
        let counter = Unsigned::new().with_name("requests");
        counter.add(100);

        let snapshot = CounterSnapshot::from_observable(&counter);
        assert_eq!(snapshot.name, "requests");
        assert_eq!(snapshot.value, CounterValue::Unsigned(100));
    }

    #[test]
    fn test_counter_snapshot_from_observable_unnamed() {
        let counter = Unsigned::new();
        counter.add(50);

        let snapshot = CounterSnapshot::from_observable(&counter);
        assert_eq!(snapshot.name, "(unnamed)");
    }

    #[test]
    fn test_counter_snapshot_from_observable_and_reset() {
        let counter = Unsigned::new().with_name("resettable");
        counter.add(75);

        let snapshot = CounterSnapshot::from_observable_and_reset(&counter);
        assert_eq!(snapshot.name, "resettable");
        assert_eq!(snapshot.value, CounterValue::Unsigned(75));
        assert_eq!(counter.value(), CounterValue::Unsigned(0));
    }

    #[test]
    fn test_metrics_snapshot_new() {
        let snapshot = MetricsSnapshot::new(vec![
            CounterSnapshot::new("a", CounterValue::Unsigned(1)),
            CounterSnapshot::new("b", CounterValue::Unsigned(2)),
        ]);

        assert_eq!(snapshot.counters.len(), 2);
        assert!(snapshot.timestamp_ms.is_none());
    }

    #[test]
    fn test_metrics_snapshot_with_timestamp() {
        let snapshot = MetricsSnapshot::with_timestamp(
            vec![CounterSnapshot::new("test", CounterValue::Unsigned(1))],
            1234567890,
        );

        assert_eq!(snapshot.timestamp_ms, Some(1234567890));
    }

    #[test]
    fn test_metrics_snapshot_get() {
        let snapshot = MetricsSnapshot::new(vec![
            CounterSnapshot::new("foo", CounterValue::Unsigned(1)),
            CounterSnapshot::new("bar", CounterValue::Unsigned(2)),
        ]);

        assert!(snapshot.get("foo").is_some());
        assert!(snapshot.get("bar").is_some());
        assert!(snapshot.get("baz").is_none());
    }

    #[test]
    fn test_metrics_snapshot_collect() {
        let counter1 = Unsigned::new().with_name("c1");
        let counter2 = Unsigned::new().with_name("c2");
        counter1.add(10);
        counter2.add(20);

        let counters: Vec<&dyn Observable> = vec![&counter1, &counter2];
        let snapshot = MetricsSnapshot::collect(counters.into_iter());

        assert_eq!(snapshot.counters.len(), 2);
        assert_eq!(snapshot.get("c1").unwrap().value, CounterValue::Unsigned(10));
        assert_eq!(snapshot.get("c2").unwrap().value, CounterValue::Unsigned(20));
    }

    #[test]
    fn test_serialize_counter_snapshot() {
        let snapshot = CounterSnapshot::new("test", CounterValue::Unsigned(42));
        let json = serde_json::to_string(&snapshot).unwrap();
        assert!(json.contains("test"));
        assert!(json.contains("42"));
    }

    #[test]
    fn test_deserialize_counter_snapshot() {
        let json = r#"{"name":"test","value":42}"#;
        let snapshot: CounterSnapshot = serde_json::from_str(json).unwrap();
        assert_eq!(snapshot.name, "test");
        assert_eq!(snapshot.value, CounterValue::Unsigned(42));
    }

    #[test]
    fn test_serialize_metrics_snapshot() {
        let snapshot = MetricsSnapshot::with_timestamp(
            vec![CounterSnapshot::new("a", CounterValue::Unsigned(1))],
            1234567890,
        );
        let json = serde_json::to_string(&snapshot).unwrap();
        assert!(json.contains("timestamp_ms"));
        assert!(json.contains("1234567890"));
    }

    #[test]
    fn test_deserialize_metrics_snapshot() {
        let json = r#"{"timestamp_ms":1234567890,"counters":[{"name":"a","value":1}]}"#;
        let snapshot: MetricsSnapshot = serde_json::from_str(json).unwrap();
        assert_eq!(snapshot.timestamp_ms, Some(1234567890));
        assert_eq!(snapshot.counters.len(), 1);
    }
}
