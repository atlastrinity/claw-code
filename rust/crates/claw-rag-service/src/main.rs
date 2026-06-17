//! `claw-rag-service` — HTTP API + `ingest` subcommand.

use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    extract::State,
    http::StatusCode,
    response::Html,
    routing::{get, post},
    Json, Router,
};
use clap::{Parser, Subcommand};
use claw_rag_service::{
    chunk_and_embed_single, chunk_count, open_db, query_index, run_ingest, EmbedConfig,
    QueryRequest, QueryResponse,
};

#[derive(Parser)]
#[command(
    name = "claw-rag-service",
    about = "Workspace RAG index + HTTP query API"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run HTTP server (default when no subcommand).
    Serve(ServeArgs),
    /// Index a workspace into `SQLite` (calls embedding API).
    Ingest(IngestArgs),
}

#[derive(Parser)]
struct ServeArgs {
    #[arg(long, env = "CLAW_RAG_DB", default_value = ".claw-rag/index.sqlite")]
    db: PathBuf,
}

#[derive(Parser)]
struct IngestArgs {
    /// Workspace roots to ingest. Repeat `--workspace` to ingest multiple repos (cross-repo RAG).
    #[arg(short, long)]
    workspace: Vec<PathBuf>,
    #[arg(long, env = "CLAW_RAG_DB", default_value = ".claw-rag/index.sqlite")]
    db: PathBuf,
}

#[derive(Clone)]
struct AppState {
    db_path: PathBuf,
    client: reqwest::Client,
    cfg: EmbedConfig,
}

/// Single-page UI for phase 3 (served at `GET /`).
static INDEX_HTML: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/static/index.html"));

async fn ui_index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

fn rag_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(ui_index))
        .route("/health", get(|| async { "ok" }))
        .route("/v1/stats", get(stats))
        .route("/v1/query", post(query))
        .route("/v1/ingest", post(ingest_single))
        .with_state(state)
}

fn resolve_embed_config() -> Result<EmbedConfig, String> {
    if let Some(c) = EmbedConfig::mock_from_env() {
        return Ok(c);
    }
    EmbedConfig::from_env()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Load `.env` if present (walks up parent directories).
    // This is a convenience for local development; CI/production should set real env vars.
    let _ = dotenvy::dotenv();

    let cli = Cli::parse();

    if let Some(Cmd::Ingest(a)) = cli.command {
        let cfg = resolve_embed_config()?;
        let client = reqwest::Client::new();
        let st = run_ingest(&a.workspace, &a.db, &cfg, &client).await?;
        eprintln!(
            "ingest: files={} chunks={} embeddings={}",
            st.files_indexed, st.chunks_total, st.embeddings_written
        );
        return Ok(());
    }

    let db = if let Some(Cmd::Serve(s)) = cli.command {
        s.db
    } else {
        PathBuf::from(
            std::env::var("CLAW_RAG_DB").unwrap_or_else(|_| ".claw-rag/index.sqlite".into()),
        )
    };

    let cfg = resolve_embed_config()?;
    let state = Arc::new(AppState {
        db_path: db,
        client: reqwest::Client::new(),
        cfg,
    });

    let app = rag_router(state.clone());

    let port: u16 = std::env::var("CLAW_RAG_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8787);
    let host: std::net::IpAddr = std::env::var("CLAW_RAG_HOST")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));
    let addr = std::net::SocketAddr::from((host, port));
    eprintln!(
        "claw-rag-service db={} listen=http://{addr}",
        state.db_path.display()
    );
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn stats(State(state): State<Arc<AppState>>) -> Result<Json<serde_json::Value>, StatusCode> {
    let path = state.db_path.clone();
    if !path.is_file() {
        return Ok(Json(serde_json::json!({
            "chunks": 0,
            "phase": "1-sqlite-no-db"
        })));
    }
    let res = tokio::task::spawn_blocking(move || {
        let conn = open_db(&path).map_err(|_| ())?;
        chunk_count(&conn).map_err(|_| ())
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .map_err(|()| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({
        "chunks": res,
        "phase": "1-sqlite"
    })))
}

async fn query(
    State(state): State<Arc<AppState>>,
    Json(req): Json<QueryRequest>,
) -> Result<Json<QueryResponse>, (StatusCode, String)> {
    query_index(&state.db_path, &state.client, &state.cfg, &req)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

// ---------------------------------------------------------------------------
// POST /v1/ingest — incremental single-document ingest
// ---------------------------------------------------------------------------

#[derive(Debug, serde::Deserialize)]
struct IngestSingleRequest {
    path: String,
    content: String,
    #[serde(default)]
    #[allow(dead_code)]
    tags: Vec<String>,
}

async fn ingest_single(
    State(state): State<Arc<AppState>>,
    Json(req): Json<IngestSingleRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let path = req.path.trim().to_string();
    if path.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "empty path".into()));
    }
    if req.content.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "empty content".into()));
    }
    if req.content.len() > 512_000 {
        return Err((StatusCode::BAD_REQUEST, "content too large (max 512 KB)".into()));
    }

    let db_path = state.db_path.clone();
    let client = state.client.clone();
    let cfg = state.cfg.clone();
    let content = req.content;
    let response_path = path.clone();

    // `rusqlite::Connection` is !Send, so we run the whole ingest (DB + embedding)
    // inside `spawn_blocking` and use `block_on` for the async embedding calls.
    let handle = tokio::runtime::Handle::current();
    let result = tokio::task::spawn_blocking(move || {
        let conn = open_db(&db_path)?;
        handle.block_on(chunk_and_embed_single(&conn, &path, &content, &client, &cfg))
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("task join error: {e}")))?
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    Ok(Json(serde_json::json!({
        "status": "ok",
        "path": response_path,
        "chunks": result.chunks,
        "embeddings": result.embeddings,
    })))
}

#[cfg(test)]
mod tests {
    use super::INDEX_HTML;

    #[test]
    fn index_html_wires_api_paths() {
        assert!(INDEX_HTML.contains("/v1/stats"));
        assert!(INDEX_HTML.contains("/v1/query"));
    }
}
