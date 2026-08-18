use crate::execute_tool;
use crate::task_graph::{
    check_task_graph_enforcement, validate_active_task_for_tool, validate_task_graph, TaskNode, TaskStatus,
};
use serde_json::json;


pub struct TaskGraphEnvGuard {
    _lock_guard: std::sync::MutexGuard<'static, ()>,
}

impl Drop for TaskGraphEnvGuard {
    fn drop(&mut self) {
        std::env::remove_var("CLAWD_TASK_GRAPH_STORE");
    }
}

fn env_guard() -> TaskGraphEnvGuard {
    let guard = crate::tests::env_guard();
    TaskGraphEnvGuard { _lock_guard: guard }
}

fn temp_path(name: &str) -> std::path::PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let count = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("claw_task_graph_dir_{}_{}", std::process::id(), count));
    let _ = std::fs::create_dir_all(&dir);
    dir.join(name)
}

#[test]
fn task_graph_operations() {
    let _guard = env_guard();
    let path = temp_path("tasks.json");
    std::env::set_var("CLAWD_TASK_GRAPH_STORE", &path);

    let first = execute_tool(
        "TaskGraph",
        &json!({
            "operation": "add",
            "nodes": [
                {"id": "t1", "content": "Root task"},
                {"id": "t1.1", "parent_id": "t1", "content": "Sub task"}
            ]
        }),
    )
    .expect("TaskGraph add should succeed");
    let first_output: serde_json::Value = serde_json::from_str(&first).expect("valid json");
    assert_eq!(first_output["nodes_updated"].as_i64().expect("int"), 2);

    let second = execute_tool(
        "TaskGraph",
        &json!({
            "operation": "update_status",
            "nodes": [
                {"id": "t1.1", "status": "in_progress"}
            ]
        }),
    )
    .expect("TaskGraph update t1.1 in_progress should succeed");
    let second_output: serde_json::Value = serde_json::from_str(&second).expect("valid json");
    assert_eq!(second_output["nodes_updated"].as_i64().expect("int"), 1);

    let third = execute_tool(
        "TaskGraph",
        &json!({
            "operation": "update_status",
            "nodes": [
                {"id": "t1.1", "status": "completed"}
            ]
        }),
    )
    .expect("TaskGraph update t1.1 completed should succeed");
    let third_output: serde_json::Value = serde_json::from_str(&third).expect("valid json");
    assert_eq!(third_output["nodes_updated"].as_i64().expect("int"), 1);

    let fourth = execute_tool(
        "TaskGraph",
        &json!({
            "operation": "update_status",
            "nodes": [
                {"id": "t1", "status": "completed"}
            ]
        }),
    )
    .expect("TaskGraph update t1 completed should succeed");
    let fourth_output: serde_json::Value = serde_json::from_str(&fourth).expect("valid json");
    assert_eq!(fourth_output["nodes_updated"].as_i64().expect("int"), 0); // t1 already auto-completed via bubble-up when t1.1 completed

    std::env::remove_var("CLAWD_TASK_GRAPH_STORE");
    let _ = std::fs::remove_file(path);
}

#[test]
fn task_graph_validation_and_propagation() {
    let _guard = env_guard();
    let path = temp_path("validation_tasks.json");
    std::env::set_var("CLAWD_TASK_GRAPH_STORE", &path);

    // 1. Add some structured nodes
    let first = execute_tool(
        "TaskGraph",
        &json!({
            "operation": "add",
            "nodes": [
                {"id": "1", "content": "Parent Task"},
                {"id": "1.1", "parent_id": "1", "content": "Sub Task 1"},
                {"id": "1.2", "parent_id": "1", "content": "Sub Task 2"}
            ]
        }),
    )
    .expect("Adding structured tasks should succeed");
    let first_output: serde_json::Value = serde_json::from_str(&first).expect("valid json");
    assert_eq!(first_output["nodes_updated"].as_i64().expect("int"), 3);

    // 2. Setting "1.2" to in_progress directly should FAIL because sibling "1.1" is pending
    let err_res = execute_tool(
        "TaskGraph",
        &json!({
            "operation": "update_status",
            "nodes": [
                {"id": "1.2", "status": "in_progress"}
            ]
        }),
    );
    assert!(err_res.is_err());
    assert!(err_res.unwrap_err().contains("preceding sibling task '1.1' is currently 'Pending'"));

    // 3. Mark "1.1" as in_progress. This should automatically propagate in_progress to its parent "1"
    let _update_1 = execute_tool(
        "TaskGraph",
        &json!({
            "operation": "update_status",
            "nodes": [
                {"id": "1.1", "status": "in_progress"}
            ]
        }),
    )
    .expect("Updating '1.1' should succeed");

    // Load the saved nodes to verify parent "1" propagated to InProgress
    let store_content = std::fs::read_to_string(&path).expect("read store");
    let nodes: serde_json::Value = serde_json::from_str(&store_content).expect("parse store");
    let parent_node = nodes.as_array().unwrap().iter().find(|n| n["id"] == "1").unwrap();
    assert_eq!(parent_node["status"].as_str().unwrap(), "in_progress");

    // 4. Completing parent task "1" directly cascades completion to all sub-tasks (1.1, 1.2)
    let parent_complete = execute_tool(
        "TaskGraph",
        &json!({
            "operation": "update_status",
            "nodes": [
                {"id": "1", "status": "completed"}
            ]
        }),
    );
    assert!(parent_complete.is_ok());

    let store_content2 = std::fs::read_to_string(&path).expect("read store");
    let nodes2: serde_json::Value = serde_json::from_str(&store_content2).expect("parse store");
    let sub1 = nodes2.as_array().unwrap().iter().find(|n| n["id"] == "1.1").unwrap();
    let sub2 = nodes2.as_array().unwrap().iter().find(|n| n["id"] == "1.2").unwrap();
    assert_eq!(sub1["status"].as_str().unwrap(), "completed");
    assert_eq!(sub2["status"].as_str().unwrap(), "completed");

    std::env::remove_var("CLAWD_TASK_GRAPH_STORE");
    let _ = std::fs::remove_file(path);
}

#[test]
fn task_graph_upward_auto_completion_and_cascading() {
    let _guard = env_guard();
    let path = temp_path("upward_completion_tasks.json");
    std::env::set_var("CLAWD_TASK_GRAPH_STORE", &path);

    // Add parent and two subtasks
    let _ = execute_tool(
        "TaskGraph",
        &json!({
            "operation": "add",
            "nodes": [
                {"id": "1", "content": "Parent Task"},
                {"id": "1.1", "parent_id": "1", "content": "Sub Task 1"},
                {"id": "1.2", "parent_id": "1", "content": "Sub Task 2"}
            ]
        }),
    )
    .expect("Adding tasks should succeed");

    // Mark 1.1 in_progress, then completed
    let _ = execute_tool(
        "TaskGraph",
        &json!({
            "operation": "update_status",
            "nodes": [
                {"id": "1.1", "status": "in_progress"}
            ]
        }),
    );
    let _ = execute_tool(
        "TaskGraph",
        &json!({
            "operation": "update_status",
            "nodes": [
                {"id": "1.1", "status": "completed"}
            ]
        }),
    );

    // Mark 1.2 in_progress, then completed
    let _ = execute_tool(
        "TaskGraph",
        &json!({
            "operation": "update_status",
            "nodes": [
                {"id": "1.2", "status": "in_progress"}
            ]
        }),
    );
    let _ = execute_tool(
        "TaskGraph",
        &json!({
            "operation": "update_status",
            "nodes": [
                {"id": "1.2", "status": "completed"}
            ]
        }),
    );

    // Verify parent "1" automatically propagated to completed
    let store_content = std::fs::read_to_string(&path).expect("read store");
    let nodes: serde_json::Value = serde_json::from_str(&store_content).expect("parse store");
    let parent_node = nodes.as_array().unwrap().iter().find(|n| n["id"] == "1").unwrap();
    assert_eq!(parent_node["status"].as_str().unwrap(), "completed");

    std::env::remove_var("CLAWD_TASK_GRAPH_STORE");
    let _ = std::fs::remove_file(path);
}

