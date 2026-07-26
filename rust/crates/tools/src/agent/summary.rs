use std::process::Command;
use serde::Serialize;
use runtime::{LaneCommitProvenance, LaneEventBlocker, LaneFailureClass};
use runtime::summary_compression::compress_summary_text;
use crate::tool_types::*;
use crate::global_cron_registry;

#[allow(dead_code)]
pub(crate) const MIN_LANE_SUMMARY_WORDS: usize = 7;
#[allow(dead_code)]
pub(crate) const REVIEW_VERDICTS: &[(&str, &str)] = &[
    ("APPROVE", "approve"),
    ("REJECT", "reject"),
    ("BLOCKED", "blocked"),
];
#[allow(dead_code)]
pub(crate) const CONTROL_ONLY_SUMMARY_WORDS: &[&str] = &[
    "ack",
    "commit",
    "continue",
    "everyting",
    "everything",
    "keep",
    "next",
    "push",
    "ralph",
    "resume",
    "retry",
    "run",
    "stop",
    "sweep",
    "sweeping",
    "team",
];
#[allow(dead_code)]
pub(crate) const CONTEXTUAL_SUMMARY_WORDS: &[&str] = &[
    "added",
    "audited",
    "blocked",
    "completed",
    "documented",
    "failed",
    "finished",
    "fixed",
    "implemented",
    "investigated",
    "merged",
    "pushed",
    "refactored",
    "removed",
    "reviewed",
    "tested",
    "updated",
    "verified",
];

