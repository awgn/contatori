//! Table observer for pretty-printing counters.
//!
//! This module provides [`TableObserver`], which renders a collection of
//! [`Observable`] counters as a formatted ASCII table using the `tabled` crate.
//!
//! # Feature Flag
//!
//! This module requires the `table` feature:
//!
//! ```toml
//! [dependencies]
//! contatori = { version = "0.7", features = ["table"] }
//! ```
//!
//! # Examples
//!
//! ## Standard format (vertical list)
//!
//! ```rust,ignore
//! use contatori::counters::unsigned::Unsigned;
//! use contatori::counters::Observable;
//! use contatori::observers::table::{TableObserver, TableStyle};
//!
//! let requests = Unsigned::new().with_name("requests");
//! let errors = Unsigned::new().with_name("errors");
//!
//! requests.add(1000);
//! errors.add(5);
//!
//! let counters: Vec<&dyn Observable> = vec![&requests, &errors];
//!
//! let observer = TableObserver::new().with_style(TableStyle::Rounded);
//! println!("{}", observer.render(counters.into_iter()));
//! // ╭──────────┬───────╮
//! // │ Name     │ Value │
//! // ├──────────┼───────┤
//! // │ requests │ 1000  │
//! // │ errors   │ 5     │
//! // ╰──────────┴───────╯
//! ```
//!
//! ## Compact format (multiple columns)
//!
//! ```rust,ignore
//! use contatori::observers::table::{TableObserver, TableStyle};
//!
//! let observer = TableObserver::new()
//!     .compact(true)
//!     .columns(3);
//!
//! println!("{}", observer.render(counters.into_iter()));
//! // ╭────────────────┬────────────┬──────────────╮
//! // │ requests: 1000 │ errors: 5  │ latency: 120 │
//! // ├────────────────┼────────────┼──────────────┤
//! // │ bytes: 2048    │ conns: 8   │              │
//! // ╰────────────────┴────────────┴──────────────╯
//! ```

use crate::counters::Observable;
use tabled::{builder::Builder, settings::Style, Table, Tabled};

/// Available table styles for rendering.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TableStyle {
    /// ASCII table with simple characters: +, -, |
    Ascii,
    /// Modern rounded corners (default)
    #[default]
    Rounded,
    /// Sharp corners with box-drawing characters
    Sharp,
    /// Modern style with clean lines
    Modern,
    /// Extended ASCII characters
    Extended,
    /// GitHub-flavored Markdown table
    Markdown,
    /// ReStructuredText table
    ReStructuredText,
    /// Dots for borders
    Dots,
    /// No borders, just spacing
    Blank,
    /// Double-line borders
    Double,
}

/// Separator style between name and value in compact mode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CompactSeparator {
    /// Colon separator: "name: value"
    #[default]
    Colon,
    /// Equals separator: "name = value"
    Equals,
    /// Arrow separator: "name → value"
    Arrow,
    /// Pipe separator: "name | value"
    Pipe,
    /// No separator, just space: "name value"
    Space,
}

impl CompactSeparator {
    /// Returns the separator string.
    pub fn as_str(&self) -> &'static str {
        match self {
            CompactSeparator::Colon => ": ",
            CompactSeparator::Equals => " = ",
            CompactSeparator::Arrow => " → ",
            CompactSeparator::Pipe => " | ",
            CompactSeparator::Space => " ",
        }
    }
}

/// Configuration for the table observer.
#[derive(Debug, Clone)]
pub struct TableConfig {
    /// The style to use for rendering.
    pub style: TableStyle,
    /// Whether to show the header row (only in non-compact mode).
    pub show_header: bool,
    /// Custom title for the table (optional).
    pub title: Option<String>,
    /// Whether to use compact format (name: value in cells).
    pub compact: bool,
    /// Number of columns in compact mode (default: 1).
    pub columns: usize,
    /// Separator between name and value in compact mode.
    pub separator: CompactSeparator,
    /// Placeholder for empty cells in compact mode.
    pub empty_cell: String,
}

impl Default for TableConfig {
    fn default() -> Self {
        Self {
            style: TableStyle::default(),
            show_header: true,
            title: None,
            compact: false,
            columns: 1,
            separator: CompactSeparator::default(),
            empty_cell: String::new(),
        }
    }
}

/// Internal row representation for tabled (standard mode).
#[derive(Tabled)]
struct CounterRow {
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Labels")]
    labels: String,
    #[tabled(rename = "Value")]
    value: String,
}

