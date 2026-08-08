use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DomainKind {
    IOS,
    Rust,
    Python,
    Web,
    Generic,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolPriorityConfig {
    pub domain: DomainKind,
    pub primary_mcp_servers: Vec<String>,
    pub primary_tools: Vec<String>,
    pub fallback_tools: Vec<String>,
    pub is_manual_override: bool,
}

impl Default for ToolPriorityConfig {
    fn default() -> Self {
        Self {
            domain: DomainKind::Generic,
            primary_mcp_servers: Vec::new(),
            primary_tools: vec![
                "read_file".into(),
                "write_file".into(),
                "edit_file".into(),
                "glob_search".into(),
                "grep_search".into(),
                "TaskGraph".into(),
                "bash".into(),
            ],
            fallback_tools: vec!["bash".into()],
            is_manual_override: false,
        }
    }
}

/// Detect project domain based on workspace files.
pub fn detect_domain_from_workspace(workspace_root: &Path) -> DomainKind {
    if workspace_root.join(".xcodegen.yml").exists()
        || workspace_root.join("project.yml").exists()
        || has_file_with_extension(workspace_root, "xcodeproj")
        || has_file_with_extension(workspace_root, "xcworkspace")
        || workspace_root.join("Package.swift").exists()
    {
        return DomainKind::IOS;
    }

    if workspace_root.join("Cargo.toml").exists() {
        return DomainKind::Rust;
    }

    if workspace_root.join("pyproject.toml").exists()
        || workspace_root.join("requirements.txt").exists()
        || workspace_root.join("Pipfile").exists()
    {
        return DomainKind::Python;
    }

    if workspace_root.join("package.json").exists()
        || workspace_root.join("tsconfig.json").exists()
    {
        return DomainKind::Web;
    }

    DomainKind::Generic
}

fn has_file_with_extension(dir: &Path, ext: &str) -> bool {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if let Some(e) = entry.path().extension() {
                if e == ext {
                    return true;
                }
            }
        }
    }
    false
}

/// Dynamically refine domain detection based on task prompt text.
pub fn detect_domain_from_prompt(task_prompt: &str) -> Option<DomainKind> {
    let lower = task_prompt.to_lowercase();
    if lower.contains("ios")
        || lower.contains("simulator")
        || lower.contains("xcode")
        || lower.contains("swift")
        || lower.contains("xcodegen")
        || lower.contains("appstore")
    {
        return Some(DomainKind::IOS);
    }
    if lower.contains("cargo") || lower.contains("rust") || lower.contains("crate") {
        return Some(DomainKind::Rust);
    }
    if lower.contains("python")
        || lower.contains("pytest")
        || lower.contains("pip")
        || lower.contains("pyscn")
        || lower.contains("notebook")
    {
        return Some(DomainKind::Python);
    }
    if lower.contains("npm")
        || lower.contains("yarn")
        || lower.contains("react")
        || lower.contains("nextjs")
        || lower.contains("firebase")
        || lower.contains("render")
    {
        return Some(DomainKind::Web);
    }
    None
}

/// Resolve priority configuration dynamically or via manual override.
pub fn resolve_tool_priorities(
    workspace_root: &Path,
    task_prompt: Option<&str>,
    manual_override: Option<ToolPriorityConfig>,
) -> ToolPriorityConfig {
    if let Some(mut manual) = manual_override {
        manual.is_manual_override = true;
        return manual;
    }

    // Check manual override via environment variable CLAWD_TOOL_PRIORITIES (JSON format)
    if let Ok(env_json) = std::env::var("CLAWD_TOOL_PRIORITIES") {
        if let Ok(config) = serde_json::from_str::<ToolPriorityConfig>(&env_json) {
            let mut c = config;
            c.is_manual_override = true;
            return c;
        }
    }

    // Dynamic resolution
    let workspace_domain = detect_domain_from_workspace(workspace_root);
    let prompt_domain = task_prompt.and_then(detect_domain_from_prompt);

    let domain = prompt_domain.unwrap_or(workspace_domain);

    match domain {
        DomainKind::IOS => ToolPriorityConfig {
            domain: DomainKind::IOS,
            primary_mcp_servers: vec![
                "xcode-bridge".into(),
                "ios-simulator".into(),
                "swiftlens".into(),
                "appstore-connect".into(),
            ],
            primary_tools: vec![
                "xcode-bridge".into(),
                "ios-simulator".into(),
                "swiftlens".into(),
                "read_file".into(),
                "write_file".into(),
                "edit_file".into(),
                "TaskGraph".into(),
            ],
            fallback_tools: vec!["bash".into()],
            is_manual_override: false,
        },
        DomainKind::Rust => ToolPriorityConfig {
            domain: DomainKind::Rust,
            primary_mcp_servers: Vec::new(),
            primary_tools: vec![
                "read_file".into(),
                "write_file".into(),
                "edit_file".into(),
                "glob_search".into(),
                "grep_search".into(),
                "bash".into(),
                "TaskGraph".into(),
            ],
            fallback_tools: vec!["bash".into()],
            is_manual_override: false,
        },
        DomainKind::Python => ToolPriorityConfig {
            domain: DomainKind::Python,
            primary_mcp_servers: vec!["pyscn-mcp".into(), "notebooks".into()],
            primary_tools: vec![
                "pyscn-mcp".into(),
                "notebooks".into(),
                "read_file".into(),
                "write_file".into(),
                "edit_file".into(),
                "TaskGraph".into(),
            ],
            fallback_tools: vec!["bash".into()],
            is_manual_override: false,
        },
        DomainKind::Web => ToolPriorityConfig {
            domain: DomainKind::Web,
            primary_mcp_servers: vec![
                "firebase-mcp-server".into(),
                "render".into(),
                "visualization".into(),
            ],
            primary_tools: vec![
                "firebase-mcp-server".into(),
                "render".into(),
                "read_file".into(),
                "write_file".into(),
                "edit_file".into(),
                "TaskGraph".into(),
            ],
            fallback_tools: vec!["bash".into()],
            is_manual_override: false,
        },
        DomainKind::Generic => ToolPriorityConfig::default(),
    }
}

