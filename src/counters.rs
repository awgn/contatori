//! Core module containing counter implementations and shared infrastructure.
//!
//! This module provides the foundational types and traits used by all counter
//! implementations, as well as the sharding infrastructure that enables
//! high-performance concurrent updates.
//!
//! # Architecture
//!
//! The sharding system works as follows:
//!
//! 1. A global atomic counter (`NEXT_SLOT_ID`) assigns sequential IDs to threads
//! 2. Each thread caches its assigned ID in thread-local storage (`Cell<usize>`)
//! 3. Each counter masks the ID with `(SHARDS - 1)` (where `SHARDS` is a power of two,
//!    64 by default) to select which shard a thread writes to
//! 4. Each shard is cache-line padded (`CachePadded`, 128-byte aligned on x86_64/aarch64)
//!    to prevent false sharing
//!
//! ```text
//!                          ┌─────────────────────────────────────┐
//!                          │         Counter Structure           │
//!                          ├─────────────────────────────────────┤
//!   Thread 0 ──writes──►   │ [Slot 0]  ████████ (CachePadded)    │
//!   Thread 1 ──writes──►   │ [Slot 1]  ████████ (CachePadded)    │
//!   Thread 2 ──writes──►   │ [Slot 2]  ████████ (CachePadded)    │
//!        ...               │    ...                              │
//!   Thread 63 ─writes──►   │ [Slot 63] ████████ (CachePadded)    │
//!                          └─────────────────────────────────────┘
//!                                          │
//!                                          ▼
//!                                   value() aggregates
//!                                   all slots on read
//! ```

pub mod average;
pub mod maximum;
pub mod minimum;
pub mod monotone;
pub mod rate;
pub mod signed;
pub mod unsigned;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
use std::{
    cell::Cell,
    fmt::{Debug, Display},
    ops::{Add, AddAssign, Sub, SubAssign},
    sync::atomic::{AtomicUsize, Ordering},
};

/// Default number of shards (slots) used by each counter.
///
/// Must be a non-zero power of two. With 64 shards and 128-byte cache-line
/// padding (`CachePadded` on `x86_64` and `aarch64`), each counter occupies
/// approximately 8 KB of memory while allowing up to 64 threads to update
/// concurrently with zero cache-line contention.
pub const DEFAULT_SHARDS: usize = 64;

/// Global counter for assigning sequential IDs to threads.
static NEXT_SLOT_ID: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    /// Thread-local raw ID assigned to the current thread.
    ///
    /// Uses `usize::MAX` as the uninitialized sentinel so TLS initialization
    /// is `const` and fast on the hot path.
    static THREAD_SLOT_ID: Cell<usize> = const { Cell::new(usize::MAX) };
}

#[cold]
#[inline(never)]
fn init_thread_slot_id(cell: &Cell<usize>) -> usize {
    let raw = NEXT_SLOT_ID.fetch_add(1, Ordering::Relaxed);
    // Reserve usize::MAX as the uninitialized sentinel.
    let id = if raw == usize::MAX {
        NEXT_SLOT_ID.fetch_add(1, Ordering::Relaxed)
    } else {
        raw
    };
    cell.set(id);
    id
}

/// Returns the current thread's shard index in `[0, SHARDS)`.
///
/// Compile-time panics if `SHARDS` is zero or not a power of two.
#[inline]
pub(crate) fn thread_slot_index<const SHARDS: usize>() -> usize {
    const {
        assert!(
            SHARDS > 0 && SHARDS.is_power_of_two(),
            "SHARDS must be a non-zero power of two"
        );
    }
    let id = THREAD_SLOT_ID.with(|cell| {
        let v = cell.get();
        if v != usize::MAX {
            v
        } else {
            init_thread_slot_id(cell)
        }
    });
    id & (SHARDS - 1)
}

/// Assigns the next available slot ID modulo [`DEFAULT_SHARDS`].
#[inline]
pub fn get_next_slot_id() -> usize {
    NEXT_SLOT_ID.fetch_add(1, Ordering::Relaxed) & (DEFAULT_SHARDS - 1)
}

/// Represents the value of a counter, supporting unsigned, signed, and floating-point types.
///
/// This enum allows the [`Observable`] trait to return values from counters
/// of different underlying types through a unified interface.
///
/// # Examples
///
/// ```rust
/// use contatori::CounterValue;
///
/// let unsigned = CounterValue::Unsigned(42);
/// let signed = CounterValue::Signed(-10);
/// let float = CounterValue::Float(3.5);
///
/// assert!(!unsigned.is_zero());
/// assert!(!signed.is_zero());
/// assert!(!float.is_zero());
/// assert!(CounterValue::Unsigned(0).is_zero());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(untagged))]
pub enum CounterValue {
    /// An unsigned 64-bit integer counter value.
    Unsigned(u64),
    /// A signed 64-bit integer counter value.
    Signed(i64),
    /// A 64-bit floating-point counter value.
    Float(f64),
}

