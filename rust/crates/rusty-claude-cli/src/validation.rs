//! Validation utilities
//!
//! This module provides functions for validating various inputs,
//! configurations, and data structures throughout the application.

use std::path::PathBuf;

/// Validation result
#[derive(Debug, Clone, PartialEq)]
pub enum ValidationResult {
    /// Validation passed
    Valid,
    /// Validation failed with message
    Invalid(String),
    /// Validation failed with message and suggestions
    InvalidWithSuggestions {
        message: String,
        suggestions: Vec<String>,
    },
}

impl ValidationResult {
    /// Check if validation passed
    pub fn is_valid(&self) -> bool {
        matches!(self, ValidationResult::Valid)
    }

    /// Get the error message if validation failed
    pub fn get_error_message(&self) -> Option<&str> {
        match self {
            ValidationResult::Valid => None,
            ValidationResult::Invalid(msg) => Some(msg),
            ValidationResult::InvalidWithSuggestions { message, .. } => Some(message),
        }
    }

    /// Get suggestions if validation failed
    pub fn get_suggestions(&self) -> Vec<&str> {
        match self {
            ValidationResult::Valid => Vec::new(),
            ValidationResult::Invalid(_) => Vec::new(),
            ValidationResult::InvalidWithSuggestions { suggestions, .. } => {
                suggestions.iter().map(|s| s.as_str()).collect()
            }
        }
    }
}

/// Validate a path exists and is accessible
pub fn validate_path_exists(path: &PathBuf, require_write: bool) -> ValidationResult {
    if !path.exists() {
        return ValidationResult::Invalid(format!("Path does not exist: {}", path.display()));
    }

    if path.is_dir() && require_write {
        // Check if directory is writable
        if std::fs::metadata(path)
            .map(|m| m.permissions().readonly())
            .unwrap_or(true)
        {
            return ValidationResult::Invalid(format!(
                "Directory is not writable: {}",
                path.display()
            ));
        }
    }

    ValidationResult::Valid
}

/// Validate that a path is within allowed directories
pub fn validate_path_allowed(path: &PathBuf, allowed_dirs: &[PathBuf]) -> ValidationResult {
    let path_str = path.display().to_string();

    for allowed_dir in allowed_dirs {
        if path_str.starts_with(allowed_dir.display().to_string().as_str()) {
            return ValidationResult::Valid;
        }
    }

    ValidationResult::Invalid(format!(
        "Path is not within allowed directories: {}",
        path.display()
    ))
}

/// Validate file permissions
pub fn validate_file_permissions(path: &PathBuf, required_perms: u32) -> ValidationResult {
    let metadata = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) => {
            return ValidationResult::Invalid(format!("Cannot access file permissions: {}", e))
        }
    };

    use std::os::unix::fs::PermissionsExt;
    let actual_perms = metadata.permissions().mode();
    if (actual_perms & required_perms) != required_perms {
        return ValidationResult::Invalid(format!(
            "File permissions insufficient: expected {:o}, got {:o}",
            required_perms, actual_perms
        ));
    }

    ValidationResult::Valid
}

/// Validate environment variable is set
pub fn validate_env_var(var_name: &str) -> ValidationResult {
    match std::env::var(var_name) {
        Ok(value) => {
            if value.trim().is_empty() {
                ValidationResult::Invalid(format!(
                    "Environment variable '{}' is set but empty",
                    var_name
                ))
            } else {
                ValidationResult::Valid
            }
        }
        Err(_) => {
            ValidationResult::Invalid(format!("Environment variable '{}' is not set", var_name))
        }
    }
}

/// Validate that a string is not empty
pub fn validate_not_empty(value: &str, field_name: &str) -> ValidationResult {
    if value.trim().is_empty() {
        ValidationResult::Invalid(format!("{} cannot be empty", field_name))
    } else {
        ValidationResult::Valid
    }
}

