//! Wrapper types for extending counter functionality.
//!
//! This module provides wrapper types that add additional behavior to
//! counters while maintaining compatibility with the [`Observable`] trait.
//!
//! # Available Wrappers
//!
//! | Wrapper | Description |
//! |---------|-------------|
//! | [`NonResettable`] | Prevents reset on `value_and_reset()` - for monotonic counters |
//! | [`Labeled`] | Adds key-value labels/tags to a counter |
//!
//! # Examples
//!
//! ## NonResettable Counter
//!
//! ```rust,ignore
//! use contatori::counters::unsigned::Unsigned;
//! use contatori::adapters::NonResettable;
//!
//! let total_requests = NonResettable::new(Unsigned::new().with_name("total_requests"));
//! total_requests.add(100);
//!
//! // value_and_reset() returns value but does NOT reset
//! assert_eq!(total_requests.value_and_reset().as_u64(), 100);
//! assert_eq!(total_requests.value().as_u64(), 100); // Still 100!
//! ```
//!
//! ## Labeled Counter
//!
//! ```rust,ignore
//! use contatori::counters::unsigned::Unsigned;
//! use contatori::adapters::Labeled;
//!
//! let requests = Labeled::new(Unsigned::new().with_name("http_requests"))
//!     .with_label("method", "GET")
//!     .with_label("path", "/api/users");
//!
//! // Labels are accessible for observers (e.g., Prometheus)
//! for (key, value) in requests.labels() {
//!     println!("{}: {}", key, value);
//! }
//! ```
//!

mod labeled;
mod non_resettable;

pub use labeled::Labeled;
pub use non_resettable::NonResettable;
