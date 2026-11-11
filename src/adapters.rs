//! Wrapper types and macros for extending counter functionality.
//!
//! - [`Resettable`] — resets counters (or entire labeled groups) when observed
//! - [`labeled_group!`](crate::labeled_group) — generates compile-time labeled counter groups
//!   (supporting both single-label and multi-label dimensions)

mod group;
mod resettable;

pub use resettable::Resettable;
