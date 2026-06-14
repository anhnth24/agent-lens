//! FR-1: nhận hook HTTP local từ Claude Code. Trả 200 nhanh, ghi async-friendly (DB nhanh).
//! Hook dùng để: phát hiện session + transcript_path + project (cwd), và log hoạt động realtime.

use crate::{store, util::repo_name, AppState};
use axum::{extract::State, http::StatusCode, Json};
use serde_json::Value;
use std::path::PathBuf;

pub async fn receive(State(state): State<AppState>, Json(v): Json<Value>) -> StatusCode {
    let get = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("");
    let session_id = get("session_id");
    let event_name = get("hook_event_name");
    let cwd = get("cwd");
    let transcript_path = get("transcript_path");
    let tool_name = get("tool_name");
    let permission_mode = get("permission_mode");
    let now = chrono::Utc::now().to_rfc3339();

    if !session_id.is_empty() {
        let project = repo_name(cwd);
        if let Ok(conn) = state.db.lock() {
            let _ = store::upsert_session(
                &conn,
                session_id,
                &project,
                cwd,
                "",
                transcript_path,
                &now,
            );
            let _ = store::insert_hook(&conn, session_id, &now, event_name, tool_name, permission_mode);
        }
    }
    if !transcript_path.is_empty() {
        if let Ok(mut set) = state.extra_paths.lock() {
            set.insert(PathBuf::from(transcript_path));
        }
    }
    let _ = state.events_tx.send(()); // báo WS có hoạt động mới
    StatusCode::OK
}
