//! Output formatting utilities
//!
//! This module provides functions for formatting and displaying
//! output throughout the application in various formats.

use std::fmt::{self, Display, Formatter};
use std::io::Write;

/// Output format enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// Human-readable text format
    Text,
    /// JSON format
    Json,
    /// YAML format
    Yaml,
    /// Markdown format
    Markdown,
    /// CSV format
    Csv,
    /// Table format
    Table,
}

impl Display for OutputFormat {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            OutputFormat::Text => write!(f, "text"),
            OutputFormat::Json => write!(f, "json"),
            OutputFormat::Yaml => write!(f, "yaml"),
            OutputFormat::Markdown => write!(f, "markdown"),
            OutputFormat::Csv => write!(f, "csv"),
            OutputFormat::Table => write!(f, "table"),
        }
    }
}

/// Output style enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputStyle {
    /// Plain text
    Plain,
    /// Colored output
    Colored,
    /// ANSI color codes
    Ansi,
    /// Terminal-friendly
    Terminal,
}

/// Output builder
pub struct OutputBuilder {
    format: OutputFormat,
    style: OutputStyle,
    verbose: bool,
}

impl OutputBuilder {
    /// Create a new output builder
    pub fn new() -> Self {
        Self {
            format: OutputFormat::Text,
            style: OutputStyle::Terminal,
            verbose: false,
        }
    }

    /// Set output format
    pub fn format(mut self, format: OutputFormat) -> Self {
        self.format = format;
        self
    }

    /// Set output style
    pub fn style(mut self, style: OutputStyle) -> Self {
        self.style = style;
        self
    }

    /// Set verbose mode
    pub fn verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
        self
    }

    /// Build output
    pub fn build(self) -> OutputFormatter {
        OutputFormatter {
            format: self.format,
            style: self.style,
            verbose: self.verbose,
        }
    }
}

/// Output formatter
pub struct OutputFormatter {
    format: OutputFormat,
    style: OutputStyle,
    verbose: bool,
}

impl OutputFormatter {
    /// Create a new output formatter
    pub fn new() -> Self {
        Self::default()
    }

    /// Format text output
    pub fn format_text(&self, text: &str) -> String {
        if self.verbose {
            format!("[TEXT] {}", text)
        } else {
            text.to_string()
        }
    }

    /// Format JSON output
    pub fn format_json(&self, json: &str) -> String {
        match self.style {
            OutputStyle::Colored => {
                format!("\x1b[36m[JSON]\x1b[0m {}", json)
            }
            OutputStyle::Terminal => {
                format!("[JSON] {}", json)
            }
            _ => json.to_string(),
        }
    }

    /// Format YAML output
    pub fn format_yaml(&self, yaml: &str) -> String {
        match self.style {
            OutputStyle::Colored => {
                format!("\x1b[33m[YAML]\x1b[0m {}", yaml)
            }
            OutputStyle::Terminal => {
                format!("[YAML] {}", yaml)
            }
            _ => yaml.to_string(),
        }
    }

    /// Format Markdown output
    pub fn format_markdown(&self, md: &str) -> String {
        match self.style {
            OutputStyle::Colored => {
                format!("\x1b[35m[MARKDOWN]\x1b[0m {}", md)
            }
            OutputStyle::Terminal => {
                format!("[MARKDOWN] {}", md)
            }
            _ => md.to_string(),
        }
    }

    /// Format table output
    pub fn format_table(&self, rows: &[Vec<String>]) -> String {
        if rows.is_empty() {
            return String::new();
        }

        // Calculate column widths
        let num_cols = rows[0].len();
        let mut col_widths = vec![0usize; num_cols];

        for row in rows {
            for (col_idx, cell) in row.iter().enumerate() {
                col_widths[col_idx] = col_widths[col_idx].max(cell.len());
            }
        }

        // Build table
        let mut output = String::new();

        for row in rows {
            for (col_idx, cell) in row.iter().enumerate() {
                if col_idx > 0 {
                    output.push(' ');
                }
                output.push_str(&format!("{:<width$}", cell, width = col_widths[col_idx]));
            }
            output.push('\n');
        }

        if self.verbose {
            output = format!("[TABLE]\n{}", output);
        }

        output
    }

    /// Format CSV output
    pub fn format_csv(&self, rows: &[Vec<String>]) -> String {
        if rows.is_empty() {
            return String::new();
        }

        let mut output = String::new();

        for row in rows {
            for (col_idx, cell) in row.iter().enumerate() {
                if col_idx > 0 {
                    output.push(',');
                }
                // Escape quotes and wrap in quotes if contains comma
                let escaped = if cell.contains(',') || cell.contains('"') {
                    format!("\"{}\"", cell.replace('"', "\"\""))
                } else {
                    cell.clone()
                };
                output.push_str(&escaped);
            }
            output.push('\n');
        }

        if self.verbose {
            output = format!("[CSV]\n{}", output);
        }

        output
    }

    /// Format success message
    pub fn format_success(&self, message: &str) -> String {
        match self.style {
            OutputStyle::Colored => format!("\x1b[32m✓ {}\x1b[0m", message),
            OutputStyle::Terminal => format!("✓ {}", message),
            _ => message.to_string(),
        }
    }

    /// Format error message
    pub fn format_error(&self, message: &str) -> String {
        match self.style {
            OutputStyle::Colored => format!("\x1b[31m✗ {}\x1b[0m", message),
            OutputStyle::Terminal => format!("✗ {}", message),
            _ => message.to_string(),
        }
    }

