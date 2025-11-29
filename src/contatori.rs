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
//! 2. Each thread stores its assigned slot index in thread-local storage
//! 3. The slot index is used modulo `NUM_COMPONENTS` (64) to select which
//!    shard a thread writes to
//! 4. Each shard is cache-line padded to prevent false sharing
//!
//! ```text
//!                          ┌─────────────────────────────────────┐
//!                          │         Counter Structure           │
//!                          ├─────────────────────────────────────┤
//!   Thread 0 ──writes──►   │ [Slot 0] ████████ (CachePadded)     │
//!   Thread 1 ──writes──►   │ [Slot 1] ████████ (CachePadded)     │
//!   Thread 2 ──writes──►   │ [Slot 2] ████████ (CachePadded)     │
//!        ...               │    ...                              │
//!   Thread 63 ─writes──►   │ [Slot 63] ███████ (CachePadded)     │
//!                          └─────────────────────────────────────┘
//!                                          │
//!                                          ▼
//!                                   value() aggregates
//!                                   all slots on read
//! ```
//!
//! # Thread Slot Assignment
//!
//! Slots are assigned round-robin: the first thread gets slot 0, the second
//! gets slot 1, and so on. After 64 threads, assignment wraps around (thread 64
//! shares slot 0 with thread 0). This is acceptable because:
//!
//! - Most applications have fewer than 64 concurrent threads updating counters
//! - Even with slot sharing, contention is reduced by 64x compared to a single atomic
//! - The assignment is deterministic and stable for the thread's lifetime

pub mod average;
pub mod maximum;
pub mod minimum;
pub mod signed;
pub mod unsigned;

use atomic_traits::Atomic;
use std::{
    fmt::Debug,
    fmt::Display,
    sync::atomic::{AtomicUsize, Ordering},
};

/// Number of shards (slots) used by each counter.
///
/// This value is chosen to:
/// - Be large enough to minimize contention (64 threads can update without any contention)
/// - Be a power of 2 for efficient modulo operations
/// - Balance memory usage (~4KB per counter) with performance benefits
///
/// Each slot is cache-line padded (64 bytes), so total memory per counter is:
/// `64 slots × 64 bytes = 4,096 bytes (4KB)`
pub(crate) const NUM_COMPONENTS: usize = 64;

/// Global counter for assigning slot IDs to threads.
///
/// This is incremented atomically each time a new thread accesses any counter,
/// ensuring each thread gets a unique slot (modulo `NUM_COMPONENTS`).
static NEXT_SLOT_ID: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    /// Thread-local slot index assigned to the current thread.
    ///
    /// Initialized lazily on first access to any counter operation.
    /// The value is stable for the lifetime of the thread.
    pub(crate) static THREAD_SLOT_INDEX: usize = get_next_slot_id();
}

/// Assigns the next available slot ID to a thread.
///
/// Called once per thread (lazily) when the thread first accesses a counter.
/// The returned value is in the range `[0, NUM_COMPONENTS)`.
///
/// # Thread Safety
///
/// Uses `Ordering::Relaxed` because we only need atomicity, not synchronization.
/// It's acceptable if two threads occasionally get the same slot ID due to
/// reordering - this slightly increases contention but doesn't affect correctness.
pub fn get_next_slot_id() -> usize {
    NEXT_SLOT_ID.fetch_add(1, Ordering::Relaxed) % NUM_COMPONENTS
}

/// Represents the value of a counter, supporting both signed and unsigned types.
///
/// This enum allows the [`Observable`] trait to return values from counters
/// of different underlying types through a unified interface.
///
/// # Examples
///
/// ```rust
/// use contatori::contatori::CounterValue;
///
/// let unsigned = CounterValue::Unsigned(42);
/// let signed = CounterValue::Signed(-10);
///
/// assert!(!unsigned.is_zero());
/// assert!(!signed.is_zero());
/// assert!(CounterValue::Unsigned(0).is_zero());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub enum CounterValue {
    /// An unsigned 64-bit counter value.
    Unsigned(u64),
    /// A signed 64-bit counter value.
    Signed(i64),
}

