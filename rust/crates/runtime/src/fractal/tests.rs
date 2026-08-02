//! Unit tests for the Rust fractal module.

#[cfg(test)]
mod tests {
    use super::super::constants::*;
    use super::super::task_graph::*;
    use super::super::rate_limiter::*;
    use super::super::model_cascade::*;
    use super::super::compact::*;

    #[test]
    fn test_feigenbaum_delta_constant() {
        assert!((FEIGENBAUM_DELTA - 4.669_201_6).abs() < 0.001);
    }

    #[test]
    fn test_level_budget_decay() {
        assert_eq!(level_budget(2000, 0), 2000);
        let b1 = level_budget(2000, 1);
        assert!((420..=430).contains(&b1));
        let b2 = level_budget(2000, 2);
        assert!((90..=95).contains(&b2));
    }

    #[test]
    fn test_is_atomic() {
        assert!(!is_atomic(2000, 0));
        assert!(!is_atomic(2000, 1));
        assert!(is_atomic(2000, 4));
    }

    #[test]
    fn test_fractal_budget_struct() {
        let budget = FractalBudget::new(2000, 1);
        assert_eq!(budget.tokens(), level_budget(2000, 1));
        let child = budget.descend();
        assert_eq!(child.depth, 2);
        let parent = child.ascend();
        assert_eq!(parent.depth, 1);
    }

    #[test]
    fn test_fractal_task_node_and_graph() {
        let mut graph = FractalTaskGraph::new(2000);
        let root = graph.add_root("1", "Root Task");
        root.add_child("1.1", "Sub Task 1", 2000);
        root.add_child("1.2", "Sub Task 2", 2000);

        assert_eq!(graph.all_nodes().len(), 3);
        assert_eq!(graph.completion_ratio(), 0.0);
    }

    #[test]
    fn test_fractal_rate_limiter_escalation() {
        let mut limiter = FractalRateLimiter::new(1.0, 30.0, 80_000, 4);
        assert_eq!(limiter.current_level, 0);

        limiter.on_failure();
        assert_eq!(limiter.current_level, 1);
        assert!((limiter.current_pause().as_secs_f64() - FEIGENBAUM_DELTA).abs() < 0.01);

        limiter.on_success();
        assert_eq!(limiter.current_level, 0);
    }

    #[test]
    fn test_model_cascade_selection() {
        let cascade = default_cascade();
        // Depth 0 -> heaviest model (tier 3)
        let model0 = select_model_for_depth(0, &cascade, 0);
        assert_eq!(model0.alias, "reasoner");

        // Depth 3 -> lighter model
        let model3 = select_model_for_depth(3, &cascade, 0);
        assert_eq!(model3.alias, "quick");
    }

    #[test]
    fn test_fractal_compact_messages() {
        let mut msgs: Vec<String> = (0..100).map(|i| format!("msg_{i}")).collect();
        fractal_compact_messages(&mut msgs, 20);

        assert!(msgs.len() < 50);
        assert_eq!(msgs.last().unwrap(), "msg_99");
    }

    #[test]
    fn test_asymmetric_sibling_weighting() {
        let w0 = asymmetric_sibling_weight(0, 3);
        let w1 = asymmetric_sibling_weight(1, 3);
        let w2 = asymmetric_sibling_weight(2, 3);

        assert!(w0 > w1);
        assert!(w1 > w2);
        assert!(((w0 + w1 + w2) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_asymmetric_sibling_budget() {
        let b0 = asymmetric_sibling_budget(1000, 0, 3);
        let b1 = asymmetric_sibling_budget(1000, 1, 3);
        let b2 = asymmetric_sibling_budget(1000, 2, 3);

        assert!(b0 > b1);
        assert!(b1 > b2);
    }

    #[test]
    fn test_bifurcation_telemetry_report_and_ascii_tree() {
        use super::super::telemetry::BifurcationTelemetryReport;

        let mut graph = FractalTaskGraph::new(2000);
        let root = graph.add_root("1", "Root Task");
        root.add_child("1.1", "Sub Task 1", 2000);
        root.add_child("1.2", "Sub Task 2", 2000);

        let report = BifurcationTelemetryReport::from_graph(&graph);
        assert_eq!(report.total_nodes, 3);
        assert_eq!(report.max_depth_reached, 1);

        let ascii = report.render_ascii_tree(&graph);
        assert!(ascii.contains("Fractal Task Tree"));
        assert!(ascii.contains("id=1"));
        assert!(ascii.contains("id=1.1"));
    }

    #[test]
    fn test_dynamic_max_depth_and_atomic_check() {
        assert_eq!(dynamic_max_depth(5_000), 4);
        assert_eq!(dynamic_max_depth(10_000), 4);
        assert!(dynamic_max_depth(50_000) > 4);
        assert!(is_atomic_dynamic(2000, 4));
    }

    #[test]
    fn test_jittered_pause() {
        let limiter = FractalRateLimiter::new(1.0, 30.0, 80_000, 4);
        let p_normal = limiter.current_pause();
        let p_jitter = limiter.current_pause_with_jitter(0.5);

        assert!(p_jitter.as_secs_f64() > 0.0);
        assert!((p_jitter.as_secs_f64() - p_normal.as_secs_f64()).abs() < 1.0);
    }
}


