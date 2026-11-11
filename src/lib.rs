#![doc = include_str!("../README.md")]

pub mod adapters;
pub mod counters;
pub mod observers;

#[cfg(feature = "serde")]
pub mod snapshot;

// Ergonomic root re-exports
pub use adapters::{Batchable, Local, Resettable};
pub use counters::{
    CounterValue, DEFAULT_SHARDS, MetricKind, Observable, ObservableEntry, average::Average,
    maximum::Maximum, minimum::Minimum, monotone::Monotone, rate::Rate, signed::Signed,
    unsigned::Unsigned,
};

/// Convenience prelude re-exporting all counter types, adapters, and core traits.
pub mod prelude {
    pub use crate::adapters::{Batchable, Local, Resettable};
    pub use crate::counters::{
        CounterValue, DEFAULT_SHARDS, MetricKind, Observable, ObservableEntry, average::Average,
        maximum::Maximum, minimum::Minimum, monotone::Monotone, rate::Rate, signed::Signed,
        unsigned::Unsigned,
    };
    pub use crate::labeled_group;
}
