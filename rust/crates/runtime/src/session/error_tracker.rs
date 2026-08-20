//! Tracks recurring tool errors and generates dynamic skills when patterns emerge.
//!
//! When the same tool fails with a similar error more than once during a session,
//! and the tool later succeeds, a `DynamicSkill` is generated and stored in
//! temporary memory. On session shutdown, effective skills are persisted to
//! permanent storage (`omc-learned/`).

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Fine-grained error classification for autonomous agent troubleshooting.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ErrorCategory {
    PermissionDenied,
    FileNotFound,
    AlreadyExists,
    InvalidArguments,
    NetworkOrTimeout,
    ConnectionError,
    RateLimitOrQuota,
    OutOfMemory,
    ResourceBusyOrLocked,
    SyntaxOrParseError,
    MissingDependency,
    UnknownTool,
    ToolSpecific(String),
}

impl ErrorCategory {
    /// Maps an error message string to a structured `ErrorCategory`.
    #[must_use]
    pub fn from_error_message(error_msg: &str) -> Self {
        let lower = error_msg.to_lowercase();

        let patterns: &[(&[&str], Self)] = &[
            (&["permission denied", "access denied", "not permitted", "eacces", "operation not permitted"], Self::PermissionDenied),
            (&["no such file", "file not found", "not found", "does not exist", "enoent"], Self::FileNotFound),
            (&["already exists", "eexist", "file exists"], Self::AlreadyExists),
            (&["invalid argument", "invalid input", "invalid parameter", "missing required", "type mismatch", "validation error", "schema validation"], Self::InvalidArguments),
            (&["timeout", "timed out", "deadline exceeded", "etimedout", "navigation timeout"], Self::NetworkOrTimeout),
            (&["connection refused", "connection reset", "econnrefused", "econnreset", "socket hang up", "network unreachable"], Self::ConnectionError),
            (&["rate limit", "too many requests", "429", "quota exceeded", "resource exhausted"], Self::RateLimitOrQuota),
            (&["out of memory", "oom", "heap limit", "javascript heap out of memory"], Self::OutOfMemory),
            (&["resource busy", "locked", "ebusy", "database is locked", "file is locked"], Self::ResourceBusyOrLocked),
            (&["syntax error", "parse error", "invalid syntax", "unexpected token", "json parse error"], Self::SyntaxOrParseError),
            (&["command not found", "executable not found", "module not found", "cannot find module", "no such command"], Self::MissingDependency),
            (&["unknown tool"], Self::UnknownTool),
        ];

        for (keywords, category) in patterns {
            if keywords.iter().any(|kw| lower.contains(kw)) {
                return category.clone();
            }
        }

        // Fallback: use first 80 clean characters as tool-specific category.
        let stripped = lower
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == ' ')
            .take(80)
            .collect::<String>();
        let cat = stripped.split_whitespace().take(8).collect::<Vec<_>>().join("_");
        if cat.is_empty() {
            Self::ToolSpecific("general_error".to_string())
        } else {
            Self::ToolSpecific(cat)
        }
    }

    /// Normalized category identifier string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::PermissionDenied => "permission_denied",
            Self::FileNotFound => "file_not_found",
            Self::AlreadyExists => "already_exists",
            Self::InvalidArguments => "invalid_argument",
            Self::NetworkOrTimeout => "timeout",
            Self::ConnectionError => "connection_error",
            Self::RateLimitOrQuota => "rate_limit",
            Self::OutOfMemory => "out_of_memory",
            Self::ResourceBusyOrLocked => "resource_locked",
            Self::SyntaxOrParseError => "syntax_error",
            Self::MissingDependency => "missing_dependency",
            Self::UnknownTool => "unknown_tool",
            Self::ToolSpecific(s) => s.as_str(),
        }
    }

    /// Explains the root cause in actionable terms.
    #[must_use]
    pub fn root_cause_explanation(&self) -> &'static str {
        match self {
            Self::PermissionDenied => "The operation was rejected due to insufficient filesystem, process, or execution permissions.",
            Self::FileNotFound => "The target file or directory path does not exist, or parent directories were not created.",
            Self::AlreadyExists => "The resource or file already exists and overwrite flag was not specified.",
            Self::InvalidArguments => "The parameters provided did not match the expected schema or contained invalid field types/names.",
            Self::NetworkOrTimeout => "The request or command timed out waiting for network response, DOM rendering, or process completion.",
            Self::ConnectionError => "Failed to establish or maintain connection to the remote endpoint or subprocess.",
            Self::RateLimitOrQuota => "API rate limit, concurrency ceiling, or quota limit was reached.",
            Self::OutOfMemory => "The process exhausted available memory limits.",
            Self::ResourceBusyOrLocked => "The file, port, or database is currently locked by another process.",
            Self::SyntaxOrParseError => "Syntax or parsing error occurred when evaluating command or structured arguments.",
            Self::MissingDependency => "The required CLI binary, package, or tool executable is not installed or missing from PATH.",
            Self::UnknownTool => "The invoked tool is not registered or supported in the runtime environment.",
            Self::ToolSpecific(_) => "The tool encountered an operational error during execution.",
        }
    }

    /// High-level guidance of what NOT to do.
    #[must_use]
    pub fn anti_patterns(&self) -> &'static [&'static str] {
        match self {
            Self::PermissionDenied => &[
                "- ❌ Do NOT repeatedly retry without elevated permissions or using an allowed writable directory.",
                "- ❌ Do NOT try to write directly into protected system paths without explicit elevation.",
            ],
            Self::FileNotFound => &[
                "- ❌ Do NOT assume parent directories exist without checking or creating them.",
                "- ❌ Do NOT use relative paths without verifying the current workspace root.",
            ],
            Self::AlreadyExists => &[
                "- ❌ Do NOT fail silently; specify overwrite options or inspect existing files first.",
            ],
            Self::InvalidArguments => &[
                "- ❌ Do NOT guess or hallucinate parameter names not defined in the tool schema.",
                "- ❌ Do NOT pass mismatched data types (e.g. strings where arrays or numbers are required).",
            ],
            Self::NetworkOrTimeout => &[
                "- ❌ Do NOT perform blocking network calls without specifying a reasonable timeout.",
                "- ❌ Do NOT wait indefinitely on dynamic SPAs without explicit wait states (e.g. domcontentloaded).",
            ],
            Self::RateLimitOrQuota => &[
                "- ❌ Do NOT bombard the API with immediate parallel retries when throttled.",
            ],
            Self::MissingDependency => &[
                "- ❌ Do NOT attempt to run uninstalled binaries; verify prerequisites or use fallback tools.",
            ],
            _ => &[
                "- ❌ Do NOT repeat identical failing inputs without parameter adjustments.",
            ],
        }
    }
}