#[derive(Debug, Clone, Serialize)]
pub(crate) struct LaneFinishedSummaryData {
    #[serde(rename = "qualityFloorApplied")]
    pub(crate) quality_floor_applied: bool,
    pub(crate) reasons: Vec<String>,
    #[serde(rename = "rawSummary", skip_serializing_if = "Option::is_none")]
    pub(crate) raw_summary: Option<String>,
    #[serde(rename = "wordCount")]
    pub(crate) word_count: usize,
    #[serde(rename = "reviewVerdict", skip_serializing_if = "Option::is_none")]
    pub(crate) review_verdict: Option<String>,
    #[serde(rename = "reviewTarget", skip_serializing_if = "Option::is_none")]
    pub(crate) review_target: Option<String>,
    #[serde(rename = "reviewRationale", skip_serializing_if = "Option::is_none")]
    pub(crate) review_rationale: Option<String>,
    #[serde(rename = "selectionOutcome", skip_serializing_if = "Option::is_none")]
    pub(crate) selection_outcome: Option<SelectionOutcome>,
    #[serde(rename = "recoveryOutcome", skip_serializing_if = "Option::is_none")]
    pub(crate) recovery_outcome: Option<RecoveryOutcome>,
    #[serde(rename = "artifactProvenance", skip_serializing_if = "Option::is_none")]
    pub(crate) artifact_provenance: Option<ArtifactProvenance>,
    #[serde(rename = "disabledCronIds", skip_serializing_if = "Vec::is_empty")]
    pub(crate) disabled_cron_ids: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct LaneFinishedSummary {
    pub(crate) detail: Option<String>,
    pub(crate) data: LaneFinishedSummaryData,
}

#[derive(Debug)]
pub(crate) struct LaneSummaryAssessment {
    pub(crate) apply_quality_floor: bool,
    pub(crate) reasons: Vec<String>,
    pub(crate) word_count: usize,
    pub(crate) review_outcome: Option<ReviewLaneOutcome>,
    recovery_outcome: Option<RecoveryOutcome>,
}

#[derive(Debug, Clone)]
pub(crate) struct ReviewLaneOutcome {
    verdict: String,
    rationale: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SelectionOutcome {
    #[serde(rename = "chosenItems", skip_serializing_if = "Vec::is_empty")]
    chosen_items: Vec<String>,
    #[serde(rename = "skippedItems", skip_serializing_if = "Vec::is_empty")]
    skipped_items: Vec<String>,
    action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    rationale: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct RecoveryOutcome {
    pub(crate) cause: String,
    #[serde(rename = "targetLane", skip_serializing_if = "Option::is_none")]
    pub(crate) target_lane: Option<String>,
    #[serde(rename = "preservedState", skip_serializing_if = "Option::is_none")]
    pub(crate) preserved_state: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ArtifactProvenance {
    #[serde(rename = "sourceLanes", skip_serializing_if = "Vec::is_empty")]
    source_lanes: Vec<String>,
    #[serde(rename = "roadmapIds", skip_serializing_if = "Vec::is_empty")]
    roadmap_ids: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    files: Vec<String>,
    #[serde(rename = "diffStat", skip_serializing_if = "Option::is_none")]
    diff_stat: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    verification: Vec<String>,
    #[serde(rename = "commitSha", skip_serializing_if = "Option::is_none")]
    commit_sha: Option<String>,
}

pub(crate) fn build_lane_finished_summary(
    manifest: &AgentOutput,
    result: Option<&str>,
) -> LaneFinishedSummary {
    let raw_summary = result.map(str::trim).filter(|value| !value.is_empty());
    let assessment = assess_lane_summary_quality(raw_summary.unwrap_or_default());
    let detail = match raw_summary {
        Some(summary) if !assessment.apply_quality_floor => Some(compress_summary_text(summary)),
        Some(summary) => Some(compose_lane_summary_fallback(
            manifest,
            Some(summary),
            assessment.recovery_outcome.as_ref(),
        )),
        None => Some(compose_lane_summary_fallback(manifest, None, None)),
    };
    let review_outcome = assessment.review_outcome.clone();
    let recovery_outcome = assessment.recovery_outcome.clone();
    let review_target = review_outcome
        .as_ref()
        .map(|_| manifest.description.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let artifact_provenance = extract_artifact_provenance(manifest, raw_summary);

    LaneFinishedSummary {
        detail,
        data: LaneFinishedSummaryData {
            quality_floor_applied: raw_summary.is_none() || assessment.apply_quality_floor,
            reasons: assessment.reasons,
            raw_summary: raw_summary.map(str::to_string),
            word_count: assessment.word_count,
            review_verdict: review_outcome
                .as_ref()
                .map(|outcome| outcome.verdict.clone()),
            review_target,
            review_rationale: review_outcome.and_then(|outcome| outcome.rationale),
            selection_outcome: extract_selection_outcome(raw_summary.unwrap_or_default()),
            recovery_outcome,
            artifact_provenance,
            disabled_cron_ids: Vec::new(),
        },
    }
}

pub(crate) fn assess_lane_summary_quality(summary: &str) -> LaneSummaryAssessment {
    let words = summary
        .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '-' || ch == '#'))
        .filter(|token| !token.is_empty())
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>();

    let word_count = words.len();
    let mut reasons = Vec::new();
    if summary.trim().is_empty() {
        reasons.push(String::from("empty"));
    }

    let review_outcome = extract_review_outcome(summary);
    let recovery_outcome = extract_recovery_outcome(summary);
    if recovery_outcome.is_some() {
        reasons.push(String::from("recovery_control_prose"));
    }

    let control_only = !words.is_empty()
        && words
            .iter()
            .all(|word| CONTROL_ONLY_SUMMARY_WORDS.contains(&word.as_str()));
    if control_only && review_outcome.is_none() {
        reasons.push(String::from("control_only"));
    }

    let has_context_signal = summary.contains('`')
        || summary.contains('/')
        || summary.contains(':')
        || summary.contains('#')
        || review_outcome.is_some()
        || words
            .iter()
            .any(|word| CONTEXTUAL_SUMMARY_WORDS.contains(&word.as_str()));
    if word_count < MIN_LANE_SUMMARY_WORDS && !has_context_signal {
        reasons.push(String::from("too_short_without_context"));
    }

    LaneSummaryAssessment {
        apply_quality_floor: !reasons.is_empty(),
        reasons,
        word_count,
        review_outcome,
        recovery_outcome,
    }
}

pub(crate) fn compose_lane_summary_fallback(
    manifest: &AgentOutput,
    raw_summary: Option<&str>,
    recovery_outcome: Option<&RecoveryOutcome>,
) -> String {
    let target = manifest.description.trim();
    let base = format!(
        "Completed lane `{}` for target: {}. Status: completed.",
        manifest.name,
        if target.is_empty() {
            "unspecified task"
        } else {
            target
        }
    );
    if let Some(outcome) = recovery_outcome {
        let mut detail = format!(
            "{base} Recovery handoff observed via tmux reinjection (cause: `{}`).",
            outcome.cause
        );
        if let Some(target_lane) = &outcome.target_lane {
            let _ = std::fmt::Write::write_fmt(
                &mut detail,
                format_args!(" Target lane: `{target_lane}`."),
            );
        }
        if let Some(preserved_state) = &outcome.preserved_state {
            let _ = std::fmt::Write::write_fmt(
                &mut detail,
                format_args!(" Preserved state: {preserved_state}."),
            );
        }
        return detail;
    }
    match raw_summary {
        Some(summary) => format!(
            "{base} Original stop summary was too vague to keep as the lane result: \"{}\".",
            summary.trim()
        ),
        None => format!("{base} No usable stop summary was produced by the lane."),
    }
}

pub(crate) fn extract_review_outcome(summary: &str) -> Option<ReviewLaneOutcome> {
    let mut lines = summary
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty());
    let first = lines.next()?;
    let verdict = REVIEW_VERDICTS.iter().find_map(|(prefix, verdict)| {
        first
            .eq_ignore_ascii_case(prefix)
            .then(|| (*verdict).to_string())
    })?;
    let rationale = lines.collect::<Vec<_>>().join(" ").trim().to_string();
    Some(ReviewLaneOutcome {
        verdict,
        rationale: (!rationale.is_empty()).then_some(compress_summary_text(&rationale)),
    })
}

pub(crate) fn extract_selection_outcome(summary: &str) -> Option<SelectionOutcome> {
    let mut chosen_items = Vec::new();
    let mut skipped_items = Vec::new();
    let mut action = None;
    let mut rationale = None;

    for line in summary
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let lowered = line.to_ascii_lowercase();
        let roadmap_items = extract_roadmap_items(line);

        if lowered.starts_with("chosen:")
            || lowered.starts_with("picked:")
            || lowered.starts_with("selected:")
            || (lowered.contains("picked") && !roadmap_items.is_empty())
            || (lowered.contains("selected") && !roadmap_items.is_empty())
        {
            chosen_items.extend(roadmap_items);
        } else if lowered.starts_with("skipped:")
            || lowered.starts_with("skip:")
            || (lowered.contains("skipped") && !roadmap_items.is_empty())
        {
            skipped_items.extend(roadmap_items);
        }

        if let Some(rest) = lowered.strip_prefix("action:") {
            if rest.contains("execute") || rest.contains("implement") || rest.contains("fix") {
                action = Some(String::from("execute"));
            } else if rest.contains("review") || rest.contains("audit") {
                action = Some(String::from("review"));
            } else if rest.contains("no-op") || rest.contains("noop") {
                action = Some(String::from("no-op"));
            }
        }

        if let Some(rest) = line.strip_prefix("Rationale:") {
            let trimmed = rest.trim();
            if !trimmed.is_empty() {
                rationale = Some(compress_summary_text(trimmed));
            }
        }
    }

