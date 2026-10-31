pub mod enforcement;
pub mod operations;
pub mod propagation;
pub mod store;
pub mod types;
pub mod validation;

#[cfg(test)]
mod tests;

pub use enforcement::{check_task_graph_enforcement, validate_active_task_for_tool};
pub use operations::{execute_task_graph, run_task_graph};
pub use store::{
    build_active_hierarchy_prompt, compute_active_recursion_branch, compute_progress,
    find_active_leaf, load_graph_nodes, parse_task_md_to_nodes, render_nodes_markdown,
    save_task_graph_output, save_task_graph_output_with_review, task_graph_store_path,
};
pub use types::{
    compare_task_ids, id_depth, id_segments, is_descendant_id, parent_id_from_task_id,
    TaskGraphInput, TaskGraphOperation, TaskGraphOutput, TaskNode, TaskStatus,
};
pub use validation::validate_task_graph;
