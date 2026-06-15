import re

with open("rust/crates/runtime/src/config.rs", "r") as f:
    text = f.read()

# Add allowed_tools method
method_addition = """    #[must_use]
    pub fn allowed_tools(&self) -> Option<Vec<String>> {
        let tools = self.merged.get("allowedTools")
            .or_else(|| self.merged.get("allowed_tools"))?;
        
        if let JsonValue::Array(arr) = tools {
            let mut result = Vec::new();
            for item in arr {
                if let JsonValue::String(s) = item {
                    result.push(s.clone());
                }
            }
            Some(result)
        } else {
            None
        }
    }

    #[must_use]
    pub fn model(&self) -> Option<&str> {"""

text = text.replace("    #[must_use]\n    pub fn model(&self) -> Option<&str> {", method_addition)

# Update test string
text = text.replace(
    '"{\\n  \\"model\\": \\"opus\\",\\n  \\"allowedTools\\": [\\"Read\\"]\\n}\\n"',
    '"{\\n  \\"model\\": \\"opus\\",\\n  \\"unknownKey\\": [\\"Read\\"]\\n}\\n"'
)

# Update test assertion
text = text.replace(
    'rendered.contains("allowedTools"),\n            "warning should name the offending field',
    'rendered.contains("unknownKey"),\n            "warning should name the offending field'
)

with open("rust/crates/runtime/src/config.rs", "w") as f:
    f.write(text)
