//! Zero-turn syntax backpressure — the fast, purely local, network-free check
//! that runs over the content of every file `write_file` / `edit_file` just
//! committed to disk.
//!
//! Without this, a syntactically broken write reports `is_error: false`, the
//! bifurcation ladder reads it as clean progress and contracts pressure by α,
//! and the model walks forward on a corrupted file. With it, the tool output
//! carries a [`SYNTAX_BACKPRESSURE_MARKER`] block naming the exact line, the
//! conversation loop flags the step with `syntax_backpressure`, and the ladder
//! refuses to discount resistance for it — the model fixes the syntax on the
//! very next beat.
//!
//! Every heuristic here is fail-open: JSON and TOML use structural scans,
//! shell/Python prefer the interpreter's own parser (`bash -n`, `python3 ast`)
//! when the binary exists, and compiled languages use a lexical
//! delimiter-balance scanner that skips strings and comments. An ambiguous
//! file is never reported as broken.

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Marker the conversation loop greps for to flag the step for the ladder.
/// The tools runners format this string through [`backpressure_block`], so the
/// marker and the grep can never drift apart.
pub const SYNTAX_BACKPRESSURE_MARKER: &str = "⚠️ SYNTAX BACKPRESSURE";

/// Wall-clock ceiling for the `bash -n` / `python3` subprocess checks. These
/// parsers finish in milliseconds; the ceiling only guarantees the tool never
/// stalls a turn on a pathological interpreter.
pub const SHELL_CHECK_TIMEOUT: Duration = Duration::from_millis(1_500);

/// Files bigger than this skip the checks entirely — the guard is tuned for
/// agent-authored config/code, not vendored blobs.
pub const MAX_SCANNED_BYTES: usize = 2 * 1024 * 1024;

/// Runs every applicable check for `path` against `content` and returns a
/// one-line human-readable defect description (with a line number) when the
/// file is provably broken. `None` means clean or inconclusive.
#[must_use]
pub fn check_file_syntax(path: &str, content: &str) -> Option<String> {
    if content.len() > MAX_SCANNED_BYTES {
        return None;
    }
    let extension = Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase());
    match extension.as_deref() {
        Some("json") | Some("jsonc") => check_json(content),
        Some("toml") => check_bracket_balance(content, ScanDialect::toml()),
        Some("sh") | Some("bash") | Some("zsh") => check_shell(path, content),
        Some("py") | Some("pyi") => check_python(path, content),
        Some("rs") => check_bracket_balance(content, ScanDialect::rust()),
        Some("ts") | Some("tsx") | Some("js") | Some("jsx") | Some("mjs") | Some("cjs") => {
            check_bracket_balance(content, ScanDialect::javascript())
        }
        Some("c") | Some("h") | Some("cpp") | Some("cc") | Some("cxx") | Some("hpp")
        | Some("hh") | Some("swift") | Some("go") => {
            check_bracket_balance(content, ScanDialect::c_like())
        }
        _ => None,
    }
}

/// Full marker block appended to a successful write/edit output.
#[must_use]
pub fn backpressure_block(detail: &str) -> String {
    format!(
        "\n\n{SYNTAX_BACKPRESSURE_MARKER}: {detail}\n\
         The file on disk is syntactically broken. The runtime does not count \
         this step as clean progress (pressure is not contracted). Fix the \
         syntax now, before continuing with any other change."
    )
}

// ---------------------------------------------------------------------------
// JSON — serde_json is exact, free, and already a dependency.
// ---------------------------------------------------------------------------

fn check_json(content: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(content)
        .err()
        .map(|error| format!("invalid JSON: {error}"))
}

// ---------------------------------------------------------------------------
// Shell / Python — the interpreter's own parser, fail-open when unavailable.
// ---------------------------------------------------------------------------

enum ParserOutcome {
    /// The parser ran and accepted the file.
    Clean,
    /// The parser ran and rejected the file; payload is the defect line.
    Defect(String),
    /// The binary is missing or the check exceeded its ceiling — inconclusive.
    Unavailable,
}

