//! Tracks recurring tool errors and generates dynamic skills when patterns emerge.
//!
//! When the same tool fails with a similar error more than once during a session,
//! and the tool later succeeds, a `DynamicSkill` is generated and stored in
//! temporary memory. On session shutdown, effective skills are persisted to
//! permanent storage (`omc-learned/`).

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// A single recorded tool error.
#[derive(Debug, Clone)]
pub struct ToolErrorRecord {
    pub tool_name: String,
    pub error_category: String,
    pub error_message: String,
    pub input_summary: String,
    pub timestamp_ms: u64,
}

/// A dynamically generated skill derived from a recurring error pattern.
#[derive(Debug, Clone)]
pub struct DynamicSkill {
    pub name: String,
    pub tool_name: String,
    pub error_pattern: String,
    pub solution: String,
    pub created_at_ms: u64,
    pub was_effective: bool,
    pub temp_path: PathBuf,
    /// Number of errors that occurred after this skill was created.
    pub errors_after_creation: usize,
}

impl DynamicSkill {
    /// Renders this skill as a SKILL.md file.
    #[must_use]
    pub fn to_skill_md(&self) -> String {
        format!(
            "---\nname: {name}\ndescription: \"Auto-learned fix for {tool} error: {error}\"\n---\n\
             # Auto-learned: {tool}\n\n\
             ## Problem\n\
             When using `{tool}`, the following error occurs:\n\
             > {error}\n\n\
             ## Solution\n\
             The correct approach is:\n\
             > {solution}\n",
            name = self.name,
            tool = self.tool_name,
            error = self.error_pattern,
            solution = self.solution,
        )
    }
}

/// Normalizes an error message into a category key for deduplication.
///
/// This extracts a stable signature from error messages so that superficially
/// different messages that refer to the same root cause are grouped together.
#[must_use]
pub fn normalize_error_category(error_msg: &str) -> String {
    let lower = error_msg.to_lowercase();

    // Map known error patterns to stable categories.
    let patterns: &[(&[&str], &str)] = &[
        (&["permission denied", "access denied", "not permitted"], "permission_denied"),
        (&["no such file", "file not found", "not found", "does not exist"], "file_not_found"),
        (&["already exists"], "already_exists"),
        (&["syntax error", "parse error", "invalid syntax"], "syntax_error"),
        (&["timeout", "timed out", "deadline exceeded"], "timeout"),
        (&["connection refused", "connection reset"], "connection_error"),
        (&["out of memory", "oom"], "out_of_memory"),
        (&["invalid argument", "invalid input", "invalid parameter"], "invalid_argument"),
        (&["unknown tool"], "unknown_tool"),
    ];

    for (keywords, category) in patterns {
        if keywords.iter().any(|kw| lower.contains(kw)) {
            return (*category).to_string();
        }
    }

    // Fallback: use the first 80 characters, stripped of paths/hashes.
    let stripped = lower
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == ' ')
        .take(80)
        .collect::<String>();
    stripped.split_whitespace().take(8).collect::<Vec<_>>().join("_")
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
    if tool_name == "MCPTool" {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(input) {
            if let Some(name) = v.get("qualifiedName").and_then(|v| v.as_str())
                .or_else(|| v.get("tool").and_then(|v| v.as_str()))
            {
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
        // Find any error categories for this tool with >= 2 occurrences
        // that don't already have a dynamic skill.
        let mut candidate_key = None;
        for ((name, category), records) in &self.error_counts {
            if name != tool_name || records.len() < 2 {
                continue;
            }
            // Check if we already have a skill for this.
            let already_has_skill = self.dynamic_skills.iter().any(|s| {
                s.tool_name == tool_name
                    && normalize_error_category(&s.error_pattern) == *category
            });
            if !already_has_skill {
                candidate_key = Some((name.clone(), category.clone()));
                break;
            }
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

        // Get the representative error message from the records.
        let error_msg = self
            .error_counts
            .get(&(tool_name.to_string(), category.clone()))
            .and_then(|records| records.last())
            .map(|r| r.error_message.clone())
            .unwrap_or_default();

        let solution_summary = truncate(output, 1024);

        let safe_tool = tool_name.to_lowercase().replace(' ', "-");
        let safe_category: String = category
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
            .take(30)
            .collect();

        let raw_name = format!("autolearn-{safe_tool}-{safe_category}");
        let name = if raw_name.len() > 50 {
            raw_name[..50].trim_end_matches('-').to_string()
        } else {
            raw_name
        };

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
            created_at_ms: now_ms(),
            was_effective: false,
            temp_path,
            errors_after_creation: 0,
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
            lines.push(format!(
                "- **{}** (`{}`): Error pattern: \"{}\". Fix: {}",
                skill.name,
                skill.tool_name,
                truncate(&skill.error_pattern, 100),
                truncate(&skill.solution, 200),
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
            return Some(format!(
                "\n\n💡 AUTO-LEARNED FIX (from session errors on `{}`):\n{}\n",
                tool_name, skill.solution,
            ));
        }

        // 2. Check persisted skills from omc-learned/ (cross-session memory).
        let expected_name = format!(
            "autolearn-{}-{}",
            tool_name.to_lowercase().replace(' ', "-"),
            &category,
        );
        let persisted_path = learned_skills_dir().join(&expected_name).join("SKILL.md");
        if persisted_path.is_file() {
            if let Ok(content) = std::fs::read_to_string(&persisted_path) {
                // Extract the Solution section from the SKILL.md.
                let solution = extract_solution_from_skill_md(&content);
                return Some(format!(
                    "\n\n💡 LEARNED FIX (from `{}`, persisted skill):\n{}\n",
                    expected_name, solution,
                ));
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
        if line.starts_with("## Solution") {
            in_solution = true;
            continue;
        }
        if in_solution && line.starts_with("## ") {
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
    PathBuf::from(home)
        .join(".claude")
        .join("skills")
        .join("omc-learned")
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
            created_at_ms: 0,
            was_effective: false,
            temp_path: PathBuf::from("/tmp/test"),
            errors_after_creation: 0,
        };
        let md = skill.to_skill_md();
        assert!(md.starts_with("---\n"));
        assert!(md.contains("name: autolearn-bash-timeout"));
        assert!(md.contains("## Problem"));
        assert!(md.contains("## Solution"));
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
}
