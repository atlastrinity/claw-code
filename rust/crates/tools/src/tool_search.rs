use crate::tool_types::*;
use crate::util::to_pretty_json;
use crate::registry::GlobalToolRegistry;


#[allow(clippy::needless_pass_by_value)]
pub(crate) fn run_tool_search(input: ToolSearchInput) -> Result<String, String> {
    let result = execute_tool_search(input);
    to_pretty_json(result)
}

pub(crate) fn execute_tool_search(input: ToolSearchInput) -> ToolSearchOutput {
    GlobalToolRegistry::builtin().search(&input.query, input.max_results.unwrap_or(5), None, None)
}

pub(crate) fn search_tool_specs(query: &str, max_results: usize, specs: &[SearchableToolSpec]) -> Vec<String> {
    let lowered = query.to_lowercase();
    if let Some(selection) = lowered.strip_prefix("select:") {
        return selection
            .split(',')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .filter_map(|wanted| {
                let wanted = canonical_tool_token(wanted);
                specs
                    .iter()
                    .find(|spec| canonical_tool_token(&spec.name) == wanted)
                    .map(|spec| spec.name.clone())
            })
            .take(max_results)
            .collect();
    }

    let mut required = Vec::new();
    let mut optional = Vec::new();
    for term in lowered.split_whitespace() {
        if let Some(rest) = term.strip_prefix('+') {
            if !rest.is_empty() {
                required.push(rest);
            }
        } else {
            optional.push(term);
        }
    }
    let terms = if required.is_empty() {
        optional.clone()
    } else {
        required.iter().chain(optional.iter()).copied().collect()
    };

    let mut scored = specs
        .iter()
        .filter_map(|spec| {
            let name = spec.name.to_lowercase();
            let canonical_name = canonical_tool_token(&spec.name);
            let normalized_description = normalize_tool_search_query(&spec.description);
            let haystack = format!(
                "{name} {} {canonical_name}",
                spec.description.to_lowercase()
            );
            let normalized_haystack = format!("{canonical_name} {normalized_description}");
            if required.iter().any(|term| !haystack.contains(term)) {
                return None;
            }

            let mut score = 0_i32;
            for term in &terms {
                let canonical_term = canonical_tool_token(term);
                if haystack.contains(term) {
                    score += 2;
                }
                if name == *term {
                    score += 8;
                }
                if name.contains(term) {
                    score += 4;
                }
                if canonical_name == canonical_term {
                    score += 12;
                }
                if normalized_haystack.contains(&canonical_term) {
                    score += 3;
                }
            }

            if score == 0 && !lowered.is_empty() {
                return None;
            }
            Some((score, spec.name.clone()))
        })
        .collect::<Vec<_>>();

    scored.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    scored
        .into_iter()
        .map(|(_, name)| name)
        .take(max_results)
        .collect()
}

pub(crate) fn normalize_tool_search_query(query: &str) -> String {
    query
        .trim()
        .split(|ch: char| ch.is_whitespace() || ch == ',')
        .filter(|term| !term.is_empty())
        .map(canonical_tool_token)
        .filter(|term| !term.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn canonical_tool_token(value: &str) -> String {
    let mut canonical = value
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .flat_map(char::to_lowercase)
        .collect::<String>();
    if let Some(stripped) = canonical.strip_suffix("tool") {
        canonical = stripped.to_string();
    }
    canonical
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_canonical_tool_token() {
        assert_eq!(canonical_tool_token("read_file_tool"), "readfile");
        assert_eq!(canonical_tool_token("Bash"), "bash");
        assert_eq!(canonical_tool_token("task-graph"), "taskgraph");
    }

    #[test]
    fn test_normalize_tool_search_query() {
        assert_eq!(
            normalize_tool_search_query("  read_file, write_file tool "),
            "readfile writefile"
        );
    }

    #[test]
    fn test_search_tool_specs_select_query() {
        let specs = vec![
            SearchableToolSpec {
                name: "read_file".to_string(),
                description: "Read a file from disk".to_string(),
            },
            SearchableToolSpec {
                name: "write_file".to_string(),
                description: "Write content to disk".to_string(),
            },
        ];

        let res = search_tool_specs("select: read_file_tool, writefile", 10, &specs);
        assert_eq!(res, vec!["read_file", "write_file"]);
    }

    #[test]
    fn test_search_tool_specs_keyword_matching() {
        let specs = vec![
            SearchableToolSpec {
                name: "read_file".to_string(),
                description: "Read contents of a file".to_string(),
            },
            SearchableToolSpec {
                name: "write_file".to_string(),
                description: "Write contents to a file".to_string(),
            },
            SearchableToolSpec {
                name: "bash".to_string(),
                description: "Execute a bash command in shell".to_string(),
            },
        ];

        let hits = search_tool_specs("read file", 5, &specs);
        assert_eq!(hits[0], "read_file");

        let required_hits = search_tool_specs("+bash shell", 5, &specs);
        assert_eq!(required_hits, vec!["bash"]);
    }
}

