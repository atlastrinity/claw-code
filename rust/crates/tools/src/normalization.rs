pub fn canonical_allowed_tool_name(value: &str) -> String {
    let trimmed = value.trim().replace('-', "_");
    let mut output = String::new();
    let chars = trimmed.chars().collect::<Vec<_>>();
    for (index, ch) in chars.iter().copied().enumerate() {
        if ch == '_' || ch.is_whitespace() {
            output.push('_');
            continue;
        }
        let previous = index.checked_sub(1).and_then(|i| chars.get(i)).copied();
        let next = chars.get(index + 1).copied();
        if ch.is_ascii_uppercase()
            && index > 0
            && !output.ends_with('_')
            && (previous.is_some_and(|p| p.is_ascii_lowercase() || p.is_ascii_digit())
                || next.is_some_and(|n| n.is_ascii_lowercase()))
        {
            output.push('_');
        }
        output.push(ch.to_ascii_lowercase());
    }
    output.trim_matches('_').to_string()
}

pub fn coerce_tool_input(mut input: serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match input {
        Value::Object(ref mut map) => {
            for (_, value) in map.iter_mut() {
                *value = coerce_tool_input(value.clone());
            }
        }
        Value::Array(ref mut arr) => {
            for value in arr.iter_mut() {
                *value = coerce_tool_input(value.clone());
            }
        }
        Value::String(ref s) => {
            let s_trimmed = s.trim();
            if s_trimmed.eq_ignore_ascii_case("true") {
                return Value::Bool(true);
            } else if s_trimmed.eq_ignore_ascii_case("false") {
                return Value::Bool(false);
            } else if let Ok(n) = s_trimmed.parse::<u64>() {
                return Value::Number(n.into());
            } else if let Ok(n) = s_trimmed.parse::<i64>() {
                return Value::Number(n.into());
            }
        }
        _ => {}
    }
    input
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_coerce_input() {
        let input = json!({
            "timeout": "120000",
            "dry_run": "true",
            "nested": {
                "flag": "FALSE"
            },
            "name": "grep"
        });

        let expected = json!({
            "timeout": 120000,
            "dry_run": true,
            "nested": {
                "flag": false
            },
            "name": "grep"
        });

        assert_eq!(coerce_tool_input(input), expected);
    }
}
