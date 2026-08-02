//! Bifurcation Telemetry and ASCII Fractal Tree Renderer.

use std::collections::HashMap;
use serde::{Deserialize, Serialize};

use super::constants::{FEIGENBAUM_ALPHA, FEIGENBAUM_DELTA};
use super::task_graph::{FractalTaskGraph, FractalTaskNode, FractalTaskStatus};

/// Detailed telemetry metrics for a fractal task execution graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BifurcationTelemetryReport {
    pub total_nodes: usize,
    pub max_depth_reached: usize,
    pub atomic_leaves_count: usize,
    pub completion_ratio: f64,
    pub observed_bifurcation_ratio: f64,
    pub asymmetry_index: f64,
    pub depth_distribution: HashMap<usize, usize>,
}

impl BifurcationTelemetryReport {
    /// Generate a telemetry report from a `FractalTaskGraph`.
    #[must_use]
    pub fn from_graph(graph: &FractalTaskGraph) -> Self {
        let nodes = graph.all_nodes();
        if nodes.is_empty() {
            return Self {
                total_nodes: 0,
                max_depth_reached: 0,
                atomic_leaves_count: 0,
                completion_ratio: 0.0,
                observed_bifurcation_ratio: 1.0,
                asymmetry_index: 0.0,
                depth_distribution: HashMap::new(),
            };
        }

        let total_nodes = nodes.len();
        let max_depth_reached = nodes.iter().map(|n| n.depth).max().unwrap_or(0);
        let atomic_leaves_count = nodes
            .iter()
            .filter(|n| n.children.is_empty() && n.is_atomic.unwrap_or(false))
            .count();
        let completion_ratio = graph.completion_ratio();

        let mut depth_distribution = HashMap::new();
        for node in &nodes {
            *depth_distribution.entry(node.depth).or_insert(0) += 1;
        }

        // Compute observed bifurcation ratio (avg children per parent)
        let parents: Vec<&&FractalTaskNode> = nodes.iter().filter(|n| !n.children.is_empty()).collect();
        let observed_bifurcation_ratio = if parents.is_empty() {
            1.0
        } else {
            let total_children: usize = parents.iter().map(|p| p.children.len()).sum();
            (total_children as f64) / (parents.len() as f64)
        };

        // Compute asymmetry index using α (variance in sibling counts)
        let asymmetry_index = if parents.is_empty() {
            0.0
        } else {
            let mean_children = observed_bifurcation_ratio;
            let variance: f64 = parents
                .iter()
                .map(|p| (p.children.len() as f64 - mean_children).powi(2))
                .sum::<f64>()
                / (parents.len() as f64);
            (variance.sqrt() / FEIGENBAUM_ALPHA).min(1.0)
        };

        Self {
            total_nodes,
            max_depth_reached,
            atomic_leaves_count,
            completion_ratio,
            observed_bifurcation_ratio,
            asymmetry_index,
            depth_distribution,
        }
    }

    /// Render a rich ASCII tree of the fractal task graph.
    #[must_use]
    pub fn render_ascii_tree(&self, graph: &FractalTaskGraph) -> String {
        let mut lines = Vec::new();
        lines.push(format!(
            "🌳 Fractal Task Tree (δ ≈ {:.4}, α ≈ {:.4})",
            FEIGENBAUM_DELTA, FEIGENBAUM_ALPHA
        ));
        lines.push(format!(
            "📊 Metrics: nodes={} max_depth={} completion={:.0}% bifurcation_ratio={:.2} asymmetry={:.2}",
            self.total_nodes,
            self.max_depth_reached,
            self.completion_ratio * 100.0,
            self.observed_bifurcation_ratio,
            self.asymmetry_index
        ));
        lines.push("─".repeat(60));

        fn render_node(node: &FractalTaskNode, prefix: &str, is_last: bool, out: &mut Vec<String>) {
            let connector = if is_last { "└── " } else { "├── " };
            let status_sym = match node.status {
                FractalTaskStatus::Completed => "✅",
                FractalTaskStatus::InProgress => "🔄",
                FractalTaskStatus::Failed => "❌",
                FractalTaskStatus::Pending => "⬜",
            };
            let atomic_mark = if node.is_atomic.unwrap_or(false) { " [💥 Atomic]" } else { "" };
            let budget_str = node
                .budget_tokens
                .map_or(String::new(), |b| format!(" ({b}tok)"));

            out.push(format!(
                "{}{}{} id={} status={:?}{}{}",
                prefix, connector, status_sym, node.id, node.status, budget_str, atomic_mark
            ));

            let child_prefix = format!("{}{}", prefix, if is_last { "    " } else { "│   " });
            let count = node.children.len();
            for (idx, child) in node.children.iter().enumerate() {
                render_node(child, &child_prefix, idx + 1 == count, out);
            }
        }

        let count = graph.roots.len();
        for (idx, root) in graph.roots.iter().enumerate() {
            render_node(root, "", idx + 1 == count, &mut lines);
        }

        lines.join("\n")
    }
}
