use crate::task_graph::store::{find_active_leaf, load_graph_nodes, task_graph_store_path};
use crate::task_graph::types::{TaskNode, TaskStatus};
use crate::task_graph::validation::validate_task_graph;
use serde_json::Value;
use std::collections::HashSet;

pub fn check_task_graph_enforcement(tool_name: &str, input: &Value) -> Result<(), String> {
    validate_active_task_for_tool(tool_name, input)
}

pub fn validate_active_task_for_tool(name: &str, input: &Value) -> Result<(), String> {
    // ── Read-only tools whitelist: these NEVER need TaskGraph validation ──
    // All tools NOT in this list are considered mutating and MUST match an
    // active in_progress task. This covers built-in tools, MCP tools, plugins,
    // and any future dynamic tools by default.
    const READ_ONLY_TOOLS: &[&str] = &[
        "read_file",
        "glob_search",
        "grep_search",
        "list_dir",
        "ToolSearch",
        "McpSearch",
        "Skill",
        "TaskGraph", // TaskGraph itself manages tasks, not work
        "web_search",
        "web_fetch",
        "WebSearch",
        "WebFetch",
        "Sleep",
        "ask_user",
        "AskUser",
        "SendUserMessage",
        "Brief",
        "structured_output",
        "StructuredOutput",
        "notebook_read",
        "NotebookRead",
        "repl", // REPL for exploration
        "retrieve_context",
        "RetrieveContext",
        "git_status",
        "git_log",
        "git_show",
        "git_diff",
        // Worker read-only tools (observation only, no mutations)
        "WorkerObserve",
        "WorkerObserveCompletion",
        "WorkerGet",
        "WorkerAwaitReady",
        "WorkerResolveTrust",
        // NOTE: WorkerCreate, WorkerSendPrompt, WorkerTerminate, WorkerRestart
        // are MUTATING and must pass TaskGraph enforcement.
        // Sequential Thinking MCP — reasoning/planning tool, always allowed
        "mcp__sequential-thinking__sequentialthinking",
        "sequentialthinking",
    ];

    if READ_ONLY_TOOLS
        .iter()
        .any(|&ro| ro.eq_ignore_ascii_case(name))
    {
        return Ok(());
    }

    // Bypass check for read-only bash commands to prevent blocking exploration
    if name == "bash" {
        if let Some(cmd_val) = input.get("command").or_else(|| input.get("Command")) {
            if let Some(cmd) = cmd_val.as_str() {
                // Grisha Supervisor: Detect faux execution / echo simulations
                crate::grisha::GrishaSupervisor::inspect_command(cmd)?;

                let raw_trimmed = cmd.trim().to_lowercase();
                let trimmed = if raw_trimmed.starts_with("cd ") {
                    if let Some(pos) = raw_trimmed.find("&&") {
                        raw_trimmed[pos + 2..].trim().to_string()
                    } else {
                        raw_trimmed
                    }
                } else {
                    raw_trimmed
                };
                let is_read_only = trimmed.starts_with("cat ")
                    || trimmed.starts_with("ls ")
                    || trimmed.starts_with("grep ")
                    || trimmed.starts_with("find ")
                    || trimmed.starts_with("file ")
                    || trimmed.starts_with("stat ")
                    || trimmed.starts_with("head ")
                    || trimmed.starts_with("tail ")
                    || trimmed.starts_with("wc ")
                    || trimmed.starts_with("echo ")
                    || trimmed.starts_with("sleep ")
                    || trimmed.starts_with("which ")
                    || trimmed.starts_with("type ")
                    || trimmed.starts_with("pwd")
                    || trimmed.starts_with("env ")
                    || trimmed.starts_with("printenv")
                    || trimmed.starts_with("date")
                    || trimmed.starts_with("whoami")
                    || trimmed.starts_with("id ")
                    || trimmed.starts_with("df ")
                    || trimmed.starts_with("du ")
                    || trimmed.starts_with("free ")
                    || trimmed.starts_with("uname ")
                    || trimmed.starts_with("uptime")
                    || trimmed.starts_with("ps ")
                    || trimmed.starts_with("top ")
                    || trimmed.starts_with("htop")
                    || trimmed.starts_with("cargo check")
                    || trimmed.starts_with("cargo test")
                    || trimmed.starts_with("cargo clippy")
                    // NOTE: cargo build is MUTATING (creates target/) — NOT whitelisted
                    || trimmed.starts_with("git status")
                    || trimmed.starts_with("git log")
                    || trimmed.starts_with("git diff")
                    || trimmed.starts_with("git show")
                    || trimmed.starts_with("git branch")
                    || trimmed.starts_with("systemctl status")
                    || trimmed.starts_with("docker inspect")
                    || trimmed.starts_with("kubectl get")
                    || trimmed.starts_with("kubectl describe")
                    // Only allow --help/--version if command is simple (at most 2 words + flag)
                    // e.g. "cargo --help" OK, "cargo build --help" OK, "rm -rf /tmp --help" BLOCKED
                    || {
                        let words: Vec<&str> = trimmed.split_whitespace().collect();
                        let last = words.last().copied().unwrap_or("");
                        let is_help_flag = last == "--help" || last == "--version" || last == "-h";
                        is_help_flag && words.len() <= 3 && !words.iter().any(|w| {
                            w.starts_with('/') || w.contains("rm") || w.contains("dd") || w.contains("mv") || w.contains("kill")
                        })
                    }
                    || (trimmed.starts_with("ssh ") && (
                        trimmed.contains("cat ")
                        || trimmed.contains("ls ")
                        || trimmed.contains("grep ")
                        || trimmed.contains("find ")
                        || trimmed.contains("head ")
                        || trimmed.contains("tail ")
                        || trimmed.contains("status")
                        || trimmed.contains("overview")
                        || trimmed.contains("--help")
                        || trimmed.contains("df ")
                        || trimmed.contains("du ")
                        || trimmed.contains("free ")
                        || trimmed.contains("docker ps")
                        || trimmed.contains("docker images")
                        || trimmed.contains("docker system df")
                        || trimmed.contains("docker volume ls")
                    ));
                if is_read_only {
                    return Ok(());
                }
            }
        }
    }

    // Resolve the store path. If CLAWD_TASK_GRAPH_STORE is not set, check
    // if the workspace default path exists. If neither is available, TaskGraph
    // enforcement is not active — allow the action.
    let explicit_store = std::env::var("CLAWD_TASK_GRAPH_STORE").ok();
    let store_path = match task_graph_store_path() {
        Ok(p) => p,
        Err(_) => return Ok(()),
    };

    let graph_present = store_path.exists()
        || store_path
            .parent()
            .map(|parent| parent.join("task.md").exists())
            .unwrap_or(false);
    if !graph_present {
        if explicit_store.is_some() {
            // CLAWD_TASK_GRAPH_STORE is explicitly set but no graph file exists yet
            // → enforce Plan-First: agent MUST create a plan before any mutating action
            return Err(
                "Error: Plan-First Enforcement.\n\
                 No TaskGraph has been created yet. Before executing any mutating action, you MUST:\n\
                 1. Analyze the user's request and break it into logical phases and steps.\n\
                 2. Call the `TaskGraph` tool with `operation: \"add\"` to create a structured plan.\n\
                 3. Set the first task to `in_progress` using `operation: \"update_status\"`.\n\
                 4. Only THEN execute your first action.\n\n\
                 You may also create an `implementation_plan.md` artifact for complex tasks before building the TaskGraph.".to_string()
            );
        }
        // No explicit store configured and default file doesn't exist
        // → TaskGraph feature is not active, allow the action
        return Ok(());
    }

    let nodes = load_graph_nodes(&store_path);

    // Ensure the task graph itself is consistent and valid
    validate_task_graph(&nodes).map_err(|err| {
        format!("Error: TaskGraph is in an inconsistent state. Please resolve task graph validation errors using the TaskGraph tool before executing this action. Validation error: {}", err)
    })?;

    // If all tasks in the graph are Completed or Failed, strict enforcement requires adding a new task before executing mutating actions
    let all_completed = !nodes.is_empty()
        && nodes.iter().all(|node| {
            node.status == Some(TaskStatus::Completed) || node.status == Some(TaskStatus::Failed)
        });
    if all_completed {
        return Err(
            "Error: Strict TaskGraph Enforcement. All existing tasks in your task.md are marked as 'completed' (or 'failed').\n\
             You CANNOT execute mutating actions when no task is 'in_progress'.\n\
             If you have additional work or actions to perform, you MUST:\n\
             1. Call `TaskGraph` with `operation: \"add\"` to add a new task node.\n\
             2. Mark that new task as `in_progress` using `operation: \"update_status\"`.\n\
             3. Only THEN execute your mutating action.".to_string()
        );
    }

    // Check if there is at least one task in progress
    let has_in_progress = nodes
        .iter()
        .any(|node| node.status == Some(TaskStatus::InProgress));
    if !has_in_progress {
        // Smart auto-promotion: scan for a promotable leaf node at depth >= 2.
        // Try to find ANY pending leaf at depth >= 2 (not just the first pending node).
        let promotable_leaf = nodes.iter().find(|node| {
            (node.status == Some(TaskStatus::Pending) || node.status.is_none())
                && node.id.split('.').count() >= 2
                && !nodes.iter().any(|n| n.parent_id.as_ref() == Some(&node.id))
        });

        if let Some(leaf) = promotable_leaf {
            let promote_id = leaf.id.clone();
            // A rejected promotion must NOT fall through as "allowed": without an
            // active task the semantic-matching layers below are skipped entirely.
            crate::task_graph::operations::execute_task_graph(
                crate::task_graph::types::TaskGraphInput {
                    operation: crate::task_graph::types::TaskGraphOperation::UpdateStatus,
                    nodes: vec![crate::task_graph::types::TaskNode {
                        id: promote_id,
                        parent_id: None,
                        content: None,
                        status: Some(TaskStatus::InProgress),
                    }],
                },
            )?;
            return Ok(());
        }

        // No promotable leaf found — check the first pending node and enforce decomposition
        if let Some(first_pending) = nodes
            .iter()
            .find(|node| node.status == Some(TaskStatus::Pending) || node.status.is_none())
        {
            let is_leaf = !nodes
                .iter()
                .any(|n| n.parent_id.as_ref() == Some(&first_pending.id));
            return Err(format!(
                "Error: Recursive Planning Required. The next pending task '{}' (\"{}\"){} and no pending leaf in the graph is executable.\n\
                 You MUST:\n\
                 1. Call `TaskGraph` with `operation: \"add\"` to create sub-tasks (e.g. '{}.1', '{}.2') under this task — each one a distinct action this task does not already state, never a restatement of it.\n\
                 2. Set the first leaf sub-task to `in_progress` using `operation: \"update_status\"`.\n\
                 3. Only THEN execute your mutating action.",
                first_pending.id,
                first_pending.content.as_deref().unwrap_or(""),
                if !is_leaf { " already has children, none of them executable," } else { " is a Level 1 phase that needs decomposition," },
                first_pending.id,
                first_pending.id,
            ));
        }

        return Err(
            "Error: Strict TaskGraph Enforcement. There are no pending tasks in your task.md. \
             To perform mutating actions, call TaskGraph with operation: \"add\" to create a new task."
                .to_string(),
        );
    }

    fn extract_all_strings(val: &Value, out: &mut String) {
        match val {
            Value::String(s) => {
                out.push(' ');
                out.push_str(s);
            }
            Value::Array(arr) => {
                for item in arr {
                    extract_all_strings(item, out);
                }
            }
            Value::Object(obj) => {
                for (k, v) in obj {
                    if k != "active_task_id" {
                        extract_all_strings(v, out);
                    }
                }
            }
            _ => {}
        }
    }

    fn extract_meaningful_words(text: &str) -> HashSet<String> {
        const STOP_WORDS: &[&str] = &[
            "a", "an", "the", "and", "or", "but", "if", "then", "else", "when", "at", "by", "for",
            "with", "about", "against", "between", "into", "through", "during", "before", "after",
            "above", "below", "to", "from", "up", "upon", "down", "in", "out", "on", "off", "over",
            "under", "again", "further", "once", "here", "there", "where", "why", "how", "all",
            "any", "both", "each", "few", "more", "most", "other", "some", "such", "no", "nor",
            "not", "only", "own", "same", "so", "than", "too", "very", "can", "will", "just",
            "don", "should", "now", "add", "new", "file", "create", "update", "run", "make", "do",
            "set", "get", "use", "edit", "change", "fix", "remove", "delete", "task", "code",
        ];
        let stop_set: HashSet<&str> = STOP_WORDS.iter().copied().collect();

        text.to_lowercase()
            .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
            .filter(|w| w.len() >= 3 && !stop_set.contains(w))
            .map(|w| w.to_string())
            .collect()
    }

    /// Extract meaningful path segments (directory names, file stems) from text
    /// that looks like file paths. This enables matching tool actions against tasks
    /// that describe work on specific modules/files.
    fn extract_path_segments(text: &str) -> HashSet<String> {
        let mut segments = HashSet::new();
        for word in text.split_whitespace() {
            // Detect path-like tokens: contain '/' or '.' with extension or start with '.'
            if word.contains('/') || word.contains('\\') || (word.contains('.') && word.len() > 4) {
                let path = std::path::Path::new(word);
                // Extract the file stem (e.g. "mod" from "mod.rs")
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    let s = stem.to_lowercase();
                    if s.len() >= 3 {
                        segments.insert(s);
                    }
                }
                // Extract each directory component (e.g. "parser" from "src/parser/mod.rs")
                for component in path.components() {
                    if let std::path::Component::Normal(c) = component {
                        if let Some(c_str) = c.to_str() {
                            let lower = c_str.to_lowercase();
                            if lower.len() >= 3
                                && lower != "src"
                                && lower != "lib"
                                && lower != "mod"
                            {
                                segments.insert(lower);
                            }
                        }
                    }
                }
            }
        }
        segments
    }

    let mut matched_node_id: Option<String> = None;

    // ── Layer 0: Explicit active_task_id or semantic text matching ──
    if let Some(active_task_id) = input.get("active_task_id").and_then(|v| v.as_str()) {
        let has_matching = nodes
            .iter()
            .any(|node| node.id == active_task_id && node.status == Some(TaskStatus::InProgress));
        if !has_matching {
            return Err(format!(
                "Error: Strict TaskGraph Enforcement. Task ID '{}' was provided but it is not currently 'in_progress'.",
                active_task_id
            ));
        }
        matched_node_id = Some(active_task_id.to_string());
    } else {
        // Find the active in_progress node (deepest active leaf)
        let active_node = find_active_leaf(&nodes);

        if let Some(active_node) = active_node {
            // Build task word set from active node + parent chain
            let mut task_text = active_node.content.clone().unwrap_or_default();
            let mut curr = active_node;
            while let Some(pid) = &curr.parent_id {
                if let Some(parent) = nodes.iter().find(|n| &n.id == pid) {
                    if let Some(content) = &parent.content {
                        task_text.push(' ');
                        task_text.push_str(content);
                    }
                    curr = parent;
                } else {
                    break;
                }
            }

            // Extract input text from tool parameters (recursively handling objects and arrays)
            let mut input_text = String::new();
            extract_all_strings(input, &mut input_text);

            let task_words = extract_meaningful_words(&task_text);
            let input_words = extract_meaningful_words(&input_text);

            // Also extract path segments for cross-matching (file paths vs task descriptions)
            let task_path_segs = extract_path_segments(&task_text);
            let input_path_segs = extract_path_segments(&input_text);

            if !task_words.is_empty() && !input_words.is_empty() {
                let has_word_overlap = input_words.iter().any(|iw| {
                    task_words.iter().any(|tw| {
                        iw == tw
                            || (tw.len() >= 4 && iw.starts_with(tw.as_str()))
                            || (iw.len() >= 4 && tw.starts_with(iw.as_str()))
                    })
                });

                // Path-based matching: check if file path segments from the tool input
                // match words in the task description, or vice versa.
                let has_path_overlap =
                    input_path_segs.iter().any(|seg| {
                        task_words.contains(seg)
                            || task_path_segs.contains(seg)
                            || task_words.iter().any(|tw| {
                                (tw.len() >= 4 && seg.contains(tw.as_str()))
                                    || (seg.len() >= 4 && tw.contains(seg.as_str()))
                            })
                    }) || task_path_segs.iter().any(|seg| input_words.contains(seg));

                if !has_word_overlap && !has_path_overlap {
                    // The active leaf is where the plan says the agent should be,
                    // and the refusal is correct while that is true. What made a
                    // live run fail five atomic tasks in ninety seconds was the
                    // dead end after it: the step it wanted to run belonged to a
                    // task the agent had just marked `failed`, so "write a
                    // matching title" produced a clone and "set a matching task
                    // in_progress" named a task that no longer existed. Reopening
                    // the failed task is the honest move, so say so.
                    let reopenable = nodes
                        .iter()
                        .filter(|node| node.status == Some(TaskStatus::Failed))
                        .take(6)
                        .map(|node| {
                            format!(
                                "{} (\"{}\")",
                                node.id,
                                node.content.as_deref().unwrap_or("(untitled)")
                            )
                        })
                        .collect::<Vec<_>>();
                    let hint = if reopenable.is_empty() {
                        "Ensure your tool description or parameters match the task in task.md, write task titles and descriptions in English ONLY, or set a matching task to 'in_progress'.".to_string()
                    } else {
                        format!(
                            "Do NOT add a new task with a title that restates this action — the plan gate rejects it as a clone. If the action belongs to one of these failed tasks, reopen it with operation: \"update_status\" ({{\"id\":\"…\",\"status\":\"in_progress\"}}) and run the action under it, using a different command, path or parameter from the attempt that failed. Otherwise set the matching pending task to 'in_progress' first.\nFailed tasks in the graph: {}",
                            reopenable.join(", ")
                        )
                    };
                    return Err(format!(
                        "Error: Strict TaskGraph Enforcement. Your action ('{}') does not match the active task '{}' (\"{}\"). {}",
                        input_text.trim(), active_node.id, active_node.content.as_deref().unwrap_or(""), hint
                    ));
                }
            }

            matched_node_id = Some(active_node.id.clone());
        }
    }

    // ── Layer 1: Parent-child & Level 1 check for the active task ──
    if let Some(ref matched_id) = matched_node_id {
        let depth = matched_id.split('.').count();
        if depth < 2 {
            return Err(format!(
                "Error: TaskGraph Enforcement. Task '{}' is a top-level task (Level 1). You MUST create sub-tasks (e.g. '{}.1') under root phases before executing mutating actions — each sub-task naming one distinct step, never a restatement of '{}'.",
                matched_id, matched_id, matched_id
            ));
        }

        let has_subtasks = nodes
            .iter()
            .any(|n| n.parent_id.as_ref() == Some(matched_id));
        if has_subtasks {
            return Err(format!(
                "Error: TaskGraph Enforcement. Task '{}' has sub-tasks. You are NOT allowed to execute actions directly under a parent task. You MUST set one of its leaf sub-tasks to 'in_progress' and run the action under it.",
                matched_id
            ));
        }
    }

    Ok(())
}

