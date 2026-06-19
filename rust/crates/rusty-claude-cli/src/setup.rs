use crate::CliOutputFormat;
use crate::setup_wizard;
use crate::init::initialize_repo;
use serde_json::json;

pub fn run_setup() -> Result<(), Box<dyn std::error::Error>> {
    setup_wizard::run_setup_wizard()
}

/// Starts a minimal Model Context Protocol server that exposes claw's
/// built-in tools over stdio.
///
/// Tool descriptors come from [`tools::mvp_tool_specs`] and calls are
/// dispatched through [`tools::execute_tool`], so this server exposes exactly
/// Read `.claw/worker-state.json` from the current working directory and print it.
/// This is the file-based worker observability surface: `push_event()` in `worker_boot.rs`
/// atomically writes state transitions here so external observers (clawhip, orchestrators)
/// can poll current `WorkerStatus` without needing an HTTP route on the opencode binary.
pub fn run_worker_state(output_format: CliOutputFormat) -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    let state_path = cwd.join(".claw").join("worker-state.json");
    if !state_path.exists() {
        return Err(format!(
            "no worker state file found at {path}\n  Hint: worker state is written by the interactive REPL or a non-interactive prompt.\n  Run:   claw               # start the REPL (writes state on first turn)\n  Or:    claw prompt <text> # run one non-interactive turn\n  Then rerun: claw state [--output-format json]",
            path = state_path.display()
        )
        .into());
    }
    let raw = std::fs::read_to_string(&state_path)?;
    match output_format {
        CliOutputFormat::Text => println!("{raw}"),
        CliOutputFormat::Json => {
            let _: serde_json::Value = serde_json::from_str(&raw)?;
            println!("{raw}");
        }
    }
    Ok(())
}

pub fn init_claude_md() -> Result<String, Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    Ok(initialize_repo(&cwd)?.render())
}

pub fn run_init(output_format: CliOutputFormat) -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    let report = initialize_repo(&cwd)?;
    let message = report.render();
    match output_format {
        CliOutputFormat::Text => println!("{message}"),
        CliOutputFormat::Json => println!(
            "{}",
            serde_json::to_string_pretty(&init_json_value(&report, &message))?
        ),
    }
    Ok(())
}

/// #142: emit first-class structured fields alongside the legacy `message`
/// string so claws can detect per-artifact state without substring matching.
pub fn init_json_value(report: &crate::init::InitReport, message: &str) -> serde_json::Value {
    use crate::init::InitStatus;
    let status = "ok";
    let already_initialized = report.artifacts_with_status(InitStatus::Created).is_empty()
        && report.artifacts_with_status(InitStatus::Updated).is_empty()
        && report.artifacts_with_status(InitStatus::Partial).is_empty();
    let hint = if already_initialized {
        "Workspace already initialised. Run `claw doctor` to verify health, or edit CLAUDE.md to customise guidance."
    } else {
        "Review and tailor CLAUDE.md to your project, then run `claw doctor` to verify the workspace."
    };
    json!({
        "kind": "init",
        "action": "init",
        "status": status,
        "already_initialized": already_initialized,
        "project_path": report.project_root.display().to_string(),
        "created": report.artifacts_with_status(InitStatus::Created),
        "updated": report.artifacts_with_status(InitStatus::Updated),
        "skipped": report.artifacts_with_status(InitStatus::Skipped),
        "partial": report.artifacts_with_status(InitStatus::Partial),
        "deferred": report.artifacts_with_status(InitStatus::Deferred),
        "artifacts": report.artifact_json_entries(),
        "hint": hint,
        "next_step": crate::init::InitReport::NEXT_STEP,
        "message": message,
    })
}
