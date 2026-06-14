//! AgentLens (lean) — thư viện lõi: hook receiver (FR-1) + JSONL tailer (FR-2)
//! + query API + UI server. Dùng chung cho bin CLI và app desktop (Tauri).

mod api;
mod hooks;
mod jsonl;
mod llm;
mod pricing;
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
    /// Báo cho client WebSocket khi có event mới (live update).
    pub events_tx: tokio::sync::broadcast::Sender<()>,
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

    // nạp bảng giá từ nguồn ngoài nếu cấu hình (AGENTLENS_PRICING_FILE/_URL)
    match pricing::refresh_from_source().await {
        Ok(n) if n > 0 => tracing::info!("Nạp {n} model giá từ nguồn ngoài"),
        Ok(_) => tracing::info!("Dùng bảng giá built-in (ước tính). Đặt AGENTLENS_PRICING_URL/_FILE để cập nhật."),
        Err(e) => tracing::warn!("Không nạp được bảng giá ngoài: {e} — dùng built-in"),
    }

    let conn = store::open(&db_path)?;
    store::recompute_costs(&conn)?; // cập nhật cost (kể cả row cũ) theo bảng giá hiện tại
    let (events_tx, _) = tokio::sync::broadcast::channel(64);
    let state = AppState {
        db: Arc::new(Mutex::new(conn)),
        extra_paths: Arc::new(Mutex::new(HashSet::new())),
        projects_dir: projects_dir.clone(),
        events_tx,
    };

    {
        let st = state.clone();
        tokio::spawn(async move { tailer::run(st).await });
    }

    // refresh bảng giá hàng ngày (nếu có nguồn ngoài) rồi tính lại cost
    {
        let st = state.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(24 * 3600)).await;
                if let Ok(n) = pricing::refresh_from_source().await {
                    if n > 0 {
                        if let Ok(c) = st.db.lock() {
                            let _ = store::recompute_costs(&c);
                        }
                        let _ = st.events_tx.send(());
                    }
                }
            }
        });
    }

    let app = Router::new()
        .route("/hook", post(hooks::receive))
        .route("/api/totals", get(api::totals))
        .route("/api/projects", get(api::projects))
        .route("/api/sessions", get(api::sessions))
        .route("/api/sessions/:id/events", get(api::session_events))
        .route("/api/sessions/:id/prompts", get(api::session_prompts))
        .route("/api/sessions/:id/models", get(api::session_models))
        .route("/api/sessions/:id/friction", get(api::session_friction))
        .route("/api/sessions/:id/errors", get(api::session_errors))
        .route("/api/sessions/:id/summarize", post(api::summarize))
        .route("/api/sessions/:id/tag", post(api::set_tag))
        .route("/api/summary", get(api::summary))
        .route("/api/tools", get(api::tools))
        .route("/api/files", get(api::hot_files))
        .route("/api/slowest", get(api::slowest))
        .route("/api/outcomes", get(api::outcomes))
        .route("/api/heatmap", get(api::heatmap))
        .route("/api/search", get(api::search))
        .route("/api/insights", get(api::list_insights))
        .route("/api/insights/analyze", post(api::analyze_insights))
        .route("/ws", get(api::ws))
        .route("/", get(ui::index))
        .with_state(state);

    let addr = default_addr();
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("AgentLens chạy tại http://{addr}  (db: {})", db_path.display());
    tracing::info!("Tailing transcripts trong: {}", projects_dir.display());
    axum::serve(listener, app).await?;
    Ok(())
}
