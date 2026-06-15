import re
with open("rust/crates/runtime/src/config_validate.rs", "r") as f:
    text = f.read()

replacement = """    FieldSpec {
        name: "agents",
        expected: FieldType::Object,
    },
    FieldSpec {
        name: "allowedTools",
        expected: FieldType::Array,
    },
    FieldSpec {
        name: "allowed_tools",
        expected: FieldType::Array,
    },
];"""

text = re.sub(r'    FieldSpec \{\n        name: "agents",\n        expected: FieldType::Object,\n    \},\n\];', replacement, text, flags=re.MULTILINE)

with open("rust/crates/runtime/src/config_validate.rs", "w") as f:
    f.write(text)