/// Validate that a string matches a pattern
pub fn validate_pattern(value: &str, pattern: &str, field_name: &str) -> ValidationResult {
    if !value.contains(pattern) {
        ValidationResult::Invalid(format!(
            "{} must contain '{}': '{}'",
            field_name, pattern, value
        ))
    } else {
        ValidationResult::Valid
    }
}

/// Validate a number is within range
pub fn validate_range(value: i64, min: i64, max: i64, field_name: &str) -> ValidationResult {
    if value < min {
        return ValidationResult::Invalid(format!(
            "{} must be >= {}: got {}",
            field_name, min, value
        ));
    }

    if value > max {
        return ValidationResult::Invalid(format!(
            "{} must be <= {}: got {}",
            field_name, max, value
        ));
    }

    ValidationResult::Valid
}

/// Validate a string length is within range
pub fn validate_length(
    value: &str,
    min_len: usize,
    max_len: usize,
    field_name: &str,
) -> ValidationResult {
    let len = value.len();

    if len < min_len {
        return ValidationResult::Invalid(format!(
            "{} must be at least {} characters: got {}",
            field_name, min_len, len
        ));
    }

    if len > max_len {
        return ValidationResult::Invalid(format!(
            "{} must be at most {} characters: got {}",
            field_name, max_len, len
        ));
    }

    ValidationResult::Valid
}

/// Validate a port number
pub fn validate_port(port: u16) -> ValidationResult {
    if port == 0 {
        return ValidationResult::Invalid("Port cannot be 0".to_string());
    }

    ValidationResult::Valid
}

/// Validate a URL
pub fn validate_url(url: &str) -> ValidationResult {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return ValidationResult::Invalid(format!(
            "URL must start with 'http://' or 'https://': '{}'",
            url
        ));
    }

    // Basic URL structure check
    if !url.contains("://") {
        return ValidationResult::Invalid(format!("Invalid URL format: '{}'", url));
    }

    ValidationResult::Valid
}

/// Validate a configuration value
pub fn validate_config_value(value: &str, config_type: &str) -> ValidationResult {
    match config_type {
        "int" => {
            if value.parse::<i64>().is_ok() {
                ValidationResult::Valid
            } else {
                ValidationResult::Invalid(format!("Invalid integer value: '{}'", value))
            }
        }
        "float" => {
            if value.parse::<f64>().is_ok() {
                ValidationResult::Valid
            } else {
                ValidationResult::Invalid(format!("Invalid float value: '{}'", value))
            }
        }
        "bool" => {
            if value.eq_ignore_ascii_case("true") || value.eq_ignore_ascii_case("false") {
                ValidationResult::Valid
            } else {
                ValidationResult::Invalid(format!(
                    "Invalid boolean value: '{}'. Must be 'true' or 'false'",
                    value
                ))
            }
        }
        "path" => {
            if PathBuf::from(value).exists() {
                ValidationResult::Valid
            } else {
                ValidationResult::Invalid(format!("Invalid path: '{}'", value))
            }
        }
        _ => ValidationResult::Valid,
    }
}

/// Validate that a string matches a regex pattern
pub fn validate_regex(value: &str, pattern: &str, field_name: &str) -> ValidationResult {
    use regex::Regex;

    let regex = match Regex::new(pattern) {
        Ok(r) => r,
        Err(_) => {
            return ValidationResult::Invalid(format!("Invalid regex pattern: '{}'", pattern))
        }
    };

    if !regex.is_match(value) {
        ValidationResult::Invalid(format!(
            "{} does not match pattern '{}': '{}'",
            field_name, pattern, value
        ))
    } else {
        ValidationResult::Valid
    }
}

/// Validate a string is alphanumeric
pub fn validate_alphanumeric(value: &str, field_name: &str) -> ValidationResult {
    if !value.chars().all(|c| c.is_alphanumeric()) {
        ValidationResult::Invalid(format!("{} must be alphanumeric: '{}'", field_name, value))
    } else {
        ValidationResult::Valid
    }
}