impl Display for CounterValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CounterValue::Unsigned(v) => write!(f, "{}", v),
            CounterValue::Signed(v) => write!(f, "{}", v),
        }
    }
}

impl CounterValue {
    /// Returns `true` if the counter value is zero.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use contatori::contatori::CounterValue;
    ///
    /// assert!(CounterValue::Unsigned(0).is_zero());
    /// assert!(CounterValue::Signed(0).is_zero());
    /// assert!(!CounterValue::Unsigned(1).is_zero());
    /// assert!(!CounterValue::Signed(-1).is_zero());
    /// ```
    pub fn is_zero(&self) -> bool {
        match self {
            CounterValue::Unsigned(v) => *v == 0,
            CounterValue::Signed(v) => *v == 0,
        }
    }
}

/// A trait for types that can be observed to retrieve their current value.
///
/// This trait provides a common interface for all counter types, allowing
/// them to be used interchangeably when reading values or collecting metrics.
///
/// # Implementors
///
/// All counter types in this crate implement `Observable`:
/// - [`Unsigned`](unsigned::Unsigned) - returns `CounterValue::Unsigned`
/// - [`Signed`](signed::Signed) - returns `CounterValue::Signed`
/// - [`Minimum`](minimum::Minimum) - returns `CounterValue::Unsigned`
/// - [`Maximum`](maximum::Maximum) - returns `CounterValue::Unsigned`
/// - [`Average`](average::Average) - returns `CounterValue::Unsigned` (the computed average)
///
/// # Examples
///
/// ```rust
/// use contatori::contatori::Observable;
/// use contatori::contatori::unsigned::Unsigned;
///
/// let counter = Unsigned::new().with_name("requests");
/// counter.add(5);
///
/// // Use the Observable interface
/// println!("Name: {}", counter.name());
/// println!("Value: {}", counter.value());
///
/// // Reset and get the value atomically
/// let final_value = counter.value_and_reset();
/// ```
pub trait Observable: Debug {
    /// Returns the name of this counter.
    ///
    /// The name is typically a static string set at counter creation time
    /// using the `with_name()` builder method. Returns an empty string if
    /// no name was set.
    fn name(&self) -> &str;

    /// Returns the current aggregated value of the counter.
    ///
    /// This method reads all shards and computes the aggregate value
    /// (sum for counters, min/max for extrema, average for Average).
    ///
    /// # Performance
    ///
    /// Reading requires iterating over all 64 shards, making it more
    /// expensive than a single atomic read. However, this is the right
    /// trade-off for counters where writes vastly outnumber reads.
    fn value(&self) -> CounterValue;

    /// Returns the current value and resets the counter atomically.
    ///
    /// This is useful for periodic metric collection where you want to
    /// capture the value since the last collection and start fresh.
    ///
    /// # Atomicity Note
    ///
    /// While each individual shard is reset atomically, the aggregate
    /// operation across all shards is not atomic. This means concurrent
    /// updates during `value_and_reset()` may be partially included in
    /// either the returned value or the next collection period. For
    /// metrics and statistics, this is typically acceptable.
    fn value_and_reset(&self) -> CounterValue;
}

impl Display for dyn Observable + '_ {
    /// Formats the counter as `name:value` if named, or just `value` otherwise.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if !self.name().is_empty() {
            write!(f, "{}:{}", self.name(), self.value())
        } else {
            write!(f, "{}", self.value())
        }
    }
}

/// Internal trait for accessing the thread-local component of a sharded counter.
///
/// This trait is used by counter implementations to get a reference to the
/// atomic value in the current thread's assigned shard.
///
/// # Safety
///
/// Implementors must ensure that the returned reference points to the correct
/// shard based on the thread's assigned slot index.
pub trait GetComponentCounter {
    /// The atomic type used for individual shards.
    type CounterType: Atomic;

    /// Returns a reference to the current thread's shard.
    ///
    /// This should use `THREAD_SLOT_INDEX` to determine which shard to return.
    fn get_component_counter(&self) -> &Self::CounterType;
}