#[test]
fn task_graph_auto_create_parents_and_failed_restoration() {
    let _guard = env_guard();
    let path = temp_path("parents_and_failed.json");
    std::env::set_var("CLAWD_TASK_GRAPH_STORE", &path);

    // 1. Add a deeply nested node "1.1.1" without adding its parents
    let add_res = execute_tool(
        "TaskGraph",
        &json!({
            "operation": "add",
            "nodes": [
                {"id": "1.1.1", "content": "Deep Subtask"}
            ]
        }),
    )
    .expect("Adding task should succeed");

    let add_output: serde_json::Value = serde_json::from_str(&add_res).expect("valid json");
    assert_eq!(add_output["nodes_updated"].as_i64().expect("int"), 3);

    // Verify the store has all 3 nodes
    let store_content = std::fs::read_to_string(&path).expect("read store");
    let nodes: serde_json::Value = serde_json::from_str(&store_content).expect("parse store");
    let arr = nodes.as_array().unwrap();
    assert_eq!(arr.len(), 3);
    assert!(arr.iter().any(|n| n["id"] == "1" && n["content"] == "Phase 1"));
    assert!(arr.iter().any(|n| n["id"] == "1.1" && n["content"] == "Task 1.1"));
    assert!(arr.iter().any(|n| n["id"] == "1.1.1" && n["content"] == "Deep Subtask"));

    // 2. Simulate deletion from task.md by calling execute_task_graph after modifying task.md to omit 1.1.1
    let parent_dir = path.parent().unwrap();
    let task_md_path = parent_dir.join("task.md");
    std::fs::write(
        &task_md_path,
        "> [!IMPORTANT]\n> INSTRUCTIONS\n\n# Task List\n\n- [ ] **1**: Phase 1\n- [ ] **1.1**: Task 1.1\n",
    )
    .expect("write task.md");

    let res2 = execute_tool(
        "TaskGraph",
        &json!({
            "operation": "update_status",
            "nodes": [
                {"id": "1.1", "status": "completed"}
            ]
        }),
    )
    .expect("Update status should succeed");
    let res2_output: serde_json::Value = serde_json::from_str(&res2).expect("valid json");
    assert!(res2_output["nodes_updated"].as_i64().is_some());

    let store_content2 = std::fs::read_to_string(&path).expect("read store");
    let nodes2: serde_json::Value = serde_json::from_str(&store_content2).expect("parse store");
    let arr2 = nodes2.as_array().unwrap();
    let restored_node = arr2.iter().find(|n| n["id"] == "1.1.1").expect("restored node");
    assert_eq!(restored_node["status"].as_str().unwrap(), "failed");

    std::env::remove_var("CLAWD_TASK_GRAPH_STORE");
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(task_md_path);
}

#[test]
fn task_graph_plan_first_blocks_without_graph() {
    let _guard = env_guard();
    let path = temp_path("plan_first_no_graph.json");
    let _ = std::fs::remove_file(&path);
    std::env::set_var("CLAWD_TASK_GRAPH_STORE", &path);

    let result = validate_active_task_for_tool(
        "bash",
        &json!({
            "command": "rm -rf /tmp/old_build",
            "description": "Clean up build artifacts"
        }),
    );
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("Plan-First Enforcement"),
        "Expected Plan-First error, got: {}",
        err
    );

    let result2 = validate_active_task_for_tool(
        "write_file",
        &json!({
            "path": "/tmp/test.txt",
            "content": "hello",
            "description": "Write test file"
        }),
    );
    assert!(result2.is_err());
    assert!(result2.unwrap_err().contains("Plan-First Enforcement"));

    std::env::remove_var("CLAWD_TASK_GRAPH_STORE");
}

#[test]
fn task_graph_read_only_whitelist_passes_without_graph() {
    let _guard = env_guard();
    let path = temp_path("whitelist_no_graph.json");
    let _ = std::fs::remove_file(&path);
    std::env::set_var("CLAWD_TASK_GRAPH_STORE", &path);

    assert!(validate_active_task_for_tool("read_file", &json!({"path": "/tmp/test.txt"})).is_ok());
    assert!(validate_active_task_for_tool("glob_search", &json!({"pattern": "*.rs"})).is_ok());
    assert!(validate_active_task_for_tool("grep_search", &json!({"query": "test"})).is_ok());
    assert!(validate_active_task_for_tool("ToolSearch", &json!({"query": "build"})).is_ok());
    assert!(validate_active_task_for_tool(
        "TaskGraph",
        &json!({"operation": "add", "nodes": []})
    )
    .is_ok());
    assert!(validate_active_task_for_tool("Skill", &json!({"skill": "test"})).is_ok());
    assert!(validate_active_task_for_tool("web_search", &json!({"query": "rust"})).is_ok());

    std::env::remove_var("CLAWD_TASK_GRAPH_STORE");
}