/// A single recorded tool error.
#[derive(Debug, Clone)]
pub struct ToolErrorRecord {
    pub tool_name: String,
    pub error_category: String,
    pub error_message: String,
    pub input_summary: String,
    pub timestamp_ms: u64,
}

/// A dynamically generated skill / rule derived from an error-recovery pattern.
#[derive(Debug, Clone)]
pub struct DynamicSkill {
    pub name: String,
    pub tool_name: String,
    pub error_pattern: String,
    pub solution: String,
    pub input_diff: Option<String>,
    pub created_at_ms: u64,
    pub was_effective: bool,
    pub temp_path: PathBuf,
    /// Number of errors that occurred after this skill was created.
    pub errors_after_creation: usize,
    /// Number of successful invocations reinforcing this skill.
    pub success_count: usize,
}

impl DynamicSkill {
    /// Computes dynamic effectiveness score [0.0..1.0].
    #[must_use]
    pub fn effectiveness_score(&self) -> f64 {
        let total = self.success_count + self.errors_after_creation;
        if total == 0 {
            1.0
        } else {
            self.success_count as f64 / total as f64
        }
    }

    /// Renders this skill as a comprehensive, standards-compliant SKILL.md file.
    #[must_use]
    pub fn to_skill_md(&self) -> String {
        let category_enum = ErrorCategory::from_error_message(&self.error_pattern);
        let category_str = category_enum.as_str();
        let root_cause = category_enum.root_cause_explanation();
        let anti_patterns = category_enum.anti_patterns().join("\n");
        let server_line = if let Some(server) = extract_server_from_tool_name(&self.tool_name) {
            format!("\nserver: {server}")
        } else {
            String::new()
        };

        let diff_section = if let Some(diff) = &self.input_diff {
            format!("\n### 🔄 Parameter Adjustments\n{diff}\n")
        } else {
            String::new()
        };

        format!(
            "---\n\
             name: {name}\n\
             description: \"Auto-learned rule for {tool} dealing with {category_str}\"\n\
             version: 1.0.0\n\
             category: {category_str}\n\
             tool: {tool}{server_line}\n\
             effectiveness_score: {score:.2}\n\
             success_count: {success_count}\n\
             failure_count: {errors_after}\n\
             ---\n\n\
             # Auto-Learned Skill: `{tool}`\n\n\
             ## 🎯 Trigger Conditions\n\
             Apply this skill when invoking `{tool}` and encountering `{category_str}` errors (e.g. \"{error_sample}\").\n\n\
             ## 🔍 Root Cause Analysis\n\
             {root_cause}\n\n\
             ## 💡 Recommended Solution & Correct Parameters\n\
             The verified successful execution pattern is:\n\n\
             ```json\n\
             {solution}\n\
             ```\n\
             {diff_section}\n\
             ## ⚠️ Anti-Patterns & Common Mistakes\n\
             {anti_patterns}\n",
            name = self.name,
            tool = self.tool_name,
            category_str = category_str,
            server_line = server_line,
            score = self.effectiveness_score(),
            success_count = self.success_count.max(1),
            errors_after = self.errors_after_creation,
            error_sample = truncate(&self.error_pattern, 100),
            root_cause = root_cause,
            solution = self.solution,
            diff_section = diff_section,
            anti_patterns = anti_patterns,
        )
    }
}