impl From<u64> for CounterValue {
    #[inline]
    fn from(v: u64) -> Self {
        Self::Unsigned(v)
    }
}

impl From<i64> for CounterValue {
    #[inline]
    fn from(v: i64) -> Self {
        Self::Signed(v)
    }
}

impl From<f64> for CounterValue {
    #[inline]
    fn from(v: f64) -> Self {
        Self::Float(v)
    }
}

impl Display for CounterValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CounterValue::Unsigned(v) => write!(f, "{v}"),
            CounterValue::Signed(v) => write!(f, "{v}"),
            CounterValue::Float(v) => write!(f, "{v}"),
        }
    }
}

impl Add for CounterValue {
    type Output = Self;

    fn add(self, other: Self) -> Self::Output {
        use CounterValue::*;
        match (self, other) {
            // Same type: keep the original type using wrapping integer arithmetic
            (Unsigned(a), Unsigned(b)) => Unsigned(a.wrapping_add(b)),
            (Signed(a), Signed(b)) => Signed(a.wrapping_add(b)),
            (Float(a), Float(b)) => Float(a + b),

            // Float mixes: always promote to Float
            (Float(a), Unsigned(b)) => Float(a + b as f64),
            (Unsigned(a), Float(b)) => Float(a as f64 + b),
            (Float(a), Signed(b)) => Float(a + b as f64),
            (Signed(a), Float(b)) => Float(a as f64 + b),

            // Signed/Unsigned mixes: promote to Signed using wrapping arithmetic
            (Signed(a), Unsigned(b)) => Signed(a.wrapping_add(b as i64)),
            (Unsigned(a), Signed(b)) => Signed((a as i64).wrapping_add(b)),
        }
    }
}

impl Sub for CounterValue {
    type Output = Self;

    fn sub(self, other: Self) -> Self::Output {
        use CounterValue::*;
        match (self, other) {
            // Same type: keep the original type using wrapping integer arithmetic
            (Unsigned(a), Unsigned(b)) => Unsigned(a.wrapping_sub(b)),
            (Signed(a), Signed(b)) => Signed(a.wrapping_sub(b)),
            (Float(a), Float(b)) => Float(a - b),

            // Float mixes
            (Float(a), Unsigned(b)) => Float(a - b as f64),
            (Unsigned(a), Float(b)) => Float(a as f64 - b),
            (Float(a), Signed(b)) => Float(a - b as f64),
            (Signed(a), Float(b)) => Float(a as f64 - b),

            // Signed/Unsigned mixes: promote to Signed using wrapping arithmetic
            (Signed(a), Unsigned(b)) => Signed(a.wrapping_sub(b as i64)),
            (Unsigned(a), Signed(b)) => Signed((a as i64).wrapping_sub(b)),
        }
    }
}

impl AddAssign for CounterValue {
    fn add_assign(&mut self, other: Self) {
        *self = *self + other;
    }
}

impl SubAssign for CounterValue {
    fn sub_assign(&mut self, other: Self) {
        *self = *self - other;
    }
}

impl CounterValue {
    /// Returns `true` if the counter value is zero.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use contatori::CounterValue;
    ///
    /// assert!(CounterValue::Unsigned(0).is_zero());
    /// assert!(CounterValue::Signed(0).is_zero());
    /// assert!(CounterValue::Float(0.0).is_zero());
    /// assert!(!CounterValue::Unsigned(1).is_zero());
    /// assert!(!CounterValue::Signed(-1).is_zero());
    /// ```
    #[must_use]
    #[inline]
    pub fn is_zero(&self) -> bool {
        match self {
            CounterValue::Unsigned(v) => *v == 0,
            CounterValue::Signed(v) => *v == 0,
            CounterValue::Float(v) => *v == 0.0,
        }
    }

    /// Returns the value as an `i64`, converting unsigned or float if necessary.
    #[must_use]
    #[inline]
    pub fn as_i64(&self) -> i64 {
        match self {
            CounterValue::Unsigned(v) => *v as i64,
            CounterValue::Signed(v) => *v,
            CounterValue::Float(v) => *v as i64,
        }
    }

