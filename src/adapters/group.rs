//! Labeled group macro for creating compile-time sets of labeled counters.
//!
//! [`labeled_group!`](crate::labeled_group) generates a struct whose fields are
//! independent sharded counters associated with static `(key, value)` label pairs.
//! Both single-label and multi-label dimensions are supported with zero runtime
//! lookup or allocation overhead.

/// Creates a struct of sharded counters sharing a metric name with static labels.
///
/// Unlike dynamic label maps, all label slices are constructed in `static` memory
/// at compile time, providing direct field access on the hot path and avoiding
/// double-counting when aggregating in Prometheus or OpenTelemetry.
///
/// # Single-Label Syntax
///
/// ```rust
/// use contatori::{Monotone, Observable, labeled_group};
///
/// labeled_group!(
///     HttpRequests,
///     "http_requests_total",
///     "method",
///     get: "GET": Monotone,
///     post: "POST": Monotone,
/// );
///
/// static HTTP: HttpRequests = HttpRequests::new();
/// HTTP.get.incr();
/// HTTP.post.add(2);
///
/// assert_eq!(HTTP.total().as_u64(), 3);
/// assert_eq!(HTTP.expand().len(), 2);
/// ```
///
/// # Multi-Label Syntax
///
/// ```rust
/// use contatori::{Monotone, Observable, labeled_group};
///
/// labeled_group!(
///     HttpResponses,
///     "http_responses_total",
///     ["method", "status"],
///     get_200: ["GET", "200"]: Monotone,
///     get_404: ["GET", "404"]: Monotone,
///     post_201: ["POST", "201"]: Monotone,
/// );
///
/// static RESP: HttpResponses = HttpResponses::new();
/// RESP.get_200.add(10);
/// RESP.get_404.incr();
///
/// let entries = RESP.expand();
/// assert_eq!(entries[0].labels, &[("method", "GET"), ("status", "200")]);
/// assert_eq!(RESP.total().as_u64(), 11);
/// ```
#[macro_export]
macro_rules! labeled_group {
    // Single-label shorthand -> delegates to the multi-label arm
    (
        $(#[$meta:meta])*
        $name:ident,
        $metric:literal,
        $label_key:literal,
        $( $field:ident : $label_val:literal : $ty:ty ),+ $(,)?
    ) => {
        $crate::labeled_group!(
            $(#[$meta])*
            $name,
            $metric,
            [$label_key],
            $( $field : [$label_val] : $ty ),+
        );
    };

    // Multi-label canonical arm
    (
        $(#[$meta:meta])*
        $name:ident,
        $metric:literal,
        [ $( $label_key:literal ),+ $(,)? ],
        $( $field:ident : [ $( $label_val:literal ),+ $(,)? ] : $ty:ty ),+ $(,)?
    ) => {
        $(#[$meta])*
        pub struct $name {
            $(pub $field: $ty,)+
        }

        #[allow(dead_code)]
        impl $name {
            /// Creates a new group with all sub-counters initialized and named.
            #[must_use]
            pub const fn new() -> Self {
                Self {
                    $($field: <$ty>::with_shards().with_name($metric),)+
                }
            }

            /// Returns the shared metric name for this group.
            #[must_use]
            #[inline]
            pub const fn metric_name(&self) -> &'static str {
                $metric
            }

            /// Returns all label keys defined for this group.
            #[must_use]
            #[inline]
            pub const fn label_keys(&self) -> &'static [&'static str] {
                &[ $( $label_key ),+ ]
            }

            /// Returns the first label key defined for this group.
            #[must_use]
            #[inline]
            pub const fn label_key(&self) -> &'static str {
                self.label_keys()[0]
            }

            /// Computes the sum of all observed sub-counters in this group.
            #[must_use]
            pub fn total(&self) -> $crate::counters::CounterValue {
                use $crate::counters::{CounterValue, Observable};
                let mut acc: ::core::option::Option<CounterValue> = ::core::option::Option::None;
                $(
                    if let ::core::option::Option::Some(v) = Observable::try_value(&self.$field) {
                        acc = ::core::option::Option::Some(match acc {
                            ::core::option::Option::Some(cur) => cur + v,
                            ::core::option::Option::None => v,
                        });
                    }
                )+
                acc.unwrap_or(CounterValue::Unsigned(0))
            }
        }

        impl $crate::counters::Observable for $name {
            #[inline]
            fn name(&self) -> &'static str {
                $metric
            }

            #[inline]
            fn value(&self) -> $crate::counters::CounterValue {
                self.total()
            }

            fn try_value(&self) -> ::core::option::Option<$crate::counters::CounterValue> {
                let mut acc: ::core::option::Option<$crate::counters::CounterValue> =
                    ::core::option::Option::None;
                $(
                    if let ::core::option::Option::Some(v) =
                        $crate::counters::Observable::try_value(&self.$field)
                    {
                        acc = ::core::option::Option::Some(match acc {
                            ::core::option::Option::Some(cur) => cur + v,
                            ::core::option::Option::None => v,
                        });
                    }
                )+
                acc
            }

            #[inline]
            fn metric_kind(&self) -> $crate::counters::MetricKind {
                #[allow(unreachable_code)]
                {
                    $(
                        return $crate::counters::Observable::metric_kind(&self.$field);
                    )+
                }
            }

            fn expand_into(&self, out: &mut ::std::vec::Vec<$crate::counters::ObservableEntry>) {
                const LABEL_KEYS: &[&str] = &[ $( $label_key ),+ ];
                const FIELD_COUNT: usize = <[()]>::len(&[ $( { let _ = stringify!($field); } ),+ ]);
                out.reserve(FIELD_COUNT);

                $(
                    {
                        const N_VALS: usize = <[&str]>::len(&[ $( $label_val ),+ ]);
                        static FIELD_LABELS: [(&'static str, &'static str); N_VALS] = {
                            const VALS: [&str; N_VALS] = [ $( $label_val ),+ ];
                            assert!(
                                LABEL_KEYS.len() == N_VALS,
                                "labeled_group!: number of label values must match number of label keys"
                            );
                            let mut arr = [("", ""); N_VALS];
                            let mut i = 0;
                            while i < N_VALS {
                                arr[i] = (LABEL_KEYS[i], VALS[i]);
                                i += 1;
                            }
                            arr
                        };
                        out.push($crate::counters::ObservableEntry {
                            name: $metric,
                            labels: &FIELD_LABELS,
                            value: $crate::counters::Observable::try_value(&self.$field),
                            metric_kind: $crate::counters::Observable::metric_kind(&self.$field),
                        });
                    }
                )+
            }
        }

        impl $crate::counters::sealed::Resettable for $name {
            fn try_value_and_reset(&self) -> ::core::option::Option<$crate::counters::CounterValue> {
                let mut acc: ::core::option::Option<$crate::counters::CounterValue> =
                    ::core::option::Option::None;
                $(
                    if let ::core::option::Option::Some(v) =
                        $crate::counters::sealed::Resettable::try_value_and_reset(&self.$field)
                    {
                        acc = ::core::option::Option::Some(match acc {
                            ::core::option::Option::Some(cur) => cur + v,
                            ::core::option::Option::None => v,
                        });
                    }
                )+
                acc
            }

            fn value_and_reset(&self) -> $crate::counters::CounterValue {
                self.try_value_and_reset()
                    .unwrap_or($crate::counters::CounterValue::Unsigned(0))
            }

            fn expand_and_reset_into(
                &self,
                out: &mut ::std::vec::Vec<$crate::counters::ObservableEntry>,
            ) {
                const LABEL_KEYS: &[&str] = &[ $( $label_key ),+ ];
                const FIELD_COUNT: usize = <[()]>::len(&[ $( { let _ = stringify!($field); } ),+ ]);
                out.reserve(FIELD_COUNT);

                $(
                    {
                        const N_VALS: usize = <[&str]>::len(&[ $( $label_val ),+ ]);
                        static FIELD_LABELS: [(&'static str, &'static str); N_VALS] = {
                            const VALS: [&str; N_VALS] = [ $( $label_val ),+ ];
                            assert!(
                                LABEL_KEYS.len() == N_VALS,
                                "labeled_group!: number of label values must match number of label keys"
                            );
                            let mut arr = [("", ""); N_VALS];
                            let mut i = 0;
                            while i < N_VALS {
                                arr[i] = (LABEL_KEYS[i], VALS[i]);
                                i += 1;
                            }
                            arr
                        };
                        out.push($crate::counters::ObservableEntry {
                            name: $metric,
                            labels: &FIELD_LABELS,
                            value: $crate::counters::sealed::Resettable::try_value_and_reset(
                                &self.$field,
                            ),
                            metric_kind: $crate::counters::Observable::metric_kind(&self.$field),
                        });
                    }
                )+
            }
        }

        impl ::core::fmt::Debug for $name {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                f.debug_struct(stringify!($name))
                    $(.field(stringify!($field), &self.$field))+
                    .finish()
            }
        }

        impl ::core::default::Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use crate::adapters::Resettable;
    use crate::counters::monotone::Monotone;
    use crate::counters::unsigned::Unsigned;
    use crate::counters::{CounterValue, MetricKind, Observable};

    labeled_group!(
        TestRequests,
        "test_requests",
        "method",
        get: "GET": Monotone,
        post: "POST": Monotone,
    );

    labeled_group!(
        TestMultiLabel,
        "http_responses_total",
        ["method", "status"],
        get_ok: ["GET", "200"]: Unsigned<16>,
        post_created: ["POST", "201"]: Unsigned<16>,
    );

    #[test]
    fn test_single_label_and_metric_kind_delegation() {
        let requests = TestRequests::new();
        assert_eq!(requests.metric_name(), "test_requests");
        assert_eq!(requests.label_key(), "method");
        assert_eq!(requests.label_keys(), &["method"]);
        // Delegates MetricKind::Counter from Monotone!
        assert_eq!(requests.metric_kind(), MetricKind::Counter);
        assert_eq!(requests.get.name(), "test_requests");

        requests.get.add(60);
        requests.post.add(40);
        assert_eq!(requests.total(), CounterValue::Unsigned(100));
        assert_eq!(requests.value(), CounterValue::Unsigned(100));

        let entries = requests.expand();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].labels, &[("method", "GET")]);
        assert_eq!(entries[0].value, Some(CounterValue::Unsigned(60)));
        assert_eq!(entries[1].labels, &[("method", "POST")]);
        assert_eq!(entries[1].value, Some(CounterValue::Unsigned(40)));
    }

    #[test]
    fn test_multi_label_and_resettable_group() {
        let group = Resettable::new(TestMultiLabel::new());
        assert_eq!(group.metric_kind(), MetricKind::Gauge);
        assert_eq!(group.label_keys(), &["method", "status"]);

        group.get_ok.add(10);
        group.post_created.add(5);

        let entries = group.expand();
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[0].labels,
            &[("method", "GET"), ("status", "200")]
        );
        assert_eq!(entries[0].value, Some(CounterValue::Unsigned(10)));
        assert_eq!(
            entries[1].labels,
            &[("method", "POST"), ("status", "201")]
        );
        assert_eq!(entries[1].value, Some(CounterValue::Unsigned(5)));

        // Second expand after reset returns 0 for both Unsigned sub-counters
        let reset_entries = group.expand();
        assert_eq!(reset_entries[0].value, Some(CounterValue::Unsigned(0)));
        assert_eq!(reset_entries[1].value, Some(CounterValue::Unsigned(0)));
    }

    #[test]
    fn test_static_initialization() {
        static STATIC_GROUP: TestRequests = TestRequests::new();
        STATIC_GROUP.get.incr();
        assert!(STATIC_GROUP.total().as_u64() >= 1);
    }
}
