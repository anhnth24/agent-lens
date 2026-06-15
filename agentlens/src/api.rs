//! Query API đọc SQLite cho UI review.

use crate::{llm, store, AppState};
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, Query, State,
    },
    http::StatusCode,
    response::Response,
    Json,
};
use serde_json::{json, Value};
use std::collections::HashMap;

type ApiResult = Result<Json<Value>, (StatusCode, String)>;

fn err<E: std::fmt::Display>(e: E) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

/// Đổi keyword range (today|7d|30d|90d|all) -> mốc 'from' ISO-8601 (server sinh, an toàn cho SQL).
fn range_from(q: &HashMap<String, String>) -> Option<String> {
    use chrono::{Duration, Utc};
    match q.get("range").map(|s| s.as_str()).unwrap_or("all") {
        "today" => Some(Utc::now().format("%Y-%m-%dT00:00:00").to_string()),
        "7d" => Some((Utc::now() - Duration::days(7)).to_rfc3339()),
        "30d" => Some((Utc::now() - Duration::days(30)).to_rfc3339()),
        "90d" => Some((Utc::now() - Duration::days(90)).to_rfc3339()),
        _ => None,
    }
}

pub async fn totals(
    State(state): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult {
    let from = range_from(&q);
    let conn = state.db.lock().unwrap();
    Ok(Json(store::totals(&conn, from.as_deref()).map_err(err)?))
}

pub async fn projects(
    State(state): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult {
    let from = range_from(&q);
    let conn = state.db.lock().unwrap();
    Ok(Json(store::projects(&conn, from.as_deref()).map_err(err)?))
}

pub async fn sessions(
    State(state): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult {
    let project = q.get("project").map(|s| s.as_str()).filter(|s| !s.is_empty());
    let from = range_from(&q);
    let conn = state.db.lock().unwrap();
    Ok(Json(store::sessions(&conn, project, from.as_deref()).map_err(err)?))
}

pub async fn session_events(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult {
    let after = q.get("after").map(|s| s.as_str()).filter(|s| !s.is_empty());
    let conn = state.db.lock().unwrap();
    Ok(Json(store::session_events(&conn, &id, after).map_err(err)?))
}

/// View Live: các session hoạt động trong `mins` phút gần nhất (mặc định 10).
pub async fn live(
    State(state): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult {
    use chrono::{Duration, Utc};
    let mins: i64 = q.get("mins").and_then(|s| s.parse().ok()).unwrap_or(10);
    let since = (Utc::now() - Duration::minutes(mins.clamp(1, 1440))).to_rfc3339();
    let project = q.get("project").map(|s| s.as_str()).filter(|s| !s.is_empty());
    let conn = state.db.lock().unwrap();
    Ok(Json(store::live_sessions(&conn, project, &since).map_err(err)?))
}

pub async fn summary(
    State(state): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult {
    let group_by = q.get("group_by").map(|s| s.as_str()).unwrap_or("project");
    let from = range_from(&q);
    let conn = state.db.lock().unwrap();
    Ok(Json(store::summary(&conn, group_by, from.as_deref()).map_err(err)?))
}

pub async fn tools(
    State(state): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult {
    let project = q.get("project").map(|s| s.as_str()).filter(|s| !s.is_empty());
    let from = range_from(&q);
    let conn = state.db.lock().unwrap();
    Ok(Json(store::tool_stats(&conn, project, from.as_deref()).map_err(err)?))
}

pub async fn session_prompts(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult {
    let conn = state.db.lock().unwrap();
    Ok(Json(store::prompt_breakdown(&conn, &id).map_err(err)?))
}

pub async fn session_models(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult {
    let conn = state.db.lock().unwrap();
    Ok(Json(store::session_models(&conn, &id).map_err(err)?))
}

pub async fn session_friction(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult {
    let conn = state.db.lock().unwrap();
    Ok(Json(store::friction(&conn, &id).map_err(err)?))
}

pub async fn session_errors(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult {
    let conn = state.db.lock().unwrap();
    Ok(Json(store::errors(&conn, &id).map_err(err)?))
}

pub async fn hot_files(
    State(state): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult {
    let project = q.get("project").map(|s| s.as_str()).filter(|s| !s.is_empty());
    let from = range_from(&q);
    let conn = state.db.lock().unwrap();
    Ok(Json(store::hot_files(&conn, project, from.as_deref()).map_err(err)?))
}

pub async fn slowest(
    State(state): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult {
    let project = q.get("project").map(|s| s.as_str()).filter(|s| !s.is_empty());
    let from = range_from(&q);
    let conn = state.db.lock().unwrap();
    Ok(Json(store::slowest(&conn, project, from.as_deref()).map_err(err)?))
}

pub async fn outcomes(
    State(state): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult {
    let project = q.get("project").map(|s| s.as_str()).filter(|s| !s.is_empty());
    let from = range_from(&q);
    let conn = state.db.lock().unwrap();
    Ok(Json(store::outcomes(&conn, project, from.as_deref()).map_err(err)?))
}

pub async fn heatmap(
    State(state): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult {
    let project = q.get("project").map(|s| s.as_str()).filter(|s| !s.is_empty());
    let from = range_from(&q);
    let conn = state.db.lock().unwrap();
    Ok(Json(store::heatmap(&conn, project, from.as_deref()).map_err(err)?))
}

pub async fn sequences(
    State(state): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult {
    let project = q.get("project").map(|s| s.as_str()).filter(|s| !s.is_empty());
    let from = range_from(&q);
    let conn = state.db.lock().unwrap();
    Ok(Json(store::sequences(&conn, project, from.as_deref()).map_err(err)?))
}

pub async fn session_metric(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult {
    let conn = state.db.lock().unwrap();
    Ok(Json(store::session_metric(&conn, &id).map_err(err)?))
}

pub async fn error_clusters(
    State(state): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult {
    let project = q.get("project").map(|s| s.as_str()).filter(|s| !s.is_empty());
    let from = range_from(&q);
    let conn = state.db.lock().unwrap();
    Ok(Json(store::error_clusters(&conn, project, from.as_deref()).map_err(err)?))
}

pub async fn agents(
    State(state): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult {
    let project = q.get("project").map(|s| s.as_str()).filter(|s| !s.is_empty());
    let from = range_from(&q);
    let conn = state.db.lock().unwrap();
    Ok(Json(store::agents(&conn, project, from.as_deref()).map_err(err)?))
}

pub async fn prompt_insights(
    State(state): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult {
    let project = q.get("project").map(|s| s.as_str()).filter(|s| !s.is_empty());
    let from = range_from(&q);
    let conn = state.db.lock().unwrap();
    Ok(Json(store::prompt_insights(&conn, project, from.as_deref()).map_err(err)?))
}

pub async fn digest(
    State(state): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult {
    let project = q.get("project").map(|s| s.as_str()).filter(|s| !s.is_empty());
    let conn = state.db.lock().unwrap();
    Ok(Json(store::digest(&conn, project).map_err(err)?))
}

pub async fn recovery(
    State(state): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult {
    let project = q.get("project").map(|s| s.as_str()).filter(|s| !s.is_empty());
    let from = range_from(&q);
    let conn = state.db.lock().unwrap();
    Ok(Json(store::recovery(&conn, project, from.as_deref()).map_err(err)?))
}

pub async fn prompt_styles(
    State(state): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult {
    let project = q.get("project").map(|s| s.as_str()).filter(|s| !s.is_empty());
    let from = range_from(&q);
    let conn = state.db.lock().unwrap();
    Ok(Json(store::prompt_styles(&conn, project, from.as_deref()).map_err(err)?))
}

pub async fn cache_advisor(
    State(state): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult {
    let project = q.get("project").map(|s| s.as_str()).filter(|s| !s.is_empty());
    let from = range_from(&q);
    let conn = state.db.lock().unwrap();
    Ok(Json(store::cache_advisor(&conn, project, from.as_deref()).map_err(err)?))
}

pub async fn model_rightsizing(
    State(state): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult {
    let project = q.get("project").map(|s| s.as_str()).filter(|s| !s.is_empty());
    let from = range_from(&q);
    let conn = state.db.lock().unwrap();
    Ok(Json(store::model_rightsizing(&conn, project, from.as_deref()).map_err(err)?))
}

pub async fn health_trend(
    State(state): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult {
    let project = q.get("project").map(|s| s.as_str()).filter(|s| !s.is_empty());
    let from = range_from(&q);
    let conn = state.db.lock().unwrap();
    Ok(Json(store::health_trend(&conn, project, from.as_deref()).map_err(err)?))
}

pub async fn leaderboard(
    State(state): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult {
    let from = range_from(&q);
    let conn = state.db.lock().unwrap();
    Ok(Json(store::leaderboard(&conn, from.as_deref()).map_err(err)?))
}

/// OTLP/HTTP JSON metrics receiver (FR-3): cost/LOC/commits chính xác từ OpenTelemetry.
pub async fn otlp_metrics(State(state): State<AppState>, Json(v): Json<Value>) -> StatusCode {
    let deltas = crate::otel::parse_metrics(&v);
    if !deltas.is_empty() {
        if let Ok(conn) = state.db.lock() {
            for d in &deltas {
                let _ = store::upsert_otel(&conn, &d.session_id, d.cost, d.loc_added, d.loc_removed, d.commits, d.prs);
            }
        }
        let _ = state.events_tx.send(());
    }
    StatusCode::OK
}

/// OTLP logs/traces: chấp nhận để không lỗi exporter, hiện chưa xử lý.
pub async fn otlp_accept() -> StatusCode {
    StatusCode::OK
}

pub async fn search(
    State(state): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult {
    let term = q.get("q").map(|s| s.as_str()).unwrap_or("").trim().to_string();
    if term.is_empty() {
        return Ok(Json(json!([])));
    }
    let project = q.get("project").map(|s| s.as_str()).filter(|s| !s.is_empty());
    let conn = state.db.lock().unwrap();
    Ok(Json(store::search(&conn, &term, project, 100).map_err(err)?))
}

pub async fn set_tag(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(v): Json<Value>,
) -> ApiResult {
    let tag = v.get("tag").and_then(|x| x.as_str()).unwrap_or("");
    let outcome = v.get("outcome").and_then(|x| x.as_str()).unwrap_or("");
    let conn = state.db.lock().unwrap();
    store::set_tag(&conn, &id, tag, outcome).map_err(err)?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn list_insights(
    State(state): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult {
    // project = repo -> chỉ insight của repo đó; không có -> tất cả.
    let project = q.get("project").map(|s| s.as_str()).filter(|s| !s.is_empty());
    let conn = state.db.lock().unwrap();
    Ok(Json(store::list_insights(&conn, project, 20).map_err(err)?))
}

/// Tách (major, minor, patch) từ chuỗi version/tag bất kỳ (vd "agentlens-v0.1.2" -> (0,1,2)).
fn ver_tuple(s: &str) -> (u32, u32, u32) {
    let nums: Vec<u32> = s
        .split(|c: char| !c.is_ascii_digit())
        .filter(|p| !p.is_empty())
        .filter_map(|p| p.parse().ok())
        .collect();
    (
        nums.first().copied().unwrap_or(0),
        nums.get(1).copied().unwrap_or(0),
        nums.get(2).copied().unwrap_or(0),
    )
}

/// Kiểm tra bản mới qua GitHub Releases (repo public, không cần token).
/// Trả {current, latest, update_available, url}. Lỗi mạng/không có release -> update_available=false.
/// Đổi repo qua env AGENTLENS_REPO ("owner/name").
pub async fn update_check() -> ApiResult {
    let current = env!("CARGO_PKG_VERSION");
    let repo =
        std::env::var("AGENTLENS_REPO").unwrap_or_else(|_| "anhnth24/agent-lens".to_string());
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");

    let fetch = async {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(8))
            .build()
            .ok()?;
        let v: Value = client
            .get(&url)
            .header("User-Agent", "agentlens")
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .ok()?
            .json()
            .await
            .ok()?;
        let tag = v.get("tag_name").and_then(|x| x.as_str())?.to_string();
        let html = v
            .get("html_url")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        Some((tag, html))
    };

    let (latest, rel_url, available) = match fetch.await {
        Some((tag, html)) => {
            let avail = ver_tuple(&tag) > ver_tuple(current);
            (tag, html, avail)
        }
        None => (String::new(), String::new(), false),
    };

    Ok(Json(json!({
        "current": current,
        "latest": latest,
        "update_available": available,
        "url": rel_url,
    })))
}

/// Trạng thái LLM/auth (FR-8) cho footer UI. Chỉ trả thông tin **đọc được**:
/// backend + auth_method (`claude auth status`) + model hiệu lực + danh sách model.
/// `month_cost_usd` là **ƯỚC TÍNH** theo bảng giá (gồm mọi session AgentLens ghi
/// được trong tháng), KHÔNG phải số dư credit subscription (Claude Code không expose).
pub async fn llm_status(State(state): State<AppState>) -> ApiResult {
    use chrono::{Datelike, Utc};
    let now = Utc::now();
    let month_start = format!("{:04}-{:02}-01T00:00:00", now.year(), now.month());
    let (month_cost, self_cost) = {
        let conn = state.db.lock().unwrap();
        (
            store::cost_since(&conn, &month_start).unwrap_or(0.0),
            store::self_cost_since(&conn, &month_start).unwrap_or(0.0),
        )
    };
    let models: Vec<Value> = llm::MODEL_CHOICES
        .iter()
        .map(|(id, label)| json!({ "id": id, "label": label }))
        .collect();
    Ok(Json(json!({
        "enabled": llm::is_enabled(),
        "backend": llm::backend_kind(),
        "backend_label": llm::backend_label(),
        "auth": llm::cli_auth_status().await,
        "model": llm::current_model(),
        "models": models,
        "month": format!("{:04}-{:02}", now.year(), now.month()),
        "month_cost_usd": month_cost,
        "self_cost_usd": self_cost,
    })))
}

/// Đặt model LLM từ footer (override runtime, lưu vào settings để giữ qua restart).
pub async fn set_llm_model(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> ApiResult {
    let model = body.get("model").and_then(|m| m.as_str()).unwrap_or("").trim().to_string();
    // chỉ chấp nhận model trong danh sách cho phép (hoặc rỗng = reset)
    if !model.is_empty() && !llm::MODEL_CHOICES.iter().any(|(id, _)| *id == model) {
        return Err((StatusCode::BAD_REQUEST, format!("model không hợp lệ: {model}")));
    }
    {
        let conn = state.db.lock().unwrap();
        store::set_setting(&conn, "llm_model", &model).map_err(err)?;
    }
    llm::set_model_override(if model.is_empty() { None } else { Some(model.clone()) });
    Ok(Json(json!({ "model": llm::current_model() })))
}

/// Cross-session LLM insight: gộp metrics (tool/lỗi/cost/prompt đắt) → đề xuất cải thiện.
pub async fn analyze_insights(
    State(state): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult {
    if !llm::is_enabled() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Cần LLM để phân tích cross-session: đặt ANTHROPIC_API_KEY, hoặc cài Claude Code \
             + `/login` subscription (backend `claude -p`)."
                .to_string(),
        ));
    }
    let project = q.get("project").map(|s| s.as_str()).filter(|s| !s.is_empty());
    let scope = project.unwrap_or("tất cả repo").to_string();

    let brief = {
        let conn = state.db.lock().unwrap();
        let tools = store::tool_stats(&conn, project, None).map_err(err)?;
        let sessions = store::sessions(&conn, project, None).map_err(err)?;
        build_metrics_brief(&tools, &sessions)
    };

    let prompt = format!(
        "Dưới đây là METRICS tổng hợp (không có nội dung code) từ nhiều session Claude Code\
 trong phạm vi '{scope}'. Hãy phân tích bằng tiếng Việt và đưa ra 5–7 **đề xuất cụ thể**\
 để cải thiện chất lượng/hiệu quả agent (workflow, prompt, skill, hook, dùng tool):\n\n{brief}"
    );
    let content = llm::ask(&prompt)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))?;
    {
        let conn = state.db.lock().unwrap();
        store::insert_insight(&conn, &scope, &content).map_err(err)?;
    }
    Ok(Json(json!({ "scope": scope, "content": content })))
}

fn build_metrics_brief(tools: &Value, sessions: &Value) -> String {
    let mut s = String::from("## Tool usage (toàn bộ scope)\n");
    if let Some(arr) = tools.as_array() {
        for t in arr.iter().take(15) {
            s.push_str(&format!(
                "- {}: {} lần, lỗi {:.0}%, avg {}ms, max {}ms\n",
                t.get("tool").and_then(|x| x.as_str()).unwrap_or(""),
                t.get("count").and_then(|x| x.as_i64()).unwrap_or(0),
                t.get("error_rate").and_then(|x| x.as_f64()).unwrap_or(0.0) * 100.0,
                t.get("avg_ms").and_then(|x| x.as_i64()).unwrap_or(0),
                t.get("max_ms").and_then(|x| x.as_i64()).unwrap_or(0),
            ));
        }
    }
    s.push_str("\n## Session tốn kém nhất\n");
    if let Some(arr) = sessions.as_array() {
        let mut v: Vec<&Value> = arr.iter().collect();
        v.sort_by(|a, b| {
            b.get("cost_usd")
                .and_then(|x| x.as_f64())
                .unwrap_or(0.0)
                .partial_cmp(&a.get("cost_usd").and_then(|x| x.as_f64()).unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for sess in v.into_iter().take(8) {
            s.push_str(&format!(
                "- {} (in {}, out {}, cached {}): ${:.2}\n",
                sess.get("project").and_then(|x| x.as_str()).unwrap_or(""),
                sess.get("input_tokens").and_then(|x| x.as_i64()).unwrap_or(0),
                sess.get("output_tokens").and_then(|x| x.as_i64()).unwrap_or(0),
                sess.get("cache_read_tokens").and_then(|x| x.as_i64()).unwrap_or(0),
                sess.get("cost_usd").and_then(|x| x.as_f64()).unwrap_or(0.0),
            ));
        }
    }
    s
}

/// WebSocket: đẩy thông báo "update" mỗi khi tailer ghi event mới (live UI).
pub async fn ws(State(state): State<AppState>, up: WebSocketUpgrade) -> Response {
    up.on_upgrade(move |socket| handle_ws(socket, state))
}

async fn handle_ws(mut socket: WebSocket, state: AppState) {
    let mut rx = state.events_tx.subscribe();
    loop {
        tokio::select! {
            recv = rx.recv() => match recv {
                Ok(()) => {
                    if socket.send(Message::Text("update".into())).await.is_err() {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            },
            msg = socket.recv() => match msg {
                Some(Ok(_)) => {}
                _ => break,
            },
        }
    }
}

/// Xây log rút gọn từ events để đưa vào LLM.
fn build_brief(events: &Value) -> String {
    let mut out = String::new();
    if let Some(arr) = events.as_array() {
        for e in arr {
            let role = e.get("role").and_then(|x| x.as_str()).unwrap_or("");
            let tool = e.get("tool_name").and_then(|x| x.as_str()).unwrap_or("");
            let text = e.get("text").and_then(|x| x.as_str()).unwrap_or("");
            let think = e.get("thinking").and_then(|x| x.as_str()).unwrap_or("");
            let snip = |s: &str| s.chars().take(400).collect::<String>();
            if !tool.is_empty() {
                out.push_str(&format!("[{role}] 🔧 {tool}\n"));
            }
            if !think.is_empty() {
                out.push_str(&format!("[{role}/thinking] {}\n", snip(think)));
            }
            if !text.is_empty() {
                out.push_str(&format!("[{role}] {}\n", snip(text)));
            }
        }
    }
    out
}

/// FR-8: tóm tắt + gợi ý cải thiện workflow bằng LLM (redact trước khi gửi).
pub async fn summarize(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult {
    if !llm::is_enabled() {
        return Err((
            StatusCode::BAD_REQUEST,
            "FR-8 chưa bật: đặt ANTHROPIC_API_KEY, hoặc cài Claude Code + `/login` subscription \
             (backend `claude -p`). Tùy chọn AGENTLENS_MODEL / AGENTLENS_LLM_BACKEND."
                .to_string(),
        ));
    }
    let brief = {
        let conn = state.db.lock().unwrap();
        let events = store::session_events(&conn, &id, None).map_err(err)?;
        build_brief(&events)
    };
    if brief.trim().is_empty() {
        return Err((StatusCode::NOT_FOUND, "Session chưa có nội dung để tóm tắt.".to_string()));
    }
    let summary = llm::summarize(&brief)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))?;
    {
        let conn = state.db.lock().unwrap();
        store::set_summary(&conn, &id, &summary).map_err(err)?;
    }
    Ok(Json(json!({ "summary": summary })))
}
