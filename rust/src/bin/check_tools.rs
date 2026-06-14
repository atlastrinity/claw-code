use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;

fn main() {
    let content = fs::read_to_string(".claw.json").expect("Failed to read .claw.json");
    let config: Value = serde_json::from_str(&content).expect("Failed to parse JSON");
    
    let allowed_tools: Vec<String> = config["allowedTools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
        
    println!("Allowed tools in .claw.json: {}", allowed_tools.len());
    for tool in allowed_tools.iter().take(5) {
        println!("  - {}", tool);
    }
    println!("  ... and {} more", allowed_tools.len() - 5);
}