    /// Returns the value as a `u64`, converting signed or float if necessary.
    ///
    /// Note: Negative signed values wrap around when cast to `u64`.
    #[must_use]
    #[inline]
    pub fn as_u64(&self) -> u64 {
        match self {
            CounterValue::Unsigned(v) => *v,
            CounterValue::Signed(v) => *v as u64,
            CounterValue::Float(v) => *v as u64,
        }
    }

    /// Returns the value as an `f64`.
    #[must_use]
    #[inline]
    pub fn as_f64(&self) -> f64 {
        match self {
            CounterValue::Unsigned(v) => *v as f64,
            CounterValue::Signed(v) => *v as f64,
            CounterValue::Float(v) => *v,
        }
    }
}

/// The kind of metric a counter represents.
///
/// Used by observers (such as Prometheus and OpenTelemetry) to export the metric
/// with the appropriate semantic instrument type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum MetricKind {
    /// A gauge is a metric that can go up and down.
    #[default]
    Gauge,

    /// A counter is a cumulative metric that only ever goes up.
    Counter,

    /// A histogram samples observations and counts them in buckets.
    ///
    /// Reserved for sharded histogram support.
    Histogram,
}

/// Represents a single observable metric entry with its metadata and labels.
///
/// Returned via [`Observable::expand_into()`] / [`Observable::expand()`].
#[derive(Debug, Clone, PartialEq)]
pub struct ObservableEntry {
    /// The metric name (e.g., `"http_requests_total"`).
    pub name: &'static str,
    /// Static slice of `(key, value)` label pairs (empty `&[]` when unlabeled).
    pub labels: &'static [(&'static str, &'static str)],
    /// Current counter value, or `None` if no observations have been recorded yet
    /// (for statistical trackers like [`Minimum`](minimum::Minimum),
    /// [`Maximum`](maximum::Maximum), and [`Average`](average::Average)).
    pub value: Option<CounterValue>,
    /// Semantic kind of metric (`Counter`, `Gauge`, `Histogram`).
    pub metric_kind: MetricKind,
}

/// A trait for types that can be observed to retrieve their current value(s).
///
/// All counter types and adapters in `contatori` are thread-safe (`Send + Sync`)
/// and implement `Observable`.
pub trait Observable: Debug + Send + Sync {
    /// Returns the static name of this counter, or `""` if unnamed.
    fn name(&self) -> &'static str;

    /// Returns the semantic kind of metric this counter represents.
    fn metric_kind(&self) -> MetricKind {
        MetricKind::Gauge
    }

    /// Returns the current aggregated value of the counter.
    ///
    /// Trackers without any recorded observations return zero (`0` or `0.0`).
    /// Use [`try_value()`](Self::try_value) to check whether at least one
    /// observation has been recorded.
    fn value(&self) -> CounterValue;

    /// Returns `Some(value)` if the counter has a value, or `None` if no
    /// observations have been recorded yet.
    ///
    /// Observers use this method (via [`expand_into()`](Self::expand_into)) to
    /// omit uninitialized statistical trackers instead of emitting placeholder values.
    fn try_value(&self) -> Option<CounterValue> {
        Some(self.value())
    }

    /// Appends all entries for this observable into `out`.
    ///
    /// Using `expand_into` allows observers to reuse a single `Vec` buffer across
    /// an entire collection pass without per-counter heap allocations.
    fn expand_into(&self, out: &mut Vec<ObservableEntry>) {
        out.push(ObservableEntry {
            name: self.name(),
            labels: &[],
            value: self.try_value(),
            metric_kind: self.metric_kind(),
        });
    }

    /// Expands this observable into a freshly allocated vector of entries.
    fn expand(&self) -> Vec<ObservableEntry> {
        let mut out = Vec::with_capacity(1);
        self.expand_into(&mut out);
        out
    }
}

impl Display for dyn Observable + '_ {
    /// Formats the counter as `name:value` if named, or `value` otherwise.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if !self.name().is_empty() {
            write!(f, "{}:{}", self.name(), self.value())
        } else {
            write!(f, "{}", self.value())
        }
    }
}

/// Internal sealed module for read-and-reset functionality.
///
/// Exposed as `#[doc(hidden)] pub` only so that the exported [`labeled_group!`](crate::labeled_group)
/// macro can implement `Resettable` for user-defined group structs in downstream crates.
#[doc(hidden)]
pub mod sealed {
    use super::{CounterValue, Observable, ObservableEntry};

    /// Internal trait for counters and groups that support read-and-reset.
    pub trait Resettable: Observable {
        /// Returns the current value (or zero fallback) and resets the counter.
        fn value_and_reset(&self) -> CounterValue;