pub fn auto_create_recovery_subtask(failed_tool: &str, error_msg: &str) -> Result<(), String> {
    let store_path = task_graph_store_path()?;
    if !store_path.exists() {
        return Ok(());
    }

    let content = std::fs::read_to_string(&store_path).map_err(|e| e.to_string())?;
    let mut nodes: Vec<TaskNode> = serde_json::from_str(&content).map_err(|e| e.to_string())?;

    // Find current active leaf in_progress node (deepest one wins)
    let Some(active_id) = find_active_leaf(&nodes).map(|n| n.id.clone()) else {
        return Ok(());
    };
    let is_leaf = !nodes
        .iter()
        .any(|n| n.parent_id.as_deref() == Some(active_id.as_str()));
    if !is_leaf {
        return Ok(());
    }
    let parent_id = active_id;
    let depth = parent_id.split('.').count();
    if depth >= 5 {
        return Ok(());
    }

    // Next child id: max existing child index + 1 (counts stay dense even if
    // a child was removed, so the new id can never collide with an old one).
    let next_index = nodes
        .iter()
        .filter(|n| n.parent_id.as_deref() == Some(&parent_id))
        .filter_map(|n| n.id.rsplit('.').next().and_then(|s| s.parse::<u32>().ok()))
        .max()
        .unwrap_or(0);
    let new_child_id = format!("{}.{}", parent_id, next_index + 1);

    let clean_error_summary = error_msg
        .lines()
        .next()
        .unwrap_or(error_msg)
        .chars()
        .take(80)
        .collect::<String>();

    let new_node = TaskNode {
        id: new_child_id.clone(),
        parent_id: Some(parent_id),
        content: Some(format!(
            "Fix execution error in {}: {}",
            failed_tool, clean_error_summary
        )),
        status: Some(TaskStatus::InProgress),
    };

    nodes.push(new_node);
    crate::task_graph::store::save_task_graph_output(
        &store_path,
        &nodes,
        1,
        Some(format!("Auto-recovery subtask {} added", new_child_id)),
    )?;
    Ok(())
}
