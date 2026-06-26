use std::path::Path;
use walkdir::WalkDir;

/// Builds a string representation of the file tree starting from `root`.
/// Traverses up to `max_depth` levels deep.
/// Ignores common hidden/build directories like .git, target, node_modules.
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

    // A simple presentation: just indent by depth
    for entry in entries {
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
            | "dist"
            | ".idea"
            | ".vscode"
            | "__pycache__"
        ) == false
    } else {
        true
    }
}
