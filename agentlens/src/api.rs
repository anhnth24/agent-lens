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

pub async fn totals(State(state): State<AppState>) -> ApiResult {
    let conn = state.db.lock().unwrap();
    Ok(Json(store::totals(&conn).map_err(err)?))
}

pub async fn projects(State(state): State<AppState>) -> ApiResult {
    let conn = state.db.lock().unwrap();
    Ok(Json(store::projects(&conn).map_err(err)?))
}

pub async fn sessions(
    State(state): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult {
    let project = q.get("project").map(|s| s.as_str()).filter(|s| !s.is_empty());
    let conn = state.db.lock().unwrap();
    Ok(Json(store::sessions(&conn, project).map_err(err)?))
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
    let conn = state.db.lock().unwrap();
    Ok(Json(store::summary(&conn, group_by).map_err(err)?))
}

pub async fn tools(
    State(state): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult {
    let project = q.get("project").map(|s| s.as_str()).filter(|s| !s.is_empty());
    let conn = state.db.lock().unwrap();
    Ok(Json(store::tool_stats(&conn, project).map_err(err)?))
}

pub async fn session_prompts(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult {
    let conn = state.db.lock().unwrap();
    Ok(Json(store::prompt_breakdown(&conn, &id).map_err(err)?))
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