        /// Returns `Some(value)` and resets the counter, or `None` if unobserved.
        fn try_value_and_reset(&self) -> Option<CounterValue> {
            Some(self.value_and_reset())
        }

        /// Appends reset entries into `out`.
        fn expand_and_reset_into(&self, out: &mut Vec<ObservableEntry>) {
            out.push(ObservableEntry {
                name: self.name(),
                labels: &[],
                value: self.try_value_and_reset(),
                metric_kind: self.metric_kind(),
            });
        }
    }

    /// Internal trait for counters and labeled groups that support thread-local batching
    /// via [`Local`](crate::adapters::Local).
    pub trait Batchable {
        /// Thread-local accumulator state.
        type Acc<'a>
        where
            Self: 'a;

        /// Creates a fresh empty accumulator for `self`.
        fn new_acc(&self) -> Self::Acc<'_>;

        /// Returns `true` if `acc` holds unflushed updates.
        fn has_pending_acc(acc: &Self::Acc<'_>) -> bool;

        /// Flushes any pending updates in `acc` into `self` and resets `acc` to empty.
        fn flush_acc(&self, acc: &mut Self::Acc<'_>);

        /// Resets `acc` to empty without flushing to `self`.
        fn discard_acc(acc: &mut Self::Acc<'_>);
    }

    /// Marker subtrait for [`labeled_group!`](crate::labeled_group) structs whose
    /// [`Local`](crate::adapters::Local) wrapper dereferences to per-label local proxies.
    pub trait BatchableGroup: Batchable {}
}

#[cfg(test)]
mod tests {
    use super::{CounterValue, thread_slot_index};

    #[test]
    fn thread_slot_masks_correctly_for_multiple_shard_counts() {
        let idx16 = thread_slot_index::<16>();
        let idx64 = thread_slot_index::<64>();
        let idx128 = thread_slot_index::<128>();
        assert!(idx16 < 16);
        assert!(idx64 < 64);
        assert!(idx128 < 128);
        assert_eq!(idx16, idx64 & 15);
        assert_eq!(idx64, idx128 & 63);
    }

    #[test]
    fn add_unsigned_unsigned() {
        let a = CounterValue::Unsigned(10);
        let b = CounterValue::Unsigned(20);
        assert!(matches!(a + b, CounterValue::Unsigned(30)));
    }

    #[test]
    fn add_signed_signed_and_wrapping_overflow() {
        let a = CounterValue::Signed(-10);
        let b = CounterValue::Signed(20);
        assert!(matches!(a + b, CounterValue::Signed(10)));

        let max = CounterValue::Signed(i64::MAX);
        assert!(matches!(
            max + CounterValue::Signed(1),
            CounterValue::Signed(i64::MIN)
        ));
    }

    #[test]
    fn add_float_float() {
        let a = CounterValue::Float(1.5);
        let b = CounterValue::Float(2.5);
        match a + b {
            CounterValue::Float(v) => assert!((v - 4.0).abs() < f64::EPSILON),
            _ => panic!("Expected Float variant"),
        }
    }

    #[test]
    fn add_mixed_promotions() {
        assert_eq!(
            CounterValue::Unsigned(10) + CounterValue::Signed(-3),
            CounterValue::Signed(7)
        );
        assert_eq!(
            CounterValue::Signed(-5) + CounterValue::Unsigned(15),
            CounterValue::Signed(10)
        );
        assert_eq!(
            CounterValue::Float(1.5) + CounterValue::Unsigned(2),
            CounterValue::Float(3.5)
        );
        assert_eq!(
            CounterValue::Signed(-2) + CounterValue::Float(5.0),
            CounterValue::Float(3.0)
        );
    }

    #[test]
    fn sub_mixed_and_wrapping() {
        assert_eq!(
            CounterValue::Unsigned(0) - CounterValue::Unsigned(1),
            CounterValue::Unsigned(u64::MAX)
        );
        assert_eq!(
            CounterValue::Signed(i64::MIN) - CounterValue::Signed(1),
            CounterValue::Signed(i64::MAX)
        );
        assert_eq!(
            CounterValue::Unsigned(10) - CounterValue::Signed(3),
            CounterValue::Signed(7)
        );
        assert_eq!(
            CounterValue::Float(5.5) - CounterValue::Unsigned(2),
            CounterValue::Float(3.5)
        );
    }

    #[test]
    fn assign_ops_work() {
        let mut a = CounterValue::Unsigned(10);
        a += CounterValue::Unsigned(20);
        a -= CounterValue::Signed(5);
        assert_eq!(a, CounterValue::Signed(25));
    }
}
