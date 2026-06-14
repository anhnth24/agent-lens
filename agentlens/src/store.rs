//! SQLite store (embedded, 1 file, WAL). Dedup idempotent qua event_id (uuid của dòng JSONL).

use crate::jsonl::Entry;
use anyhow::Result;
use rusqlite::{params, Connection};
use serde_json::{json, Value};

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS sessions (
  session_id      TEXT PRIMARY KEY,
  project         TEXT DEFAULT '',
  cwd             TEXT DEFAULT '',
  git_branch      TEXT DEFAULT '',
  transcript_path TEXT DEFAULT '',
  started_at      TEXT DEFAULT '',
  last_activity   TEXT DEFAULT '',
  summary         TEXT
);
CREATE TABLE IF NOT EXISTS events (
  event_id              TEXT PRIMARY KEY,
  session_id            TEXT,
  prompt_id             TEXT,
  ts                    TEXT,
  kind                  TEXT,
  role                  TEXT,
  text                  TEXT,
  thinking              TEXT,
  tool_name             TEXT,
  tool_input            TEXT,
  tool_result           TEXT,
  model                 TEXT,
  input_tokens          INTEGER DEFAULT 0,
  output_tokens         INTEGER DEFAULT 0,
  cache_read_tokens     INTEGER DEFAULT 0,
  cache_creation_tokens INTEGER DEFAULT 0,
  cost_usd              REAL DEFAULT 0,
  git_branch            TEXT,
  cwd                   TEXT
);
CREATE TABLE IF NOT EXISTS hooks (
  id              INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id      TEXT,
  ts              TEXT,
  event_name      TEXT,
  tool_name       TEXT,
  permission_mode TEXT
);
CREATE INDEX IF NOT EXISTS idx_events_session ON events(session_id, ts);
CREATE INDEX IF NOT EXISTS idx_events_prompt  ON events(prompt_id);
CREATE INDEX IF NOT EXISTS idx_sessions_proj  ON sessions(project);
"#;

pub fn open(path: &std::path::Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.execute_batch(SCHEMA)?;
    Ok(conn)
}

/// Upsert session, giữ started_at sớm nhất / last_activity muộn nhất (ISO-8601 so sánh lexicographic).
pub fn upsert_session(
    conn: &Connection,
    session_id: &str,
    project: &str,
    cwd: &str,
    git_branch: &str,
    transcript_path: &str,
    ts: &str,
) -> Result<()> {
    conn.execute(
        r#"INSERT INTO sessions(session_id,project,cwd,git_branch,transcript_path,started_at,last_activity)
           VALUES(?1,?2,?3,?4,?5,?6,?6)
           ON CONFLICT(session_id) DO UPDATE SET
             project        = CASE WHEN sessions.project='' OR sessions.project IS NULL THEN excluded.project ELSE sessions.project END,
             cwd            = CASE WHEN excluded.cwd<>'' THEN excluded.cwd ELSE sessions.cwd END,
             git_branch     = CASE WHEN excluded.git_branch<>'' THEN excluded.git_branch ELSE sessions.git_branch END,
             transcript_path= CASE WHEN excluded.transcript_path<>'' THEN excluded.transcript_path ELSE sessions.transcript_path END,
             started_at     = CASE WHEN sessions.started_at='' OR (excluded.started_at<>'' AND excluded.started_at<sessions.started_at) THEN excluded.started_at ELSE sessions.started_at END,
             last_activity  = CASE WHEN excluded.last_activity>sessions.last_activity THEN excluded.last_activity ELSE sessions.last_activity END
        "#,
        params![session_id, project, cwd, git_branch, transcript_path, ts],
    )?;
    Ok(())
}

pub fn upsert_session_from_entry(
    conn: &Connection,
    e: &Entry,
    project: &str,
    transcript_path: &str,
) -> Result<()> {
    upsert_session(
        conn,
        &e.session_id,
        project,
        e.cwd.as_deref().unwrap_or(""),
        e.git_branch.as_deref().unwrap_or(""),
        transcript_path,
        &e.ts,
    )
}

