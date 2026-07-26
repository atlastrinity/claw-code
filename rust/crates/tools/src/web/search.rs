use std::collections::BTreeSet;
use crate::tool_types::*;
use crate::web::fetch::{html_to_text, decode_html_entities};

pub(crate) fn extract_search_hits(html: &str) -> Vec<SearchHit> {
    if html.contains("compTitle options-toggle") {
        return extract_search_hits_yahoo(html);
    }

    let mut hits = Vec::new();
    let mut remaining = html;

    while let Some(anchor_start) = remaining.find("result__a") {
        let after_class = &remaining[anchor_start..];
        let Some(href_idx) = after_class.find("href=") else {
            remaining = &after_class[1..];
            continue;
        };
        let href_slice = &after_class[href_idx + 5..];
        let Some((url, rest)) = extract_quoted_value(href_slice) else {
            remaining = &after_class[1..];
            continue;
        };
        let Some(close_tag_idx) = rest.find('>') else {
            remaining = &after_class[1..];
            continue;
        };
        let after_tag = &rest[close_tag_idx + 1..];
        let Some(end_anchor_idx) = after_tag.find("</a>") else {
            remaining = &after_tag[1..];
            continue;
        };
        let title = html_to_text(&after_tag[..end_anchor_idx]);
        if let Some(decoded_url) = decode_duckduckgo_redirect(&url) {
            hits.push(SearchHit {
                title: title.trim().to_string(),
                url: decoded_url,
            });
        }
        remaining = &after_tag[end_anchor_idx + 4..];
    }

    hits
}

pub(crate) fn extract_search_hits_from_generic_links(html: &str) -> Vec<SearchHit> {
    let mut hits = Vec::new();
    let mut remaining = html;

    while let Some(anchor_start) = remaining.find("<a") {
        let after_anchor = &remaining[anchor_start..];
        let Some(href_idx) = after_anchor.find("href=") else {
            remaining = &after_anchor[2..];
            continue;
        };
        let href_slice = &after_anchor[href_idx + 5..];
        let Some((url, rest)) = extract_quoted_value(href_slice) else {
            remaining = &after_anchor[2..];
            continue;
        };
        let Some(close_tag_idx) = rest.find('>') else {
            remaining = &after_anchor[2..];
            continue;
        };
        let after_tag = &rest[close_tag_idx + 1..];
        let Some(end_anchor_idx) = after_tag.find("</a>") else {
            remaining = &after_anchor[2..];
            continue;
        };
        let title = html_to_text(&after_tag[..end_anchor_idx]);
        if title.trim().is_empty() {
            remaining = &after_tag[end_anchor_idx + 4..];
            continue;
        }
        let decoded_url = decode_duckduckgo_redirect(&url).unwrap_or(url);
        if decoded_url.starts_with("http://") || decoded_url.starts_with("https://") {
            hits.push(SearchHit {
                title: title.trim().to_string(),
                url: decoded_url,
            });
        }
        remaining = &after_tag[end_anchor_idx + 4..];
    }

    hits
}

pub(crate) fn extract_search_hits_yahoo(html: &str) -> Vec<SearchHit> {
    let mut hits = Vec::new();
    let mut remaining = html;

    while let Some(anchor_start) = remaining.find("compTitle options-toggle") {
        let after_class = &remaining[anchor_start..];
        let Some(href_idx) = after_class.find("href=") else {
            remaining = &after_class[1..];
            continue;
        };
        let href_slice = &after_class[href_idx + 5..];
        let Some((url, rest)) = extract_quoted_value(href_slice) else {
            remaining = &after_class[1..];
            continue;
        };

        let Some(h3_start) = rest.find("<h3") else {
            remaining = &after_class[1..];
            continue;
        };
        let Some(h3_end) = rest[h3_start..].find("</h3>") else {
            remaining = &after_class[1..];
            continue;
        };

        let title_html = &rest[h3_start..h3_start + h3_end];
        let title = html_to_text(title_html);

        let decoded_url = decode_yahoo_redirect(&url).unwrap_or(url);

        hits.push(SearchHit {
            title: title.trim().to_string(),
            url: decoded_url,
        });

        let Some(close_tag_idx) = rest.find("</a>") else {
            remaining = &rest[h3_start + h3_end..];
            continue;
        };
        remaining = &rest[close_tag_idx + 4..];
    }
    hits
}

