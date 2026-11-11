//! Table observer for pretty-printing counters using `tabled`.
//!
//! Requires the `table` feature.

use crate::counters::{Observable, ObservableEntry};
use tabled::{Table, Tabled, builder::Builder, settings::Style};

/// Available table styles for rendering.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TableStyle {
    /// ASCII table with simple characters (`+`, `-`, `|`).
    Ascii,
    /// Modern rounded corners (default).
    #[default]
    Rounded,
    /// Sharp corners with single-line box-drawing characters.
    Sharp,
    /// Modern style with clean horizontal/vertical lines.
    Modern,
    /// Extended double-line box-drawing characters (`╔`, `═`, `║`).
    Extended,
    /// GitHub-flavored Markdown table.
    Markdown,
    /// ReStructuredText table.
    ReStructuredText,
    /// Dotted borders.
    Dots,
    /// Borderless whitespace-aligned columns.
    Blank,
    /// Double-line box-drawing borders (`╔`, `═`, `║`).
    Double,
}

/// Separator style between name and value in compact mode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CompactSeparator {
    /// Colon separator: `"name: value"`.
    #[default]
    Colon,
    /// Equals separator: `"name = value"`.
    Equals,
    /// Arrow separator: `"name → value"`.
    Arrow,
    /// Pipe separator: `"name | value"`.
    Pipe,
    /// Space separator: `"name value"`.
    Space,
}

impl CompactSeparator {
    /// Returns the separator string slice.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            CompactSeparator::Colon => ": ",
            CompactSeparator::Equals => " = ",
            CompactSeparator::Arrow => " → ",
            CompactSeparator::Pipe => " | ",
            CompactSeparator::Space => " ",
        }
    }
}

/// Configuration for [`TableObserver`].
#[derive(Debug, Clone)]
pub struct TableConfig {
    /// Border style.
    pub style: TableStyle,
    /// Whether to display the header row in standard mode.
    pub show_header: bool,
    /// Optional title printed above the table.
    pub title: Option<String>,
    /// Whether to render a multi-column compact grid.
    pub compact: bool,
    /// Number of columns in compact mode (minimum `1`).
    pub columns: usize,
    /// Separator between metric name and value in compact mode.
    pub separator: CompactSeparator,
    /// Placeholder string for trailing empty cells in compact mode.
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

#[derive(Tabled)]
struct CounterRow {
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Labels")]
    labels: String,
    #[tabled(rename = "Value")]
    value: String,
}

/// An observer that renders [`Observable`] counters as formatted tables.
#[derive(Debug, Clone, Default)]
pub struct TableObserver {
    config: TableConfig,
}

impl TableObserver {
    /// Creates a new table observer with default settings (`Rounded`, standard mode).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a table observer from an explicit [`TableConfig`].
    #[must_use]
    pub fn with_config(config: TableConfig) -> Self {
        Self { config }
    }

    /// Sets the border style.
    #[must_use]
    pub fn with_style(mut self, style: TableStyle) -> Self {
        self.config.style = style;
        self
    }

    /// Sets whether to display the header row in standard mode.
    #[must_use]
    pub fn with_header(mut self, show: bool) -> Self {
        self.config.show_header = show;
        self
    }

    /// Sets an optional title above the rendered table.
    #[must_use]
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.config.title = Some(title.into());
        self
    }

    /// Enables or disables compact multi-column mode.
    #[must_use]
    pub fn compact(mut self, enabled: bool) -> Self {
        self.config.compact = enabled;
        self
    }

    /// Sets the number of columns in compact mode (clamped to at least `1`).
    #[must_use]
    pub fn columns(mut self, count: usize) -> Self {
        self.config.columns = count.max(1);
        self
    }

    /// Sets the name/value separator in compact mode.
    #[must_use]
    pub fn separator(mut self, sep: CompactSeparator) -> Self {
        self.config.separator = sep;
        self
    }

