//! Wrapper types and macros for extending counter functionality.
//!
//! - [`Local`] — thread-local / batch-scoped write proxy that buffers updates in non-atomic
//!   memory and flushes once per batch (explicitly via [`Local::flush()`] or on `Drop`)
//! - [`Resettable`] — resets counters (or entire labeled groups) when observed
//! - [`labeled_group!`](crate::labeled_group) — generates compile-time labeled counter groups
//!   (supporting both single-label and multi-label dimensions)

mod group;
pub mod local;
mod resettable;

pub use local::{Batchable, Local};
pub use resettable::Resettable;