    chosen_items.sort();
    chosen_items.dedup();
    skipped_items.sort();
    skipped_items.dedup();

    if chosen_items.is_empty() && skipped_items.is_empty() && action.is_none() {
        return None;
    }

    let default_action = if chosen_items.is_empty() {
        String::from("no-op")
    } else {
        String::from("execute")
    };

    Some(SelectionOutcome {
        chosen_items,
        skipped_items,
        action: action.unwrap_or(default_action),
        rationale,
    })
}

pub(crate) fn extract_recovery_outcome(summary: &str) -> Option<RecoveryOutcome> {
    let trimmed = summary.trim();
    if trimmed.is_empty() {
        return None;
    }

    let lowered = trimmed.to_ascii_lowercase();
    let has_tmux_inject_marker = lowered.contains("omx_tmux_inject");
    let has_recovery_phrase = lowered.contains("continue from current mode state")
        || (lowered.starts_with("team ") && lowered.contains(" next:"));
    if !has_tmux_inject_marker && !has_recovery_phrase {
        return None;
    }

    let cause = if lowered.contains("current mode state") {
        "resume_after_stop"
    } else if lowered.contains("tool failure") {
        "retry_after_tool_failure"
    } else if lowered.contains("worker panes stalled")
        || lowered.contains("no progress")
        || lowered.contains("leader stale")
        || lowered.contains("all workers idle")
        || lowered.contains("all 1 worker idle")
        || lowered.contains("pane(s) active")
    {
        "tmux_reinject_after_idle"
    } else {
        "manual_recovery"
    };

    let target_lane = trimmed.lines().map(str::trim).find_map(|line| {
        let lower = line.to_ascii_lowercase();
        if !lower.starts_with("team ") {
            return None;
        }
        line[5..]
            .split_once(':')
            .map(|(name, _)| name.trim())
            .filter(|name| !name.is_empty())
            .map(str::to_string)
    });

    let preserved_state = lowered
        .contains("current mode state")
        .then(|| String::from("current mode state"));

    Some(RecoveryOutcome {
        cause: cause.to_string(),
        target_lane,
        preserved_state,
    })
}

pub(crate) fn extract_roadmap_items(line: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '#' {
            let mut digits = String::new();
            while let Some(next) = chars.peek() {
                if next.is_ascii_digit() {
                    digits.push(*next);
                    chars.next();
                } else {
                    break;
                }
            }
            if !digits.is_empty() {
                items.push(format!("ROADMAP #{digits}"));
            }
        }
    }
    items
}