/// An observer that renders counters as a formatted ASCII table.
///
/// Supports two rendering modes:
///
/// 1. **Standard mode**: Traditional two-column table with Name and Value headers
/// 2. **Compact mode**: Multi-column grid with "name: value" cells
///
/// # Examples
///
/// Standard mode:
///
/// ```rust,ignore
/// use contatori::counters::unsigned::Unsigned;
/// use contatori::counters::Observable;
/// use contatori::observers::table::TableObserver;
///
/// let counter = Unsigned::new().with_name("requests");
/// counter.add(42);
///
/// let counters: Vec<&dyn Observable> = vec![&counter];
/// let output = TableObserver::new().render(counters.into_iter());
/// ```
///
/// Compact mode with 3 columns:
///
/// ```rust,ignore
/// use contatori::observers::table::{TableObserver, TableStyle, CompactSeparator};
///
/// let observer = TableObserver::new()
///     .compact(true)
///     .columns(3)
///     .separator(CompactSeparator::Colon)
///     .with_style(TableStyle::Rounded);
///
/// let output = observer.render(counters.into_iter());
/// ```
#[derive(Debug, Clone, Default)]
pub struct TableObserver {
    config: TableConfig,
}

impl TableObserver {
    /// Creates a new table observer with default settings.
    ///
    /// Default style is [`TableStyle::Rounded`] in standard (non-compact) mode.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a new table observer with the specified configuration.
    pub fn with_config(config: TableConfig) -> Self {
        Self { config }
    }

    /// Sets the table style.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use contatori::observers::table::{TableObserver, TableStyle};
    ///
    /// let observer = TableObserver::new().with_style(TableStyle::Ascii);
    /// ```
    pub fn with_style(mut self, style: TableStyle) -> Self {
        self.config.style = style;
        self
    }

    /// Sets whether to show the header row.
    ///
    /// Only applies in standard (non-compact) mode.
    pub fn with_header(mut self, show: bool) -> Self {
        self.config.show_header = show;
        self
    }

