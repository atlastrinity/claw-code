use claw_runtime::config_validate::{validate_config_file, ValidationResult};
use std::collections::BTreeMap;
use std::path::Path;

fn main() {
    let mut obj = BTreeMap::new();
    obj.insert("allowedTools".to_string(), claw_runtime::json::JsonValue::Array(vec![]));
    let res = validate_config_file(&obj, "{}", Path::new(".claw.json"));
    println!("{:?}", res.warnings);
    println!("{:?}", res.errors);
}