pub(crate) fn decode_yahoo_redirect(url: &str) -> Option<String> {
    if let Some(start) = url.find("/RU=") {
        if let Some(end) = url[start + 4..].find("/R") {
            let encoded = &url[start + 4..start + 4 + end];
            let decoded = encoded
                .replace("%3a", ":")
                .replace("%3A", ":")
                .replace("%2f", "/")
                .replace("%2F", "/")
                .replace("%3f", "?")
                .replace("%3F", "?")
                .replace("%3d", "=")
                .replace("%3D", "=")
                .replace("%26", "&")
                .replace("%2b", "+")
                .replace("%2B", "+")
                .replace("%25", "%");
            return Some(decoded);
        }
    }
    None
}

pub(crate) fn extract_quoted_value(input: &str) -> Option<(String, &str)> {
    let quote = input.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let rest = &input[quote.len_utf8()..];
    let end = rest.find(quote)?;
    Some((rest[..end].to_string(), &rest[end + quote.len_utf8()..]))
}

pub(crate) fn decode_duckduckgo_redirect(url: &str) -> Option<String> {
    let decoded = html_entity_decode_url(url);
    let parsed = if decoded.starts_with("http://") || decoded.starts_with("https://") {
        reqwest::Url::parse(&decoded).ok()
    } else if decoded.starts_with("//") {
        reqwest::Url::parse(&format!("https:{decoded}")).ok()
    } else if decoded.starts_with('/') {
        reqwest::Url::parse(&format!("https://duckduckgo.com{decoded}")).ok()
    } else {
        return None;
    }?;

    let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
    if (host == "duckduckgo.com" || host.ends_with(".duckduckgo.com"))
        && (parsed.path() == "/l/" || parsed.path() == "/l")
    {
        for (key, value) in parsed.query_pairs() {
            if key == "uddg" {
                return Some(html_entity_decode_url(value.as_ref()));
            }
        }
    }

    if decoded.starts_with("http://") || decoded.starts_with("https://") {
        Some(decoded)
    } else {
        Some(parsed.to_string())
    }
}

pub(crate) fn html_entity_decode_url(url: &str) -> String {
    decode_html_entities(url)
}

pub(crate) fn host_matches_list(url: &str, domains: &[String]) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    let Some(host) = parsed.host_str() else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    domains.iter().any(|domain| {
        let normalized = normalize_domain_filter(domain);
        !normalized.is_empty() && (host == normalized || host.ends_with(&format!(".{normalized}")))
    })
}

pub(crate) fn normalize_domain_filter(domain: &str) -> String {
    let trimmed = domain.trim();
    let candidate = reqwest::Url::parse(trimmed)
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))
        .unwrap_or_else(|| trimmed.to_string());
    candidate
        .trim()
        .trim_start_matches('.')
        .trim_end_matches('/')
        .to_ascii_lowercase()
}

pub(crate) fn dedupe_hits(hits: &mut Vec<SearchHit>) {
    let mut seen = BTreeSet::new();
    hits.retain(|hit| seen.insert(hit.url.clone()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_domain_filter() {
        assert_eq!(normalize_domain_filter("  .Example.Com/ "), "example.com");
        assert_eq!(normalize_domain_filter("https://docs.rs/foo"), "docs.rs");
    }

    #[test]
    fn test_host_matches_list() {
        let allowed = vec!["github.com".to_string(), "rust-lang.org".to_string()];
        assert!(host_matches_list("https://github.com/rust-lang/rust", &allowed));
        assert!(host_matches_list("https://api.github.com/user", &allowed));
        assert!(!host_matches_list("https://notgithub.com/foo", &allowed));
        assert!(!host_matches_list("invalid_url", &allowed));
    }

    #[test]
    fn test_dedupe_hits() {
        let mut hits = vec![
            SearchHit {
                title: "Page A".to_string(),
                url: "https://example.com/a".to_string(),
            },
            SearchHit {
                title: "Page A Dup".to_string(),
                url: "https://example.com/a".to_string(),
            },
            SearchHit {
                title: "Page B".to_string(),
                url: "https://example.com/b".to_string(),
            },
        ];

        dedupe_hits(&mut hits);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].url, "https://example.com/a");
        assert_eq!(hits[1].url, "https://example.com/b");
    }
}




