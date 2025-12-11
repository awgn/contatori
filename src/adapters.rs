//! Wrapper types for extending counter functionality.
//!
//! This module provides wrapper types that add additional behavior to
//! counters while maintaining compatibility with the [`Observable`](crate::counters::Observable) trait.
//!
//! # Available Wrappers
//!
//! | Wrapper | Description |
//! |---------|-------------|
//! | [`Resettable`] | Resets counter when `value()` is called - for periodic metrics |
//!
//! # Macros
//!
//! | Macro | Description |
//! |-------|-------------|
//! | [`labeled_group!`](crate::labeled_group) | Creates a struct of labeled counters |
//!
//! # Examples
//!
//! ## Resettable Counter
//!
//! ```rust
//! use contatori::counters::unsigned::Unsigned;
//! use contatori::counters::Observable;
//! use contatori::adapters::Resettable;
//!
//! let requests_per_period = Resettable::new(Unsigned::new().with_name("requests_per_period"));
//! requests_per_period.add(100);
//!
//! // value() returns the value AND resets the counter
//! assert_eq!(requests_per_period.value().as_u64(), 100);
//! assert_eq!(requests_per_period.value().as_u64(), 0); // Reset to 0!
//! ```
//!
//! ## Labeled Group
//!
//! ```rust
//! use contatori::labeled_group;
//! use contatori::counters::unsigned::Unsigned;
//! use contatori::counters::Observable;
//!
//! labeled_group!(
//!     HttpRequests,
//!     "http_requests",
//!     "method",
//!     value: Unsigned,
//!     get: "GET": Unsigned,
//!     post: "POST": Unsigned,
//! );
//!
//! static HTTP: HttpRequests = HttpRequests::new();
//!
//! // Direct field access for incrementing
//! HTTP.value.add(1);
//! HTTP.get.add(1);
//!
//! // expand() returns all sub-counters with their label
//! for entry in HTTP.expand() {
//!     println!("{}: {:?}", entry.name, entry.label);
//! }
//! ```

mod group;
mod resettable;

pub use resettable::Resettable;
