//! Plugin utilities
//!
//! This module provides functions for managing plugins, extensions,
//! and third-party integrations throughout the application.

use std::collections::HashMap;
use std::path::PathBuf;

/// Plugin metadata
#[derive(Debug, Clone)]
pub struct PluginMetadata {
    /// Plugin name
    pub name: String,
    /// Plugin version
    pub version: String,
    /// Plugin author
    pub author: String,
    /// Plugin description
    pub description: String,
    /// Plugin dependencies
    pub dependencies: Vec<String>,
    /// Plugin entry point
    pub entry_point: String,
    /// Plugin type
    pub plugin_type: PluginType,
    /// Plugin status
    pub status: PluginStatus,
}

/// Plugin type enumeration
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PluginType {
    /// Command plugin
    Command,
    /// Tool plugin
    Tool,
    /// Provider plugin
    Provider,
    /// Integration plugin
    Integration,
    /// Custom plugin
    Custom,
}

/// Plugin status enumeration
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginStatus {
    /// Plugin is installed and active
    Active,
    /// Plugin is installed but disabled
    Disabled,
    /// Plugin is not installed
    NotInstalled,
    /// Plugin has errors
    Error,
    /// Plugin is being updated
    Updating,
}

/// Plugin information
#[derive(Debug, Clone)]
pub struct PluginInfo {
    /// Plugin ID
    pub id: String,
    /// Plugin metadata
    pub metadata: PluginMetadata,
    /// Plugin path
    pub path: PathBuf,
    /// Plugin configuration
    pub config: HashMap<String, String>,
}

impl PluginInfo {
    /// Create a new plugin info
    pub fn new(id: String, metadata: PluginMetadata, path: PathBuf) -> Self {
        Self {
            id,
            metadata,
            path,
            config: HashMap::new(),
        }
    }

    /// Get the plugin display name
    pub fn display_name(&self) -> String {
        format!("{} ({})", self.metadata.name, self.metadata.version)
    }

    /// Check if plugin is active
    pub fn is_active(&self) -> bool {
        matches!(self.metadata.status, PluginStatus::Active)
    }

    /// Check if plugin is installed
    pub fn is_installed(&self) -> bool {
        matches!(self.metadata.status, PluginStatus::Active | PluginStatus::Disabled)
    }

    /// Check if plugin can be loaded
    pub fn can_load(&self) -> bool {
        matches!(self.metadata.status, PluginStatus::Active | PluginStatus::Disabled)
    }
}

/// Plugin manager
pub struct PluginManager {
    /// Registered plugins
    plugins: HashMap<String, PluginInfo>,
    /// Plugin directories
    plugin_dirs: Vec<PathBuf>,
}

