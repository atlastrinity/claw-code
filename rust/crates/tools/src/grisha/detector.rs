use super::types::{GrishaErrorCode, GrishaExecutionError};

/// Inspects tool execution and commands to detect simulation and faux execution.
pub struct GrishaSimulationDetector;

impl GrishaSimulationDetector {
    /// Inspect a bash command for analytical faux execution / simulation patterns.
    ///
    /// Returns `Err(GrishaExecutionError)` if the command is detected as synthetic output simulation.
    pub fn check_bash_command(command: &str) -> Result<(), GrishaExecutionError> {
        let trimmed = command.trim();

        // 1. Detect chained echo statements trying to simulate structured analytical output
        if Self::is_echo_simulation_chain(trimmed) {
            return Err(GrishaExecutionError::new(
                GrishaErrorCode::SimFakeDiagnostic,
                "Faux Execution / Simulation Detected: You executed synthetic 'echo' statements to fake analytical findings instead of inspecting real files or running genuine diagnostic tools.",
                "1. Use 'read_file' or 'grep_search' to inspect the actual codebase/files.\n2. Inspect real log files in ~/.claw/logs/ or session transcripts.\n3. Run real diagnostic commands (e.g. cargo check, git status, etc.).\n4. Never use 'echo' to fabricate inspection results.",
            ));
        }

        // 2. Detect single echo statements attempting to assert unverified system metrics
        if Self::is_single_echo_analysis(trimmed) {
            return Err(GrishaExecutionError::new(
                GrishaErrorCode::SimFauxExecution,
                "Faux Execution Detected: Single 'echo' command used to assert analysis or verification claims without real execution.",
                "Execute the actual inspection or verification tool directly rather than echoing conclusions.",
            ));
        }

        Ok(())
    }

    fn is_echo_simulation_chain(cmd: &str) -> bool {
        // Look for multiple echo statements chained with && or ;
        let parts: Vec<&str> = cmd.split("&&").flat_map(|s| s.split(';')).collect();
        if parts.len() < 2 {
            return false;
        }

        let mut echo_count = 0;
        let mut analytical_markers = 0;

        let markers = [
            "=== ",
            "Analysis",
            "Duplicate count",
            "Total injected tools",
            "Security posture",
            "Current environment",
            "Error handling status",
            "Current interaction",
            "Prompt engineering",
            "Context management",
            "Verified:",
            "Status: ✓",
            "Status: ⚠",
            "Status: ❌",
        ];

        for part in &parts {
            let p_trimmed = part.trim();
            if p_trimmed.starts_with("echo ") || p_trimmed.starts_with("printf ") {
                echo_count += 1;
                for marker in markers {
                    if p_trimmed.contains(marker) {
                        analytical_markers += 1;
                        break;
                    }
                }
            }
        }

        // If command is predominantly echo statements (>=2) and contains analytical markers
        echo_count >= 2 && analytical_markers >= 1
    }

    fn is_single_echo_analysis(cmd: &str) -> bool {
        let trimmed = cmd.trim();
        if !trimmed.starts_with("echo ") && !trimmed.starts_with("printf ") {
            return false;
        }

        let analytical_headers = [
            "echo \"=== Tool Invocation",
            "echo \"=== Tool Injection",
            "echo \"=== Security Policy",
            "echo \"=== Error Handling",
            "echo \"=== User Interaction",
            "echo \"=== Context Management",
            "echo \"=== Prompt Engineering",
            "echo \"=== Verification",
            "echo '=== Tool Invocation",
            "echo '=== Tool Injection",
            "echo '=== Security Policy",
            "echo '=== Error Handling",
            "echo '=== User Interaction",
            "echo '=== Context Management",
            "echo '=== Prompt Engineering",
            "echo '=== Verification",
        ];

        analytical_headers.iter().any(|h| trimmed.starts_with(h))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detects_echo_simulation_chain() {
        let bad_cmd = r#"echo "=== Tool Injection Analysis ===" && echo "Total injected tools in current session:" && echo "Duplicate count: 14 tools duplicated""#;
        let res = GrishaSimulationDetector::check_bash_command(bad_cmd);
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert_eq!(err.code, GrishaErrorCode::SimFakeDiagnostic);
    }

    #[test]
    fn test_detects_single_echo_analysis_header() {
        let bad_cmd = r#"echo "=== Security Policy Analysis === Current environment: Sandbox enabled: false""#;
        let res = GrishaSimulationDetector::check_bash_command(bad_cmd);
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert_eq!(err.code, GrishaErrorCode::SimFauxExecution);
    }

    #[test]
    fn test_allows_legitimate_commands() {
        let good_cmd1 = "cargo test --lib";
        assert!(GrishaSimulationDetector::check_bash_command(good_cmd1).is_ok());

        let good_cmd2 = "git status";
        assert!(GrishaSimulationDetector::check_bash_command(good_cmd2).is_ok());

        let good_cmd3 = "echo 'Hello world' > test.txt";
        assert!(GrishaSimulationDetector::check_bash_command(good_cmd3).is_ok());

        let good_cmd4 = "echo $PATH";
        assert!(GrishaSimulationDetector::check_bash_command(good_cmd4).is_ok());
    }
}
