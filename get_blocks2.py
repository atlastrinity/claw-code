import re

def get_blocks(filename):
    with open(filename) as f:
        text = f.read()

    blocks = {}
    pattern = r'^(?:pub\s+)?(?:async\s+)?(?:fn|struct|enum|impl)\s+([a-zA-Z0-9_]+)[^{;]*\{'
    for match in re.finditer(pattern, text, re.MULTILINE):
        name = match.group(1)
        start_pos = match.start()
        
        brace_count = 1
        pos = match.end()
        while pos < len(text) and brace_count > 0:
            if text[pos] == '{':
                brace_count += 1
            elif text[pos] == '}':
                brace_count -= 1
            pos += 1
            
        start_line = text.count('\n', 0, start_pos) + 1
        end_line = text.count('\n', 0, pos) + 1
        blocks[name] = (start_line, end_line)
        
    return blocks

blocks = get_blocks("rust/crates/rusty-claude-cli/src/live_core.rs")
for name in ["parse_resume_args", "resume_command_can_absorb_token", 
             "classify_session_lifecycle_for", "classify_session_lifecycle_from_panes", 
             "discover_tmux_panes", "parse_tmux_pane_snapshots", 
             "pane_path_matches_workspace", "is_idle_shell_command", 
             "run_resume_command", "SessionLifecycleSummary", 
             "SessionLifecycleKind", "TmuxPaneSnapshot", "ResumeCommandOutcome"]:
    if name in blocks:
        start, end = blocks[name]
        print(f"{name}: {start} to {end}")
