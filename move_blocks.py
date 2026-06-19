import re

blocks_to_move = {
    "parse_resume_args": (333, 383),
    "resume_command_can_absorb_token": (428, 433),
    "classify_session_lifecycle_for": (1021, 1023),
    "classify_session_lifecycle_from_panes": (1025, 1078),
    "discover_tmux_panes": (1080, 1097),
    "parse_tmux_pane_snapshots": (1099, 1117),
    "pane_path_matches_workspace": (1119, 1126),
    "is_idle_shell_command": (1128, 1134),
    "run_resume_command": (1252, 1718),
    "ResumeCommandOutcome": (752, 756)
}

with open("rust/crates/rusty-claude-cli/src/live_core.rs") as f:
    lines = f.readlines()

new_module_lines = [
    "use crate::*;\n",
    "use crate::session::*;\n",
    "use crate::status::*;\n",
    "use std::path::{Path, PathBuf};\n",
    "use std::process::Command;\n",
    "use std::io::Write;\n",
    "\n"
]

lines_to_keep = []
skip_ranges = []
for name, (start, end) in blocks_to_move.items():
    skip_ranges.append((start - 1, end - 1))
    new_module_lines.extend(lines[start - 1 : end])
    new_module_lines.append("\n")

for i, line in enumerate(lines):
    skip = False
    for start, end in skip_ranges:
        if start <= i <= end:
            skip = True
            break
    if not skip:
        lines_to_keep.append(line)

with open("rust/crates/rusty-claude-cli/src/session_orchestrator.rs", "w") as f:
    f.writelines(new_module_lines)

with open("rust/crates/rusty-claude-cli/src/live_core.rs", "w") as f:
    f.writelines(lines_to_keep)

