use std::time::Duration;
use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::{json, Value};

// ---------------------------------------------------------------------------
// RAG tools: retrieve_context + ingest_context
// ---------------------------------------------------------------------------

pub(crate) const RAG_QUERY_MAX_CHARS: usize = 12_000;
pub(crate) const RAG_INGEST_MAX_BYTES: usize = 512_000;
pub(crate) const RAG_HTTP_TIMEOUT_SECS: u64 = 30;
pub(crate) const RAG_INGEST_TIMEOUT_SECS: u64 = 60;

pub(crate) fn resolve_rag_base_url() -> Result<String, String> {
    if let Ok(url) = std::env::var("RAG_BASE_URL") {
        let trimmed = url.trim().to_string();
        if !trimmed.is_empty() {
            return Ok(trimmed);
        }
    }
    // Fall back to default local claw-rag-service port 8787
    Ok("http://127.0.0.1:8787".to_string())
}

#[derive(Debug, Deserialize)]
pub(crate) struct RetrieveContextInput {
    query: String,
    top_k: Option<u32>,
}

pub(crate) fn run_retrieve_context(input: RetrieveContextInput) -> Result<String, String> {
    let base_url = resolve_rag_base_url()?;

    let q = input.query.trim();
    if q.is_empty() {
        return Err("empty query".to_string());
    }
    if q.chars().count() > RAG_QUERY_MAX_CHARS {
        return Err(format!("query too long (max {RAG_QUERY_MAX_CHARS} chars)"));
    }

    let top_k = input.top_k.unwrap_or(8).clamp(1, 32);
    let base = base_url.trim_end_matches('/');
    let url = format!("{base}/v1/query");
    let body = json!({ "query": q, "top_k": top_k });

    let client = Client::builder()
        .timeout(Duration::from_secs(RAG_HTTP_TIMEOUT_SECS))
        .build()
        .map_err(|e| format!("HTTP client error: {e}"))?;

    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .map_err(|e| {
            if e.is_connect() {
                format!("RAG service unreachable at '{base_url}'. Ensure 'claw-rag-service' is running (e.g. 'cargo run -p claw-rag-service -- serve'). Error: {e}")
            } else {
                format!("RAG request failed: {e}")
            }
        })?;

    let status = resp.status();
    let text = resp.text().map_err(|e| format!("RAG response body: {e}"))?;

    if !status.is_success() {
        return Err(format!("RAG HTTP {status}: {text}"));
    }

    format_rag_response_for_model(&text)
}

pub(crate) fn format_rag_response_for_model(body: &str) -> Result<String, String> {
    let v: Value = serde_json::from_str(body).map_err(|e| format!("invalid RAG JSON: {e}"))?;
    let phase = v.get("phase").and_then(|x| x.as_str()).unwrap_or("unknown");
    let hits = v
        .get("hits")
        .and_then(|h| h.as_array())
        .ok_or_else(|| "missing hits array in RAG response".to_string())?;

    use std::fmt::Write;
    let mut out = String::new();
    writeln!(&mut out, "phase: {phase}").map_err(|e| e.to_string())?;

    if hits.is_empty() {
        writeln!(&mut out, "(no results)").map_err(|e| e.to_string())?;
        return Ok(out);
    }

    for (i, h) in hits.iter().enumerate() {
        let path = h.get("path").and_then(|x| x.as_str()).unwrap_or("");
        let snippet = h.get("snippet").and_then(|x| x.as_str()).unwrap_or("");
        let score = h.get("score").and_then(|x| x.as_f64());
        write!(&mut out, "{}. ", i + 1).map_err(|e| e.to_string())?;
        if let Some(s) = score {
            write!(&mut out, "score={s:.4} ").map_err(|e| e.to_string())?;
        }
        writeln!(&mut out, "path={path}").map_err(|e| e.to_string())?;
        for line in snippet.lines().take(32) {
            writeln!(&mut out, "    {line}").map_err(|e| e.to_string())?;
        }
        writeln!(&mut out).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

#[derive(Debug, Deserialize)]
pub(crate) struct IngestContextInput {
    path: String,
    content: String,
    #[allow(dead_code)]
    tags: Option<Vec<String>>,
}

pub(crate) fn run_ingest_context(input: IngestContextInput) -> Result<String, String> {
    let base_url = resolve_rag_base_url()?;

    let path = input.path.trim();
    if path.is_empty() {
        return Err("empty path".to_string());
    }
    if input.content.trim().is_empty() {
        return Err("empty content".to_string());
    }
    if input.content.len() > RAG_INGEST_MAX_BYTES {
        return Err(format!(
            "content too large (max {RAG_INGEST_MAX_BYTES} bytes)"
        ));
    }

    let base = base_url.trim_end_matches('/');
    let url = format!("{base}/v1/ingest");
    let body = json!({
        "path": path,
        "content": input.content,
        "tags": input.tags.unwrap_or_default(),
    });

    let client = Client::builder()
        .timeout(Duration::from_secs(RAG_INGEST_TIMEOUT_SECS))
        .build()
        .map_err(|e| format!("HTTP client error: {e}"))?;

    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .map_err(|e| format!("RAG ingest failed: {e}"))?;

    let status = resp.status();
    let text = resp
        .text()
        .map_err(|e| format!("RAG ingest response: {e}"))?;

    if !status.is_success() {
        return Err(format!("RAG ingest HTTP {status}: {text}"));
    }

    // Parse and format the success response for the model
    let resp_json: Value = serde_json::from_str(&text).unwrap_or(json!({ "status": "ok" }));
    let chunks = resp_json
        .get("chunks")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let embeddings = resp_json
        .get("embeddings")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    Ok(format!(
        "Indexed content at path '{}' ({} bytes, {} chunks, {} embeddings). Content is now searchable via retrieve_context.",
        path,
        input.content.len(),
        chunks,
        embeddings
    ))
}
