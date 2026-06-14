//! AgentLens (lean) — thư viện lõi: hook receiver (FR-1) + JSONL tailer (FR-2)
//! + query API + UI server. Dùng chung cho bin CLI và app desktop (Tauri).

mod api;
mod hooks;
mod jsonl;
mod llm;
mod store;
mod tailer;
mod ui;
mod util;

use axum::{
    routing::{get, post},
    Router,
};
use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{Arc, Mutex},
};

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Mutex<rusqlite::Connection>>,
    pub extra_paths: Arc<Mutex<HashSet<PathBuf>>>,
    pub projects_dir: PathBuf,
}

/// Địa chỉ bind mặc định (API + UI + /hook).
pub fn default_addr() -> String {
    std::env::var("AGENTLENS_ADDR").unwrap_or_else(|_| "127.0.0.1:8787".to_string())
}

/// Khởi tạo state + tailer + axum router và serve mãi mãi.
pub async fn run() -> anyhow::Result<()> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("không tìm thấy home dir"))?;

    let data_dir = std::env::var("AGENTLENS_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home.join(".agentlens"));
    std::fs::create_dir_all(&data_dir)?;
    let db_path = data_dir.join("agentlens.db");

    let projects_dir = std::env::var("AGENTLENS_PROJECTS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home.join(".claude").join("projects"));

    let conn = store::open(&db_path)?;
    let state = AppState {
        db: Arc::new(Mutex::new(conn)),
        extra_paths: Arc::new(Mutex::new(HashSet::new())),
        projects_dir: projects_dir.clone(),
    };

    {
        let st = state.clone();
        tokio::spawn(async move { tailer::run(st).await });
    }

    let app = Router::new()
        .route("/hook", post(hooks::receive))
        .route("/api/totals", get(api::totals))
        .route("/api/projects", get(api::projects))
        .route("/api/sessions", get(api::sessions))
        .route("/api/sessions/:id/events", get(api::session_events))
        .route("/api/sessions/:id/summarize", post(api::summarize))
        .route("/api/summary", get(api::summary))
        .route("/", get(ui::index))
        .with_state(state);

    let addr = default_addr();
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("AgentLens chạy tại http://{addr}  (db: {})", db_path.display());
    tracing::info!("Tailing transcripts trong: {}", projects_dir.display());
    axum::serve(listener, app).await?;
    Ok(())
}
