//! Transactional working memory for the current turn (M4).
//!
//! Every successful `write_file` / `edit_file` checkpoints the file's prior
//! content here — up to [`MAX_SNAPSHOT_BYTES`] per file and
//! [`MAX_TRACKED_FILES_PER_TURN`] files per turn. The bifurcation engine
//! renders [`modified_files_summary`] into its directive once the agent is
//! past the first period-doubling, so a model that is looping gets an exact
//! list of what it has already changed this turn instead of having to
//! reconstruct it from pruned tool results.
//!
//! The registry is process-global because the tools runners (which know the
//! before/after content) and the conversation loop (which owns the engine)
//! live in different layers; `begin_turn` at the top of each turn keeps it
//! turn-scoped.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::OnceLock;

/// A file is snapshotted only when its prior content fits this budget —
/// vendored/generated blobs are tracked by name and line counts, not bytes.
pub const MAX_SNAPSHOT_BYTES: usize = 256 * 1024;

/// Hard ceiling on distinct files checkpointed within one turn.
pub const MAX_TRACKED_FILES_PER_TURN: usize = 16;

/// How many files the directive-level summary enumerates (the rest are
/// counted in the trailing "… and N more" line).
const MAX_SUMMARIZED_FILES: usize = 8;

#[derive(Debug, Clone)]
struct FileMemory {
    path: String,
    created_this_turn: bool,
    initial_lines: usize,
    current_lines: usize,
    mutations: usize,
    #[allow(dead_code)] // kept for future per-file rollback; bounds memory now
    snapshot: Option<String>,
    snapshot_too_large: bool,
}

fn registry() -> &'static Mutex<HashMap<String, FileMemory>> {
    static REGISTRY: OnceLock<Mutex<HashMap<String, FileMemory>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Clears the working set. Called at the top of every turn.
pub fn begin_turn() {
    if let Ok(mut map) = registry().lock() {
        map.clear();
    }
}

/// Records that `path` was written or edited with `before`/`after` content.
/// `before` is `None` when the file did not exist prior to this turn.
pub fn record_file_mutation(path: &str, before: Option<&str>, after: &str) {
    let key = path.to_string();
    let Ok(mut map) = registry().lock() else {
        return;
    };
    if let Some(entry) = map.get_mut(&key) {
        entry.mutations += 1;
        entry.current_lines = count_lines(after);
        return;
    }
    if map.len() >= MAX_TRACKED_FILES_PER_TURN {
        return;
    }
    let created = before.is_none();
    let snapshot = match before {
        Some(content) if content.len() <= MAX_SNAPSHOT_BYTES => Some(content.to_string()),
        _ => None,
    };
    map.insert(
        key,
        FileMemory {
            path: path.to_string(),
            created_this_turn: created,
            initial_lines: if created {
                0
            } else {
                count_lines(before.unwrap_or(""))
            },
            current_lines: count_lines(after),
            mutations: 1,
            snapshot,
            snapshot_too_large: before.is_some() && before.unwrap_or("").len() > MAX_SNAPSHOT_BYTES,
        },
    );
}

/// One-line, stable-ordered summary of everything this turn has touched.
/// `None` when the turn has mutated no files.
#[must_use]
pub fn modified_files_summary() -> Option<String> {
    let guard = registry().lock().ok()?;
    let mut entries: Vec<&FileMemory> = guard.values().collect();
    if entries.is_empty() {
        return None;
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    let shown: Vec<String> = entries
        .iter()
        .take(MAX_SUMMARIZED_FILES)
        .map(|entry| {
            if entry.created_this_turn {
                format!("{} (created, {} lines)", entry.path, entry.current_lines)
            } else {
                let size_note = if entry.snapshot_too_large {
                    ", not snapshotted"
                } else {
                    ""
                };
                let edits = if entry.mutations == 1 {
                    "edit"
                } else {
                    "edits"
                };
                format!(
                    "{} ({}→{} lines, {} {}{})",
                    entry.path,
                    entry.initial_lines,
                    entry.current_lines,
                    entry.mutations,
                    edits,
                    size_note
                )
            }
        })
        .collect();
    let mut line = shown.join(" · ");
    if let Some(rest) = entries.len().checked_sub(MAX_SUMMARIZED_FILES) {
        line.push_str(&format!(" · … and {rest} more"));
    }
    Some(line)
}

fn count_lines(content: &str) -> usize {
    if content.is_empty() {
        0
    } else {
        content.lines().count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_creation_and_edits_within_a_turn() {
        let _guard = crate::test_env_lock();
        begin_turn();
        record_file_mutation(
            "/tmp/b.toml",
            Some("[package]\n"),
            "[package]\nname = \"b\"\n",
        );
        record_file_mutation("/tmp/a.rs", None, "fn main() {}\n");
        record_file_mutation(
            "/tmp/a.rs",
            Some("fn main() {}\n"),
            "fn main() {\n    println!(\"x\");\n}\n",
        );
        let summary = modified_files_summary().expect("working set is non-empty");
        // A file created this turn stays labelled `created`, however often it is
        // rewritten afterwards — that is the distinction the model needs.
        assert!(
            summary.contains("/tmp/a.rs (created, 3 lines)"),
            "{summary}"
        );
        assert!(
            summary.contains("/tmp/b.toml (1→2 lines, 1 edit)"),
            "{summary}"
        );
        begin_turn();
    }

    #[test]
    fn created_files_are_labelled() {
        let _guard = crate::test_env_lock();
        begin_turn();
        record_file_mutation("/tmp/new.rs", None, "line1\nline2\n");
        let summary = modified_files_summary().expect("one file tracked");
        assert!(
            summary.contains("/tmp/new.rs (created, 2 lines)"),
            "{summary}"
        );
    }

    #[test]
    fn oversized_prior_content_is_counted_but_not_snapshotted() {
        let _guard = crate::test_env_lock();
        begin_turn();
        let huge = "x".repeat(MAX_SNAPSHOT_BYTES + 1);
        record_file_mutation("/tmp/huge.rs", Some(&huge), "small\n");
        let summary = modified_files_summary().expect("tracked");
        assert!(summary.contains("not snapshotted"), "{summary}");
    }

    #[test]
    fn begin_turn_resets_the_working_set() {
        let _guard = crate::test_env_lock();
        begin_turn();
        record_file_mutation("/tmp/x.rs", None, "a\n");
        assert!(modified_files_summary().is_some());
        begin_turn();
        assert!(modified_files_summary().is_none());
    }

    #[test]
    fn caps_tracked_files_per_turn() {
        let _guard = crate::test_env_lock();
        begin_turn();
        for i in 0..(MAX_TRACKED_FILES_PER_TURN + 6) {
            record_file_mutation(&format!("/tmp/cap-{i}.rs"), None, "a\n");
        }
        let summary = modified_files_summary().expect("at cap");
        assert!(summary.contains("more"), "{summary}");
        begin_turn();
    }
}
