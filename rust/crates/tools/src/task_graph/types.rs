use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskGraphOperation {
    Add,
    UpdateStatus,
    View,
    Remove,
}

#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
}

fn deserialize_string_or_number<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(serde::Deserialize)]
    #[serde(untagged)]
    enum StringOrNumber {
        String(String),
        Number(serde_json::Number),
    }

    match StringOrNumber::deserialize(deserializer)? {
        StringOrNumber::String(s) => Ok(s),
        StringOrNumber::Number(n) => Ok(n.to_string()),
    }
}

fn deserialize_optional_string_or_number<'de, D>(
    deserializer: D,
) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(serde::Deserialize)]
    #[serde(untagged)]
    enum StringOrNumber {
        String(String),
        Number(serde_json::Number),
    }

    let opt = Option::<StringOrNumber>::deserialize(deserializer)?;
    match opt {
        Some(StringOrNumber::String(s)) => Ok(Some(s)),
        Some(StringOrNumber::Number(n)) => Ok(Some(n.to_string())),
        None => Ok(None),
    }
}

fn generate_id() -> String {
    format!(
        "auto_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    )
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
pub struct TaskNode {
    #[serde(
        default = "generate_id",
        deserialize_with = "deserialize_string_or_number"
    )]
    pub id: String,

    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_string_or_number"
    )]
    pub parent_id: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<TaskStatus>,
}

impl TaskNode {
    #[must_use]
    pub fn depth(&self) -> usize {
        id_depth(&self.id)
    }

    #[must_use]
    pub fn is_descendant_id(&self, ancestor_id: &str) -> bool {
        is_descendant_id(&self.id, ancestor_id)
    }
}

/// Hierarchy depth of a dotted task id ("1" -> 1, "1.2.3" -> 3).
#[must_use]
pub fn id_depth(id: &str) -> usize {
    id.split('.').count()
}

/// Numeric segment key used for ordering task ids semantically ("1.9" < "1.10").
#[must_use]
pub fn id_segments(id: &str) -> Vec<u32> {
    id.split('.')
        .map(|s| s.trim().parse::<u32>().unwrap_or(u32::MAX))
        .collect()
}

/// Compare two dotted task ids by hierarchical position, then lexicographically.
#[must_use]
pub fn compare_task_ids(a: &str, b: &str) -> Ordering {
    id_segments(a).cmp(&id_segments(b)).then_with(|| a.cmp(b))
}

/// True when `id` equals `ancestor_id` or sits below it in the dotted hierarchy.
///
/// Deliberately segment-based: a naive `id.starts_with("1.")` also matches
/// "10.x", which wrongly counted phase-10 tasks as descendants of phase 1.
#[must_use]
pub fn is_descendant_id(id: &str, ancestor_id: &str) -> bool {
    if id == ancestor_id {
        return true;
    }
    let id_segs = id_segments(id);
    let anc_segs = id_segments(ancestor_id);
    if id_segs.len() <= anc_segs.len() {
        return false;
    }
    id_segs[..anc_segs.len()] == anc_segs[..]
}

/// Parent id implied by the dotted id structure ("1.2.3" -> "1.2").
#[must_use]
pub fn parent_id_from_task_id(id: &str) -> Option<String> {
    let parts: Vec<&str> = id.split('.').collect();
    if parts.len() > 1 {
        Some(parts[..parts.len() - 1].join("."))
    } else {
        None
    }
}

#[derive(Debug, Deserialize)]
pub struct TaskGraphInput {
    pub operation: TaskGraphOperation,
    #[serde(default)]
    pub nodes: Vec<TaskNode>,
}

#[derive(Debug, Serialize, Clone)]
pub struct TaskGraphOutput {
    pub nodes_updated: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_leaf_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub active_recursion_chain: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_branch_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grisha_review: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alert: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<String>,
}