    /// Sets the placeholder text for trailing empty grid cells in compact mode.
    #[must_use]
    pub fn empty_cell(mut self, placeholder: impl Into<String>) -> Self {
        self.config.empty_cell = placeholder.into();
        self
    }

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
            TableStyle::Extended | TableStyle::Double => {
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
        }
    }

    fn format_labels_inline(labels: &[(&str, &str)]) -> String {
        let mut out = String::new();
        for (i, (k, v)) in labels.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            out.push_str(k);
            out.push('=');
            out.push_str(v);
        }
        out
    }

    fn collect_entries<'a>(counters: impl Iterator<Item = &'a dyn Observable>) -> Vec<ObservableEntry> {
        let mut entries = Vec::new();
        for counter in counters {
            counter.expand_into(&mut entries);
        }
        entries
    }

    fn render_compact<'a>(&self, counters: impl Iterator<Item = &'a dyn Observable>) -> String {
        let entries = Self::collect_entries(counters);
        let cells: Vec<String> = entries
            .into_iter()
            .filter_map(|entry| {
                let value = entry.value?;
                let base_name = if entry.name.is_empty() {
                    "(unnamed)"
                } else {
                    entry.name
                };
                let full_name = if entry.labels.is_empty() {
                    base_name.to_string()
                } else {
                    format!("{base_name}{{{}}}", Self::format_labels_inline(entry.labels))
                };
                Some(format!(
                    "{full_name}{}{value}",
                    self.config.separator.as_str()
                ))
            })
            .collect();

        if cells.is_empty() {
            return String::new();
        }

        let cols = self.config.columns;
        let mut builder = Builder::default();

        for chunk in cells.chunks(cols) {
            let mut row: Vec<String> = chunk.to_vec();
            while row.len() < cols {
                row.push(self.config.empty_cell.clone());
            }
            builder.push_record(row);
        }

        let mut table = builder.build();
        self.apply_style(&mut table);

        if let Some(title) = &self.config.title {
            format!("{title}\n{table}")
        } else {
            table.to_string()
        }
    }

    fn render_standard<'a>(&self, counters: impl Iterator<Item = &'a dyn Observable>) -> String {
        let entries = Self::collect_entries(counters);
        let rows: Vec<CounterRow> = entries
            .into_iter()
            .filter_map(|entry| {
                let value = entry.value?;
                Some(CounterRow {
                    name: if entry.name.is_empty() {
                        "(unnamed)".to_string()
                    } else {
                        entry.name.to_string()
                    },
                    labels: Self::format_labels_inline(entry.labels),
                    value: value.to_string(),
                })
            })
            .collect();

        let mut table = Table::new(&rows);
        self.apply_style(&mut table);

        if !self.config.show_header {
            table.with(tabled::settings::Remove::row(
                tabled::settings::object::Rows::first(),
            ));
        }

        if let Some(title) = &self.config.title {
            format!("{title}\n{table}")
        } else {
            table.to_string()
        }
    }

    /// Renders the given counters into a formatted table string.
    ///
    /// Uninitialized trackers (`try_value() == None`) are omitted.
    #[must_use]
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
    use crate::counters::minimum::Minimum;
    use crate::counters::unsigned::Unsigned;
    use crate::labeled_group;

    #[test]
    fn test_double_border_style_uses_double_lines() {
        let counter = Unsigned::new().with_name("req");
        counter.add(10);
        let counters: Vec<&dyn Observable> = vec![&counter];
        let output = TableObserver::new()
            .with_style(TableStyle::Double)
            .render(counters.into_iter());
        assert!(output.contains('═'));
        assert!(output.contains('║'));
    }

    #[test]
    fn test_omits_unobserved_trackers() {
        let min_empty = Minimum::new().with_name("empty_min");
        let req = Unsigned::new().with_name("req");
        req.add(5);

        let counters: Vec<&dyn Observable> = vec![&min_empty, &req];
        let output = TableObserver::new().render(counters.into_iter());
        assert!(!output.contains("empty_min"));
        assert!(output.contains("req"));
    }

    #[test]
    fn test_multi_label_rendering_standard_and_compact() {
        labeled_group!(
            RespGroup,
            "resp",
            ["method", "status"],
            get_200: ["GET", "200"]: Unsigned,
        );

        let g = RespGroup::new();
        g.get_200.add(7);

        let counters: Vec<&dyn Observable> = vec![&g];
        let std_out = TableObserver::new().render(counters.iter().copied());
        assert!(std_out.contains("method=GET, status=200"));

        let compact_out = TableObserver::new()
            .compact(true)
            .render(counters.into_iter());
        assert!(compact_out.contains("resp{method=GET, status=200}: 7"));
    }
}