fn check_shell(path: &str, content: &str) -> Option<String> {
    let mut inconclusive = false;
    for (program, label) in [("bash", "bash -n"), ("sh", "sh -n")] {
        match run_parser_check(program, &["-n", path]) {
            ParserOutcome::Clean => return None,
            ParserOutcome::Defect(detail) => {
                return Some(format!("{label} rejected the file: {detail}"))
            }
            ParserOutcome::Unavailable => inconclusive = true,
        }
    }
    if inconclusive {
        return check_bracket_balance(content, ScanDialect::shell());
    }
    None
}

const PYTHON_PARSE_SNIPPET: &str =
    "import ast,sys; ast.parse(open(sys.argv[1], encoding='utf-8').read())";

fn check_python(path: &str, content: &str) -> Option<String> {
    let mut inconclusive = false;
    for program in ["python3", "python"] {
        match run_parser_check(program, &["-c", PYTHON_PARSE_SNIPPET, path]) {
            ParserOutcome::Clean => return None,
            ParserOutcome::Defect(detail) => {
                return Some(format!("python syntax check failed: {detail}"))
            }
            ParserOutcome::Unavailable => inconclusive = true,
        }
    }
    if inconclusive {
        return check_bracket_balance(content, ScanDialect::python());
    }
    None
}

fn run_parser_check(program: &str, args: &[&str]) -> ParserOutcome {
    let Some(output) = run_captured(program, args) else {
        return ParserOutcome::Unavailable;
    };
    if output.status.success() {
        return ParserOutcome::Clean;
    }
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let detail = stderr
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("Traceback"))
        .find(|line| line.contains("Error") || line.contains("error"))
        .or_else(|| {
            stderr
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .next_back()
        })
        .unwrap_or("parser reported an error")
        .to_string();
    if detail.is_empty() {
        return ParserOutcome::Clean;
    }
    ParserOutcome::Defect(truncate_chars(&detail, 200))
}

fn truncate_chars(value: &str, max: usize) -> String {
    if let Some((idx, _)) = value.char_indices().nth(max) {
        format!("{}…", &value[..idx])
    } else {
        value.to_string()
    }
}

/// Poll-based subprocess runner with a hard wall-clock ceiling; the tool path
/// must never hang a turn. `None` when the binary cannot even be spawned.
fn run_captured(program: &str, args: &[&str]) -> Option<std::process::Output> {
    let mut child = Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().ok(),
            Ok(None) => {
                if started.elapsed() > SHELL_CHECK_TIMEOUT {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(_) => return None,
        }
    }
}

// ---------------------------------------------------------------------------
// Lexical delimiter-balance scanner
// ---------------------------------------------------------------------------

/// Dialect knobs for [`check_bracket_balance`]. Everything unlisted is treated
/// as plain code, which keeps the scanner fail-open on unknown constructs.
#[derive(Debug, Clone, Copy)]
struct ScanDialect {
    hash_comments: bool,
    slash_comments: bool,
    block_comments: bool,
    nested_block_comments: bool,
    /// C/Go/Swift `'a'` literals. Off for Rust on purpose: lifetimes (`'a`)
    /// would be misread as literals far more often than `'{'` occurs.
    char_literals: bool,
    /// Rust: recognize only *punctuation* char literals (`'{'`, `';'`, `'\''`).
    /// An alphanumeric `'a` is overwhelmingly a lifetime, and a plain `'a'`
    /// contains no delimiter, so ignoring it is harmless either way.
    punctuation_char_literals: bool,
    /// Rust `r"…"`, `r#"…"#`.
    raw_strings: bool,
    /// JS/TS backtick literals with `${ … }` interpolation.
    template_strings: bool,
    /// Python `"""…"""` / `'''…'''`.
    triple_quotes: bool,
    /// Treat `'…'` as a string (Python, shell, TOML literals, JS).
    single_quote_strings: bool,
    /// Backslash escapes inside single-quoted strings (Python/JS yes,
    /// shell/TOML literals no).
    single_quote_escapes: bool,
}

impl ScanDialect {
    const fn rust() -> Self {
        Self {
            hash_comments: false,
            slash_comments: true,
            block_comments: true,
            nested_block_comments: true,
            char_literals: false,
            punctuation_char_literals: true,
            raw_strings: true,
            template_strings: false,
            triple_quotes: false,
            single_quote_strings: false,
            single_quote_escapes: false,
        }
    }

    const fn javascript() -> Self {
        Self {
            hash_comments: false,
            slash_comments: true,
            block_comments: true,
            nested_block_comments: false,
            char_literals: false,
            punctuation_char_literals: false,
            raw_strings: false,
            template_strings: true,
            triple_quotes: false,
            single_quote_strings: true,
            single_quote_escapes: true,
        }
    }

