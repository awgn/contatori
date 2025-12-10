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
//! contatori = { version = "0.7", features = ["serde"] }
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
///     labels: vec![],
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
    /// Optional label as (key, value) pair (e.g., ("method", "GET")).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<(String, String)>,
    /// The value of the counter.
    pub value: CounterValue,
}

impl CounterSnapshot {
    /// Creates a new counter snapshot.
    pub fn new(name: impl Into<String>, value: CounterValue) -> Self {
        Self {
            name: name.into(),
            label: None,
            value,
        }
    }

    /// Creates a new counter snapshot with a label.
    pub fn with_label(
        name: impl Into<String>,
        label: Option<(String, String)>,
        value: CounterValue,
    ) -> Self {
        Self {
            name: name.into(),
            label,
            value,
        }
    }

    /// Creates snapshots from an observable counter using expand().
    ///
    /// For single counters, returns one snapshot.
    /// For labeled groups, returns multiple snapshots (one per sub-counter).
    pub fn from_observable(counter: &dyn Observable) -> Vec<Self> {
        counter
            .expand()
            .into_iter()
            .map(|entry| Self {
                name: if entry.name.is_empty() {
                    "(unnamed)".to_string()
                } else {
                    entry.name.to_string()
                },
                label: entry.label.map(|(k, v)| (k.to_string(), v.to_string())),
                value: entry.value,
            })
            .collect()
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
    ///
    /// Uses `expand()` on each counter, so labeled groups will produce
    /// multiple snapshots.
    pub fn collect<'a>(counters: impl Iterator<Item = &'a dyn Observable>) -> Self {
        Self::new(
            counters
                .flat_map(CounterSnapshot::from_observable)
                .collect(),
        )
    }

    /// Collects snapshots with a timestamp.
    ///
    /// Uses `expand()` on each counter, so labeled groups will produce
    /// multiple snapshots.
    pub fn collect_with_timestamp<'a>(
        counters: impl Iterator<Item = &'a dyn Observable>,
        timestamp_ms: u64,
    ) -> Self {
        Self::with_timestamp(
            counters
                .flat_map(CounterSnapshot::from_observable)
                .collect(),
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
        assert!(snapshot.label.is_none());
        assert_eq!(snapshot.value, CounterValue::Unsigned(42));
    }

    #[test]
    fn test_counter_snapshot_with_label() {
        let snapshot = CounterSnapshot::with_label(
            "test",
            Some(("method".to_string(), "GET".to_string())),
            CounterValue::Unsigned(42),
        );
        assert_eq!(snapshot.name, "test");
        assert!(snapshot.label.is_some());
        assert_eq!(
            snapshot.label.unwrap(),
            ("method".to_string(), "GET".to_string())
        );
        assert_eq!(snapshot.value, CounterValue::Unsigned(42));
    }

    #[test]
    fn test_counter_snapshot_from_observable() {
        let counter = Unsigned::new().with_name("requests");
        counter.add(100);

        let snapshots = CounterSnapshot::from_observable(&counter);
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].name, "requests");
        assert!(snapshots[0].label.is_none());
        assert_eq!(snapshots[0].value, CounterValue::Unsigned(100));
    }

    #[test]
    fn test_counter_snapshot_from_observable_unnamed() {
        let counter = Unsigned::new();
        counter.add(50);

        let snapshots = CounterSnapshot::from_observable(&counter);
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].name, "(unnamed)");
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
        assert_eq!(
            snapshot.get("c1").unwrap().value,
            CounterValue::Unsigned(10)
        );
        assert_eq!(
            snapshot.get("c2").unwrap().value,
            CounterValue::Unsigned(20)
        );
    }

    #[test]
    fn test_metrics_snapshot_collect_labeled_group() {
        use crate::labeled_group;

        labeled_group!(
            TestGroup,
            "test_metric",
            "label",
            total: Unsigned,
            a: "A": Unsigned,
            b: "B": Unsigned,
        );

        let group = TestGroup::new();
        group.total.add(100);
        group.a.add(60);
        group.b.add(40);

        let counters: Vec<&dyn Observable> = vec![&group];
        let snapshot = MetricsSnapshot::collect(counters.into_iter());

        // Should have 3 entries: total (no label), a (label=A), b (label=B)
        assert_eq!(snapshot.counters.len(), 3);

        // Check that labels are preserved
        let with_labels: Vec<_> = snapshot
            .counters
            .iter()
            .filter(|c| c.label.is_some())
            .collect();
        assert_eq!(with_labels.len(), 2);
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