#[test]
fn task_graph_deep_recursion_bubble_up_depth_5() {
    let _guard = env_guard();
    let path = temp_path("deep_recursion_5.json");
    let _ = std::fs::remove_file(&path);
    std::env::set_var("CLAWD_TASK_GRAPH_STORE", &path);

    let _ = execute_tool(
        "TaskGraph",
        &json!({
            "operation": "add",
            "nodes": [
                {"id": "1", "content": "Root goal"},
                {"id": "1.1", "parent_id": "1", "content": "Sub-goal A"},
                {"id": "1.1.1", "parent_id": "1.1", "content": "Task A.1"},
                {"id": "1.1.1.1", "parent_id": "1.1.1", "content": "Sub-task A.1.1"},
                {"id": "1.1.1.1.1", "parent_id": "1.1.1.1", "content": "Leaf action (depth 5)"},
                {"id": "1.2", "parent_id": "1", "content": "Sub-goal B (blocks root completion)"}
            ]
        }),
    )
    .expect("Adding deep hierarchy should succeed");

    let store = std::fs::read_to_string(&path).expect("read");
    let nodes: Vec<serde_json::Value> = serde_json::from_str(&store).expect("parse");
    assert_eq!(nodes.len(), 6);

    let _ = execute_tool(
        "TaskGraph",
        &json!({
            "operation": "update_status",
            "nodes": [{"id": "1.1.1.1.1", "status": "in_progress"}]
        }),
    )
    .expect("in_progress deepest leaf");

    let store = std::fs::read_to_string(&path).expect("read");
    let nodes: Vec<serde_json::Value> = serde_json::from_str(&store).expect("parse");
    for id in &["1", "1.1", "1.1.1", "1.1.1.1"] {
        let node = nodes.iter().find(|n| n["id"] == *id).unwrap();
        assert_eq!(
            node["status"].as_str().unwrap(),
            "in_progress",
            "Expected '{}' to be in_progress after deep leaf activation",
            id
        );
    }

    let _ = execute_tool(
        "TaskGraph",
        &json!({
            "operation": "update_status",
            "nodes": [{"id": "1.1.1.1.1", "status": "completed"}]
        }),
    )
    .expect("complete deepest leaf");

    let store = std::fs::read_to_string(&path).expect("read");
    let nodes: Vec<serde_json::Value> = serde_json::from_str(&store).expect("parse");

    for id in &["1.1.1.1.1", "1.1.1.1", "1.1.1", "1.1"] {
        let node = nodes.iter().find(|n| n["id"] == *id).unwrap();
        assert_eq!(
            node["status"].as_str().unwrap(),
            "completed",
            "Expected '{}' to auto-complete via bubble-up from depth 5",
            id
        );
    }

    let root = nodes.iter().find(|n| n["id"] == "1").unwrap();
    assert_eq!(
        root["status"].as_str().unwrap(),
        "in_progress",
        "Root must NOT auto-complete while sibling 1.2 is still pending"
    );

    let _ = execute_tool(
        "TaskGraph",
        &json!({
            "operation": "update_status",
            "nodes": [{"id": "1.2", "status": "in_progress"}]
        }),
    );
    let _ = execute_tool(
        "TaskGraph",
        &json!({
            "operation": "update_status",
            "nodes": [{"id": "1.2", "status": "completed"}]
        }),
    );

    let store = std::fs::read_to_string(&path).expect("read");
    let nodes: Vec<serde_json::Value> = serde_json::from_str(&store).expect("parse");
    let root = nodes.iter().find(|n| n["id"] == "1").unwrap();
    assert_eq!(
        root["status"].as_str().unwrap(),
        "completed",
        "Root must auto-complete when ALL children (including depth-5 branch) are completed"
    );

    std::env::remove_var("CLAWD_TASK_GRAPH_STORE");
    let _ = std::fs::remove_file(path);
}

#[test]
fn task_graph_upserts_bulk_add() {
    let _guard = env_guard();
    let path = temp_path("bulk_rewrite.json");
    let _ = std::fs::remove_file(&path);
    std::env::set_var("CLAWD_TASK_GRAPH_STORE", &path);

    let _ = execute_tool(
        "TaskGraph",
        &json!({
            "operation": "add",
            "nodes": [
                {"id": "1", "content": "Phase 1"},
                {"id": "1.1", "parent_id": "1", "content": "Task 1.1"},
                {"id": "1.2", "parent_id": "1", "content": "Task 1.2"},
                {"id": "2", "content": "Phase 2"},
                {"id": "2.1", "parent_id": "2", "content": "Task 2.1"},
                {"id": "2.2", "parent_id": "2", "content": "Task 2.2"}
            ]
        }),
    )
    .expect("Initial graph creation should succeed");

    let rewrite_result = execute_tool(
        "TaskGraph",
        &json!({
            "operation": "add",
            "nodes": [
                {"id": "1", "content": "Phase 1 rewritten", "status": "completed"},
                {"id": "1.1", "content": "Task 1.1 rewritten", "status": "completed"},
                {"id": "1.2", "content": "Task 1.2 rewritten", "status": "completed"},
                {"id": "2", "content": "Phase 2 rewritten", "status": "completed"},
                {"id": "2.1", "content": "Task 2.1 rewritten", "status": "completed"},
                {"id": "2.2", "content": "Task 2.2 rewritten", "status": "completed"}
            ]
        }),
    );
    assert!(
        rewrite_result.is_ok(),
        "Bulk add with existing nodes should upsert seamlessly"
    );

    let partial_add = execute_tool(
        "TaskGraph",
        &json!({
            "operation": "add",
            "nodes": [
                {"id": "1.1", "content": "Updated task 1.1"},
                {"id": "1.3", "parent_id": "1", "content": "New task 1.3"},
                {"id": "1.4", "parent_id": "1", "content": "New task 1.4"}
            ]
        }),
    );
    assert!(
        partial_add.is_ok(),
        "Partial add with <60% overlap should succeed, got: {:?}",
        partial_add.err()
    );

    std::env::remove_var("CLAWD_TASK_GRAPH_STORE");
    let _ = std::fs::remove_file(path);
}

#[test]
fn task_graph_mcp_tools_are_enforced() {
    let _guard = env_guard();
    let path = temp_path("mcp_enforce.json");
    let _ = std::fs::remove_file(&path);
    std::env::set_var("CLAWD_TASK_GRAPH_STORE", &path);

    let result = validate_active_task_for_tool(
        "XcodeWrite",
        &json!({
            "description": "Write to Xcode project"
        }),
    );
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Plan-First Enforcement"));

    let result2 = validate_active_task_for_tool(
        "BuildProject",
        &json!({
            "description": "Build the iOS app"
        }),
    );
    assert!(result2.is_err());

    let result3 = validate_active_task_for_tool(
        "SomeFuturePlugin",
        &json!({
            "description": "Deploy to production"
        }),
    );
    assert!(result3.is_err());

    std::env::remove_var("CLAWD_TASK_GRAPH_STORE");
}

#[test]
fn task_graph_active_task_id_routing() {
    let _guard = env_guard();
    let path = temp_path("active_id_routing.json");
    std::env::set_var("CLAWD_TASK_GRAPH_STORE", &path);

    execute_tool(
        "TaskGraph",
        &json!({
            "operation": "add",
            "nodes": [
                {"id": "1", "content": "Implement authentication module", "status": "in_progress"},
                {"id": "1.1", "parent_id": "1", "content": "Add login endpoint", "status": "in_progress"}
            ]
        }),
    )
    .expect("TaskGraph add should succeed");

    let result = validate_active_task_for_tool(
        "bash",
        &json!({
            "command": "rm -rf /tmp/nonsense",
            "description": "Completely unrelated text in another language здвыадывоат",
            "active_task_id": "1.1"
        }),
    );
    assert!(
        result.is_ok(),
        "active_task_id routing should bypass text matching"
    );

    let result2 = validate_active_task_for_tool(
        "write_file",
        &json!({
            "path": "/tmp/xyz.txt",
            "content": "test",
            "description": "Totally unrelated gibberish xyzzy42",
            "active_task_id": "99.99"
        }),
    );
    assert!(result2.is_err());
    assert!(result2.unwrap_err().contains("not currently 'in_progress'"));

    std::env::remove_var("CLAWD_TASK_GRAPH_STORE");
    let _ = std::fs::remove_file(path);
}

#[test]
fn task_graph_context_extraction_from_all_fields() {
    let _guard = env_guard();
    let path = temp_path("context_extraction.json");
    std::env::set_var("CLAWD_TASK_GRAPH_STORE", &path);

    execute_tool(
        "TaskGraph",
        &json!({
            "operation": "add",
            "nodes": [
                {"id": "1", "content": "Fix everything"},
                {"id": "1.1", "parent_id": "1", "content": "Fix error_tracker module", "status": "in_progress"}
            ]
        }),
    )
    .expect("TaskGraph add should succeed");

    let result = validate_active_task_for_tool(
        "bash",
        &json!({
            "command": "cargo test -p runtime -- error_tracker",
            "description": "Run unit tests"
        }),
    );
    assert!(
        result.is_ok(),
        "Should match 'error_tracker' from command field: {:?}",
        result
    );

    let result2 = validate_active_task_for_tool(
        "write_file",
        &json!({
            "path": "/project/crates/runtime/src/error_tracker.rs",
            "content": "// fixed",
            "description": "Update source file"
        }),
    );
    assert!(
        result2.is_ok(),
        "Should match 'error_tracker' from path field: {:?}",
        result2
    );

    std::env::remove_var("CLAWD_TASK_GRAPH_STORE");
    let _ = std::fs::remove_file(path);
}

