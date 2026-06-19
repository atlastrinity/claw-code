use std::path::Path;
use std::fs;

fn resolve_sudo_password(cwd: &Path) -> Option<String> {
    // 1. Process environment
    if let Ok(value) = std::env::var("SUDO_PASSWORD") {
        if !value.is_empty() {
            println!("DEBUG: Found SUDO_PASSWORD in env: {}", value);
            return Some(value);
        }
    }

    // 2. .env file in cwd (minimal parser, mirrors api crate's parse_dotenv)
    let env_path = cwd.join(".env");
    let content = fs::read_to_string(env_path).ok()?;
    println!("DEBUG: Read .env file content: {:?}", content);
    
    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((raw_key, raw_value)) = line.split_once('=') else {
            continue;
        };
        let key = raw_key.trim().strip_prefix("export ").map_or_else(|| raw_key.trim(), str::trim);
        println!("DEBUG: Processing line, key: {:?}", key);
        if key != "SUDO_PASSWORD" {
            continue;
        }
        let trimmed = raw_value.trim();
        let unquoted = if (trimmed.starts_with('"') && trimmed.ends_with('"')
            || trimmed.starts_with('\'') && trimmed.ends_with('\''))
            && trimmed.len() >= 2
        {
            &trimmed[1..trimmed.len() - 1]
        } else {
            trimmed
        };
        println!("DEBUG: Found SUDO_PASSWORD: {:?}", unquoted);
        return Some(unquoted.to_string());
    }
    println!("DEBUG: No SUDO_PASSWORD found");
    None
}

fn rewrite_sudo_command(command: &str, cwd: &Path) -> Option<String> {
    // Check if command starts with "sudo "
    if !command.starts_with("sudo ") {
        return None;
    }
    
    // Check if user has sudo access
    if !has_sudo_access() {
        return None;
    }
    
    // Resolve password from env or .env file
    let password = resolve_sudo_password(cwd)?;
    println!("DEBUG: Using password: {:?}", password);
    
    // Rewrite the command
    let mut new_command = String::from("sudo -S ");
    new_command.push_str(command);
    Some(new_command)
}

fn has_sudo_access() -> bool {
    true
}

fn main() {
    // Create a temp directory
    let temp_dir = std::env::temp_dir().join("test_sudo_debug");
    fs::create_dir_all(&temp_dir).expect("create temp dir");
    
    println!("Test 1: No .env file");
    let result = rewrite_sudo_command("sudo ls", &temp_dir);
    println!("Result: {:?}\n", result);
    
    println!("Test 2: .env with OTHER_VAR=foo");
    fs::write(temp_dir.join(".env"), "OTHER_VAR=foo").expect("write .env");
    let result = rewrite_sudo_command("sudo ls", &temp_dir);
    println!("Result: {:?}\n", result);
    
    // Cleanup
    fs::remove_dir_all(&temp_dir).ok();
}