    /// Sets an optional title for the table.
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.config.title = Some(title.into());
        self
    }

    /// Enables or disables compact mode.
    ///
    /// In compact mode, counters are displayed as "name: value" cells
    /// arranged in a grid with the specified number of columns.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let observer = TableObserver::new()
    ///     .compact(true)
    ///     .columns(4);
    /// ```
    pub fn compact(mut self, enabled: bool) -> Self {
        self.config.compact = enabled;
        self
    }

    /// Sets the number of columns in compact mode.
    ///
    /// Default is 1. Values less than 1 are treated as 1.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Display counters in 3 columns
    /// let observer = TableObserver::new()
    ///     .compact(true)
    ///     .columns(3);
    /// ```
    pub fn columns(mut self, count: usize) -> Self {
        self.config.columns = count.max(1);
        self
    }

    /// Sets the separator between name and value in compact mode.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use contatori::observers::table::{TableObserver, CompactSeparator};
    ///
    /// let observer = TableObserver::new()
    ///     .compact(true)
    ///     .separator(CompactSeparator::Arrow); // "name → value"
    /// ```
    pub fn separator(mut self, sep: CompactSeparator) -> Self {
        self.config.separator = sep;
        self
    }

    /// Sets the placeholder text for empty cells in compact mode.
    ///
    /// Default is an empty string.
    pub fn empty_cell(mut self, placeholder: impl Into<String>) -> Self {
        self.config.empty_cell = placeholder.into();
        self
    }

    /// Applies the configured style to a table.
    fn apply_style(&self, table: &mut Table) {
        match self.config.style {
            TableStyle::Ascii => {
                table.with(Style::ascii());
            }
            TableStyle::Rounded => {
                table.with(Style::rounded());
            }
            TableStyle::Sharp => {
                table.with(Style::sharp());
            }
            TableStyle::Modern => {
                table.with(Style::modern());
            }
            TableStyle::Extended => {
                table.with(Style::extended());
            }
            TableStyle::Markdown => {
                table.with(Style::markdown());
            }
            TableStyle::ReStructuredText => {
                table.with(Style::re_structured_text());
            }
            TableStyle::Dots => {
                table.with(Style::dots());
            }
            TableStyle::Blank => {
                table.with(Style::blank());
            }
            TableStyle::Double => {
                table.with(Style::ascii());
            } // Fallback
        }
    }

    /// Formats a counter as a compact cell string.
    fn format_compact_cell(&self, name: &str, value: &str) -> String {
        format!("{}{}{}", name, self.config.separator.as_str(), value)
    }

    /// Renders counters in compact mode (grid layout).
    fn render_compact<'a>(&self, counters: impl Iterator<Item = &'a dyn Observable>) -> String {
        let cells: Vec<String> = counters
            .flat_map(|c| c.expand())
            .map(|entry| {
                let name = if entry.name.is_empty() {
                    "(unnamed)".to_string()
                } else if entry.label.is_none() {
                    entry.name.to_string()
                } else {
                    // Format as name{label=value}
                    let (k, v) = entry.label.as_ref().unwrap();
                    format!("{}{{{}={}}}", entry.name, k, v)
                };
                self.format_compact_cell(&name, &entry.value.to_string())
            })
            .collect();

        if cells.is_empty() {
            return String::new();
        }

        let cols = self.config.columns;
        let mut builder = Builder::default();

        for chunk in cells.chunks(cols) {
            let mut row: Vec<String> = chunk.to_vec();
            // Pad the last row with empty cells
            while row.len() < cols {
                row.push(self.config.empty_cell.clone());
            }
            builder.push_record(row);
        }

        let mut table = builder.build();
        self.apply_style(&mut table);

        if let Some(ref title) = self.config.title {
            format!("{}\n{}", title, table)
        } else {
            table.to_string()
        }
    }

    /// Renders counters in standard mode (three-column table).
    fn render_standard<'a>(&self, counters: impl Iterator<Item = &'a dyn Observable>) -> String {
        let rows: Vec<CounterRow> = counters
            .flat_map(|c| c.expand())
            .map(|entry| {
                let labels_str = match &entry.label {
                    None => String::new(),
                    Some((k, v)) => format!("{}={}", k, v),
                };
                CounterRow {
                    name: if entry.name.is_empty() {
                        "(unnamed)".to_string()
                    } else {
                        entry.name.to_string()
                    },
                    labels: labels_str,
                    value: entry.value.to_string(),
                }
            })
            .collect();

        let mut table = Table::new(&rows);
        self.apply_style(&mut table);

        if !self.config.show_header {
            table.with(tabled::settings::Remove::row(
                tabled::settings::object::Rows::first(),
            ));
        }

        if let Some(ref title) = self.config.title {
            format!("{}\n{}", title, table)
        } else {
            table.to_string()
        }
    }

    /// Renders the counters as a formatted table string.
    ///
    /// # Arguments
    ///
    /// * `counters` - An iterator over references to [`Observable`] trait objects
    ///
    /// # Returns
    ///
    /// A `String` containing the formatted table.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use contatori::counters::unsigned::Unsigned;
    /// use contatori::counters::Observable;
    /// use contatori::observers::table::TableObserver;
    ///
    /// let requests = Unsigned::new().with_name("requests");
    /// let errors = Unsigned::new().with_name("errors");
    ///
    /// requests.add(100);
    /// errors.add(5);
    ///
    /// let counters: Vec<&dyn Observable> = vec![&requests, &errors];
    ///
    /// // Standard mode
    /// let table = TableObserver::new().render(counters.iter().copied());
    ///
    /// // Compact mode with 2 columns
    /// let table = TableObserver::new()
    ///     .compact(true)
    ///     .columns(2)
    ///     .render(counters.iter().copied());
    /// ```
    pub fn render<'a>(&self, counters: impl Iterator<Item = &'a dyn Observable>) -> String {
        if self.config.compact {
            self.render_compact(counters)
        } else {
            self.render_standard(counters)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::counters::average::Average;
    use crate::counters::maximum::Maximum;
    use crate::counters::minimum::Minimum;
    use crate::counters::signed::Signed;
    use crate::counters::unsigned::Unsigned;

    #[test]
    fn test_render_empty() {
        let observer = TableObserver::new();
        let counters: Vec<&dyn Observable> = vec![];
        let output = observer.render(counters.into_iter());
        assert!(!output.is_empty());
    }

    #[test]
    fn test_render_empty_compact() {
        let observer = TableObserver::new().compact(true).columns(3);
        let counters: Vec<&dyn Observable> = vec![];
        let output = observer.render(counters.into_iter());
        assert!(output.is_empty());
    }

    #[test]
    fn test_render_single_counter() {
        let counter = Unsigned::new().with_name("test_counter");
        counter.add(42);

        let observer = TableObserver::new();
        let counters: Vec<&dyn Observable> = vec![&counter];
        let output = observer.render(counters.into_iter());

        assert!(output.contains("test_counter"));
        assert!(output.contains("42"));
    }

    #[test]
    fn test_render_compact_single() {
        let counter = Unsigned::new().with_name("requests");
        counter.add(100);

        let observer = TableObserver::new().compact(true);
        let counters: Vec<&dyn Observable> = vec![&counter];
        let output = observer.render(counters.into_iter());

        assert!(output.contains("requests: 100"));
    }

    #[test]
    fn test_render_compact_multiple_columns() {
        let c1 = Unsigned::new().with_name("a");
        let c2 = Unsigned::new().with_name("b");
        let c3 = Unsigned::new().with_name("c");
        let c4 = Unsigned::new().with_name("d");
        let c5 = Unsigned::new().with_name("e");

        c1.add(1);
        c2.add(2);
        c3.add(3);
        c4.add(4);
        c5.add(5);

        let observer = TableObserver::new().compact(true).columns(3);
        let counters: Vec<&dyn Observable> = vec![&c1, &c2, &c3, &c4, &c5];
        let output = observer.render(counters.into_iter());

        assert!(output.contains("a: 1"));
        assert!(output.contains("b: 2"));
        assert!(output.contains("c: 3"));
        assert!(output.contains("d: 4"));
        assert!(output.contains("e: 5"));
    }

    #[test]
    fn test_render_compact_with_separator() {
        let counter = Unsigned::new().with_name("test");
        counter.add(42);

        let counters: Vec<&dyn Observable> = vec![&counter];

        // Test different separators
        let observer = TableObserver::new()
            .compact(true)
            .separator(CompactSeparator::Equals);
        let output = observer.render(counters.iter().copied());
        assert!(output.contains("test = 42"));

        let observer = TableObserver::new()
            .compact(true)
            .separator(CompactSeparator::Arrow);
        let output = observer.render(counters.iter().copied());
        assert!(output.contains("test → 42"));

        let observer = TableObserver::new()
            .compact(true)
            .separator(CompactSeparator::Pipe);
        let output = observer.render(counters.iter().copied());
        assert!(output.contains("test | 42"));
    }

    #[test]
    fn test_render_multiple_counters() {
        let requests = Unsigned::new().with_name("requests");
        let errors = Unsigned::new().with_name("errors");
        let balance = Signed::new().with_name("balance");

        requests.add(1000);
        errors.add(5);
        balance.sub(100);

        let observer = TableObserver::new();
        let counters: Vec<&dyn Observable> = vec![&requests, &errors, &balance];
        let output = observer.render(counters.into_iter());

        assert!(output.contains("requests"));
        assert!(output.contains("1000"));
        assert!(output.contains("errors"));
        assert!(output.contains("5"));
        assert!(output.contains("balance"));
        assert!(output.contains("-100"));
    }

    #[test]
    fn test_render_with_different_styles() {
        let counter = Unsigned::new().with_name("test");
        counter.add(1);

        let counters: Vec<&dyn Observable> = vec![&counter];

        let styles = [
            TableStyle::Ascii,
            TableStyle::Rounded,
            TableStyle::Sharp,
            TableStyle::Modern,
            TableStyle::Markdown,
            TableStyle::Blank,
        ];

        for style in styles {
            let observer = TableObserver::new().with_style(style);
            let output = observer.render(counters.iter().copied());
            assert!(!output.is_empty());
        }
    }

    #[test]
    fn test_render_compact_with_styles() {
        let counter = Unsigned::new().with_name("test");
        counter.add(1);

        let counters: Vec<&dyn Observable> = vec![&counter];

        let styles = [TableStyle::Ascii, TableStyle::Rounded, TableStyle::Sharp];

        for style in styles {
            let observer = TableObserver::new()
                .compact(true)
                .columns(2)
                .with_style(style);
            let output = observer.render(counters.iter().copied());
            assert!(output.contains("test: 1"));
        }
    }

    #[test]
    fn test_render_with_title() {
        let counter = Unsigned::new().with_name("metric");
        counter.add(123);

        let observer = TableObserver::new().with_title("My Metrics");
        let counters: Vec<&dyn Observable> = vec![&counter];
        let output = observer.render(counters.into_iter());

        assert!(output.starts_with("My Metrics"));
        assert!(output.contains("metric"));
        assert!(output.contains("123"));
    }

    #[test]
    fn test_render_compact_with_title() {
        let counter = Unsigned::new().with_name("metric");
        counter.add(123);

        let observer = TableObserver::new()
            .compact(true)
            .with_title("Compact Metrics");
        let counters: Vec<&dyn Observable> = vec![&counter];
        let output = observer.render(counters.into_iter());

        assert!(output.starts_with("Compact Metrics"));
        assert!(output.contains("metric: 123"));
    }

    #[test]
    fn test_render_unnamed_counter() {
        let counter = Unsigned::new();
        counter.add(99);

        let observer = TableObserver::new();
        let counters: Vec<&dyn Observable> = vec![&counter];
        let output = observer.render(counters.into_iter());

        assert!(output.contains("(unnamed)"));
        assert!(output.contains("99"));
    }

    #[test]
    fn test_render_compact_unnamed() {
        let counter = Unsigned::new();
        counter.add(99);

        let observer = TableObserver::new().compact(true);
        let counters: Vec<&dyn Observable> = vec![&counter];
        let output = observer.render(counters.into_iter());

        assert!(output.contains("(unnamed): 99"));
    }

    #[test]
    fn test_render_without_header() {
        let counter = Unsigned::new().with_name("test");
        counter.add(42);

        let observer = TableObserver::new().with_header(false);
        let counters: Vec<&dyn Observable> = vec![&counter];
        let output = observer.render(counters.into_iter());

        assert!(!output.contains("Name"));
        assert!(!output.contains("Value"));
        assert!(output.contains("test"));
        assert!(output.contains("42"));
    }

    #[test]
    fn test_render_all_counter_types() {
        let unsigned = Unsigned::new().with_name("unsigned");
        let signed = Signed::new().with_name("signed");
        let minimum = Minimum::new().with_name("minimum");
        let maximum = Maximum::new().with_name("maximum");
        let average = Average::new().with_name("average");

        unsigned.add(100);
        signed.sub(50);
        minimum.observe(25);
        maximum.observe(200);
        average.observe(100);
        average.observe(200);

        let counters: Vec<&dyn Observable> = vec![&unsigned, &signed, &minimum, &maximum, &average];

        let observer = TableObserver::new().with_style(TableStyle::Rounded);
        let output = observer.render(counters.into_iter());

        assert!(output.contains("unsigned"));
        assert!(output.contains("100"));
        assert!(output.contains("signed"));
        assert!(output.contains("-50"));
        assert!(output.contains("minimum"));
        assert!(output.contains("25"));
        assert!(output.contains("maximum"));
        assert!(output.contains("200"));
        assert!(output.contains("average"));
        assert!(output.contains("150"));
    }

    #[test]
    fn test_render_all_counter_types_compact() {
        let unsigned = Unsigned::new().with_name("uns");
        let signed = Signed::new().with_name("sig");
        let minimum = Minimum::new().with_name("min");
        let maximum = Maximum::new().with_name("max");
        let average = Average::new().with_name("avg");

        unsigned.add(100);
        signed.sub(50);
        minimum.observe(25);
        maximum.observe(200);
        average.observe(150);

        let counters: Vec<&dyn Observable> = vec![&unsigned, &signed, &minimum, &maximum, &average];

        let observer = TableObserver::new()
            .compact(true)
            .columns(3)
            .with_style(TableStyle::Rounded);
        let output = observer.render(counters.into_iter());

        assert!(output.contains("uns: 100"));
        assert!(output.contains("sig: -50"));
        assert!(output.contains("min: 25"));
        assert!(output.contains("max: 200"));
        assert!(output.contains("avg: 150"));
    }

    #[test]
    fn test_config_builder() {
        let config = TableConfig {
            style: TableStyle::Markdown,
            show_header: false,
            title: Some("Custom Title".to_string()),
            compact: true,
            columns: 4,
            separator: CompactSeparator::Arrow,
            empty_cell: "-".to_string(),
        };

        let observer = TableObserver::with_config(config);
        assert!(observer.config.title.is_some());
        assert!(observer.config.compact);
        assert_eq!(observer.config.columns, 4);
        assert_eq!(observer.config.separator, CompactSeparator::Arrow);
    }

    #[test]
    fn test_empty_cell_placeholder() {
        let c1 = Unsigned::new().with_name("a");
        let c2 = Unsigned::new().with_name("b");

        c1.add(1);
        c2.add(2);

        let observer = TableObserver::new()
            .compact(true)
            .columns(3)
            .empty_cell("-");

        let counters: Vec<&dyn Observable> = vec![&c1, &c2];
        let output = observer.render(counters.into_iter());

        // Should have one empty cell filled with "-"
        assert!(output.contains("a: 1"));
        assert!(output.contains("b: 2"));
        // The third column should have "-" as placeholder
        assert!(
            output.contains("-")
                || output
                    .lines()
                    .any(|l| l.contains("│ - │") || l.contains("│  │"))
        );
    }

    #[test]
    fn test_columns_min_value() {
        let observer = TableObserver::new().columns(0);
        assert_eq!(observer.config.columns, 1);
    }
}
