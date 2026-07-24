use std::path::Path;
use walkdir::WalkDir;

const MAX_FILE_TREE_LINES: usize = 120;
const MAX_FILE_TREE_CHARS: usize = 8_000;

/// Builds a string representation of the file tree starting from `root`.
/// Traverses up to `max_depth` levels deep.
/// Ignores common hidden/build directories like .git, target, node_modules.
/// Caps length to prevent blowing up model context window limits.
pub fn build_file_tree(root: &Path, max_depth: usize) -> String {
    if !root.exists() {
        return String::from("<directory does not exist>");
    }

    let mut tree_out = String::new();
    let root_name = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "workspace".to_string());
    
    tree_out.push_str(&format!("{}\n", root_name));

    // walkdir yields the root directory itself first, so we skip it
    let walker = WalkDir::new(root)
        .max_depth(max_depth)
        .into_iter()
        .filter_entry(is_not_ignored);

    let mut entries = Vec::new();
    for entry in walker.skip(1).filter_map(Result::ok) {
        entries.push(entry);
    }

    if entries.is_empty() {
        tree_out.push_str("  (empty)\n");
        return tree_out;
    }

    let total_entries = entries.len();
    let mut rendered_lines = 0;

    for entry in entries {
        if rendered_lines >= MAX_FILE_TREE_LINES || tree_out.len() >= MAX_FILE_TREE_CHARS {
            let omitted = total_entries.saturating_sub(rendered_lines);
            tree_out.push_str(&format!("  ... [{} directory entries omitted — size capped for prompt context]\n", omitted));
            break;
        }

        let depth = entry.depth();
        if depth == 0 {
            continue;
        }
        let indent = "  ".repeat(depth);
        
        let mut name = entry.file_name().to_string_lossy().into_owned();
        if entry.file_type().is_dir() {
            name.push('/');
        }
        
        tree_out.push_str(&format!("{}|- {}\n", indent, name));
        rendered_lines += 1;
    }

    tree_out
}

fn is_not_ignored(entry: &walkdir::DirEntry) -> bool {
    let name = entry.file_name().to_string_lossy();
    // Ignore common large or irrelevant directories
    if entry.file_type().is_dir() {
        matches!(
            name.as_ref(),
            ".git"
            | "target"
            | "node_modules"
            | "build"
            | ".build"
            | "dist"
            | ".idea"
            | ".vscode"
            | "__pycache__"
            | ".sandbox-tmp"
            | ".sandbox-home"
            | ".claw"
            | ".claw-rag"
            | ".pyscn"
            | ".port_sessions"
            | "vendor"
            | ".omx"
            | "audio_output"
            | "audio_output_test"
        ) == false
    } else {
        true
    }
}

