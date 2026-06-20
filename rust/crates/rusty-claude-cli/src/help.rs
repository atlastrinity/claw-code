use crate::*;
use runtime::session_control::PRIMARY_SESSION_EXTENSION;
use std::io::{self, Write};

pub fn print_help_topic(
    topic: LocalHelpTopic,
    output_format: CliOutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir().unwrap_or_default();
    // For subsystem topics in JSON mode, delegate to the subsystem's usage JSON.
    if output_format == CliOutputFormat::Json {
        match topic {
            LocalHelpTopic::Agents => {
                let json = commands::handle_agents_slash_command_json(Some("--help"), &cwd)
                    .unwrap_or_else(
                        |_| serde_json::json!({"kind":"agents","action":"help","status":"error"}),
                    );
                println!("{}", serde_json::to_string_pretty(&json)?);
                return Ok(());
            }
            LocalHelpTopic::Skills => {
                let json = commands::handle_skills_slash_command_json(Some("--help"), &cwd)
                    .unwrap_or_else(
                        |_| serde_json::json!({"kind":"skills","action":"help","status":"error"}),
                    );
                println!("{}", serde_json::to_string_pretty(&json)?);
                return Ok(());
            }
            _ => {}
        }
    }
    match output_format {
        CliOutputFormat::Text => println!("{}", render_help_topic(topic)),
        CliOutputFormat::Json | CliOutputFormat::Ndjson => println!(
            "{}",
            serde_json::to_string_pretty(&render_help_topic_json(topic))?
        ),
    }
    Ok(())
}