// ---------------------------------------------------------------------------
// Non-Repetition & Fallback Cascade Tracker
// ---------------------------------------------------------------------------

static FAILURE_TRACKER: std::sync::OnceLock<Mutex<HashMap<String, u32>>> = std::sync::OnceLock::new();

fn get_tracker() -> &'static Mutex<HashMap<String, u32>> {
    FAILURE_TRACKER.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Track consecutive tool failures and recommend switching priority / tools.
pub fn record_and_evaluate_tool_failure(tool_name: &str, command_or_input: &str) -> Option<String> {
    let key = format!("{}:{}", tool_name, command_or_input.trim());
    let mut tracker = get_tracker().lock().unwrap();
    let count = tracker.entry(key.clone()).or_insert(0);
    *count += 1;

    if *count >= 2 {
        Some(format!(
            "⚠️ REPETITION BLOCKER & FALLBACK CASCADE: Tool '{}' with input ('{}') has failed {} consecutive times.\n\
             DO NOT repeat the exact same tool call again!\n\
             REQUIRED ACTION: Switch to the next priority tool in your cascade (e.g. use fallback MCP server or fix underlying source files before retrying).",
            tool_name, command_or_input.trim(), *count
        ))
    } else {
        None
    }
}

/// Clear failure counts (called on successful execution).
pub fn clear_tool_failure(tool_name: &str, command_or_input: &str) {
    let key = format!("{}:{}", tool_name, command_or_input.trim());
    if let Ok(mut tracker) = get_tracker().lock() {
        tracker.remove(&key);
    }
}

/// Static priority mapping for all 10 registered MCP servers in the ecosystem.
pub fn get_static_mcp_priority_map() -> Vec<(&'static str, &'static str, u32)> {
    vec![
        // (MCP Server Name, Primary Domain, Static Priority Rank [1=highest])
        ("xcode-bridge", "iOS", 1),
        ("ios-simulator", "iOS", 2),
        ("swiftlens", "iOS", 3),
        ("appstore-connect", "iOS", 4),
        ("pyscn-mcp", "Python", 1),
        ("notebooks", "Python", 2),
        ("firebase-mcp-server", "Web", 1),
        ("render", "Web", 2),
        ("visualization", "Web", 3),
        ("github", "DevOps", 1),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_domain_detection_from_prompt() {
        assert_eq!(detect_domain_from_prompt("Build iOS app on simulator"), Some(DomainKind::IOS));
        assert_eq!(detect_domain_from_prompt("Fix cargo build error in crate"), Some(DomainKind::Rust));
        assert_eq!(detect_domain_from_prompt("Run pytest unit tests"), Some(DomainKind::Python));
        assert_eq!(detect_domain_from_prompt("Deploy to Firebase hosting"), Some(DomainKind::Web));
        assert_eq!(detect_domain_from_prompt("Hello world"), None);
    }

    #[test]
    fn test_resolve_tool_priorities_all_domains() {
        let dir = std::env::temp_dir();

        // iOS domain test
        let config_ios = resolve_tool_priorities(&dir, Some("Build Xcode project for simulator"), None);
        assert_eq!(config_ios.domain, DomainKind::IOS);
        assert_eq!(config_ios.primary_mcp_servers[0], "xcode-bridge");
        assert_eq!(config_ios.primary_mcp_servers[1], "ios-simulator");

        // Python domain test
        let config_py = resolve_tool_priorities(&dir, Some("Analyze python code with pyscn and notebook"), None);
        assert_eq!(config_py.domain, DomainKind::Python);
        assert!(config_py.primary_mcp_servers.contains(&"pyscn-mcp".to_string()));

        // Web domain test
        let config_web = resolve_tool_priorities(&dir, Some("Deploy React app to firebase and render"), None);
        assert_eq!(config_web.domain, DomainKind::Web);
        assert!(config_web.primary_mcp_servers.contains(&"firebase-mcp-server".to_string()));
    }

    #[test]
    fn test_static_mcp_priority_map() {
        let table = get_static_mcp_priority_map();
        assert_eq!(table.len(), 10);
        let names: Vec<&str> = table.iter().map(|(name, _, _)| *name).collect();
        assert!(names.contains(&"xcode-bridge"));
        assert!(names.contains(&"ios-simulator"));
        assert!(names.contains(&"appstore-connect"));
        assert!(names.contains(&"firebase-mcp-server"));
        assert!(names.contains(&"github"));
    }

    #[test]
    fn test_failure_tracker_repetition_blocker() {
        let tool = "xcodebuild_test";
        let input = "build scheme";
        clear_tool_failure(tool, input);

        assert!(record_and_evaluate_tool_failure(tool, input).is_none());
        let alert = record_and_evaluate_tool_failure(tool, input);
        assert!(alert.is_some());
        assert!(alert.unwrap().contains("REPETITION BLOCKER"));

        clear_tool_failure(tool, input);
    }
}