/// Helper function to extract server name from qualified MCP tool names (e.g. `mcp:server:tool` or `mcp__server__tool`).
fn extract_server_from_tool_name(tool_name: &str) -> Option<&str> {
    if let Some(rest) = tool_name.strip_prefix("mcp:") {
        return rest.split(':').next();
    }
    if let Some(rest) = tool_name.strip_prefix("mcp__") {
        return rest.split("__").next();
    }
    None
}

/// Semantic input diffing between failed input and winning input.
#[must_use]
pub fn diff_tool_inputs(failed_input: &str, successful_input: &str) -> Option<String> {
    if failed_input.trim() == successful_input.trim() {
        return None;
    }
    let failed_val = serde_json::from_str::<serde_json::Value>(failed_input).ok();
    let succ_val = serde_json::from_str::<serde_json::Value>(successful_input).ok();

    match (failed_val, succ_val) {
        (Some(serde_json::Value::Object(f_obj)), Some(serde_json::Value::Object(s_obj))) => {
            let mut diffs = Vec::new();
            for (k, v_succ) in &s_obj {
                match f_obj.get(k) {
                    Some(v_fail) if v_fail != v_succ => {
                        diffs.push(format!("- Changed `{k}`: `{}` ➔ `{}`", truncate_val(v_fail), truncate_val(v_succ)));
                    }
                    None => {
                        diffs.push(format!("- Added parameter `{k}`: `{}`", truncate_val(v_succ)));
                    }
                    _ => {}
                }
            }
            for (k, v_fail) in &f_obj {
                if !s_obj.contains_key(k) {
                    diffs.push(format!("- Removed parameter `{k}` (was `{}`)", truncate_val(v_fail)));
                }
            }
            if diffs.is_empty() {
                None
            } else {
                Some(diffs.join("\n"))
            }
        }
        _ => {
            Some(format!("- Adjusted input payload to: `{}`", truncate(successful_input, 150)))
        }
    }
}

fn truncate_val(v: &serde_json::Value) -> String {
    let s = v.to_string();
    truncate(&s, 60)
}

/// Normalizes an error message into a category key for deduplication.
#[must_use]
pub fn normalize_error_category(error_msg: &str) -> String {
    ErrorCategory::from_error_message(error_msg).as_str().to_string()
}