pub(crate) fn extract_artifact_provenance(
    manifest: &AgentOutput,
    raw_summary: Option<&str>,
) -> Option<ArtifactProvenance> {
    let summary = raw_summary?;
    let mut roadmap_ids = extract_roadmap_items(summary);
    roadmap_ids.extend(extract_roadmap_items(&manifest.description));
    roadmap_ids.sort();
    roadmap_ids.dedup();

    let mut files = extract_file_paths(summary);
    files.sort();
    files.dedup();

    let mut verification = Vec::new();
    let lowered = summary.to_ascii_lowercase();
    for (needle, label) in [
        ("tested", "tested"),
        ("committed", "committed"),
        ("pushed", "pushed"),
        ("merged", "merged"),
    ] {
        if lowered.contains(needle) {
            verification.push(label.to_string());
        }
    }

    let commit_sha = extract_commit_sha(summary);
    let diff_stat = extract_diff_stat(summary);
    let source_lanes = vec![manifest.name.clone()];

    if roadmap_ids.is_empty()
        && files.is_empty()
        && verification.is_empty()
        && commit_sha.is_none()
        && diff_stat.is_none()
    {
        return None;
    }

    Some(ArtifactProvenance {
        source_lanes,
        roadmap_ids,
        files,
        diff_stat,
        verification,
        commit_sha,
    })
}

pub(crate) fn extract_file_paths(summary: &str) -> Vec<String> {
    summary
        .split(|ch: char| ch.is_whitespace() || matches!(ch, ',' | ';' | '(' | ')' | '[' | ']'))
        .map(|token| {
            token
                .trim_matches('`')
                .trim_matches('"')
                .trim_matches('\'')
                .trim_end_matches('.')
        })
        .filter(|token| {
            token.contains('.')
                && !token.starts_with("http")
                && !token
                    .chars()
                    .all(|ch| ch.is_ascii_digit() || ch == '.' || ch == '+' || ch == '-')
        })
        .map(str::to_string)
        .collect()
}

pub(crate) fn extract_diff_stat(summary: &str) -> Option<String> {
    summary
        .split('\n')
        .map(str::trim)
        .find_map(|line| {
            line.find("Diff stat:")
                .map(|index| normalize_diff_stat(&line[(index + "Diff stat:".len())..]))
                .or_else(|| {
                    line.find("Diff:")
                        .map(|index| normalize_diff_stat(&line[(index + "Diff:".len())..]))
                })
        })
        .filter(|value| !value.is_empty())
}

pub(crate) fn normalize_diff_stat(value: &str) -> String {
    let trimmed = value.trim();
    for marker in [" Tested", " Committed", " committed", " pushed", " merged"] {
        if let Some((prefix, _)) = trimmed.split_once(marker) {
            return prefix.trim().to_string();
        }
    }
    trimmed.to_string()
}

pub(crate) fn disable_matching_crons(manifest: &AgentOutput, result: Option<&str>) -> Vec<String> {
    let tokens = cron_match_tokens(manifest, result);
    if tokens.is_empty() {
        return Vec::new();
    }

    let mut disabled = Vec::new();
    for entry in global_cron_registry().list(true) {
        let haystack = format!(
            "{} {}",
            entry.prompt,
            entry.description.as_deref().unwrap_or_default()
        )
        .to_ascii_lowercase();
        if tokens.iter().any(|token| haystack.contains(token))
            && global_cron_registry().disable(&entry.cron_id).is_ok()
        {
            disabled.push(entry.cron_id);
        }
    }
    disabled.sort();
    disabled
}

pub(crate) fn cron_match_tokens(manifest: &AgentOutput, result: Option<&str>) -> Vec<String> {
    let mut tokens = extract_roadmap_items(manifest.description.as_str())
        .into_iter()
        .chain(extract_roadmap_items(result.unwrap_or_default()))
        .map(|item| item.to_ascii_lowercase())
        .collect::<Vec<_>>();

    if tokens.is_empty() && !manifest.name.trim().is_empty() {
        tokens.push(manifest.name.trim().to_ascii_lowercase());
    }

    tokens.sort();
    tokens.dedup();
    tokens
}