impl PluginManager {
    /// Create a new plugin manager
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
            plugin_dirs: Vec::new(),
        }
    }

    /// Add a plugin directory to scan
    pub fn add_plugin_dir(&mut self, dir: PathBuf) {
        self.plugin_dirs.push(dir);
    }

    /// Register a plugin
    pub fn register_plugin(&mut self, plugin: PluginInfo) -> Result<(), String> {
        if self.plugins.contains_key(&plugin.id) {
            return Err(format!(
                "Plugin '{}' is already registered",
                plugin.id
            ));
        }

        self.plugins.insert(plugin.id.clone(), plugin);
        Ok(())
    }

    /// Get a plugin by ID
    pub fn get_plugin(&self, id: &str) -> Option<&PluginInfo> {
        self.plugins.get(id)
    }

    /// Get all registered plugins
    pub fn get_all_plugins(&self) -> Vec<&PluginInfo> {
        self.plugins.values().collect()
    }

    /// Get plugins by type
    pub fn get_plugins_by_type(&self, plugin_type: PluginType) -> Vec<&PluginInfo> {
        self.plugins
            .values()
            .filter(|p| p.metadata.plugin_type == plugin_type)
            .collect()
    }

    /// Get active plugins
    pub fn get_active_plugins(&self) -> Vec<&PluginInfo> {
        self.plugins
            .values()
            .filter(|p| p.is_active())
            .collect()
    }

    /// Get disabled plugins
    pub fn get_disabled_plugins(&self) -> Vec<&PluginInfo> {
        self.plugins
            .values()
            .filter(|p| matches!(p.metadata.status, PluginStatus::Disabled))
            .collect()
    }

    /// Enable a plugin
    pub fn enable_plugin(&mut self, id: &str) -> Result<(), String> {
        let plugin = self
            .plugins
            .get_mut(id)
            .ok_or_else(|| format!("Plugin '{}' not found", id))?;

        if !plugin.is_installed() {
            return Err(format!("Plugin '{}' is not installed", id));
        }

        plugin.metadata.status = PluginStatus::Active;
        Ok(())
    }

    /// Disable a plugin
    pub fn disable_plugin(&mut self, id: &str) -> Result<(), String> {
        let plugin = self
            .plugins
            .get_mut(id)
            .ok_or_else(|| format!("Plugin '{}' not found", id))?;

        if !plugin.is_installed() {
            return Err(format!("Plugin '{}' is not installed", id));
        }

        plugin.metadata.status = PluginStatus::Disabled;
        Ok(())
    }

    /// Uninstall a plugin
    pub fn uninstall_plugin(&mut self, id: &str) -> Result<(), String> {
        if !self.plugins.contains_key(id) {
            return Err(format!("Plugin '{}' not found", id));
        }

        self.plugins.remove(id);
        Ok(())
    }

    /// Reload a plugin
    pub fn reload_plugin(&mut self, id: &str) -> Result<(), String> {
        let plugin = self
            .plugins
            .get_mut(id)
            .ok_or_else(|| format!("Plugin '{}' not found", id))?;

        plugin.metadata.status = PluginStatus::Updating;

        // Simulate reload (in real implementation, this would actually reload the plugin)
        std::thread::sleep(std::time::Duration::from_millis(100));

        plugin.metadata.status = PluginStatus::Active;
        Ok(())
    }

    /// Update plugin metadata
    pub fn update_metadata(&mut self, id: &str, metadata: PluginMetadata) -> Result<(), String> {
        let plugin = self
            .plugins
            .get_mut(id)
            .ok_or_else(|| format!("Plugin '{}' not found", id))?;

        plugin.metadata = metadata;
        Ok(())
    }

    /// Get plugin count
    pub fn plugin_count(&self) -> usize {
        self.plugins.len()
    }

    /// Check if plugin is registered
    pub fn is_registered(&self, id: &str) -> bool {
        self.plugins.contains_key(id)
    }

    /// Get plugin statistics
    pub fn get_statistics(&self) -> PluginStatistics {
        PluginStatistics {
            total: self.plugins.len(),
            active: self.get_active_plugins().len(),
            disabled: self.get_disabled_plugins().len(),
            by_type: self.get_statistics_by_type(),
        }
    }

    /// Get statistics by plugin type
    fn get_statistics_by_type(&self) -> HashMap<PluginType, usize> {
        let mut stats = HashMap::new();

        for plugin in self.plugins.values() {
            *stats.entry(plugin.metadata.plugin_type.clone()).or_insert(0) += 1;
        }

        stats
    }
}

/// Plugin statistics
#[derive(Debug, Clone)]
pub struct PluginStatistics {
    /// Total plugin count
    pub total: usize,
    /// Active plugin count
    pub active: usize,
    /// Disabled plugin count
    pub disabled: usize,
    /// Statistics by plugin type
    pub by_type: HashMap<PluginType, usize>,
}

impl PluginStatistics {
    /// Calculate activation rate
    pub fn activation_rate(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            (self.active as f64 / self.total as f64) * 100.0
        }
    }
}

/// Create default plugin manager
pub fn create_default_plugin_manager() -> PluginManager {
    let mut manager = PluginManager::new();

    // Add default plugin directories
    let home = std::env::var("HOME").unwrap_or_else(|_| String::new());
    let plugin_dir = PathBuf::from(home).join(".claw").join("plugins");
    manager.add_plugin_dir(plugin_dir);

    manager
}