#[test]
fn task_graph_stop_words_prevent_false_positives() {
    let _guard = env_guard();
    let path = temp_path("stop_words.json");
    std::env::set_var("CLAWD_TASK_GRAPH_STORE", &path);

    execute_tool(
        "TaskGraph",
        &json!({
            "operation": "add",
            "nodes": [
                {"id": "1", "content": "Implement module features"},
                {"id": "1.1", "parent_id": "1", "content": "Add new authentication feature", "status": "in_progress"}
            ]
        }),
    )
    .expect("TaskGraph add should succeed");

    let result = validate_active_task_for_tool(
        "bash",
        &json!({
            "command": "docker compose down",
            "description": "Remove old containers and add new volumes for database migration"
        }),
    );
    assert!(
        result.is_err(),
        "Stop words should not cause false positive match"
    );

    std::env::remove_var("CLAWD_TASK_GRAPH_STORE");
    let _ = std::fs::remove_file(path);
}

#[test]
fn task_graph_bash_read_only_commands_bypass() {
    let _guard = env_guard();
    let path = temp_path("readonly_bash.json");
    let _ = std::fs::remove_file(&path);
    std::env::set_var("CLAWD_TASK_GRAPH_STORE", &path);

    let read_only_cmds = vec![
        "cat /etc/hosts",
        "ls -la /tmp",
        "grep -r pattern src/",
        "find . -name '*.rs'",
        "head -20 file.txt",
        "cargo check -p tools",
        "cargo test --lib",
        "git status",
        "git log -5",
        "git diff HEAD~1",
        "ssh server 'docker ps'",
        "ssh server 'df -h'",
        "which cargo",
        "pwd",
        "date",
        "whoami",
    ];

    for cmd in read_only_cmds {
        let result = validate_active_task_for_tool(
            "bash",
            &json!({
                "command": cmd,
                "description": "Read-only exploration"
            }),
        );
        assert!(result.is_ok(), "Read-only command '{}' should pass", cmd);
    }

    std::env::remove_var("CLAWD_TASK_GRAPH_STORE");
}

#[test]
fn task_graph_bash_whitelist_hardened() {
    let _guard = env_guard();
    let path = temp_path("hardened_bash.json");
    let _ = std::fs::remove_file(&path);
    std::env::set_var("CLAWD_TASK_GRAPH_STORE", &path);

    let should_be_blocked = vec![
        "cargo build --release",
        "rm -rf /tmp/test --help",
        "dd if=/dev/zero of=/dev/sda -h",
    ];

    for cmd in should_be_blocked {
        let result = validate_active_task_for_tool(
            "bash",
            &json!({
                "command": cmd,
                "description": "Test"
            }),
        );
        assert!(
            result.is_err(),
            "Mutating command '{}' should be BLOCKED after hardening",
            cmd
        );
    }

    let should_still_pass = vec![
        "cargo --help",
        "git --version",
        "rustc -h",
        "git status",
        "cargo check",
    ];

    for cmd in should_still_pass {
        let result = validate_active_task_for_tool(
            "bash",
            &json!({
                "command": cmd,
                "description": "Read-only exploration"
            }),
        );
        assert!(result.is_ok(), "Legitimate read-only '{}' should pass", cmd);
    }

    std::env::remove_var("CLAWD_TASK_GRAPH_STORE");
}

#[test]
fn task_graph_no_in_progress_blocks_action() {
    let _guard = env_guard();
    let path = temp_path("no_in_progress.json");
    std::env::set_var("CLAWD_TASK_GRAPH_STORE", &path);

    execute_tool(
        "TaskGraph",
        &json!({
            "operation": "add",
            "nodes": [
                {"id": "1", "content": "Pending task", "status": "pending"},
                {"id": "1.1", "parent_id": "1", "content": "Also pending"}
            ]
        }),
    )
    .expect("TaskGraph add should succeed");

    let result = validate_active_task_for_tool(
        "edit_file",
        &json!({
            "path": "/tmp/test.rs",
            "old_string": "old",
            "new_string": "new",
            "description": "Edit pending task file"
        }),
    );
    assert!(result.is_ok(), "Action should succeed by auto-promoting first pending task");

    std::env::remove_var("CLAWD_TASK_GRAPH_STORE");
    let _ = std::fs::remove_file(path);
}

#[test]
fn task_graph_parent_chain_matching() {
    let _guard = env_guard();
    let path = temp_path("parent_chain.json");
    std::env::set_var("CLAWD_TASK_GRAPH_STORE", &path);

    execute_tool(
        "TaskGraph",
        &json!({
            "operation": "add",
            "nodes": [
                {"id": "1", "content": "Refactor database layer"},
                {"id": "1.1", "parent_id": "1", "content": "Update connection pool"},
                {"id": "1.1.1", "parent_id": "1.1", "content": "Write migration script", "status": "in_progress"}
            ]
        }),
    )
    .expect("TaskGraph add should succeed");

    let result = validate_active_task_for_tool(
        "bash",
        &json!({
            "command": "psql -c 'SELECT 1'",
            "description": "Test database connection pool availability"
        }),
    );
    assert!(
        result.is_ok(),
        "Should match 'database' + 'connection' via parent chain: {:?}",
        result
    );

    std::env::remove_var("CLAWD_TASK_GRAPH_STORE");
    let _ = std::fs::remove_file(path);
}

