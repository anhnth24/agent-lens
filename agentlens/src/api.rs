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
) -> ApiResult {
    let conn = state.db.lock().unwrap();
    Ok(Json(store::session_events(&conn, &id).map_err(err)?))
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

pub async fn list_insights(State(state): State<AppState>) -> ApiResult {
    let conn = state.db.lock().unwrap();
    Ok(Json(store::list_insights(&conn, 20).map_err(err)?))
}

/// Cross-session LLM insight: gộp metrics (tool/lỗi/cost/prompt đắt) → đề xuất cải thiện.
pub async fn analyze_insights(
    State(state): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult {
    if !llm::is_enabled() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Cần ANTHROPIC_API_KEY để phân tích cross-session.".to_string(),
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
            "FR-8 chưa bật: đặt ANTHROPIC_API_KEY (và tùy chọn AGENTLENS_MODEL) rồi chạy lại.".to_string(),
        ));
    }
    let brief = {
        let conn = state.db.lock().unwrap();
        let events = store::session_events(&conn, &id).map_err(err)?;
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