/// Truncates tool input JSON to a short summary for storage.
#[must_use]
fn summarize_input(input: &str) -> String {
    if let Some((idx, _)) = input.char_indices().nth(1024) {
        format!("{}…", &input[..idx])
    } else {
        input.to_string()
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Resolves the effective tool name for ErrorTracker tracking.
///
/// For generic MCP tool wrappers (`MCPTool`, `McpSearch`), extracts the
/// `qualifiedName` or `tool` field from the input JSON so we track errors
/// per-actual-MCP-tool, not under the generic wrapper name.
#[must_use]
pub fn resolve_effective_tool_name<'a>(tool_name: &'a str, input: &'a str) -> std::borrow::Cow<'a, str> {
    if tool_name == "MCPTool" || tool_name == "call_mcp_tool" || tool_name == "McpTool" {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(input) {
            if let Some(name) = v.get("qualifiedName").and_then(|v| v.as_str())
                .or_else(|| v.get("tool").and_then(|v| v.as_str()))
                .or_else(|| v.get("ToolName").and_then(|v| v.as_str()))
                .or_else(|| v.get("name").and_then(|v| v.as_str()))
            {
                let server = v.get("ServerName").and_then(|v| v.as_str())
                    .or_else(|| v.get("server").and_then(|v| v.as_str()))
                    .unwrap_or("");
                if !server.is_empty() {
                    return std::borrow::Cow::Owned(format!("mcp:{server}:{name}"));
                }
                return std::borrow::Cow::Owned(format!("mcp:{name}"));
            }
        }
    }
    std::borrow::Cow::Borrowed(tool_name)
}

/// Maximum number of dynamic skills allowed at once.
const MAX_DYNAMIC_SKILLS: usize = 10;

/// Tracks tool errors and successes to detect recurring patterns.
#[derive(Debug)]
pub struct ErrorTracker {
    /// All recorded errors, keyed by `(tool_name, error_category)`.
    error_counts: HashMap<(String, String), Vec<ToolErrorRecord>>,
    /// Dynamic skills generated during this session.
    dynamic_skills: Vec<DynamicSkill>,
}

impl Default for ErrorTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl ErrorTracker {
    #[must_use]
    pub fn new() -> Self {
        Self {
            error_counts: HashMap::new(),
            dynamic_skills: Vec::new(),
        }
    }

    /// Records a tool execution error. Returns `true` if this is a recurring error.
    pub fn record_error(&mut self, tool_name: &str, error_msg: &str, input: &str) -> bool {
        let category = normalize_error_category(error_msg);
        let key = (tool_name.to_string(), category.clone());

        let record = ToolErrorRecord {
            tool_name: tool_name.to_string(),
            error_category: category,
            error_message: error_msg.to_string(),
            input_summary: summarize_input(input),
            timestamp_ms: now_ms(),
        };

        let entries = self.error_counts.entry(key.clone()).or_default();
        entries.push(record);

        let is_recurring = entries.len() >= 2;

        // Track errors that happen after a dynamic skill was created for this tool.
        if is_recurring {
            for skill in &mut self.dynamic_skills {
                if skill.tool_name == tool_name
                    && normalize_error_category(&skill.error_pattern)
                        == key.1
                {
                    skill.errors_after_creation += 1;
                }
            }
        }

        is_recurring
    }

    /// Records a successful tool execution. If there was a recurring error
    /// on this tool before, generates a dynamic skill and returns it.
    pub fn record_success(
        &mut self,
        tool_name: &str,
        input: &str,
        output: &str,
    ) -> Option<DynamicSkill> {
        // Find any error categories for this tool with >= 2 occurrences.
        let mut candidate_key = None;
        for ((name, category), records) in &self.error_counts {
            if name != tool_name || records.len() < 2 {
                continue;
            }
            // Check if we already have a skill for this. If so, reinforce it.
            if let Some(existing) = self.dynamic_skills.iter_mut().find(|s| {
                s.tool_name == tool_name
                    && normalize_error_category(&s.error_pattern) == *category
            }) {
                existing.success_count += 1;
                existing.was_effective = true;
                let _ = write_temp_skill(existing);
                let _ = persist_skill_to_learned(existing);
                return None;
            }
            candidate_key = Some((name.clone(), category.clone()));
            break;
        }

        let (_, category) = candidate_key?;

        if self.dynamic_skills.len() >= MAX_DYNAMIC_SKILLS {
            tracing::warn!(
                tool = %tool_name,
                "Skipping dynamic skill creation: limit of {} reached",
                MAX_DYNAMIC_SKILLS
            );
            return None;
        }

        // Get the representative error message and failing input from the records.
        let records = self.error_counts.get(&(tool_name.to_string(), category.clone()));
        let error_msg = records
            .and_then(|r| r.last())
            .map(|r| r.error_message.clone())
            .unwrap_or_default();
        let failing_input = records
            .and_then(|r| r.last())
            .map(|r| r.input_summary.as_str())
            .unwrap_or("");

        let input_diff = diff_tool_inputs(failing_input, input);
        let solution_summary = truncate(output, 1024);

        let name = make_dynamic_skill_name(tool_name, &category);

        let temp_dir = temp_skills_dir();
        let skill_dir = temp_dir.join(&name);
        let temp_path = skill_dir.join("SKILL.md");

        let skill = DynamicSkill {
            name,
            tool_name: tool_name.to_string(),
            error_pattern: error_msg,
            solution: format!(
                "Successful input: {}\nResult: {}",
                summarize_input(input),
                solution_summary,
            ),
            input_diff,
            created_at_ms: now_ms(),
            was_effective: true,
            temp_path,
            errors_after_creation: 0,
            success_count: 1,
        };

        // Write to temporary storage.
        if let Err(e) = write_temp_skill(&skill) {
            tracing::error!(error = %e, "Failed to write temporary skill");
            return None;
        }

        // Persist immediately to permanent learned storage so stopping the agent never loses data
        if let Err(e) = persist_skill_to_learned(&skill) {
            tracing::warn!(error = %e, "Failed to immediately persist dynamic skill");
        }

        tracing::info!(
            skill_name = %skill.name,
            tool = %tool_name,
            "Generated and immediately persisted dynamic skill from recurring error pattern"
        );

        self.dynamic_skills.push(skill.clone());
        Some(skill)
    }

    /// Returns a read-only view of the dynamic skills created during this session.
    #[must_use]
    pub fn dynamic_skills(&self) -> &[DynamicSkill] {
        &self.dynamic_skills
    }

    /// Returns a summary of dynamic skills suitable for injection into the system prompt.
    #[must_use]
    pub fn prompt_section(&self) -> Option<String> {
        if self.dynamic_skills.is_empty() {
            return None;
        }

        let mut lines = vec![
            "## DYNAMIC LEARNED SKILLS (auto-generated from recurring errors)".to_string(),
            String::new(),
            "The following patterns were learned from repeated errors in this session. \
             Apply them to avoid the same mistakes:"
                .to_string(),
            String::new(),
        ];

        for skill in &self.dynamic_skills {
            let diff_note = skill.input_diff.as_deref().map(|d| format!(" (Adjusted: {d})")).unwrap_or_default();
            lines.push(format!(
                "- **{}** (`{}`): Error pattern: \"{}\". Fix: {}{}",
                skill.name,
                skill.tool_name,
                truncate(&skill.error_pattern, 100),
                truncate(&skill.solution, 200),
                diff_note
            ));
        }

        Some(lines.join("\n"))
    }

    /// Returns a per-tool skill hint if a dynamic skill exists for this
    /// tool+error combination — either from the current session or from
    /// persisted skills in `omc-learned/`. This hint is appended directly
    /// to the tool's error output so the AI sees the fix right where it needs it.
    #[must_use]
    pub fn get_skill_hint(&self, tool_name: &str, error_msg: &str) -> Option<String> {
        let category = normalize_error_category(error_msg);

        // 1. Check dynamic skills from the current session.
        if let Some(skill) = self.dynamic_skills.iter().find(|s| {
            s.tool_name == tool_name
                && normalize_error_category(&s.error_pattern) == category
        }) {
            let cat_enum = ErrorCategory::from_error_message(error_msg);
            let root_cause = cat_enum.root_cause_explanation();
            let diff_hint = if let Some(diff) = &skill.input_diff {
                format!("\n▶ Verified parameter adjustments:\n{diff}\n")
            } else {
                String::new()
            };
            return Some(format!(
                "\n\n💡 AUTO-LEARNED FIX (from session errors on `{}`):\n▶ Root Cause: {}\n▶ Verified Solution:\n{}\n{}",
                tool_name, root_cause, skill.solution, diff_hint
            ));
        }

        // 2. Check persisted skills from omc-learned/ (cross-session memory).
        let expected_name = make_dynamic_skill_name(tool_name, &category);
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_else(|_| ".".to_string());
        let candidate_paths = [
            PathBuf::from(&home).join(".claw").join("skills").join("omc-learned").join(&expected_name).join("SKILL.md"),
            PathBuf::from(&home).join(".claude").join("skills").join("omc-learned").join(&expected_name).join("SKILL.md"),
            learned_skills_dir().join(&expected_name).join("SKILL.md"),
        ];
        for persisted_path in &candidate_paths {
            if persisted_path.is_file() {
                if let Ok(content) = std::fs::read_to_string(persisted_path) {
                    // Extract the Solution section from the SKILL.md.
                    let solution = extract_solution_from_skill_md(&content);
                    return Some(format!(
                        "\n\n💡 LEARNED FIX (from `{}`, persisted skill):\n{}\n",
                        expected_name, solution,
                    ));
                }
            }
        }

        None
    }

    /// Evaluates which dynamic skills were effective and returns them.
    ///
    /// A skill is effective if no errors of the same category occurred after
    /// it was created.
    #[must_use]
    pub fn effective_skills(&self) -> Vec<&DynamicSkill> {
        self.dynamic_skills
            .iter()
            .filter(|s| s.errors_after_creation == 0)
            .collect()
    }

    /// Returns all error counts for diagnostics.
    #[must_use]
    pub fn error_summary(&self) -> Vec<(String, String, usize)> {
        self.error_counts
            .iter()
            .map(|((tool, cat), records)| (tool.clone(), cat.clone(), records.len()))
            .collect()
    }
}

fn truncate(s: &str, max: usize) -> String {
    if let Some((idx, _)) = s.char_indices().nth(max) {
        format!("{}…", &s[..idx])
    } else {
        s.to_string()
    }
}

/// Extracts the "## Solution" section from a SKILL.md file content.
fn extract_solution_from_skill_md(content: &str) -> String {
    let mut in_solution = false;
    let mut lines = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("## ") && (trimmed.contains("Solution") || trimmed.contains("Recommended Solution")) {
            in_solution = true;
            continue;
        }
        if in_solution && trimmed.starts_with("## ") {
            break; // Next section starts.
        }
        if in_solution {
            lines.push(line);
        }
    }

    let result = lines.join("\n").trim().to_string();
    if result.is_empty() {
        // Fallback: return everything after the frontmatter.
        content
            .find("---\n")
            .and_then(|first| content[first + 4..].find("---\n").map(|second| first + 4 + second + 4))
            .map(|body_start| content[body_start..].trim().to_string())
            .unwrap_or_else(|| content.to_string())
    } else {
        result
    }
}

/// Helper function to construct a consistent dynamic skill name from tool_name and category.
pub(crate) fn make_dynamic_skill_name(tool_name: &str, category: &str) -> String {
    let safe_tool = tool_name.to_lowercase().replace([' ', ':'], "-");
    let safe_category: String = category
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .take(30)
        .collect();

    let raw_name = format!("autolearn-{safe_tool}-{safe_category}");
    if raw_name.len() > 50 {
        raw_name[..50].trim_end_matches('-').to_string()
    } else {
        raw_name
    }
}

// ──────────────────── Temporary skill persistence ────────────────────

/// Returns the directory used for temporary dynamic skills.
#[must_use]
pub fn temp_skills_dir() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
        .join(".claw")
        .join("tmp_skills")
}