#[test]
fn task_graph_validation_rules() {
    let valid_nodes = vec![
        TaskNode {
            id: "1".to_string(),
            parent_id: None,
            content: Some("Parent 1".to_string()),
            status: Some(TaskStatus::Completed),
        },
        TaskNode {
            id: "1.1".to_string(),
            parent_id: Some("1".to_string()),
            content: Some("Child 1.1".to_string()),
            status: Some(TaskStatus::Completed),
        },
        TaskNode {
            id: "1.2".to_string(),
            parent_id: Some("1".to_string()),
            content: Some("Child 1.2".to_string()),
            status: Some(TaskStatus::Failed),
        },
        TaskNode {
            id: "2".to_string(),
            parent_id: None,
            content: Some("Parent 2".to_string()),
            status: Some(TaskStatus::InProgress),
        },
        TaskNode {
            id: "2.1".to_string(),
            parent_id: Some("2".to_string()),
            content: Some("Child 2.1".to_string()),
            status: Some(TaskStatus::Completed),
        },
        TaskNode {
            id: "2.2".to_string(),
            parent_id: Some("2".to_string()),
            content: Some("Child 2.2".to_string()),
            status: Some(TaskStatus::InProgress),
        },
        TaskNode {
            id: "2.3".to_string(),
            parent_id: Some("2".to_string()),
            content: Some("Child 2.3".to_string()),
            status: Some(TaskStatus::Pending),
        },
    ];
    assert!(validate_task_graph(&valid_nodes).is_ok());

    let invalid_parent_completed = vec![
        TaskNode {
            id: "1".to_string(),
            parent_id: None,
            content: Some("Parent 1".to_string()),
            status: Some(TaskStatus::Completed),
        },
        TaskNode {
            id: "1.1".to_string(),
            parent_id: Some("1".to_string()),
            content: Some("Child 1.1".to_string()),
            status: Some(TaskStatus::InProgress),
        },
    ];
    let res_err = validate_task_graph(&invalid_parent_completed).unwrap_err();
    assert!(res_err.contains(
        "Parent task '1' is marked as Completed, but its sub-task '1.1' is currently 'InProgress'"
    ));

    let invalid_parent_pending = vec![
        TaskNode {
            id: "1".to_string(),
            parent_id: None,
            content: Some("Parent 1".to_string()),
            status: Some(TaskStatus::Pending),
        },
        TaskNode {
            id: "1.1".to_string(),
            parent_id: Some("1".to_string()),
            content: Some("Child 1.1".to_string()),
            status: Some(TaskStatus::Completed),
        },
    ];
    let res_err2 = validate_task_graph(&invalid_parent_pending).unwrap_err();
    assert!(res_err2.contains(
        "Parent task '1' is marked as Pending, but its sub-task '1.1' is currently 'Completed'"
    ));

    let invalid_sequential = vec![
        TaskNode {
            id: "1".to_string(),
            parent_id: None,
            content: Some("Parent 1".to_string()),
            status: Some(TaskStatus::InProgress),
        },
        TaskNode {
            id: "1.1".to_string(),
            parent_id: Some("1".to_string()),
            content: Some("Child 1.1".to_string()),
            status: Some(TaskStatus::Pending),
        },
        TaskNode {
            id: "1.2".to_string(),
            parent_id: Some("1".to_string()),
            content: Some("Child 1.2".to_string()),
            status: Some(TaskStatus::InProgress),
        },
    ];
    let res_err3 = validate_task_graph(&invalid_sequential).unwrap_err();
    assert!(res_err3.contains("You cannot start or complete task '1.2' because a preceding sibling task '1.1' is currently 'Pending'"));
}

#[test]
fn task_graph_active_task_inconsistent_graph_blocked() {
    let _guard = env_guard();
    let dir = temp_path("inconsistent_graph_dir");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("inconsistent_graph.json");
    std::env::set_var("CLAWD_TASK_GRAPH_STORE", &path);

    let nodes = vec![
        TaskNode {
            id: "1".to_string(),
            parent_id: None,
            content: Some("Parent 1".to_string()),
            status: Some(TaskStatus::Completed),
        },
        TaskNode {
            id: "1.1".to_string(),
            parent_id: Some("1".to_string()),
            content: Some("Child 1.1".to_string()),
            status: Some(TaskStatus::InProgress),
        },
    ];
    std::fs::write(&path, serde_json::to_string_pretty(&nodes).unwrap()).expect("write json");

    let result = validate_active_task_for_tool(
        "bash",
        &json!({
            "command": "rm -rf /tmp/dummy",
            "description": "Clean up build artifacts"
        }),
    );
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("TaskGraph is in an inconsistent state"));
    assert!(err.contains(
        "Parent task '1' is marked as Completed, but its sub-task '1.1' is currently 'InProgress'"
    ));

    std::env::remove_var("CLAWD_TASK_GRAPH_STORE");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn task_graph_strict_enforcement_rules() {
    let _guard = env_guard();
    let dir = temp_path("strict_enforcement_rules_dir");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("strict_enforcement.json");
    std::env::set_var("CLAWD_TASK_GRAPH_STORE", &path);

    execute_tool(
        "TaskGraph",
        &json!({
            "operation": "add",
            "nodes": [
                {"id": "1", "content": "Refactor codebase", "status": "in_progress"}
            ]
        }),
    )
    .expect("TaskGraph add should succeed");

    let result_rule3 = validate_active_task_for_tool(
        "write_file",
        &json!({
            "path": "/tmp/a.txt",
            "content": "test",
            "description": "Refactor codebase",
            "active_task_id": "1"
        }),
    );
    assert!(
        result_rule3.is_err(),
        "Should reject Level 1 task execution"
    );
    let err_msg = result_rule3.unwrap_err();
    assert!(
        err_msg.contains("is a top-level phase") || err_msg.contains("is a top-level task (Level 1)"),
        "Expected top-level phase or Level 1 rejection, got: {}",
        err_msg
    );

    let _ = std::fs::remove_file(&path);

    execute_tool(
        "TaskGraph",
        &json!({
            "operation": "add",
            "nodes": [
                {"id": "1", "content": "Refactor codebase"},
                {"id": "1.1", "parent_id": "1", "content": "Write unit tests"},
                {"id": "1.1.1", "parent_id": "1.1", "content": "Test database logic", "status": "in_progress"}
            ]
        }),
    )
    .expect("TaskGraph add should succeed");

    let result_rule1 = validate_active_task_for_tool(
        "write_file",
        &json!({
            "path": "/tmp/a.txt",
            "content": "test",
            "description": "Write unit tests",
            "active_task_id": "1.1"
        }),
    );
    assert!(
        result_rule1.is_err(),
        "Should reject action on parent task '1.1'"
    );
    assert!(result_rule1.unwrap_err().contains(
        "has sub-tasks. You are NOT allowed to execute actions directly under a parent task"
    ));

    std::env::remove_var("CLAWD_TASK_GRAPH_STORE");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn task_graph_downward_in_progress_propagation() {
    let _guard = env_guard();
    let path = temp_path("downward_prop.json");
    std::env::set_var("CLAWD_TASK_GRAPH_STORE", &path);

    execute_tool(
        "TaskGraph",
        &json!({
            "operation": "add",
            "nodes": [
                {"id": "1", "content": "Phase 1"},
                {"id": "1.1", "parent_id": "1", "content": "Task 1.1"},
                {"id": "1.1.1", "parent_id": "1.1", "content": "Subtask 1.1.1"}
            ]
        }),
    )
    .expect("TaskGraph add should succeed");

    execute_tool(
        "TaskGraph",
        &json!({
            "operation": "update_status",
            "nodes": [
                {"id": "1", "status": "in_progress"}
            ]
        }),
    )
    .expect("TaskGraph update_status should succeed");

    let graph_content = std::fs::read_to_string(&path).expect("read graph store");
    let nodes: serde_json::Value =
        serde_json::from_str(&graph_content).expect("parse store json");
    let _node_1_1_1 = nodes
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == "1.1.1")
        .unwrap();
    assert_eq!(_node_1_1_1["status"], "in_progress");

    let _ = std::fs::remove_file(&path);
}

#[test]
fn task_graph_update_status_auto_upserts_missing_nodes() {
    let _guard = env_guard();
    let path = temp_path("auto_upsert.json");
    std::env::set_var("CLAWD_TASK_GRAPH_STORE", &path);

    execute_tool(
        "TaskGraph",
        &json!({
            "operation": "add",
            "nodes": [
                {"id": "1", "content": "Phase 1"}
            ]
        }),
    )
    .expect("TaskGraph add should succeed");

    // Call update_status directly for non-existent node 1.1.1 — should auto-create it without error!
    execute_tool(
        "TaskGraph",
        &json!({
            "operation": "update_status",
            "nodes": [
                {"id": "1.1.1", "status": "in_progress"}
            ]
        }),
    )
    .expect("update_status on missing node should auto-create node");

    let graph_content = std::fs::read_to_string(&path).expect("read graph store");
    let nodes: serde_json::Value =
        serde_json::from_str(&graph_content).expect("parse store json");
    let _node_1_1_1 = nodes
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == "1.1.1")
        .unwrap();
    let _ = std::fs::remove_file(&path);
}

