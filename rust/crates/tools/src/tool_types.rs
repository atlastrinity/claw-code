use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use runtime::{LaneEvent, LaneEventBlocker, McpDegradedReport};
use runtime::worker_boot::WorkerTaskReceipt;
use crate::util::deserialize_optional_usize;

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub(crate) struct McpSearchInput {
    pub query: Option<String>,
    pub load_server: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ReadFileInput {
    pub path: String,
    #[serde(default, deserialize_with = "deserialize_optional_usize")]
    pub offset: Option<usize>,
    #[serde(default, deserialize_with = "deserialize_optional_usize")]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WriteFileInput {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct EditFileInput {
    pub path: String,
    pub old_string: String,
    pub new_string: String,
    pub replace_all: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct GlobSearchInputValue {
    pub pattern: String,
    pub path: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WebFetchInput {
    pub url: String,
    pub prompt: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WebSearchInput {
    pub query: String,
    pub allowed_domains: Option<Vec<String>>,
    pub blocked_domains: Option<Vec<String>>,
}



#[derive(Debug, Deserialize)]
pub(crate) struct SkillInput {
    pub skill: String,
    pub args: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AgentInput {
    pub description: String,
    pub prompt: String,
    pub subagent_type: Option<String>,
    pub name: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ToolSearchInput {
    pub query: String,
    pub max_results: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct NotebookEditInput {
    pub notebook_path: String,
    pub cell_id: Option<String>,
    pub new_source: Option<String>,
    pub cell_type: Option<NotebookCellType>,
    pub edit_mode: Option<NotebookEditMode>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum NotebookCellType {
    Code,
    Markdown,
}

#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum NotebookEditMode {
    Replace,
    Insert,
    Delete,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SleepInput {
    pub duration_ms: u64,
}

#[derive(Debug, Deserialize)]
pub(crate) struct BriefInput {
    pub message: String,
    pub attachments: Option<Vec<String>>,
    pub status: BriefStatus,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum BriefStatus {
    Normal,
    Proactive,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ConfigInput {
    pub setting: String,
    pub value: Option<ConfigValue>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct EnterPlanModeInput {}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct ExitPlanModeInput {}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum ConfigValue {
    String(String),
    Bool(bool),
    Number(f64),
}

#[derive(Debug, Deserialize)]
#[serde(transparent)]
pub(crate) struct StructuredOutputInput(pub BTreeMap<String, Value>);

#[derive(Debug, Deserialize)]
pub(crate) struct ReplInput {
    pub code: String,
    pub language: String,
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PowerShellInput {
    pub command: String,
    pub timeout: Option<u64>,
    pub description: Option<String>,
    pub run_in_background: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AskUserQuestionInput {
    pub question: String,
    #[serde(default)]
    pub options: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct TaskCreateInput {
    pub prompt: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct TaskIdInput {
    pub task_id: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct TaskUpdateInput {
    pub task_id: String,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WorkerCreateInput {
    pub cwd: String,
    #[serde(default)]
    pub trusted_roots: Vec<String>,
    #[serde(default = "default_auto_recover_prompt_misdelivery")]
    pub auto_recover_prompt_misdelivery: bool,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WorkerIdInput {
    pub worker_id: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WorkerObserveCompletionInput {
    pub worker_id: String,
    pub finish_reason: String,
    pub tokens_output: u64,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WorkerObserveInput {
    pub worker_id: String,
    pub screen_text: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WorkerSendPromptInput {
    pub worker_id: String,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub task_receipt: Option<WorkerTaskReceipt>,
}

pub(crate) const fn default_auto_recover_prompt_misdelivery() -> bool {
    true
}

#[derive(Debug, Deserialize)]
pub(crate) struct TeamCreateInput {
    pub name: String,
    pub tasks: Vec<Value>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct TeamDeleteInput {
    pub team_id: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CronCreateInput {
    pub schedule: String,
    pub prompt: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CronDeleteInput {
    pub cron_id: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct LspInput {
    pub action: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub line: Option<u32>,
    #[serde(default)]
    pub character: Option<u32>,
    #[serde(default)]
    pub query: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct McpResourceInput {
    #[serde(default)]
    pub server: Option<String>,
    #[serde(default)]
    pub uri: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct McpAuthInput {
    pub server: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RemoteTriggerInput {
    pub url: String,
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub headers: Option<Value>,
    #[serde(default)]
    pub body: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct McpToolInput {
    pub server: String,
    pub tool: String,
    #[serde(default)]
    pub arguments: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct TestingPermissionInput {
    pub action: String,
}

/// Input for the GitStatus tool: shows working tree status.
/// Defaults to --short --branch mode for concise, parseable output.
#[derive(Debug, Deserialize)]
pub(crate) struct GitStatusInput {
    #[serde(default)]
    /// If true, use --short --branch format. Defaults to true.
    pub short: Option<bool>,
}

/// Input for the GitDiff tool: shows changes between commits, index, and working tree.
/// All fields are optional - calling with no options is equivalent to `git diff`.
#[derive(Debug, Deserialize)]
pub(crate) struct GitDiffInput {
    #[serde(default)]
    /// File path to diff. Prepends `--` before the path.
    pub path: Option<String>,
    #[serde(default)]
    /// If true, show staged changes (`git diff --cached`).
    pub staged: Option<bool>,
    #[serde(default)]
    /// A commit hash, tag, or branch to diff against.
    pub commit: Option<String>,
    #[serde(default)]
    /// A second commit for range diffs (commit...commit2).
    pub commit2: Option<String>,
}

/// Input for the GitLog tool: shows commit history.
/// Defaults to the last 20 commits in full format.
#[derive(Debug, Deserialize)]
pub(crate) struct GitLogInput {
    #[serde(default)]
    /// File or directory path to filter commits by.
    pub path: Option<String>,
    #[serde(default)]
    /// Maximum number of commits to return. Defaults to 20.
    pub count: Option<usize>,
    #[serde(default)]
    /// If true, use --oneline format (hash + subject only).
    pub oneline: Option<bool>,
    #[serde(default)]
    /// Filter commits by author pattern.
    pub author: Option<String>,
    #[serde(default)]
    /// Filter commits since date (e.g. "2024-01-01" or "2.weeks").
    pub since: Option<String>,
    #[serde(default)]
    /// Filter commits until date.
    pub until: Option<String>,
}

/// Input for the GitShow tool: shows a commit, tag, or tree object.
#[derive(Debug, Deserialize)]
pub(crate) struct GitShowInput {
    /// Commit hash, tag, or branch ref to show. Required.
    pub commit: String,
    #[serde(default)]
    /// If set, show only this file at the given commit (commit:path syntax).
    pub path: Option<String>,
    #[serde(default)]
    /// If true, show diffstat summary instead of full diff.
    pub stat: Option<bool>,
    #[serde(default)]
    /// Output format: "patch" (default) shows the full diff, "stat" shows a diffstat summary, and "metadata" shows commit info without the diff. When set, takes priority over `stat`.
    pub format: Option<String>,
}

/// Input for the GitBlame tool: shows per-line author/revision info for a file.
#[derive(Debug, Deserialize)]
pub(crate) struct GitBlameInput {
    /// File path to blame. Required.
    pub path: String,
    #[serde(rename = "start_line")]
    #[serde(default)]
    /// Start of line range (1-based). Only used if end_line is also set.
    pub start_line: Option<usize>,
    #[serde(rename = "end_line")]
    #[serde(default)]
    /// End of line range (1-based). Only used if start_line is also set.
    pub end_line: Option<usize>,
}

#[derive(Debug, Serialize)]
pub(crate) struct WebFetchOutput {
    pub bytes: usize,
    pub code: u16,
    #[serde(rename = "codeText")]
    pub code_text: String,
    pub result: String,
    #[serde(rename = "durationMs")]
    pub duration_ms: u128,
    pub url: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct WebSearchOutput {
    pub query: String,
    pub results: Vec<WebSearchResultItem>,
    #[serde(rename = "durationSeconds")]
    pub duration_seconds: f64,
}



#[derive(Debug, Serialize)]
pub(crate) struct SkillOutput {
    pub skill: String,
    pub path: String,
    pub args: Option<String>,
    pub description: Option<String>,
    pub prompt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AgentOutput {
    #[serde(rename = "agentId")]
    pub agent_id: String,
    pub name: String,
    pub description: String,
    #[serde(rename = "subagentType")]
    pub subagent_type: Option<String>,
    pub model: Option<String>,
    pub status: String,
    #[serde(rename = "outputFile")]
    pub output_file: String,
    #[serde(rename = "manifestFile")]
    pub manifest_file: String,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "startedAt", skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(rename = "completedAt", skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    #[serde(rename = "laneEvents", default, skip_serializing_if = "Vec::is_empty")]
    pub lane_events: Vec<LaneEvent>,
    #[serde(rename = "currentBlocker", skip_serializing_if = "Option::is_none")]
    pub current_blocker: Option<LaneEventBlocker>,
    #[serde(rename = "derivedState")]
    pub derived_state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct AgentJob {
    pub manifest: AgentOutput,
    pub prompt: String,
    pub system_prompt: Vec<String>,
    pub tools: BTreeSet<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ToolSearchOutput {
    pub matches: Vec<String>,
    pub query: String,
    pub normalized_query: String,
    #[serde(rename = "total_deferred_tools")]
    pub total_deferred_tools: usize,
    #[serde(rename = "pending_mcp_servers")]
    pub pending_mcp_servers: Option<Vec<String>>,
    #[serde(rename = "mcp_degraded", skip_serializing_if = "Option::is_none")]
    pub mcp_degraded: Option<McpDegradedReport>,
}

#[derive(Debug, Serialize)]
pub(crate) struct NotebookEditOutput {
    pub new_source: String,
    pub cell_id: Option<String>,
    pub cell_type: Option<NotebookCellType>,
    pub language: String,
    pub edit_mode: String,
    pub error: Option<String>,
    pub notebook_path: String,
    pub original_file: String,
    pub updated_file: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct SleepOutput {
    pub duration_ms: u64,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct BriefOutput {
    pub message: String,
    pub attachments: Option<Vec<ResolvedAttachment>>,
    #[serde(rename = "sentAt")]
    pub sent_at: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct ResolvedAttachment {
    pub path: String,
    pub size: u64,
    #[serde(rename = "isImage")]
    pub is_image: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct ConfigOutput {
    pub success: bool,
    pub operation: Option<String>,
    pub setting: Option<String>,
    pub value: Option<Value>,
    #[serde(rename = "previousValue")]
    pub previous_value: Option<Value>,
    #[serde(rename = "newValue")]
    pub new_value: Option<Value>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PlanModeState {
    #[serde(rename = "hadLocalOverride")]
    pub had_local_override: bool,
    #[serde(rename = "previousLocalMode")]
    pub previous_local_mode: Option<Value>,
}

#[derive(Debug, Serialize)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct PlanModeOutput {
    pub success: bool,
    pub operation: String,
    pub changed: bool,
    pub active: bool,
    pub managed: bool,
    pub message: String,
    #[serde(rename = "settingsPath")]
    pub settings_path: String,
    #[serde(rename = "statePath")]
    pub state_path: String,
    #[serde(rename = "previousLocalMode")]
    pub previous_local_mode: Option<Value>,
    #[serde(rename = "currentLocalMode")]
    pub current_local_mode: Option<Value>,
}

#[derive(Debug, Clone)]
pub(crate) struct SearchableToolSpec {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct StructuredOutputResult {
    pub data: String,
    pub structured_output: BTreeMap<String, Value>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ReplOutput {
    pub language: String,
    pub stdout: String,
    pub stderr: String,
    #[serde(rename = "exitCode")]
    pub exit_code: i32,
    #[serde(rename = "durationMs")]
    pub duration_ms: u128,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub(crate) enum WebSearchResultItem {
    SearchResult {
        tool_use_id: String,
        content: Vec<SearchHit>,
    },
    Commentary(String),
}

#[derive(Debug, Serialize)]
pub(crate) struct SearchHit {
    pub title: String,
    pub url: String,
}