/// Writes a dynamic skill to temporary storage.
pub fn write_temp_skill(skill: &DynamicSkill) -> std::io::Result<()> {
    if let Some(parent) = skill.temp_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&skill.temp_path, skill.to_skill_md())
}

/// Reads all temporary dynamic skills from disk.
#[must_use]
pub fn load_temp_skills() -> Vec<PathBuf> {
    let dir = temp_skills_dir();
    if !dir.is_dir() {
        return Vec::new();
    }
    let mut paths = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let skill_md = entry.path().join("SKILL.md");
            if skill_md.is_file() {
                paths.push(skill_md);
            }
        }
    }
    paths
}

/// Removes all temporary dynamic skills.
pub fn clear_temp_skills() {
    let dir = temp_skills_dir();
    if dir.is_dir() {
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Persists an effective dynamic skill to permanent `omc-learned/` storage.
pub fn persist_skill_to_learned(skill: &DynamicSkill) -> std::io::Result<PathBuf> {
    let learned_dir = learned_skills_dir();
    let dest_dir = learned_dir.join(&skill.name);
    std::fs::create_dir_all(&dest_dir)?;
    let dest_path = dest_dir.join("SKILL.md");
    std::fs::write(&dest_path, skill.to_skill_md())?;
    tracing::info!(
        skill = %skill.name,
        path = %dest_path.display(),
        "Persisted effective skill to permanent storage"
    );
    Ok(dest_path)
}

/// Returns the directory for permanent learned skills.
#[must_use]
pub fn learned_skills_dir() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    let claw_dir = PathBuf::from(&home).join(".claw").join("skills").join("omc-learned");
    if claw_dir.exists() {
        return claw_dir;
    }
    let claude_dir = PathBuf::from(&home).join(".claude").join("skills").join("omc-learned");
    if claude_dir.exists() {
        return claude_dir;
    }
    claw_dir
}

// ──────────────────── Tests ────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_error_category_maps_common_patterns() {
        assert_eq!(normalize_error_category("Permission denied: /foo/bar"), "permission_denied");
        assert_eq!(normalize_error_category("No such file or directory: test.txt"), "file_not_found");
        assert_eq!(normalize_error_category("Connection refused on port 8080"), "connection_error");
        assert_eq!(normalize_error_category("Request timed out after 30s"), "timeout");
        assert_eq!(normalize_error_category("unknown tool: foobar"), "unknown_tool");
    }

    #[test]
    fn first_error_is_not_recurring() {
        let mut tracker = ErrorTracker::new();
        let is_recurring = tracker.record_error("write_file", "Permission denied: /etc/hosts", "{}");
        assert!(!is_recurring);
    }

    #[test]
    fn second_error_same_category_is_recurring() {
        let mut tracker = ErrorTracker::new();
        tracker.record_error("write_file", "Permission denied: /etc/hosts", "{}");
        let is_recurring = tracker.record_error("write_file", "Access denied for /etc/passwd", "{}");
        assert!(is_recurring);
    }

    #[test]
    fn different_tools_are_tracked_independently() {
        let mut tracker = ErrorTracker::new();
        tracker.record_error("write_file", "Permission denied", "{}");
        let is_recurring = tracker.record_error("read_file", "Permission denied", "{}");
        assert!(!is_recurring);
    }

    #[test]
    fn success_after_recurring_error_generates_skill() {
        let mut tracker = ErrorTracker::new();
        tracker.record_error("bash", "command not found: cargo", "{}");
        tracker.record_error("bash", "command not found: cargo", "{}");
        let skill = tracker.record_success("bash", "{\"command\": \"~/.cargo/bin/cargo build\"}", "Build succeeded");
        assert!(skill.is_some());
        let skill = skill.unwrap();
        assert!(skill.name.starts_with("autolearn-bash-"));
        assert_eq!(skill.tool_name, "bash");
    }

    #[test]
    fn success_without_recurring_error_generates_nothing() {
        let mut tracker = ErrorTracker::new();
        tracker.record_error("bash", "command not found: cargo", "{}");
        // Only one error — not recurring.
        let skill = tracker.record_success("bash", "{}", "ok");
        assert!(skill.is_none());
    }

    #[test]
    fn duplicate_skill_not_created() {
        let mut tracker = ErrorTracker::new();
        tracker.record_error("bash", "command not found: cargo", "{}");
        tracker.record_error("bash", "command not found: cargo", "{}");
        let first = tracker.record_success("bash", "{}", "ok");
        assert!(first.is_some());
        // Now record the same error pattern again + success — should not create a second skill.
        tracker.record_error("bash", "command not found: cargo", "{}");
        let second = tracker.record_success("bash", "{}", "ok");
        assert!(second.is_none());
    }

    #[test]
    fn effective_skills_excludes_skills_with_subsequent_errors() {
        let mut tracker = ErrorTracker::new();
        tracker.record_error("bash", "permission denied", "{}");
        tracker.record_error("bash", "permission denied", "{}");
        let _skill = tracker.record_success("bash", "{}", "ok");

        // Skill was created but then the same error happened again.
        tracker.record_error("bash", "access denied", "{}");

        let effective = tracker.effective_skills();
        assert!(effective.is_empty());
    }

    #[test]
    fn effective_skills_includes_skills_without_subsequent_errors() {
        let mut tracker = ErrorTracker::new();
        tracker.record_error("bash", "permission denied", "{}");
        tracker.record_error("bash", "permission denied", "{}");
        let _skill = tracker.record_success("bash", "{}", "ok");

        // No more errors of the same type.
        let effective = tracker.effective_skills();
        assert_eq!(effective.len(), 1);
    }

    #[test]
    fn prompt_section_is_none_when_empty() {
        let tracker = ErrorTracker::new();
        assert!(tracker.prompt_section().is_none());
    }

    #[test]
    fn prompt_section_includes_skill_info() {
        let mut tracker = ErrorTracker::new();
        tracker.record_error("write_file", "permission denied: /root/x", "{}");
        tracker.record_error("write_file", "permission denied: /root/y", "{}");
        tracker.record_success("write_file", "{\"path\": \"/tmp/x\"}", "ok");

        let section = tracker.prompt_section();
        assert!(section.is_some());
        let text = section.unwrap();
        assert!(text.contains("DYNAMIC LEARNED SKILLS"));
        assert!(text.contains("autolearn-write_file-"));
    }

    #[test]
    fn max_dynamic_skills_limit_enforced() {
        let mut tracker = ErrorTracker::new();
        for i in 0..12 {
            let tool_name = format!("tool_{i}");
            tracker.record_error(&tool_name, "error X", "{}");
            tracker.record_error(&tool_name, "error X", "{}");
            tracker.record_success(&tool_name, "{}", "ok");
        }
        assert_eq!(tracker.dynamic_skills().len(), MAX_DYNAMIC_SKILLS);
    }

    #[test]
    fn skill_to_skill_md_produces_valid_frontmatter() {
        let skill = DynamicSkill {
            name: "autolearn-bash-timeout".to_string(),
            tool_name: "bash".to_string(),
            error_pattern: "command timed out".to_string(),
            solution: "Use --timeout flag".to_string(),
            input_diff: Some("- Added parameter `timeout`: `30000`".to_string()),
            created_at_ms: 0,
            was_effective: true,
            temp_path: PathBuf::from("/tmp/test"),
            errors_after_creation: 0,
            success_count: 2,
        };
        let md = skill.to_skill_md();
        assert!(md.starts_with("---\n"));
        assert!(md.contains("name: autolearn-bash-timeout"));
        assert!(md.contains("effectiveness_score: 1.00"));
        assert!(md.contains("## 🎯 Trigger Conditions"));
        assert!(md.contains("## 🔍 Root Cause Analysis"));
        assert!(md.contains("## 💡 Recommended Solution & Correct Parameters"));
        assert!(md.contains("## ⚠️ Anti-Patterns & Common Mistakes"));
        assert!(md.contains("Parameter Adjustments"));
    }

    #[test]
    fn diff_tool_inputs_detects_parameter_changes() {
        let failed = r#"{"url": "https://example.com"}"#;
        let success = r#"{"url": "https://example.com", "timeout": 30000, "waitUntil": "domcontentloaded"}"#;
        let diff = diff_tool_inputs(failed, success);
        assert!(diff.is_some());
        let d = diff.unwrap();
        assert!(d.contains("Added parameter `timeout`"));
        assert!(d.contains("Added parameter `waitUntil`"));
    }

    #[test]
    fn error_category_provides_actionable_context() {
        let cat = ErrorCategory::from_error_message("EACCES: permission denied to /etc/hosts");
        assert_eq!(cat, ErrorCategory::PermissionDenied);
        assert_eq!(cat.as_str(), "permission_denied");
        assert!(cat.root_cause_explanation().contains("permissions"));
        assert!(!cat.anti_patterns().is_empty());
    }

    fn run_isolated_test<F: FnOnce()>(test_fn: F) {
        let _guard = crate::test_env_lock();
        let temp = tempfile::tempdir().unwrap();
        let original_home = std::env::var("HOME").ok();
        std::env::set_var("HOME", temp.path());

        test_fn();

        if let Some(home) = original_home {
            std::env::set_var("HOME", home);
        } else {
            std::env::remove_var("HOME");
        }
    }

    #[test]
    fn get_skill_hint_returns_none_when_no_skill() {
        run_isolated_test(|| {
            let tracker = ErrorTracker::new();
            assert!(tracker.get_skill_hint("bash", "permission denied").is_none());
        });
    }

    #[test]
    fn get_skill_hint_returns_hint_for_matching_tool_and_error() {
        run_isolated_test(|| {
            let mut tracker = ErrorTracker::new();
            tracker.record_error("bash", "permission denied: /root/x", "{}");
            tracker.record_error("bash", "permission denied: /root/y", "{}");
            tracker.record_success("bash", "{\"cmd\": \"sudo ...\"}", "ok");

            let hint = tracker.get_skill_hint("bash", "access denied for /etc/shadow");
            assert!(hint.is_some());
            let text = hint.unwrap();
            assert!(text.contains("AUTO-LEARNED FIX"));
            assert!(text.contains("bash"));
        });
    }

    #[test]
    fn get_skill_hint_does_not_match_different_tool() {
        run_isolated_test(|| {
            let mut tracker = ErrorTracker::new();
            tracker.record_error("bash", "permission denied", "{}");
            tracker.record_error("bash", "permission denied", "{}");
            tracker.record_success("bash", "{}", "ok");

            // Different tool.
            assert!(tracker.get_skill_hint("write_file", "permission denied").is_none());
        });
    }

    #[test]
    fn get_skill_hint_does_not_match_different_error_category() {
        run_isolated_test(|| {
            let mut tracker = ErrorTracker::new();
            tracker.record_error("bash", "permission denied", "{}");
            tracker.record_error("bash", "permission denied", "{}");
            tracker.record_success("bash", "{}", "ok");

            // Different error category.
            assert!(tracker.get_skill_hint("bash", "connection refused on port 80").is_none());
        });
    }

    #[test]
    fn extract_solution_from_skill_md_finds_section() {
        let content = "---\nname: test\n---\n\n## Problem\nSome problem\n\n## Solution\nUse sudo or /tmp/ path.\n\n## Details\nMore info\n";
        let solution = super::extract_solution_from_skill_md(content);
        assert_eq!(solution, "Use sudo or /tmp/ path.");
    }

    #[test]
    fn extract_solution_falls_back_to_body_when_no_section() {
        let content = "---\nname: test\n---\n\nJust some body text without sections.\n";
        let solution = super::extract_solution_from_skill_md(content);
        assert!(solution.contains("Just some body text"));
    }

    #[test]
    fn get_skill_hint_finds_persisted_skill_from_omc_learned() {
        run_isolated_test(|| {
            let home = std::env::var("HOME").map(PathBuf::from).unwrap();
            let skill_dir = home
                .join(".claude")
                .join("skills")
                .join("omc-learned")
                .join("autolearn-bash-permission_denied");
            std::fs::create_dir_all(&skill_dir).unwrap();
            std::fs::write(
                skill_dir.join("SKILL.md"),
                "---\nname: autolearn-bash-permission_denied\ndescription: Fix for bash permission errors\n---\n\n## Problem\npermission denied\n\n## Solution\nUse /tmp/ instead of /root/.\n",
            ).unwrap();

            let tracker = ErrorTracker::new();
            // No dynamic skills — but persisted skill exists.
            let hint = tracker.get_skill_hint("bash", "permission denied: /root/x");
            assert!(hint.is_some(), "Should find persisted skill");
            let text = hint.unwrap();
            assert!(text.contains("LEARNED FIX"));
            assert!(text.contains("Use /tmp/ instead of /root/"));
        });
    }

    #[test]
    fn resolve_effective_tool_name_passes_through_normal_tools() {
        assert_eq!(
            super::resolve_effective_tool_name("bash", "{}").as_ref(),
            "bash"
        );
        assert_eq!(
            super::resolve_effective_tool_name("write_file", "{\"path\": \"/tmp/x\"}").as_ref(),
            "write_file"
        );
    }

    #[test]
    fn resolve_effective_tool_name_extracts_qualified_name_from_mcp_tool() {
        let input = r#"{"qualifiedName": "xcode-bridge/BuildProject", "arguments": {}}"#;
        assert_eq!(
            super::resolve_effective_tool_name("MCPTool", input).as_ref(),
            "mcp:xcode-bridge/BuildProject"
        );
    }

    #[test]
    fn resolve_effective_tool_name_extracts_tool_field_from_mcp_tool() {
        let input = r#"{"tool": "firebase/deploy", "arguments": {}}"#;
        assert_eq!(
            super::resolve_effective_tool_name("MCPTool", input).as_ref(),
            "mcp:firebase/deploy"
        );
    }

    #[test]
    fn resolve_effective_tool_name_extracts_server_and_tool_from_call_mcp_tool() {
        let input = r#"{"ServerName": "puppeteer", "ToolName": "puppeteer_navigate", "Arguments": {"url": "https://example.com"}}"#;
        assert_eq!(
            super::resolve_effective_tool_name("call_mcp_tool", input).as_ref(),
            "mcp:puppeteer:puppeteer_navigate"
        );
    }

    #[test]
    fn resolve_effective_tool_name_falls_back_for_mcp_without_name() {
        let input = r#"{"arguments": {}}"#;
        assert_eq!(
            super::resolve_effective_tool_name("MCPTool", input).as_ref(),
            "MCPTool"
        );
    }

    #[test]
    fn summarize_input_truncates_safely() {
        let long_input = "a".repeat(2000);
        let summary = super::summarize_input(&long_input);
        assert!(summary.ends_with('…'));
        assert_eq!(summary.chars().count(), 1025); // 1024 + 1 for '…'
    }

    #[test]
    fn summarize_input_handles_multibyte_chars_safely() {
        let long_input = "в".repeat(2000); // Cyrillic 'в' is 2 bytes
        let summary = super::summarize_input(&long_input);
        assert!(summary.ends_with('…'));
        assert_eq!(summary.chars().count(), 1025); // 1024 + 1 for '…'
    }

    #[test]
    fn truncate_handles_multibyte_chars_safely() {
        let long_input = "в".repeat(2000);
        let truncated = super::truncate(&long_input, 1024);
        assert!(truncated.ends_with('…'));
        assert_eq!(truncated.chars().count(), 1025); // 1024 + 1 for '…'
    }

    #[test]
    fn make_dynamic_skill_name_consistency() {
        let name1 = super::make_dynamic_skill_name("bash", "PermissionDenied");
        assert_eq!(name1, "autolearn-bash-PermissionDenied");

        let name2 = super::make_dynamic_skill_name("MCP Tool", "Special chars in category & % #!");
        assert!(name2.starts_with("autolearn-mcp-tool-"));
        assert!(!name2.contains('%'));
        assert!(name2.len() <= 50);
    }
}