/// Validate plugin metadata
pub fn validate_plugin_metadata(metadata: &PluginMetadata) -> Result<(), String> {
    if metadata.name.is_empty() {
        return Err("Plugin name cannot be empty".to_string());
    }

    if metadata.version.is_empty() {
        return Err("Plugin version cannot be empty".to_string());
    }

    if metadata.author.is_empty() {
        return Err("Plugin author cannot be empty".to_string());
    }

    if metadata.description.is_empty() {
        return Err("Plugin description cannot be empty".to_string());
    }

    if metadata.entry_point.is_empty() {
        return Err("Plugin entry point cannot be empty".to_string());
    }

    // Validate plugin type
    match metadata.plugin_type {
        PluginType::Command | PluginType::Tool | PluginType::Provider
        | PluginType::Integration | PluginType::Custom => {}
    }

    Ok(())
}

/// Check if plugin path is valid
pub fn validate_plugin_path(path: &PathBuf) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("Plugin path does not exist: {}", path.display()));
    }

    if !path.is_dir() {
        return Err(format!(
            "Plugin path is not a directory: {}",
            path.display()
        ));
    }

    // Check for common plugin files
    let has_manifest = path.join("plugin.json").exists();
    let has_rust = path.join("Cargo.toml").exists();

    if !has_manifest && !has_rust {
        return Err(format!(
            "Invalid plugin directory: {} (missing plugin.json or Cargo.toml)",
            path.display()
        ));
    }

    Ok(())
}

/// Scan for plugins in directories
pub fn scan_plugin_directories(directories: &[PathBuf]) -> Vec<PluginMetadata> {
    let mut plugins = Vec::new();

    for dir in directories {
        if !dir.exists() {
            continue;
        }

        // Check for plugin.json files
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if let Some(manifest) = path.join("plugin.json").to_str() {
                        if let Ok(metadata) = load_plugin_manifest(manifest) {
                            plugins.push(metadata);
                        }
                    }
                }
            }
        }
    }

    plugins
}

/// Load plugin manifest
pub fn load_plugin_manifest(manifest_path: &str) -> Result<PluginMetadata, String> {
    let content = std::fs::read_to_string(manifest_path)
        .map_err(|e| format!("Failed to read plugin manifest: {}", e))?;

    let manifest: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse plugin manifest: {}", e))?;

    // Extract plugin metadata from manifest
    let name = manifest
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or("Plugin name missing")?
        .to_string();

    let version = manifest
        .get("version")
        .and_then(|v| v.as_str())
        .ok_or("Plugin version missing")?
        .to_string();

    let author = manifest
        .get("author")
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown")
        .to_string();

    let description = manifest
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let entry_point = manifest
        .get("entry_point")
        .and_then(|v| v.as_str())
        .ok_or("Plugin entry point missing")?
        .to_string();

    // Parse plugin type
    let plugin_type = manifest
        .get("type")
        .and_then(|v| v.as_str())
        .and_then(|t| match t {
            "command" => Some(PluginType::Command),
            "tool" => Some(PluginType::Tool),
            "provider" => Some(PluginType::Provider),
            "integration" => Some(PluginType::Integration),
            "custom" => Some(PluginType::Custom),
            _ => None,
        })
        .unwrap_or(PluginType::Custom);

    let dependencies = manifest
        .get("dependencies")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect()
        })
        .unwrap_or_default();

    Ok(PluginMetadata {
        name,
        version,
        author,
        description,
        dependencies,
        entry_point,
        plugin_type,
        status: PluginStatus::Active,
    })
}

/// Create a default plugin metadata
pub fn create_default_metadata(name: &str) -> PluginMetadata {
    PluginMetadata {
        name: name.to_string(),
        version: "0.1.0".to_string(),
        author: "Unknown".to_string(),
        description: format!("Plugin {}", name),
        dependencies: Vec::new(),
        entry_point: "main".to_string(),
        plugin_type: PluginType::Custom,
        status: PluginStatus::Active,
    }
}

/// Check plugin compatibility
pub fn check_plugin_compatibility(
    plugin_version: &str,
    required_version: &str,
) -> Result<(), String> {
    let plugin_parts: Vec<&str> = plugin_version.split('.').collect();
    let required_parts: Vec<&str> = required_version.split('.').collect();

    // Check major version
    let plugin_major: u32 = plugin_parts
        .get(0)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    let required_major: u32 = required_parts
        .get(0)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    if plugin_major != required_major {
        return Err(format!(
            "Version mismatch: plugin is {}, required {}",
            plugin_version, required_version
        ));
    }

    Ok(())
}
