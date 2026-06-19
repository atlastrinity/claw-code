import re
import sys

def get_blocks(filename):
    with open(filename) as f:
        lines = f.readlines()
    
    blocks = {}
    current_name = None
    start_line = None
    brace_count = 0
    in_block = False
    
    for i, line in enumerate(lines):
        if not in_block:
            match = re.match(r'^(?:pub\s+)?(?:async\s+)?(?:fn|struct|enum|impl)(?:\s+|<\s*)([a-zA-Z0-9_]+)', line)
            if match:
                current_name = match.group(1)
                start_line = i
                in_block = True
                brace_count = line.count('{') - line.count('}')
        else:
            brace_count += line.count('{') - line.count('}')
            if brace_count == 0:
                blocks[current_name] = (start_line, i)
                in_block = False
                
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
        print(f"{name}: {start+1} to {end+1}")