    const fn c_like() -> Self {
        Self {
            hash_comments: false,
            slash_comments: true,
            block_comments: true,
            nested_block_comments: false,
            char_literals: true,
            punctuation_char_literals: false,
            raw_strings: false,
            template_strings: false,
            triple_quotes: false,
            single_quote_strings: false,
            single_quote_escapes: false,
        }
    }

    const fn toml() -> Self {
        Self {
            hash_comments: true,
            slash_comments: false,
            block_comments: false,
            nested_block_comments: false,
            char_literals: false,
            punctuation_char_literals: false,
            raw_strings: false,
            template_strings: false,
            triple_quotes: false,
            single_quote_strings: true,
            single_quote_escapes: false,
        }
    }

    const fn shell() -> Self {
        Self {
            hash_comments: true,
            slash_comments: false,
            block_comments: false,
            nested_block_comments: false,
            char_literals: false,
            punctuation_char_literals: false,
            raw_strings: false,
            template_strings: false,
            triple_quotes: false,
            single_quote_strings: true,
            single_quote_escapes: false,
        }
    }

    const fn python() -> Self {
        Self {
            hash_comments: true,
            slash_comments: false,
            block_comments: false,
            nested_block_comments: false,
            char_literals: false,
            punctuation_char_literals: false,
            raw_strings: false,
            template_strings: false,
            triple_quotes: true,
            single_quote_strings: true,
            single_quote_escapes: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScanMode {
    Code,
    LineComment,
    BlockComment,
    DoubleString,
    SingleString,
    RawString { hashes: usize },
    TripleDouble,
    TripleSingle,
    Template,
}

/// One entry on the delimiter stack. `interpolation` marks a `${` opener of a
/// JS/TS template literal: when it closes, the scanner re-enters template mode.
#[derive(Debug, Clone, Copy)]
struct StackEntry {
    open: char,
    line: usize,
    interpolation: bool,
}

/// Reports the first provable delimiter defect: an unmatched closer, an
/// unclosed opener at EOF, or an unterminated string/comment — always with the
/// line number. Balanced-but-broken code is intentionally not this scanner's
/// job (it never false-positives a working file).
fn check_bracket_balance(content: &str, dialect: ScanDialect) -> Option<String> {
    let mut stack: Vec<StackEntry> = Vec::new();
    let mut block_depth: usize = 0;
    let mut mode = ScanMode::Code;
    let mut line: usize = 1;
    let mut chars = content.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\n' {
            line += 1;
            match mode {
                // Double-quoted strings do not span newlines in any scanned
                // language except triple-quoted ones; treat the break as the
                // string ending and keep scanning (conservative: no report).
                ScanMode::LineComment | ScanMode::DoubleString | ScanMode::SingleString => {
                    mode = ScanMode::Code;
                }
                _ => {}
            }
            continue;
        }

        match mode {
            ScanMode::Code => {
                if ch == '/' && dialect.slash_comments {
                    match chars.peek() {
                        Some('/') => {
                            chars.next();
                            mode = ScanMode::LineComment;
                            continue;
                        }
                        Some('*') => {
                            if dialect.block_comments {
                                chars.next();
                                block_depth = 1;
                                mode = ScanMode::BlockComment;
                                continue;
                            }
                        }
                        _ => {}
                    }
                }
                if ch == '#' && dialect.hash_comments {
                    mode = ScanMode::LineComment;
                    continue;
                }
                if ch == '"' {
                    if dialect.triple_quotes && starts_triple(&mut chars, '"') {
                        mode = ScanMode::TripleDouble;
                        continue;
                    }
                    mode = ScanMode::DoubleString;
                    continue;
                }
                if ch == '\'' {
                    if dialect.triple_quotes && starts_triple(&mut chars, '\'') {
                        mode = ScanMode::TripleSingle;
                        continue;
                    }
                    if dialect.char_literals && starts_char_literal(&mut chars) {
                        continue;
                    }
                    if dialect.punctuation_char_literals && starts_punct_char_literal(&mut chars) {
                        continue;
                    }
                    if dialect.single_quote_strings {
                        mode = ScanMode::SingleString;
                        continue;
                    }
                }
                if ch == '`' && dialect.template_strings {
                    mode = ScanMode::Template;
                    continue;
                }
                if ch == 'r' && dialect.raw_strings && starts_raw_string(&mut chars) {
                    let mut hashes = 0usize;
                    while chars.peek() == Some(&'#') {
                        chars.next();
                        hashes += 1;
                    }
                    // The opening quote follows the hashes; consume it too.
                    chars.next();
                    mode = ScanMode::RawString { hashes };
                    continue;
                }
                match ch {
                    '(' | '[' | '{' => stack.push(StackEntry {
                        open: ch,
                        line,
                        interpolation: false,
                    }),
                    ')' | ']' | '}' => {
                        let expected_open = match ch {
                            ')' => '(',
                            ']' => '[',
                            _ => '{',
                        };
                        let Some(top) = stack.last().copied() else {
                            return Some(format!(
                                "unbalanced delimiters: unexpected closing '{ch}' at line {line} \
                                 (nothing is open)"
                            ));
                        };
                        if top.open != expected_open {
                            return Some(format!(
                                "unbalanced delimiters: unexpected closing '{ch}' at line {line} \
                                 (expected '{expected_open}' to close the '{}' opened at line {})",
                                top.open, top.line
                            ));
                        }
                        stack.pop();
                        if top.interpolation {
                            mode = ScanMode::Template;
                        }
                    }
                    _ => {}
                }
            }
            ScanMode::LineComment => {}
            ScanMode::BlockComment => {
                if ch == '*' {
                    if chars.peek() == Some(&'/') {
                        chars.next();
                        block_depth = block_depth.saturating_sub(1);
                        if block_depth == 0 {
                            mode = ScanMode::Code;
                        }
                    }
                } else if ch == '/' && dialect.nested_block_comments && chars.peek() == Some(&'*') {
                    chars.next();
                    block_depth += 1;
                }
            }
            ScanMode::DoubleString => {
                if ch == '\\' {
                    chars.next();
                } else if ch == '"' {
                    mode = ScanMode::Code;
                }
            }
            ScanMode::SingleString => {
                if ch == '\\' && dialect.single_quote_escapes {
                    chars.next();
                } else if ch == '\'' {
                    mode = ScanMode::Code;
                }
            }
            ScanMode::RawString { hashes } => {
                if ch == '"' {
                    let mut matched = 0usize;
                    while matched < hashes && chars.peek() == Some(&'#') {
                        chars.next();
                        matched += 1;
                    }
                    if matched == hashes {
                        mode = ScanMode::Code;
                    }
                }
            }
            ScanMode::TripleDouble => {
                if ch == '"' && starts_triple(&mut chars, '"') {
                    mode = ScanMode::Code;
                }
            }
            ScanMode::TripleSingle => {
                if ch == '\'' && starts_triple(&mut chars, '\'') {
                    mode = ScanMode::Code;
                }
            }
            ScanMode::Template => {
                if ch == '\\' {
                    chars.next();
                } else if ch == '`' {
                    mode = ScanMode::Code;
                } else if ch == '$' && chars.peek() == Some(&'{') {
                    chars.next();
                    stack.push(StackEntry {
                        open: '{',
                        line,
                        interpolation: true,
                    });
                    mode = ScanMode::Code;
                }
            }
        }
    }

    match mode {
        ScanMode::BlockComment => Some(format!("unterminated block comment at line {line}")),
        ScanMode::DoubleString | ScanMode::TripleDouble => {
            Some(format!("unterminated double-quoted string at line {line}"))
        }
        ScanMode::SingleString | ScanMode::TripleSingle => {
            Some(format!("unterminated quoted string at line {line}"))
        }
        ScanMode::RawString { .. } => Some(format!("unterminated raw string at line {line}")),
        ScanMode::Template => Some(format!("unterminated template literal at line {line}")),
        _ => stack.last().map(|entry| {
            format!(
                "unbalanced delimiters: '{}' opened at line {} is never closed by end of file",
                entry.open, entry.line
            )
        }),
    }
}

/// The caller already consumed the first quote; consume the remaining two when
/// they complete `"""` / `'''`.
fn starts_triple(chars: &mut std::iter::Peekable<std::str::Chars<'_>>, quote: char) -> bool {
    let after: Vec<char> = chars.clone().take(2).collect();
    if after.first() == Some(&quote) && after.get(1) == Some(&quote) {
        chars.next();
        chars.next();
        return true;
    }
    false
}

/// C-style char literal: `x'` or `'\\x'` — recognize and consume the body up to
/// and including the closing quote.
fn starts_char_literal(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> bool {
    let after: Vec<char> = chars.clone().take(3).collect();
    let body_len = match after.as_slice() {
        [c, '\'', ..] if !c.is_whitespace() => 1,
        ['\\', _, '\'', ..] => 3,
        _ => return false,
    };
    for _ in 0..body_len {
        chars.next();
    }
    true
}

/// Rust-style punctuation char literal: `'{'`, `'('`, `';'`, `'\'` … — recognized
/// only when the enclosed character is a delimiter the scanner would otherwise
/// count. Lifetimes (`'a`, `'static`) and ordinary literals (`'x'`) are left as
/// plain code, which is safe because their bodies cannot unbalance anything.
fn starts_punct_char_literal(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> bool {
    let after: Vec<char> = chars.clone().take(3).collect();
    let body_len = match after.as_slice() {
        [c, '\'', ..] if c.is_ascii_punctuation() && *c != '\'' => 1,
        ['\\', _, '\'', ..] => 3,
        _ => return false,
    };
    for _ in 0..body_len {
        chars.next();
    }
    true
}

/// Rust raw string opener: `r"` / `r#"` / `r##"` … (quote after any hashes).
fn starts_raw_string(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> bool {
    let after: Vec<char> = chars.clone().take(5).collect();
    let mut saw_quote = false;
    let mut only_hashes_then_quote = true;
    for c in after {
        match c {
            '#' => {}
            '"' => {
                saw_quote = true;
                break;
            }
            _ => {
                only_hashes_then_quote = false;
                break;
            }
        }
    }
    saw_quote && only_hashes_then_quote
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_detects_unclosed_brace_with_position() {
        let broken = "{\n  \"a\": 1,\n  \"b\": {\"c\": 2\n}\n";
        let defect = check_file_syntax("/tmp/x.json", broken).expect("broken json detected");
        assert!(defect.contains("invalid JSON"), "{defect}");
        assert!(defect.contains("line"), "{defect}");
    }

    #[test]
    fn json_accepts_valid_content() {
        assert!(check_file_syntax("/tmp/x.json", r#"{"a": [1, 2], "b": {"c": 3}}"#).is_none());
    }

    #[test]
    fn rust_detects_unclosed_fn_brace_with_line() {
        let broken = "fn main() {\n    println!(\"hi\");\n";
        let defect = check_file_syntax("/tmp/x.rs", broken).expect("unclosed fn detected");
        assert!(defect.contains("line 1"), "{defect}");
        assert!(defect.contains('{'), "{defect}");
    }

    #[test]
    fn rust_detects_stray_closer_with_line() {
        let broken = "fn main() {\n    let x = 1;\n}}\n";
        let defect = check_file_syntax("/tmp/x.rs", broken).expect("stray } detected");
        assert!(defect.contains("line 3"), "{defect}");
    }

    #[test]
    fn rust_ignores_braces_in_strings_and_comments() {
        let ok = "// a comment with { and (\n\
                  /* block with } */\n\
                  fn main() {\n\
                      let s = \"literal { with ( brace\";\n\
                      let r = r##\"raw \"# still string } still string\"##;\n\
                      let c = '{';\n\
                      let t = ('{' , 2);\n\
                      foo((1), [2]);\n\
                  }\n";
        assert!(check_file_syntax("/tmp/x.rs", ok).is_none());
    }

    #[test]
    fn rust_lifetimes_are_not_string_delimiters() {
        let ok = "fn pick<'a>(items: &'a [String]) -> &'a String { &items[0] }\n";
        assert!(check_file_syntax("/tmp/x.rs", ok).is_none());
    }

    #[test]
    fn rust_punctuation_char_literals_are_consumed() {
        let ok = "fn ops() -> [char; 3] { ['{', '(', ';'] }\nfn esc() -> char { '\\'' }\n";
        assert!(check_file_syntax("/tmp/x.rs", ok).is_none());
        let broken = "fn ops() { let c = '}'; let d = '(' ;\n";
        assert!(check_file_syntax("/tmp/x.rs", broken).is_some());
    }

    #[test]
    fn toml_detects_unbalanced_table_brackets() {
        let broken = "[package]\nname = \"x\"\n[dependencies\nserde = \"1\"\n";
        let defect = check_file_syntax("/tmp/x.toml", broken).expect("unclosed [ detected");
        assert!(defect.contains("never closed"), "{defect}");
    }

    #[test]
    fn toml_accepts_valid_content_with_quotes_and_comments() {
        let ok = "# config [not a real section]\n[deps]\nlib = \"a]b\"\nother = 'c]d'\n";
        assert!(check_file_syntax("/tmp/x.toml", ok).is_none());
    }

    #[test]
    fn javascript_template_interpolation_balances() {
        let ok =
            "const t = `x ${ obj.field } y {literal} ${ a[0] }`;\nfunction f() { return t; }\n";
        assert!(check_file_syntax("/tmp/x.js", ok).is_none());
    }

    #[test]
    fn javascript_detects_unclosed_template_literal() {
        let broken = "const t = `never closed;\nfunction f() {\n}\n";
        let defect = check_file_syntax("/tmp/x.js", broken).expect("template defect");
        assert!(
            defect.contains("raw") || defect.contains("template") || defect.contains("string"),
            "{defect}"
        );
    }

    #[test]
    fn c_like_skips_braces_inside_char_literals() {
        let ok = "int main() { char c = '{'; if (c == '}') { return 0; } }\n";
        assert!(check_file_syntax("/tmp/x.c", ok).is_none());
    }

    #[test]
    fn rust_unterminated_block_comment_reported() {
        let broken = "fn main() {\n    /* never closed\n}\n";
        // The block comment swallows the closing braces, so the report is the
        // unterminated comment or an unclosed brace — either is a true signal.
        assert!(check_file_syntax("/tmp/x.rs", broken).is_some());
    }

    #[test]
    fn unknown_extensions_are_never_checked() {
        assert!(check_file_syntax("/tmp/x.md", "{[(").is_none());
        assert!(check_file_syntax("/tmp/noext", "{[(").is_none());
    }

    #[test]
    fn oversized_files_fail_open() {
        let huge = "{".repeat(MAX_SCANNED_BYTES + 1);
        assert!(check_file_syntax("/tmp/x.json", &huge).is_none());
    }

    #[test]
    fn shell_check_uses_bash_when_available() {
        // `bash -n` on a broken script fails with a line number; a missing
        // binary falls back to the scanner, which also reports the imbalance.
        let broken = "#!/bin/bash\nif true; then\n  echo hi\n";
        let dir = std::env::temp_dir().join(format!(
            "claw-syntax-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("broken.sh");
        std::fs::write(&path, broken).expect("write fixture");
        let defect = check_file_syntax(path.to_string_lossy().as_ref(), broken);
        let _ = std::fs::remove_dir_all(&dir);
        let defect = defect.expect("unterminated if must be caught (bash -n or scanner)");
        assert!(
            defect.contains("line") || defect.contains("syntax"),
            "{defect}"
        );
    }

    #[test]
    fn backpressure_block_contains_marker() {
        let block = backpressure_block("unbalanced delimiters at line 2");
        assert!(block.contains(SYNTAX_BACKPRESSURE_MARKER));
        assert!(block.contains("not count"));
    }

    /// "Zero-turn" only holds if the guard is cheap: the scan is linear and
    /// bounded by [`MAX_SCANNED_BYTES`], so a large file must stay inside the
    /// tool-call budget instead of becoming a second compile.
    #[test]
    fn scanning_a_large_source_file_stays_in_milliseconds() {
        let unit =
            "fn f() {\n    let s = \"a { b ( c\";\n    // comment { ( }\n    v[(1)] = '{';\n}\n";
        let content = unit.repeat(2_000);
        assert!(
            content.len() > 100_000,
            "fixture is {} bytes",
            content.len()
        );
        let started = std::time::Instant::now();
        assert!(
            check_file_syntax("/tmp/big.rs", &content).is_none(),
            "a balanced file must not raise backpressure"
        );
        let elapsed = started.elapsed();
        assert!(
            elapsed < std::time::Duration::from_millis(20),
            "the guard promised sub-20ms backpressure but took {elapsed:?} on {} bytes \
             (measured baseline: ~6ms)",
            content.len()
        );
    }
}