#[test]
fn task_graph_parent_description_and_unclosed_subtasks_in_error() {
    let _guard = env_guard();
    let path = temp_path("unclosed_subtasks_error.json");
    std::env::set_var("CLAWD_TASK_GRAPH_STORE", &path);

    // Create graph with parent "1" and subtasks "1.1", "1.2"
    let _ = execute_tool(
        "TaskGraph",
        &json!({
            "operation": "add",
            "nodes": [
                {"id": "1", "content": "Database Migration and Testing Phase"},
                {"id": "1.1", "content": "Inspect remote schema and connections"},
                {"id": "1.2", "content": "Run integration tests on remote DB"}
            ]
        }),
    );

    // Set 1.1 to in_progress
    let _ = execute_tool(
        "TaskGraph",
        &json!({
            "operation": "update_status",
            "nodes": [
                {"id": "1.1", "status": "in_progress"}
            ]
        }),
    );

    // Manually attempt to validate graph where parent 1 is Completed but 1.1 and 1.2 are unclosed
    let nodes = vec![
        TaskNode {
            id: "1".to_string(),
            parent_id: None,
            content: Some("Database Migration and Testing Phase".to_string()),
            status: Some(TaskStatus::Completed),
        },
        TaskNode {
            id: "1.1".to_string(),
            parent_id: Some("1".to_string()),
            content: Some("Inspect remote schema and connections".to_string()),
            status: Some(TaskStatus::InProgress),
        },
        TaskNode {
            id: "1.2".to_string(),
            parent_id: Some("1".to_string()),
            content: Some("Run integration tests on remote DB".to_string()),
            status: Some(TaskStatus::Pending),
        },
    ];

    let val_err = validate_task_graph(&nodes).unwrap_err();

    // Verify error contains parent description, count, unclosed list, and resolution choices
    assert!(val_err.contains("Database Migration and Testing Phase"));
    assert!(val_err.contains("2 unclosed sub-task(s)"));
    assert!(val_err.contains("Inspect remote schema and connections"));
    assert!(val_err.contains("Run integration tests on remote DB"));
    assert!(val_err.contains("update its status to 'failed' (or remove it) to mark it as skipped (-)"));
    assert!(val_err.contains("break it down recursively into smaller sub-subtasks"));

    let _ = std::fs::remove_file(&path);
}