pub(crate) fn derive_agent_state(
    status: &str,
    result: Option<&str>,
    error: Option<&str>,
    blocker: Option<&LaneEventBlocker>,
) -> &'static str {
    let normalized_status = status.trim().to_ascii_lowercase();
    let normalized_error = error.unwrap_or_default().to_ascii_lowercase();

    if normalized_status == "running" {
        return "working";
    }
    if normalized_status == "completed" {
        return if result.is_some_and(|value| !value.trim().is_empty()) {
            "finished_cleanable"
        } else {
            "finished_pending_report"
        };
    }
    if normalized_error.contains("background") {
        return "blocked_background_job";
    }
    if normalized_error.contains("merge conflict") || normalized_error.contains("cherry-pick") {
        return "blocked_merge_conflict";
    }
    if normalized_error.contains("mcp") {
        return "degraded_mcp";
    }
    if normalized_error.contains("transport")
        || normalized_error.contains("broken pipe")
        || normalized_error.contains("connection")
        || normalized_error.contains("interrupted")
    {
        return "interrupted_transport";
    }
    if blocker.is_some() {
        return "truly_idle";
    }
    "truly_idle"
}

pub(crate) fn maybe_commit_provenance(result: Option<&str>) -> Option<LaneCommitProvenance> {
    let commit = extract_commit_sha(result?)?;
    let branch = current_git_branch().unwrap_or_else(|| "unknown".to_string());
    let worktree = std::env::current_dir()
        .ok()
        .map(|path| path.display().to_string());
    Some(LaneCommitProvenance {
        commit: commit.clone(),
        branch,
        worktree,
        canonical_commit: Some(commit.clone()),
        superseded_by: None,
        lineage: vec![commit],
    })
}

pub(crate) fn extract_commit_sha(result: &str) -> Option<String> {
    result
        .split(|c: char| !c.is_ascii_hexdigit())
        .find(|token| token.len() >= 7 && token.len() <= 40)
        .map(str::to_string)
}

pub(crate) fn current_git_branch() -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub(crate) fn append_agent_output(path: &str, suffix: &str) -> Result<(), String> {
    use std::io::Write as _;

    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(path)
        .map_err(|error| error.to_string())?;
    file.write_all(suffix.as_bytes())
        .map_err(|error| error.to_string())
}

pub(crate) fn format_agent_terminal_output(
    status: &str,
    result: Option<&str>,
    blocker: Option<&LaneEventBlocker>,
    error: Option<&str>,
) -> String {
    let mut sections = vec![format!("\n## Result\n\n- status: {status}\n")];
    if let Some(blocker) = blocker {
        sections.push(format!(
            "\n### Blocker\n\n- failure_class: {}\n- detail: {}\n",
            serde_json::to_string(&blocker.failure_class)
                .unwrap_or_else(|_| "\"infra\"".to_string())
                .trim_matches('"'),
            blocker.detail.trim()
        ));
    }
    if let Some(result) = result.filter(|value| !value.trim().is_empty()) {
        sections.push(format!("\n### Final response\n\n{}\n", result.trim()));
    }
    if let Some(error) = error.filter(|value| !value.trim().is_empty()) {
        sections.push(format!("\n### Error\n\n{}\n", error.trim()));
    }
    sections.join("")
}

pub(crate) fn classify_lane_blocker(error: &str) -> LaneEventBlocker {
    let detail = error.trim().to_string();
    LaneEventBlocker {
        failure_class: classify_lane_failure(error),
        detail,
        subphase: None,
    }
}

pub(crate) fn classify_lane_failure(error: &str) -> LaneFailureClass {
    let normalized = error.to_ascii_lowercase();

    if normalized.contains("prompt") && normalized.contains("deliver") {
        LaneFailureClass::PromptDelivery
    } else if normalized.contains("trust") {
        LaneFailureClass::TrustGate
    } else if normalized.contains("branch")
        && (normalized.contains("stale") || normalized.contains("diverg"))
    {
        LaneFailureClass::BranchDivergence
    } else if normalized.contains("gateway") || normalized.contains("routing") {
        LaneFailureClass::GatewayRouting
    } else if normalized.contains("compile")
        || normalized.contains("build failed")
        || normalized.contains("cargo check")
    {
        LaneFailureClass::Compile
    } else if normalized.contains("test") {
        LaneFailureClass::Test
    } else if normalized.contains("tool failed")
        || normalized.contains("runtime tool")
        || normalized.contains("tool runtime")
    {
        LaneFailureClass::ToolRuntime
    } else if normalized.contains("workspace") && normalized.contains("mismatch") {
        LaneFailureClass::WorkspaceMismatch
    } else if normalized.contains("plugin") {
        LaneFailureClass::PluginStartup
    } else if normalized.contains("mcp") && normalized.contains("handshake") {
        LaneFailureClass::McpHandshake
    } else if normalized.contains("mcp") {
        LaneFailureClass::McpStartup
    } else {
        LaneFailureClass::Infra
    }
}