/// Validate a string is a valid hostname
pub fn validate_hostname(hostname: &str) -> ValidationResult {
    // Check length
    if hostname.len() > 253 {
        return ValidationResult::Invalid(format!(
            "Hostname too long: {} characters",
            hostname.len()
        ));
    }

    // Check it contains only valid characters
    if !hostname
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '.')
    {
        return ValidationResult::Invalid(format!(
            "Hostname contains invalid characters: '{}'",
            hostname
        ));
    }

    // Check it doesn't start or end with hyphen
    if hostname.starts_with('-') || hostname.ends_with('-') {
        return ValidationResult::Invalid(format!(
            "Hostname cannot start or end with hyphen: '{}'",
            hostname
        ));
    }

    // Check it doesn't have consecutive dots
    if hostname.contains("..") {
        return ValidationResult::Invalid(format!(
            "Hostname cannot have consecutive dots: '{}'",
            hostname
        ));
    }

    ValidationResult::Valid
}

/// Validate a Git ref
pub fn validate_git_ref(ref_str: &str) -> ValidationResult {
    if ref_str.is_empty() {
        return ValidationResult::Invalid("Git ref cannot be empty".to_string());
    }

    // Check for valid ref formats
    let valid_patterns = [
        r"^[a-fA-F0-9]{40}$", // SHA-1
        r"^refs/heads/.*",
        r"^refs/tags/.*",
        r"^refs/remotes/.*",
        r"^[^/]+$", // Branch name
    ];

    for pattern in valid_patterns {
        if regex::Regex::new(pattern).unwrap().is_match(ref_str) {
            return ValidationResult::Valid;
        }
    }

    ValidationResult::Invalid(format!("Invalid Git ref format: '{}'", ref_str))
}

/// Validate a commit message
pub fn validate_commit_message(message: &str, max_length: usize) -> ValidationResult {
    // Check length
    if message.len() > max_length {
        return ValidationResult::Invalid(format!(
            "Commit message too long: {} characters (max {})",
            message.len(),
            max_length
        ));
    }

    // Check it's not empty
    if message.trim().is_empty() {
        return ValidationResult::Invalid("Commit message cannot be empty".to_string());
    }

    ValidationResult::Valid
}

/// Validate configuration file format
pub fn validate_config_file_format(content: &str) -> ValidationResult {
    // Check for valid JSON
    if serde_json::from_str::<serde_json::Value>(content).is_err() {
        return ValidationResult::Invalid("Invalid JSON format".to_string());
    }

    ValidationResult::Valid
}

/// Validate that required fields are present
pub fn validate_required_fields<T: std::fmt::Display>(
    fields: &[(&str, Option<T>)],
    field_names: &[&str],
) -> ValidationResult {
    for name in field_names {
        if !fields.iter().any(|(k, v)| k == name && v.is_some()) {
            return ValidationResult::Invalid(format!("Required field missing: '{}'", name));
        }
    }

    ValidationResult::Valid
}

/// Create a validation error with suggestions
pub fn create_validation_error(message: String, suggestions: Vec<String>) -> ValidationResult {
    ValidationResult::InvalidWithSuggestions {
        message,
        suggestions,
    }
}

/// Get validation error from result
pub fn get_validation_error(result: &Result<(), String>) -> ValidationResult {
    match result {
        Ok(()) => ValidationResult::Valid,
        Err(e) => ValidationResult::Invalid(e.to_string()),
    }
}

/// Validate that a string is a valid email
pub fn validate_email(email: &str) -> ValidationResult {
    let email_regex =
        regex::Regex::new(r"^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$").unwrap();

    if !email_regex.is_match(email) {
        ValidationResult::Invalid(format!("Invalid email format: '{}'", email))
    } else {
        ValidationResult::Valid
    }
}

/// Validate that a string is a valid UUID
pub fn validate_uuid(uuid: &str) -> ValidationResult {
    let uuid_regex = regex::Regex::new(
        r"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$",
    )
    .unwrap();

    if !uuid_regex.is_match(uuid) {
        ValidationResult::Invalid(format!("Invalid UUID format: '{}'", uuid))
    } else {
        ValidationResult::Valid
    }
}