pub fn insert_event(conn: &Connection, e: &Entry, _project: &str) -> Result<()> {
    conn.execute(
        r#"INSERT OR IGNORE INTO events
           (event_id,session_id,prompt_id,ts,kind,role,text,thinking,tool_name,tool_input,tool_result,
            model,input_tokens,output_tokens,cache_read_tokens,cache_creation_tokens,cost_usd,git_branch,cwd)
           VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)"#,
        params![
            e.uuid,
            e.session_id,
            e.prompt_id,
            e.ts,
            e.kind,
            e.role,
            e.text,
            e.thinking,
            e.tool_name,
            e.tool_input,
            e.tool_result,
            e.model,
            e.input_tokens,
            e.output_tokens,
            e.cache_read_tokens,
            e.cache_creation_tokens,
            0.0_f64,
            e.git_branch,
            e.cwd,
        ],
    )?;
    Ok(())
}

pub fn insert_hook(
    conn: &Connection,
    session_id: &str,
    ts: &str,
    event_name: &str,
    tool_name: &str,
    permission_mode: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO hooks(session_id,ts,event_name,tool_name,permission_mode) VALUES(?1,?2,?3,?4,?5)",
        params![session_id, ts, event_name, tool_name, permission_mode],
    )?;
    Ok(())
}

/// Repo/project list + tổng token (in/out/cached) cho UI "xem session theo repo".
pub fn projects(conn: &Connection) -> Result<Value> {
    let mut stmt = conn.prepare(
        r#"SELECT s.project,
                  COUNT(DISTINCT s.session_id) AS sessions,
                  COALESCE(SUM(e.input_tokens),0),
                  COALESCE(SUM(e.output_tokens),0),
                  COALESCE(SUM(e.cache_read_tokens),0),
                  COALESCE(SUM(e.cache_creation_tokens),0),
                  MIN(NULLIF(s.started_at,'')),
                  MAX(NULLIF(s.last_activity,''))
           FROM sessions s LEFT JOIN events e ON e.session_id = s.session_id
           GROUP BY s.project
           ORDER BY MAX(NULLIF(s.last_activity,'')) DESC"#,
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(json!({
            "project": r.get::<_, Option<String>>(0)?,
            "sessions": r.get::<_, i64>(1)?,
            "input_tokens": r.get::<_, i64>(2)?,
            "output_tokens": r.get::<_, i64>(3)?,
            "cache_read_tokens": r.get::<_, i64>(4)?,
            "cache_creation_tokens": r.get::<_, i64>(5)?,
            "started_at": r.get::<_, Option<String>>(6)?,
            "last_activity": r.get::<_, Option<String>>(7)?,
        }))
    })?;
    Ok(Value::Array(rows.collect::<rusqlite::Result<_>>()?))
}

/// Sessions (lọc theo project nếu có) kèm tổng token mỗi session.
pub fn sessions(conn: &Connection, project: Option<&str>) -> Result<Value> {
    let base = r#"SELECT s.session_id, s.project, s.git_branch, s.started_at, s.last_activity,
                         COALESCE(SUM(e.input_tokens),0),
                         COALESCE(SUM(e.output_tokens),0),
                         COALESCE(SUM(e.cache_read_tokens),0),
                         COALESCE(SUM(e.cache_creation_tokens),0),
                         COUNT(e.event_id)
                  FROM sessions s LEFT JOIN events e ON e.session_id = s.session_id"#;
    let tail = " GROUP BY s.session_id ORDER BY s.last_activity DESC";
    let map = |r: &rusqlite::Row| {
        Ok(json!({
            "session_id": r.get::<_, String>(0)?,
            "project": r.get::<_, Option<String>>(1)?,
            "git_branch": r.get::<_, Option<String>>(2)?,
            "started_at": r.get::<_, Option<String>>(3)?,
            "last_activity": r.get::<_, Option<String>>(4)?,
            "input_tokens": r.get::<_, i64>(5)?,
            "output_tokens": r.get::<_, i64>(6)?,
            "cache_read_tokens": r.get::<_, i64>(7)?,
            "cache_creation_tokens": r.get::<_, i64>(8)?,
            "events": r.get::<_, i64>(9)?,
        }))
    };
    let out: Vec<Value> = match project {
        Some(p) => {
            let sql = format!("{base} WHERE s.project = ?1{tail}");
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(params![p], map)?.collect::<rusqlite::Result<_>>()?;
            rows
        }
        None => {
            let sql = format!("{base}{tail}");
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map([], map)?.collect::<rusqlite::Result<_>>()?;
            rows
        }
    };
    Ok(Value::Array(out))
}

