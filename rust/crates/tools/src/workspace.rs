use std::path::{Path, PathBuf};

/// Environment variable name for the workspace root override.
const WORKSPACE_ROOT_ENV: &str = "CLAW_WORKSPACE_ROOT";

/// Marker directories used for anchor discovery, in priority order.
const ANCHOR_MARKERS: &[&str] = &[".claw", ".git"];

/// Resolve the canonical workspace root.
///
/// Priority:
/// 1. `CLAW_WORKSPACE_ROOT` env var (set by launcher or parent process)
/// 2. Walk up from `cwd` looking for `.claw/` directory
/// 3. Walk up from `cwd` looking for `.git/` directory
/// 4. Fall back to `cwd` itself
///
/// This function is intentionally NOT cached so that tests which change
/// `current_dir` or environment variables get correct results.
#[must_use]
pub fn workspace_root() -> PathBuf {
    // 1. Explicit env var takes priority
    if let Ok(explicit) = std::env::var(WORKSPACE_ROOT_ENV) {
        let path = PathBuf::from(&explicit);
        if path.is_absolute() && path.is_dir() {
            return path;
        }
    }

    // 2 & 3. Walk up from cwd looking for marker directories
    if let Ok(cwd) = std::env::current_dir() {
        for marker in ANCHOR_MARKERS {
            if let Some(root) = walk_up_for_marker(&cwd, marker) {
                return root;
            }
        }
        // 4. Fall back to cwd
        return cwd;
    }

    // Last resort
    PathBuf::from(".")
}

/// Resolve a potentially-relative path against the workspace root.
///
/// - Absolute paths are returned unchanged.
/// - Relative paths are joined to the workspace root.
/// - Home-dir prefix `~/` is expanded to the user's home directory.
#[must_use]
pub fn resolve_path(path: &str) -> PathBuf {
    let path = path.trim();

    // Expand ~ prefix
    if path == "~" {
        if let Some(home) = home_dir() {
            return home;
        }
    }
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = home_dir() {
            return home.join(rest);
        }
    }

    let p = Path::new(path);
    if p.is_absolute() {
        return p.to_path_buf();
    }

    workspace_root().join(path)
}

/// Walk up from `start` looking for a directory containing `marker`.
fn walk_up_for_marker(start: &Path, marker: &str) -> Option<PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        if current.join(marker).is_dir() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

/// Cross-platform home directory lookup.
fn home_dir() -> Option<PathBuf> {
    #[cfg(unix)]
    {
        std::env::var("HOME").ok().map(PathBuf::from)
    }
    #[cfg(windows)]
    {
        std::env::var("USERPROFILE").ok().map(PathBuf::from)
    }
    #[cfg(not(any(unix, windows)))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn resolve_path_absolute_unchanged() {
        let abs = "/usr/local/bin/foo";
        assert_eq!(resolve_path(abs), PathBuf::from(abs));
    }

    #[test]
    fn resolve_path_relative_joins_to_root() {
        let result = resolve_path("src/main.rs");
        assert!(result.is_absolute());
        assert!(result.ends_with("src/main.rs"));
    }

    #[test]
    fn resolve_path_tilde_expands() {
        if let Ok(home) = env::var("HOME") {
            let result = resolve_path("~/foo/bar");
            assert_eq!(result, PathBuf::from(home).join("foo/bar"));
        }
    }

    #[test]
    fn walk_up_finds_marker() {
        if let Ok(cwd) = env::current_dir() {
            let found = walk_up_for_marker(&cwd, ".git");
            if let Some(root) = found {
                assert!(root.join(".git").is_dir());
            }
        }
    }
}