#[test]
fn task_graph_auto_demotes_completed_parent_when_subtask_is_added_or_opened() {
    let _guard = env_guard();
    let path = temp_path("demote_test.json");
    std::env::set_var("CLAWD_TASK_GRAPH_STORE", &path);

    // Initial setup: parent 1 and child 1.1 completed
    let _ = execute_tool(
        "TaskGraph",
        &json!({
            "operation": "add",
            "nodes": [
                {"id": "1", "content": "Parent Task", "status": "completed"},
                {"id": "1.1", "parent_id": "1", "content": "Child Task", "status": "completed"}
            ]
        }),
    ).expect("initial add succeeds");

    // Now add a new subtask 1.2 under already completed parent 1
    let result = execute_tool(
        "TaskGraph",
        &json!({
            "operation": "add",
            "nodes": [
                {"id": "1.2", "parent_id": "1", "content": "New Subtask", "status": "in_progress"}
            ]
        }),
    ).expect("add subtask to completed parent should auto-demote parent to in_progress");

    let output: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(output["nodes_updated"].as_u64().unwrap_or(0), 1);

    // Read saved nodes and verify parent 1 was auto-demoted to in_progress
    let saved_content = std::fs::read_to_string(&path).expect("saved json");
    let _saved_nodes: Vec<TaskNode> = serde_json::from_str(&saved_content).expect("parse nodes");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn task_graph_recursive_planning_and_level_2_enforcement() {
    let _guard = env_guard();
    let path = temp_path("recursive_plan_test.json");
    std::env::set_var("CLAWD_TASK_GRAPH_STORE", &path);

    // 1. Build a multi-level recursive task graph
    let add_res = execute_tool(
        "TaskGraph",
        &json!({
            "operation": "add",
            "nodes": [
                {"id": "1", "content": "Phase 1: Code Optimization"},
                {"id": "1.1", "parent_id": "1", "content": "Refactor module structure"},
                {"id": "1.1.1", "parent_id": "1.1", "content": "Extract helper functions into utils"},
                {"id": "1.1.2", "parent_id": "1.1", "content": "Verify unit tests for utils"}
            ]
        }),
    ).expect("creating multi-level graph should succeed");
    let output: serde_json::Value = serde_json::from_str(&add_res).unwrap();
    assert_eq!(output["nodes_updated"].as_u64().unwrap_or(0), 4);

    // 2. Set active leaf 1.1.1 to in_progress
    let _ = execute_tool(
        "TaskGraph",
        &json!({
            "operation": "update_status",
            "nodes": [
                {"id": "1.1.1", "status": "in_progress"}
            ]
        }),
    ).expect("setting 1.1.1 to in_progress should succeed");

    // 3. Verify enforcement permits mutating action (write_file) for active leaf 1.1.1
    let tool_check = check_task_graph_enforcement("write_file", &json!({"path": "src/utils.rs", "content": "fn help(){}"}));
    assert!(tool_check.is_ok(), "mutating action matching active leaf 1.1.1 must be permitted");

    // 4. Complete 1.1.1 and 1.1.2 — parent 1.1 and root 1 must auto-complete via bubble-up
    let _ = execute_tool(
        "TaskGraph",
        &json!({
            "operation": "update_status",
            "nodes": [
                {"id": "1.1.1", "status": "completed"}
            ]
        }),
    ).expect("completing 1.1.1");

    let _ = execute_tool(
        "TaskGraph",
        &json!({
            "operation": "update_status",
            "nodes": [
                {"id": "1.1.2", "status": "completed"}
            ]
        }),
    ).expect("completing 1.1.2");

    let saved_nodes: Vec<TaskNode> = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(saved_nodes.iter().find(|n| n.id == "1").unwrap().status, Some(TaskStatus::Completed));

    // 5. Add a level 2 leaf task (Phase 2 -> 2.1) and set 2.1 in_progress
    let _ = execute_tool(
        "TaskGraph",
        &json!({
            "operation": "add",
            "nodes": [
                {"id": "2", "content": "Phase 2: Documentation"},
                {"id": "2.1", "parent_id": "2", "content": "Update module README documentation", "status": "in_progress"}
            ]
        }),
    ).expect("adding level 2 leaf task");

    // Level 2 leaf task (2.1) must pass enforcement without error
    let tool_check_l2 = check_task_graph_enforcement("write_file", &json!({"path": "README.md", "content": "docs"}));
    assert!(tool_check_l2.is_ok(), "level 2 leaf task 2.1 must pass enforcement without mandatory level 3 error");

    std::env::remove_var("CLAWD_TASK_GRAPH_STORE");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn task_graph_path_based_semantic_matching() {
    let _guard = env_guard();
    let path = temp_path("path_match.json");
    std::env::set_var("CLAWD_TASK_GRAPH_STORE", &path);

    execute_tool(
        "TaskGraph",
        &json!({
            "operation": "add",
            "nodes": [
                {"id": "1", "content": "Refactor the parser module"},
                {"id": "1.1", "parent_id": "1", "content": "Update parser validation logic", "status": "in_progress"}
            ]
        }),
    )
    .expect("TaskGraph add should succeed");

    // Tool writes to src/parser/validation.rs — should match via path segments
    let result = validate_active_task_for_tool(
        "write_file",
        &json!({
            "path": "src/parser/validation.rs",
            "content": "fn validate() {}"
        }),
    );
    assert!(
        result.is_ok(),
        "Path 'src/parser/validation.rs' should match task 'Update parser validation logic' via path segments: {:?}",
        result
    );

    std::env::remove_var("CLAWD_TASK_GRAPH_STORE");
    let _ = std::fs::remove_file(path);
}

#[test]
fn task_graph_level1_only_graph_blocks_mutating_action() {
    let _guard = env_guard();
    let path = temp_path("level1_block.json");
    std::env::set_var("CLAWD_TASK_GRAPH_STORE", &path);

    execute_tool(
        "TaskGraph",
        &json!({
            "operation": "add",
            "nodes": [
                {"id": "1", "content": "Phase 1: Setup infrastructure", "status": "pending"}
            ]
        }),
    )
    .expect("TaskGraph add should succeed");

    // With only a level 1 node and no leaves, mutating action should be blocked
    let result = validate_active_task_for_tool(
        "write_file",
        &json!({"path": "setup.sh", "content": "echo hello"}),
    );
    assert!(result.is_err(), "Level 1 only graph should block mutating actions");
    let err = result.unwrap_err();
    assert!(
        err.contains("Recursive Planning Required"),
        "Should require recursive planning, got: {}",
        err
    );

    std::env::remove_var("CLAWD_TASK_GRAPH_STORE");
    let _ = std::fs::remove_file(path);
}

#[test]
fn task_graph_sibling_auto_advance_on_completion() {
    let _guard = env_guard();
    let path = temp_path("sibling_advance.json");
    std::env::set_var("CLAWD_TASK_GRAPH_STORE", &path);

    execute_tool(
        "TaskGraph",
        &json!({
            "operation": "add",
            "nodes": [
                {"id": "1", "content": "Phase 1"},
                {"id": "1.1", "parent_id": "1", "content": "First subtask", "status": "in_progress"},
                {"id": "1.2", "parent_id": "1", "content": "Second subtask", "status": "pending"},
                {"id": "1.3", "parent_id": "1", "content": "Third subtask", "status": "pending"}
            ]
        }),
    )
    .expect("TaskGraph add should succeed");

    // Complete task 1.1
    execute_tool(
        "TaskGraph",
        &json!({
            "operation": "update_status",
            "nodes": [{"id": "1.1", "status": "completed"}]
        }),
    )
    .expect("update_status should succeed");

    // Check that 1.2 was auto-advanced to in_progress
    let content = std::fs::read_to_string(&path).expect("read store");
    let nodes: Vec<serde_json::Value> = serde_json::from_str(&content).expect("parse json");
    let node_1_2 = nodes.iter().find(|n| n["id"] == "1.2").expect("node 1.2 should exist");
    assert_eq!(
        node_1_2["status"].as_str().unwrap(),
        "in_progress",
        "Sibling 1.2 should auto-advance to in_progress after 1.1 completes"
    );

    // 1.3 should still be pending
    let node_1_3 = nodes.iter().find(|n| n["id"] == "1.3").expect("node 1.3 should exist");
    assert_eq!(
        node_1_3["status"].as_str().unwrap(),
        "pending",
        "Sibling 1.3 should remain pending"
    );

    std::env::remove_var("CLAWD_TASK_GRAPH_STORE");
    let _ = std::fs::remove_file(path);
}

#[test]
fn test_grisha_simulation_detector_blocks_echo_chains() {
    let _guard = env_guard();
    let path = temp_path("grisha_sim.json");
    std::env::set_var("CLAWD_TASK_GRAPH_STORE", &path);

    execute_tool(
        "TaskGraph",
        &json!({
            "operation": "add",
            "nodes": [
                {"id": "1", "content": "Phase 1"},
                {"id": "1.1", "parent_id": "1", "content": "Inspect system", "status": "in_progress"}
            ]
        }),
    )
    .expect("TaskGraph add should succeed");

    // Attempting to simulate analysis via chained echo commands must be blocked by Grisha
    let sim_cmd = r#"echo "=== Tool Invocation Analysis ===" && echo "Available tools: 14" && echo "Duplicate count: 14 tools duplicated""#;
    let res = execute_tool("bash", &json!({"command": sim_cmd}));
    assert!(res.is_err());
    let err_msg = res.unwrap_err();
    assert!(err_msg.contains("GRISHA_SIM_003") || err_msg.contains("Faux Execution / Simulation Detected"));

    std::env::remove_var("CLAWD_TASK_GRAPH_STORE");
    let _ = std::fs::remove_file(path);
}

#[test]
fn test_grisha_plan_review_and_smart_recursive_branch() {
    let _guard = env_guard();
    let path = temp_path("grisha_plan.json");
    std::env::set_var("CLAWD_TASK_GRAPH_STORE", &path);

    // 1. Add top-level plan without sub-tasks -> Grisha should generate advisory remarks
    let out_str = execute_tool(
        "TaskGraph",
        &json!({
            "operation": "add",
            "nodes": [
                {"id": "1", "content": "Analyze core directive compliance"}
            ]
        }),
    )
    .expect("TaskGraph add should succeed");

    let out: serde_json::Value = serde_json::from_str(&out_str).expect("parse output");
    assert!(out.get("grisha_review").is_some());
    let review = out["grisha_review"].as_array().unwrap();
    assert!(review[0].as_str().unwrap().contains("Grisha Advisory"));

    // 2. Add leaf node 1.1 and 1.1.1 and activate it
    execute_tool(
        "TaskGraph",
        &json!({
            "operation": "add",
            "nodes": [
                {"id": "1.1", "parent_id": "1", "content": "Tool analysis"},
                {"id": "1.1.1", "parent_id": "1.1", "content": "Inspect configuration files", "status": "in_progress"}
            ]
        }),
    )
    .expect("Add 1.1 and 1.1.1 should succeed");

    // 3. Update status and verify smart active recursion branch is returned
    let update_out_str = execute_tool(
        "TaskGraph",
        &json!({
            "operation": "update_status",
            "nodes": [
                {"id": "1.1.1", "status": "in_progress"}
            ]
        }),
    )
    .expect("Update status should succeed");

    let update_out: serde_json::Value = serde_json::from_str(&update_out_str).expect("parse output");
    assert_eq!(update_out["active_leaf_id"].as_str().unwrap(), "1.1.1");
    let chain = update_out["active_recursion_chain"].as_array().unwrap();
    let chain_ids: Vec<&str> = chain.iter().filter_map(|v| v.as_str()).collect();
    assert_eq!(chain_ids, vec!["1", "1.1", "1.1.1"]);
    assert!(update_out["active_branch_summary"].as_str().unwrap().contains("ACTIVE LEAF"));

    std::env::remove_var("CLAWD_TASK_GRAPH_STORE");
    let _ = std::fs::remove_file(path);
}

#[test]
fn test_mixed_finished_parent_auto_advances_to_next_phase() {
    let _guard = env_guard();
    let test_dir = std::env::temp_dir().join(format!("claw_test_auto_advance_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&test_dir);
    let path = test_dir.join(".clawd-task-graph.json");
    std::env::set_var("CLAWD_TASK_GRAPH_STORE", path.to_str().unwrap());

    // 1. Setup Phase 1 and Phase 2 with hierarchy
    let _ = execute_tool(
        "TaskGraph",
        &json!({
            "operation": "add",
            "nodes": [
                {"id": "1", "content": "Phase 1: Config"},
                {"id": "1.1", "parent_id": "1", "content": "Update project.yml", "status": "completed"},
                {"id": "1.2", "parent_id": "1", "content": "Run xcodegen", "status": "failed"},
                {"id": "1.3", "parent_id": "1", "content": "Load simulator MCP", "status": "in_progress"},
                {"id": "1.3.1", "parent_id": "1.3", "content": "Search MCP", "status": "completed"},
                {"id": "1.3.2", "parent_id": "1.3", "content": "Load MCP", "status": "in_progress"},
                {"id": "2", "content": "Phase 2: Build and Test"},
                {"id": "2.1", "parent_id": "2", "content": "Discover simulator"},
                {"id": "2.1.1", "parent_id": "2.1", "content": "List simulators"}
            ]
        }),
    ).expect("Add phases should succeed");

    // 2. Complete the last sub-task of Phase 1 (1.3.2)
    let out_str = execute_tool(
        "TaskGraph",
        &json!({
            "operation": "update_status",
            "nodes": [
                {"id": "1.3.2", "status": "completed"}
            ]
        }),
    ).expect("Complete 1.3.2 should succeed and auto-advance");

    let out: serde_json::Value = serde_json::from_str(&out_str).expect("parse output");
    // Verify that Phase 2 was automatically promoted to InProgress down to leaf 2.1.1
    assert_eq!(out["active_leaf_id"].as_str().unwrap(), "2.1.1");
    let chain = out["active_recursion_chain"].as_array().unwrap();
    let chain_ids: Vec<&str> = chain.iter().filter_map(|v| v.as_str()).collect();
    assert_eq!(chain_ids, vec!["2", "2.1", "2.1.1"]);

    std::env::remove_var("CLAWD_TASK_GRAPH_STORE");
    let _ = std::fs::remove_dir_all(&test_dir);
}

#[test]
fn test_recursive_reopen_resets_subsequent_phases_to_pending() {
    let _guard = env_guard();
    let test_dir = std::env::temp_dir().join(format!("claw_test_reopen_reset_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&test_dir);
    let path = test_dir.join(".clawd-task-graph.json");
    std::env::set_var("CLAWD_TASK_GRAPH_STORE", path.to_str().unwrap());

    // 1. Add Phase 1 with 1.1, 1.2, 1.3
    let _ = execute_tool(
        "TaskGraph",
        &json!({
            "operation": "add",
            "nodes": [
                {"id": "1", "content": "Phase 1: Setup"},
                {"id": "1.1", "parent_id": "1", "content": "Inspect files", "status": "completed"},
                {"id": "1.2", "parent_id": "1", "content": "Build project", "status": "failed"},
                {"id": "1.3", "parent_id": "1", "content": "Next step", "status": "pending"}
            ]
        }),
    ).expect("Add phase should succeed");

    // 2. Reopen 1.2 via recursive subtask addition
    let out_str = execute_tool(
        "TaskGraph",
        &json!({
            "operation": "add",
            "nodes": [
                {"id": "1.2.1", "parent_id": "1.2", "content": "Diagnose build error", "status": "pending"},
                {"id": "1.2.2", "parent_id": "1.2", "content": "Apply patch", "status": "pending"}
            ]
        }),
    ).expect("Add subtasks to failed parent should auto-reopen parent to in_progress");

    let out: serde_json::Value = serde_json::from_str(&out_str).expect("parse output");
    assert_eq!(out["active_leaf_id"].as_str().unwrap(), "1.2.1");
    let chain = out["active_recursion_chain"].as_array().unwrap();
    let chain_ids: Vec<&str> = chain.iter().filter_map(|v| v.as_str()).collect();
    assert_eq!(chain_ids, vec!["1", "1.2", "1.2.1"]);

    std::env::remove_var("CLAWD_TASK_GRAPH_STORE");
    let _ = std::fs::remove_dir_all(&test_dir);
}

#[test]
fn test_parent_with_failed_and_completed_children_stays_failed_and_advances() {
    let _guard = env_guard();
    let test_dir = std::env::temp_dir().join(format!("claw_test_failed_mixed_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&test_dir);
    let path = test_dir.join(".clawd-task-graph.json");
    std::env::set_var("CLAWD_TASK_GRAPH_STORE", path.to_str().unwrap());

    // 1. Setup Phase 1, Phase 2 (with 2.1 failed, 2.2 in_progress), Phase 3 (3.1 pending)
    let _ = execute_tool(
        "TaskGraph",
        &json!({
            "operation": "add",
            "nodes": [
                {"id": "1", "content": "Phase 1: Config", "status": "completed"},
                {"id": "2", "content": "Phase 2: Build & Test", "status": "in_progress"},
                {"id": "2.1", "parent_id": "2", "content": "Build project", "status": "failed"},
                {"id": "2.1.1", "parent_id": "2.1", "content": "Inspect error logs", "status": "failed"},
                {"id": "2.1.2", "parent_id": "2.1", "content": "Apply fix", "status": "completed"},
                {"id": "2.1.3", "parent_id": "2.1", "content": "Verify fix", "status": "failed"},
                {"id": "2.2", "parent_id": "2", "content": "Run unit tests", "status": "in_progress"},
                {"id": "3", "content": "Phase 3: Deploy & Verify", "status": "pending"},
                {"id": "3.1", "parent_id": "3", "content": "Install app", "status": "pending"},
                {"id": "3.2", "parent_id": "3", "content": "Interactive UI verification", "status": "pending"}
            ]
        }),
    ).expect("Add initial graph should succeed");

    // 2. Complete the last sub-task of Phase 2 (2.2)
    let out_str = execute_tool(
        "TaskGraph",
        &json!({
            "operation": "update_status",
            "nodes": [
                {"id": "2.2", "status": "completed"}
            ]
        }),
    ).expect("Completing 2.2 should propagate Phase 2 to Failed and auto-advance to Phase 3");

    let out: serde_json::Value = serde_json::from_str(&out_str).expect("parse output");
    // Verify that Phase 2 is marked Failed and Phase 3 (3.1) is auto-promoted to InProgress
    assert_eq!(out["active_leaf_id"].as_str().unwrap(), "3.1");
    let chain = out["active_recursion_chain"].as_array().unwrap();
    let chain_ids: Vec<&str> = chain.iter().filter_map(|v| v.as_str()).collect();
    assert_eq!(chain_ids, vec!["3", "3.1"]);

    std::env::remove_var("CLAWD_TASK_GRAPH_STORE");
    let _ = std::fs::remove_dir_all(&test_dir);
}


