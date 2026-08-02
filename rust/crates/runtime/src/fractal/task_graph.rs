//! Self-similar fractal TaskGraph with δ-governed budget decay.

use std::fs;
use std::path::Path;
use serde::{Deserialize, Serialize};

use super::constants::{bifurcation_ratio, optimal_children, FractalBudget};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FractalTaskStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
}

impl Default for FractalTaskStatus {
    fn default() -> Self {
        Self::Pending
    }
}

/// A node in the fractal task tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FractalTaskNode {
    pub id: String,
    pub content: String,
    #[serde(default)]
    pub status: FractalTaskStatus,
    pub depth: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_tokens: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_fraction: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_atomic: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<FractalTaskNode>,
}

impl FractalTaskNode {
    pub fn new(id: impl Into<String>, content: impl Into<String>, depth: usize, total_budget: usize) -> Self {
        let budget = FractalBudget::new(total_budget, depth);
        Self {
            id: id.into(),
            content: content.into(),
            status: FractalTaskStatus::Pending,
            depth,
            parent_id: None,
            budget_tokens: Some(budget.tokens()),
            budget_fraction: Some((budget.fraction() * 1_000_000.0).round() / 1_000_000.0),
            is_atomic: Some(budget.is_atomic()),
            children: Vec::new(),
        }
    }

    #[must_use]
    pub fn bifurcation_ratio(&self) -> f64 {
        bifurcation_ratio(self.depth)
    }

    #[must_use]
    pub fn should_split(&self, total_budget: usize) -> bool {
        let budget = FractalBudget::new(total_budget, self.depth);
        !budget.is_atomic() && self.depth < 4
    }

    #[must_use]
    pub fn optimal_children_count(&self, cap: usize) -> usize {
        optimal_children(self.depth, cap)
    }

    pub fn add_child(&mut self, child_id: impl Into<String>, content: impl Into<String>, total_budget: usize) -> &mut FractalTaskNode {
        let mut child = FractalTaskNode::new(child_id, content, self.depth + 1, total_budget);
        child.parent_id = Some(self.id.clone());
        self.children.push(child);
        self.children.last_mut().unwrap()
    }
}

/// A tree/forest of fractal task nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FractalTaskGraph {
    pub roots: Vec<FractalTaskNode>,
    pub total_budget: usize,
}

impl FractalTaskGraph {
    #[must_use]
    pub fn new(total_budget: usize) -> Self {
        Self {
            roots: Vec::new(),
            total_budget,
        }
    }

    pub fn add_root(&mut self, id: impl Into<String>, content: impl Into<String>) -> &mut FractalTaskNode {
        let node = FractalTaskNode::new(id, content, 0, self.total_budget);
        self.roots.push(node);
        self.roots.last_mut().unwrap()
    }

    pub fn all_nodes(&self) -> Vec<&FractalTaskNode> {
        let mut result = Vec::new();
        fn collect<'a>(node: &'a FractalTaskNode, out: &mut Vec<&'a FractalTaskNode>) {
            out.push(node);
            for child in &node.children {
                collect(child, out);
            }
        }
        for root in &self.roots {
            collect(root, &mut result);
        }
        result
    }

    pub fn completion_ratio(&self) -> f64 {
        let nodes = self.all_nodes();
        if nodes.is_empty() {
            return 0.0;
        }
        let completed = nodes.iter().filter(|n| n.status == FractalTaskStatus::Completed).count();
        completed as f64 / nodes.len() as f64
    }

    pub fn save_to_file(&self, path: impl AsRef<Path>) -> Result<(), std::io::Error> {
        let json = serde_json::to_string_pretty(self)?;
        fs::write(path, json)
    }

    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Self, Box<dyn std::error::Error>> {
        let content = fs::read_to_string(path)?;
        let graph: Self = serde_json::from_str(&content)?;
        Ok(graph)
    }
}