pub fn print_help_to(out: &mut impl Write) -> io::Result<()> {
    writeln!(out, "claw v{VERSION}")?;
    writeln!(out)?;
    writeln!(out, "Usage:")?;
    writeln!(out, "  claw [--model MODEL] [--tools TOOL[,TOOL...]]")?;
    writeln!(out, "      Start the interactive REPL")?;
    writeln!(
        out,
        "  claw [--model MODEL] [--output-format text|json] prompt [--stdin] [TEXT]"
    )?;
    writeln!(
        out,
        "      Send one prompt and exit; reads stdin when TEXT is omitted"
    )?;
    writeln!(
        out,
        "  claw [--model MODEL] [--output-format text|json] TEXT"
    )?;
    writeln!(out, "      Shorthand non-interactive prompt mode")?;
    writeln!(
        out,
        "      Use `--` before TEXT when the prompt itself starts with '-' or '--'"
    )?;
    writeln!(
        out,
        "  claw --resume [SESSION.jsonl|session-id|latest] [/status] [/compact] [...]"
    )?;
    writeln!(
        out,
        "      Inspect or maintain a saved session without entering the REPL"
    )?;
    writeln!(out, "  claw help")?;
    writeln!(out, "      Alias for --help")?;
    writeln!(out, "  claw version")?;
    writeln!(out, "      Alias for --version")?;
    writeln!(out, "  claw status")?;
    writeln!(
        out,
        "      Show the current local workspace status snapshot"
    )?;
    writeln!(out, "  claw sandbox")?;
    writeln!(out, "      Show the current sandbox isolation snapshot")?;
    writeln!(out, "  claw doctor")?;
    writeln!(
        out,
        "      Diagnose local auth, config, workspace, and sandbox health"
    )?;
    writeln!(out, "  claw acp [serve]")?;
    writeln!(
        out,
        "      Show ACP/Zed editor integration status (currently unsupported; aliases: --acp, -acp)"
    )?;
    writeln!(out, "      Source of truth: {OFFICIAL_REPO_SLUG}")?;
    writeln!(
        out,
        "      Warning: do not `{DEPRECATED_INSTALL_COMMAND}` (deprecated stub)"
    )?;
    writeln!(out, "  claw dump-manifests [--manifests-dir PATH]")?;
    writeln!(out, "  claw bootstrap-plan")?;
    writeln!(out, "  claw agents")?;
    writeln!(out, "  claw mcp")?;
    writeln!(out, "  claw skills")?;
    writeln!(out, "  claw system-prompt [--cwd PATH] [--date YYYY-MM-DD]")?;
    writeln!(out, "  claw init")?;
    writeln!(
        out,
        "  claw export [PATH] [--session SESSION] [--output PATH]"
    )?;
    writeln!(
        out,
        "      Dump the latest (or named) session as markdown; writes to PATH or stdout"
    )?;
    writeln!(out)?;
    writeln!(out, "Flags:")?;
    writeln!(
        out,
        "  --model MODEL              Override the active model"
    )?;
    writeln!(
        out,
        "  --output-format FORMAT     Non-interactive output format: text or json (case-insensitive)"
    )?;
    writeln!(
        out,
        "                              CLAW_OUTPUT_FORMAT sets the default; flags override env"
    )?;
    writeln!(
        out,
        "                              Log env vars: CLAW_LOG or RUST_LOG"
    )?;
    writeln!(
        out,
        "  --cwd PATH, -C PATH, --directory PATH  Run as if launched from PATH"
    )?;
    writeln!(
        out,
        "  --compact                  Strip tool call details; print only the final assistant text (text mode only; useful for piping)"
    )?;
    writeln!(
        out,
        "  --permission-mode MODE     Set read-only, workspace-write, or danger-full-access"
    )?;
    writeln!(
        out,
        "  --preset PRESET             Load extra system prompt (audit, explain, implement)
  --dangerously-skip-permissions, --skip-permissions  Skip all permission checks"
    )?;
    writeln!(
        out,
        "  --tools TOOLS       Restrict enabled tools by canonical snake_case name or alias"
    )?;
    writeln!(out, "                              Examples: read, glob, web_fetch, WebFetch; status JSON exposes aliases")?;
    writeln!(
        out,
        "  --version, -V              Print version and build information locally"
    )?;
    writeln!(out)?;
    writeln!(out, "Interactive slash commands:")?;
    writeln!(out, "{}", render_slash_command_help_filtered(STUB_COMMANDS))?;
    writeln!(out)?;
    let resume_commands = resume_supported_slash_commands()
        .into_iter()
        .filter(|spec| !STUB_COMMANDS.contains(&spec.name))
        .map(|spec| match spec.argument_hint {
            Some(argument_hint) => format!("/{} {}", spec.name, argument_hint),
            None => format!("/{}", spec.name),
        })
        .collect::<Vec<_>>()
        .join(", ");
    writeln!(out, "Resume-safe commands: {resume_commands}")?;
    writeln!(out)?;
    writeln!(out, "Session shortcuts:")?;
    writeln!(
        out,
        "  REPL turns auto-save to .claw/sessions/<session-id>.{PRIMARY_SESSION_EXTENSION}"
    )?;
    writeln!(
        out,
        "  Use `{LATEST_SESSION_REFERENCE}` with --resume, /resume, or /session switch to target the newest saved session"
    )?;
    writeln!(
        out,
        "  Use /session list in the REPL to browse managed sessions"
    )?;
    writeln!(out, "Examples:")?;
    writeln!(out, "  claw --model claude-opus \"summarize this repo\"")?;
    writeln!(
        out,
        "  claw --output-format json prompt \"explain src/main.rs\""
    )?;
    writeln!(out, "  claw --compact \"summarize Cargo.toml\" | wc -l")?;
    writeln!(out, "  claw --tools read,glob \"summarize Cargo.toml\"")?;
    writeln!(out, "  claw --resume {LATEST_SESSION_REFERENCE}")?;
    writeln!(
        out,
        "  claw --resume {LATEST_SESSION_REFERENCE} /status /diff /export notes.txt"
    )?;
    writeln!(out, "  claw agents")?;
    writeln!(out, "  claw mcp show my-server")?;
    writeln!(out, "  claw /skills")?;
    writeln!(out, "  claw doctor")?;
    writeln!(out, "  source of truth: {OFFICIAL_REPO_URL}")?;
    writeln!(
        out,
        "  do not run `{DEPRECATED_INSTALL_COMMAND}` — it installs a deprecated stub"
    )?;
    writeln!(out, "  claw init")?;
    writeln!(out, "  claw export")?;
    writeln!(out, "  claw export conversation.md")?;
    Ok(())
}

pub fn print_help(output_format: CliOutputFormat) -> Result<(), Box<dyn std::error::Error>> {
    let mut buffer = Vec::new();
    print_help_to(&mut buffer)?;
    let message = String::from_utf8(buffer)?;
    match output_format {
        CliOutputFormat::Text => print!("{message}"),
        CliOutputFormat::Json | CliOutputFormat::Ndjson => {
            // #325: include structured command list in top-level help JSON
            let commands: Vec<serde_json::Value> = commands::slash_command_specs()
                .iter()
                .map(|spec| {
                    serde_json::json!({
                        "name": spec.name,
                        "summary": spec.summary,
                        "resume_supported": spec.resume_supported,
                    })
                })
                .collect();
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "kind": "help",
                    "action": "help",
                    "status": "ok",
                    "message": message,
                    "commands": commands,
                    "total_commands": commands.len(),
                }))?
            );
        }
    }
    Ok(())
}