    /// Format warning message
    pub fn format_warning(&self, message: &str) -> String {
        match self.style {
            OutputStyle::Colored => format!("\x1b[33m⚠ {}\x1b[0m", message),
            OutputStyle::Terminal => format!("⚠ {}", message),
            _ => message.to_string(),
        }
    }

    /// Format info message
    pub fn format_info(&self, message: &str) -> String {
        match self.style {
            OutputStyle::Colored => format!("\x1b[34mℹ {}\x1b[0m", message),
            OutputStyle::Terminal => format!("ℹ {}", message),
            _ => message.to_string(),
        }
    }

    /// Format progress bar
    pub fn format_progress(&self, current: u64, total: u64) -> String {
        let percentage = if total > 0 {
            (current as f64 / total as f64 * 100.0) as u8
        } else {
            100
        };

        let filled = (percentage / 2) as usize;
        let empty = 50 - filled;

        let bar = format!("[{}{}] {}%", "=".repeat(filled), " ".repeat(empty), percentage);

        match self.style {
            OutputStyle::Colored => {
                format!("\x1b[36m{}\x1b[0m", bar)
            }
            OutputStyle::Terminal => bar,
            _ => bar,
        }
    }

    /// Format JSON array
    pub fn format_json_array(&self, items: &[String]) -> String {
        let json = format!("[{}]", items.join(", "));
        self.format_json(&json)
    }

    /// Format JSON object
    pub fn format_json_object(&self, items: &[(&str, String)]) -> String {
        let pairs: Vec<String> = items
            .iter()
            .map(|(k, v)| format!(r#""{}":"{}""#, k, v))
            .collect();
        let json = format!("{{{}}}", pairs.join(", "));
        self.format_json(&json)
    }

    /// Format table with headers
    pub fn format_table_with_headers(&self, headers: &[&str], rows: &[Vec<String>]) -> String {
        if rows.is_empty() {
            return String::new();
        }

        // Build header row
        let header_row: Vec<String> = headers
            .iter()
            .map(|&h| h.to_string())
            .collect();

        // Build table
        let mut all_rows = vec![header_row];
        all_rows.extend_from_slice(rows);

        self.format_table(&all_rows)
    }

    /// Format list output
    pub fn format_list(&self, items: &[String], prefix: Option<&str>) -> String {
        let prefix = prefix.unwrap_or("• ");
        items
            .iter()
            .map(|item| format!("{}{}", prefix, item))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Format summary
    pub fn format_summary(&self, title: &str, items: &[(&str, String)]) -> String {
        let formatted_items: Vec<String> = items
            .iter()
            .map(|(k, v)| format!("  {}: {}", k, v))
            .collect();

        format!(
            "\n{}:\n{}",
            title,
            formatted_items.join("\n")
        )
    }

    /// Print to stdout
    pub fn print(&self, output: String) {
        print!("{}", output);
        std::io::stdout().flush().ok();
    }

    /// Print to stderr
    pub fn print_error(&self, output: String) {
        eprint!("{}", output);
        std::io::stderr().flush().ok();
    }
}

impl Default for OutputFormatter {
    fn default() -> Self {
        Self::new()
    }
}

/// Create a default output formatter
pub fn create_formatter() -> OutputFormatter {
    OutputFormatter::new()
}

/// Format a duration
pub fn format_duration(duration: std::time::Duration) -> String {
    let secs = duration.as_secs();

    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        let mins = secs / 60;
        let secs = secs % 60;
        format!("{}m {}s", mins, secs)
    } else {
        let hours = secs / 3600;
        let mins = (secs % 3600) / 60;
        format!("{}h {}m", hours, mins)
    }
}

/// Format a file size
pub fn format_file_size(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];

    let mut size = bytes as f64;
    let mut unit = 0;

    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }

    format!("{:.2} {}", size, UNITS[unit])
}

/// Format a percentage
pub fn format_percentage(value: f64, total: f64) -> String {
    let percentage = if total > 0.0 {
        (value / total * 100.0).round()
    } else {
        0.0
    };
    format!("{:.1}%", percentage)
}

/// Format a version string
pub fn format_version(version: &str) -> String {
    let parts: Vec<&str> = version.split('.').collect();

    let major = parts.get(0).and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
    let minor = parts.get(1).and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
    let patch = parts.get(2).and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);

    format!("v{}.{}.{}", major, minor, patch)
}

/// Format a timestamp
pub fn format_timestamp(timestamp: i64) -> String {
    use std::time::{UNIX_EPOCH, SystemTime};

    let dt = UNIX_EPOCH + std::time::Duration::from_secs(timestamp as u64);
    let datetime = SystemTime::from(dt)
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let secs = datetime % 60;
    let minutes = (datetime / 60) % 60;
    let hours = (datetime / 3600) % 24;

    format!("{:02}:{:02}:{:02}", hours, minutes, secs)
}

/// Create a colored text
pub fn colored_text(text: &str, color: Color) -> String {
    match color {
        Color::Red => format!("\x1b[31m{}\x1b[0m", text),
        Color::Green => format!("\x1b[32m{}\x1b[0m", text),
        Color::Yellow => format!("\x1b[33m{}\x1b[0m", text),
        Color::Blue => format!("\x1b[34m{}\x1b[0m", text),
        Color::Magenta => format!("\x1b[35m{}\x1b[0m", text),
        Color::Cyan => format!("\x1b[36m{}\x1b[0m", text),
        Color::White => format!("\x1b[37m{}\x1b[0m", text),
        Color::Reset => text.to_string(),
    }
}

/// Color enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    Reset,
}
