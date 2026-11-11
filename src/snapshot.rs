//! Snapshot types for serializing counter state with `serde`.
//!
//! Requires the `serde` feature (automatically enabled by the `json` feature).

use crate::counters::{CounterValue, Observable, ObservableEntry};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A serializable point-in-time snapshot of a single counter entry.
///
/// # Examples
///
/// ```rust
/// use contatori::{CounterValue, Observable, Unsigned};
/// use contatori::snapshot::CounterSnapshot;
///
/// let counter = Unsigned::new().with_name("requests");
/// counter.add(42);
///
/// let snapshots = CounterSnapshot::from_observable(&counter);
/// assert_eq!(snapshots.len(), 1);
/// assert_eq!(snapshots[0].name, "requests");
/// assert_eq!(snapshots[0].value, CounterValue::Unsigned(42));
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CounterSnapshot {
    /// The name of the counter.
    pub name: String,
    /// Key-value labels associated with this entry (omitted when empty).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
    /// The observed value of the counter.
    pub value: CounterValue,
}

impl CounterSnapshot {
    /// Creates an unlabeled counter snapshot.
    #[must_use]
    pub fn new(name: impl Into<String>, value: CounterValue) -> Self {
        Self {
            name: name.into(),
            labels: BTreeMap::new(),
            value,
        }
    }

    /// Creates a counter snapshot with the given `(key, value)` labels.
    #[must_use]
    pub fn with_labels<K, V, I>(name: impl Into<String>, labels: I, value: CounterValue) -> Self
    where
        K: Into<String>,
        V: Into<String>,
        I: IntoIterator<Item = (K, V)>,
    {
        Self {
            name: name.into(),
            labels: labels
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
            value,
        }
    }

    pub(crate) fn from_entry(entry: ObservableEntry) -> Option<Self> {
        let value = entry.value?;
        let name = if entry.name.is_empty() {
            "(unnamed)".to_string()
        } else {
            entry.name.to_string()
        };
        let labels = entry
            .labels
            .iter()
            .map(|&(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Some(Self {
            name,
            labels,
            value,
        })
    }

    /// Expands an [`Observable`] into snapshots, omitting entries without observations (`None`).
    #[must_use]
    pub fn from_observable(counter: &dyn Observable) -> Vec<Self> {
        let mut entries = Vec::new();
        counter.expand_into(&mut entries);
        entries.into_iter().filter_map(Self::from_entry).collect()
    }
}

/// A point-in-time collection of counter snapshots with an optional Unix timestamp.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MetricsSnapshot {
    /// Optional timestamp in milliseconds since the Unix epoch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp_ms: Option<u64>,
    /// Captured counter snapshots.
    pub counters: Vec<CounterSnapshot>,
}

impl MetricsSnapshot {
    /// Creates a new metrics snapshot without a timestamp.
    #[must_use]
    pub fn new(counters: Vec<CounterSnapshot>) -> Self {
        Self {
            timestamp_ms: None,
            counters,
        }
    }

    /// Creates a new metrics snapshot with the given timestamp in milliseconds.
    #[must_use]
    pub fn with_timestamp(counters: Vec<CounterSnapshot>, timestamp_ms: u64) -> Self {
        Self {
            timestamp_ms: Some(timestamp_ms),
            counters,
        }
    }

    /// Finds the first snapshot matching `name`.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&CounterSnapshot> {
        self.counters.iter().find(|c| c.name == name)
    }

    /// Collects snapshots from an iterator of [`Observable`] counters.
    ///
    /// Uninitialized trackers (`try_value() == None`) are automatically omitted.
    #[must_use]
    pub fn collect<'a>(counters: impl Iterator<Item = &'a dyn Observable>) -> Self {
        let mut entries = Vec::new();
        for counter in counters {
            counter.expand_into(&mut entries);
        }
        Self::new(
            entries
                .into_iter()
                .filter_map(CounterSnapshot::from_entry)
                .collect(),
        )
    }

    /// Collects snapshots from an iterator of [`Observable`] counters with a timestamp.
    #[must_use]
    pub fn collect_with_timestamp<'a>(
        counters: impl Iterator<Item = &'a dyn Observable>,
        timestamp_ms: u64,
    ) -> Self {
        let mut snapshot = Self::collect(counters);
        snapshot.timestamp_ms = Some(timestamp_ms);
        snapshot
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::counters::minimum::Minimum;
    use crate::counters::unsigned::Unsigned;
    use crate::labeled_group;

    #[test]
    fn test_snapshot_omits_unobserved_trackers() {
        let min_empty = Minimum::new().with_name("min_empty");
        let min_set = Minimum::new().with_name("min_set");
        min_set.observe(42);

        let counters: Vec<&dyn Observable> = vec![&min_empty, &min_set];
        let snapshot = MetricsSnapshot::collect(counters.into_iter());

        assert_eq!(snapshot.counters.len(), 1);
        assert_eq!(snapshot.counters[0].name, "min_set");
        assert_eq!(snapshot.counters[0].value, CounterValue::Unsigned(42));
    }

    #[test]
    fn test_labeled_group_multi_label_snapshot() {
        labeled_group!(
            TestGroup,
            "http_responses",
            ["method", "status"],
            get_200: ["GET", "200"]: Unsigned,
            post_201: ["POST", "201"]: Unsigned,
        );

        let group = TestGroup::new();
        group.get_200.add(60);
        group.post_201.add(40);

        let counters: Vec<&dyn Observable> = vec![&group];
        let snapshot = MetricsSnapshot::collect(counters.into_iter());

        assert_eq!(snapshot.counters.len(), 2);
        assert_eq!(snapshot.counters[0].labels.get("method").unwrap(), "GET");
        assert_eq!(snapshot.counters[0].labels.get("status").unwrap(), "200");
    }

    #[cfg(feature = "json")]
    #[test]
    fn test_serde_roundtrip() {
        let snap = CounterSnapshot::with_labels(
            "http_req",
            [("method", "GET")],
            CounterValue::Unsigned(10),
        );
        let json = serde_json::to_string(&snap).unwrap();
        assert_eq!(
            json,
            r#"{"name":"http_req","labels":{"method":"GET"},"value":10}"#
        );
        let decoded: CounterSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, snap);
    }
}
