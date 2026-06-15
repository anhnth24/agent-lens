//! AgentLens (lean) — thư viện lõi: hook receiver (FR-1) + JSONL tailer (FR-2)
//! + query API + UI server. Dùng chung cho bin CLI và app desktop (Tauri).

mod api;
mod hooks;
mod jsonl;
mod llm;
mod otel;
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

    // Bảng giá ngoài KHÔNG nạp ở đây để tránh chặn startup (mạng/proxy có thể chậm/lỗi):
    // server bind ngay với bảng built-in, việc nạp + recompute chạy trong task nền bên dưới.
    let conn = store::open(&db_path)?;
    store::recompute_costs(&conn)?; // cập nhật cost (kể cả row cũ) theo bảng giá hiện tại
    // khôi phục model LLM đã chọn ở footer (nếu có)
    if let Ok(Some(m)) = store::get_setting(&conn, "llm_model") {
        if !m.is_empty() {
            llm::set_model_override(Some(m));
        }
    }
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

    // Nạp bảng giá ngoài trong nền: chạy ngay lần đầu (không chặn startup) rồi lặp mỗi 24h.
    // Thành công > 0 model → tính lại cost + đẩy WS để UI cập nhật. Lỗi mạng chỉ log, vẫn dùng built-in.
    {
        let st = state.clone();
        tokio::spawn(async move {
            loop {
                match pricing::refresh_from_source().await {
                    Ok(n) if n > 0 => {
                        if let Ok(c) = st.db.lock() {
                            let _ = store::recompute_costs(&c);
                        }
                        let _ = st.events_tx.send(());
                        tracing::info!("Nạp {n} model giá từ nguồn ngoài");
                    }
                    Ok(_) => {}
                    Err(e) => tracing::warn!("Không nạp được bảng giá ngoài: {e} — dùng built-in"),
                }
                tokio::time::sleep(std::time::Duration::from_secs(24 * 3600)).await;
            }
        });
    }

    let app = Router::new()
        .route("/hook", post(hooks::receive))
        .route("/api/totals", get(api::totals))
        .route("/api/projects", get(api::projects))
        .route("/api/sessions", get(api::sessions))
        .route("/api/live", get(api::live))
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
        .route("/api/sequences", get(api::sequences))
        .route("/api/error-clusters", get(api::error_clusters))
        .route("/api/agents", get(api::agents))
        .route("/api/prompt-insights", get(api::prompt_insights))
        .route("/api/digest", get(api::digest))
        .route("/api/recovery", get(api::recovery))
        .route("/api/prompt-styles", get(api::prompt_styles))
        .route("/api/cache-advisor", get(api::cache_advisor))
        .route("/api/model-rightsizing", get(api::model_rightsizing))
        .route("/api/health-trend", get(api::health_trend))
        .route("/api/leaderboard", get(api::leaderboard))
        .route("/api/sessions/:id/otel", get(api::session_metric))
        .route("/v1/metrics", post(api::otlp_metrics))
        .route("/v1/logs", post(api::otlp_accept))
        .route("/v1/traces", post(api::otlp_accept))
        .route("/api/search", get(api::search))
        .route("/api/llm-status", get(api::llm_status))
        .route("/api/update-check", get(api::update_check))
        .route("/api/llm-model", post(api::set_llm_model))
        .route("/api/insights", get(api::list_insights))
        .route("/api/insights/analyze", post(api::analyze_insights))
        .route("/ws", get(api::ws))
        .route("/", get(ui::index))
        .with_state(state);

    let addr = default_addr();
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("AgentLens chạy tại http://{addr}  (db: {})", db_path.display());
    tracing::info!("Tailing transcripts trong: {}", projects_dir.display());
    if llm::is_enabled() {
        tracing::info!("LLM (FR-8) bật — backend: {}", llm::backend_label());
    } else {
        tracing::info!("LLM (FR-8) tắt — đặt ANTHROPIC_API_KEY hoặc `claude` + /login subscription");
    }
    axum::serve(listener, app).await?;
    Ok(())
}
