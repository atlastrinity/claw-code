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

    /// Inspect assistant output text to detect simulated/imitated tool calls in plain text.
    ///
    /// Returns `Err(GrishaExecutionError)` with `GRISHA_SIM_004` if the assistant printed
    /// faux invocation text (e.g. `[Асистент викликав інструмент...]` or raw JSON tool calls in text)
    /// without sending an actual API tool_use.
    pub fn check_assistant_text_for_simulated_tool_call(text: &str) -> Result<(), GrishaExecutionError> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Ok(());
        }

        // 1. Textual markers imitating tool calls
        let fake_call_markers = [
            "[Асистент викликав інструмент",
            "[Ассистент вызвал инструмент",
            "[Викликано інструмент",
            "[Вызван инструмент",
            "[Assistant called tool",
            "[Assistant invoked tool",
            "[Called tool",
            "[Invoked tool",
            "[Tool call:",
            "[Tool use:",
            "[Action:",
        ];

        for marker in fake_call_markers {
            if trimmed.contains(marker) {
                return Err(GrishaExecutionError::new(
                    GrishaErrorCode::SimTextualToolCall,
                    format!("Textual Tool Call Imitation Detected: You printed plain text containing \"{}\" instead of invoking the actual API tool via structured function calling.", marker),
                    "1. Textual descriptions of tool calls are plain chat text and are NEVER executed by the system.\n2. You MUST invoke the real tool through the structured JSON function calling interface (tool_use / tool_calls API).\n3. Never output \"[Асистент викликав інструмент...]\" or markdown pseudo-tool calls as text. Dispatch real tool calls directly.",
                ));
            }
        }

        // 2. Raw tool call JSON payloads emitted as plain chat text
        let is_raw_taskgraph_json = (trimmed.contains(r#""operation":"update_status""#)
            || trimmed.contains(r#""operation": "update_status""#)
            || trimmed.contains(r#""operation":"add""#)
            || trimmed.contains(r#""operation": "add""#))
            && (trimmed.contains(r#""nodes":"#) || trimmed.contains(r#""nodes": "#));

        if is_raw_taskgraph_json {
            return Err(GrishaExecutionError::new(
                GrishaErrorCode::SimTextualToolCall,
                "Textual Tool Call Imitation Detected: Raw TaskGraph JSON payload was output as chat text instead of a structured tool_use invocation.",
                "1. Invoke the TaskGraph tool directly using your tool execution interface.\n2. Do NOT print JSON tool payloads in the text block.",
            ));
        }

        Ok(())
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
    fn test_detects_ukrainian_and_russian_textual_tool_imitation() {
        let text1 = r#"[Асистент викликав інструмент 'TaskGraph' із аргументами: {"nodes":[{"content":"Open movie","id":"4","status":"completed"}],"operation":"update_status"}]"#;
        let res1 = GrishaSimulationDetector::check_assistant_text_for_simulated_tool_call(text1);
        assert!(res1.is_err());
        assert_eq!(res1.unwrap_err().code, GrishaErrorCode::SimTextualToolCall);

        let text2 = r#"[Ассистент вызвал инструмент 'bash' с аргументами: {"command":"ls"}]"#;
        let res2 = GrishaSimulationDetector::check_assistant_text_for_simulated_tool_call(text2);
        assert!(res2.is_err());
        assert_eq!(res2.unwrap_err().code, GrishaErrorCode::SimTextualToolCall);
    }

    #[test]
    fn test_detects_english_textual_tool_imitation() {
        let text1 = r#"[Assistant called tool 'TaskGraph' with arguments: {"operation":"update_status"}]"#;
        let res1 = GrishaSimulationDetector::check_assistant_text_for_simulated_tool_call(text1);
        assert!(res1.is_err());
        assert_eq!(res1.unwrap_err().code, GrishaErrorCode::SimTextualToolCall);

        let text2 = r#"I completed the task. {"operation":"update_status", "nodes":[{"id":"1", "status":"completed"}]}"#;
        let res2 = GrishaSimulationDetector::check_assistant_text_for_simulated_tool_call(text2);
        assert!(res2.is_err());
        assert_eq!(res2.unwrap_err().code, GrishaErrorCode::SimTextualToolCall);
    }

    #[test]
    fn test_allows_legitimate_commands_and_text() {
        let good_cmd1 = "cargo test --lib";
        assert!(GrishaSimulationDetector::check_bash_command(good_cmd1).is_ok());

        let good_cmd2 = "git status";
        assert!(GrishaSimulationDetector::check_bash_command(good_cmd2).is_ok());

        let good_cmd3 = "echo 'Hello world' > test.txt";
        assert!(GrishaSimulationDetector::check_bash_command(good_cmd3).is_ok());

        let good_cmd4 = "echo $PATH";
        assert!(GrishaSimulationDetector::check_bash_command(good_cmd4).is_ok());

        let good_text = "I have reviewed your project and found 3 components.";
        assert!(GrishaSimulationDetector::check_assistant_text_for_simulated_tool_call(good_text).is_ok());
    }
}