/// Timeline 1 session.
pub fn session_events(conn: &Connection, session_id: &str) -> Result<Value> {
    let mut stmt = conn.prepare(
        r#"SELECT ts,kind,role,text,thinking,tool_name,tool_input,tool_result,model,
                  input_tokens,output_tokens,cache_read_tokens,cache_creation_tokens,prompt_id
           FROM events WHERE session_id = ?1 ORDER BY ts, rowid"#,
    )?;
    let rows = stmt.query_map(params![session_id], |r| {
        Ok(json!({
            "ts": r.get::<_, Option<String>>(0)?,
            "kind": r.get::<_, Option<String>>(1)?,
            "role": r.get::<_, Option<String>>(2)?,
            "text": r.get::<_, Option<String>>(3)?,
            "thinking": r.get::<_, Option<String>>(4)?,
            "tool_name": r.get::<_, Option<String>>(5)?,
            "tool_input": r.get::<_, Option<String>>(6)?,
            "tool_result": r.get::<_, Option<String>>(7)?,
            "model": r.get::<_, Option<String>>(8)?,
            "input_tokens": r.get::<_, i64>(9)?,
            "output_tokens": r.get::<_, i64>(10)?,
            "cache_read_tokens": r.get::<_, i64>(11)?,
            "cache_creation_tokens": r.get::<_, i64>(12)?,
            "prompt_id": r.get::<_, Option<String>>(13)?,
        }))
    })?;
    Ok(Value::Array(rows.collect::<rusqlite::Result<_>>()?))
}

/// Thống kê token in/out/cached theo nhóm: project | day | model.
pub fn summary(conn: &Connection, group_by: &str) -> Result<Value> {
    let (label_expr, join, where_c, group, order): (&str, &str, &str, &str, &str) = match group_by {
        "day" => ("substr(e.ts,1,10)", "", "WHERE e.ts<>''", "1", "1 DESC"),
        "model" => (
            "e.model",
            "",
            "WHERE e.model IS NOT NULL AND e.model<>''",
            "1",
            "in_out DESC",
        ),
        _ => (
            "s.project",
            "JOIN sessions s ON s.session_id = e.session_id",
            "",
            "1",
            "in_out DESC",
        ),
    };
    let sql = format!(
        r#"SELECT {label_expr} AS label,
                  COALESCE(SUM(e.input_tokens),0)  AS input_tokens,
                  COALESCE(SUM(e.output_tokens),0) AS output_tokens,
                  COALESCE(SUM(e.cache_read_tokens),0)     AS cache_read_tokens,
                  COALESCE(SUM(e.cache_creation_tokens),0) AS cache_creation_tokens,
                  (COALESCE(SUM(e.input_tokens),0)+COALESCE(SUM(e.output_tokens),0)) AS in_out
           FROM events e {join} {where_c}
           GROUP BY {group} ORDER BY {order}"#
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |r| {
        Ok(json!({
            "label": r.get::<_, Option<String>>(0)?,
            "input_tokens": r.get::<_, i64>(1)?,
            "output_tokens": r.get::<_, i64>(2)?,
            "cache_read_tokens": r.get::<_, i64>(3)?,
            "cache_creation_tokens": r.get::<_, i64>(4)?,
        }))
    })?;
    Ok(Value::Array(rows.collect::<rusqlite::Result<_>>()?))
}

pub fn set_summary(conn: &Connection, session_id: &str, summary: &str) -> Result<()> {
    conn.execute(
        "UPDATE sessions SET summary = ?2 WHERE session_id = ?1",
        params![session_id, summary],
    )?;
    Ok(())
}

/// Tổng toàn cục (header UI).
pub fn totals(conn: &Connection) -> Result<Value> {
    conn.query_row(
        r#"SELECT
             (SELECT COUNT(*) FROM sessions),
             COALESCE(SUM(input_tokens),0),
             COALESCE(SUM(output_tokens),0),
             COALESCE(SUM(cache_read_tokens),0),
             COALESCE(SUM(cache_creation_tokens),0)
           FROM events"#,
        [],
        |r| {
            Ok(json!({
                "sessions": r.get::<_, i64>(0)?,
                "input_tokens": r.get::<_, i64>(1)?,
                "output_tokens": r.get::<_, i64>(2)?,
                "cache_read_tokens": r.get::<_, i64>(3)?,
                "cache_creation_tokens": r.get::<_, i64>(4)?,
            }))
        },
    )
    .map_err(Into::into)
}
