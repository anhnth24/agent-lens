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
  summary         TEXT,
  tag             TEXT DEFAULT '',
  outcome         TEXT DEFAULT ''
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
  tool_error            INTEGER DEFAULT 0,
  git_branch            TEXT,
  cwd                   TEXT
);
CREATE TABLE IF NOT EXISTS tools (
  tool_use_id  TEXT PRIMARY KEY,
  session_id   TEXT,
  prompt_id    TEXT,
  tool_name    TEXT,
  ts_use       TEXT,
  ts_result    TEXT,
  duration_ms  INTEGER,
  is_error     INTEGER DEFAULT 0,
  target       TEXT DEFAULT ''
);
CREATE INDEX IF NOT EXISTS idx_tools_session ON tools(session_id);
CREATE INDEX IF NOT EXISTS idx_tools_name    ON tools(tool_name);
CREATE TABLE IF NOT EXISTS insights (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  scope       TEXT,
  created_at  TEXT,
  content     TEXT
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

/// Mệnh đề thời gian an toàn (from do server sinh từ keyword range, không phải input thô).
fn since(col: &str, from: Option<&str>, kw: &str) -> String {
    match from {
        Some(f) if !f.is_empty() => format!(" {kw} {col} >= '{f}'"),
        _ => String::new(),
    }
}

pub fn open(path: &std::path::Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.execute_batch(SCHEMA)?;
    // migrate cột mới cho DB cũ (bỏ qua nếu đã có)
    for stmt in [
        "ALTER TABLE tools ADD COLUMN target TEXT DEFAULT ''",
        "ALTER TABLE sessions ADD COLUMN tag TEXT DEFAULT ''",
        "ALTER TABLE sessions ADD COLUMN outcome TEXT DEFAULT ''",
    ] {
        let _ = conn.execute(stmt, []);
    }
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

pub fn insert_event(conn: &Connection, e: &Entry, _project: &str) -> Result<usize> {
    let cost = crate::pricing::cost(
        e.model.as_deref().unwrap_or(""),
        e.input_tokens,
        e.output_tokens,
        e.cache_read_tokens,
        e.cache_creation_tokens,
    );
    let n = conn.execute(
        r#"INSERT OR IGNORE INTO events
           (event_id,session_id,prompt_id,ts,kind,role,text,thinking,tool_name,tool_input,tool_result,
            model,input_tokens,output_tokens,cache_read_tokens,cache_creation_tokens,cost_usd,tool_error,git_branch,cwd)
           VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20)"#,
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
            cost,
            e.tool_error as i64,
            e.git_branch,
            e.cwd,
        ],
    )?;
    Ok(n)
}

/// Ghi 1 tool_use (idempotent theo tool_use_id).
pub fn insert_tool_use(
    conn: &Connection,
    session_id: &str,
    prompt_id: Option<&str>,
    id: &str,
    name: &str,
    ts_use: &str,
    target: &str,
) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO tools(tool_use_id,session_id,prompt_id,tool_name,ts_use,target) VALUES(?1,?2,?3,?4,?5,?6)",
        params![id, session_id, prompt_id, name, ts_use, target],
    )?;
    Ok(())
}

/// Tìm kiếm substring (case-insensitive) trong text/thinking/tool_input/tool_result.
pub fn search(conn: &Connection, q: &str, project: Option<&str>, limit: i64) -> Result<Value> {
    let like = format!("%{}%", q.replace('%', "").replace('_', ""));
    let proj_clause = if project.is_some() {
        "AND s.project = ?3"
    } else {
        ""
    };
    let sql = format!(
        r#"SELECT e.session_id, s.project, e.ts, e.role, e.tool_name,
                  COALESCE(NULLIF(e.text,''), NULLIF(e.thinking,''), NULLIF(e.tool_input,''), e.tool_result, '')
           FROM events e JOIN sessions s ON s.session_id = e.session_id
           WHERE (e.text LIKE ?1 OR e.thinking LIKE ?1 OR e.tool_input LIKE ?1 OR e.tool_result LIKE ?1)
                 {proj_clause}
           ORDER BY e.ts DESC LIMIT ?2"#
    );
    let map = |r: &rusqlite::Row| {
        let snip: String = r.get(5)?;
        Ok(json!({
            "session_id": r.get::<_, String>(0)?,
            "project": r.get::<_, Option<String>>(1)?,
            "ts": r.get::<_, Option<String>>(2)?,
            "role": r.get::<_, Option<String>>(3)?,
            "tool_name": r.get::<_, Option<String>>(4)?,
            "snippet": snip.chars().take(220).collect::<String>(),
        }))
    };
    let out: Vec<Value> = match project {
        Some(p) => {
            let mut stmt = conn.prepare(&sql)?;
            let v = stmt.query_map(params![like, limit, p], map)?.collect::<rusqlite::Result<_>>()?;
            v
        }
        None => {
            let mut stmt = conn.prepare(&sql)?;
            let v = stmt.query_map(params![like, limit], map)?.collect::<rusqlite::Result<_>>()?;
            v
        }
    };
    Ok(Value::Array(out))
}

/// Friction/loop detection cho 1 session: rework file, lỗi tool, tool chậm.
pub fn friction(conn: &Connection, session_id: &str) -> Result<Value> {
    let mut findings: Vec<Value> = Vec::new();

    // 1) rework: file bị Edit/Write nhiều lần
    {
        let mut stmt = conn.prepare(
            r#"SELECT target, COUNT(*) c FROM tools
               WHERE session_id=?1 AND tool_name IN ('Edit','Write','MultiEdit','NotebookEdit')
                 AND target<>'' GROUP BY target HAVING c>=4 ORDER BY c DESC LIMIT 10"#,
        )?;
        let rows = stmt.query_map(params![session_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })?;
        for row in rows {
            let (target, c) = row?;
            findings.push(json!({
                "kind": "rework",
                "severity": if c >= 8 { "high" } else { "med" },
                "text": format!("File sửa {} lần: {}", c, target),
            }));
        }
    }
    // 2) tool lỗi nhiều
    {
        let mut stmt = conn.prepare(
            r#"SELECT tool_name, COUNT(*) total, COALESCE(SUM(is_error),0) err FROM tools
               WHERE session_id=?1 GROUP BY tool_name HAVING err>0 ORDER BY err DESC"#,
        )?;
        let rows = stmt.query_map(params![session_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?))
        })?;
        for row in rows {
            let (tool, total, err) = row?;
            findings.push(json!({
                "kind": "error",
                "severity": if err as f64 / total.max(1) as f64 >= 0.25 { "high" } else { "med" },
                "text": format!("{}: {}/{} lần lỗi", tool, err, total),
            }));
        }
    }
    // 3) tool chậm bất thường (>30s)
    {
        let mut stmt = conn.prepare(
            r#"SELECT tool_name, target, duration_ms FROM tools
               WHERE session_id=?1 AND duration_ms>30000 ORDER BY duration_ms DESC LIMIT 5"#,
        )?;
        let rows = stmt.query_map(params![session_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?))
        })?;
        for row in rows {
            let (tool, target, ms) = row?;
            findings.push(json!({
                "kind": "slow",
                "severity": "low",
                "text": format!("{} chậm {}s{}", tool, ms / 1000, if target.is_empty() { String::new() } else { format!(" ({})", target) }),
            }));
        }
    }
    Ok(Value::Array(findings))
}

pub fn set_tag(conn: &Connection, session_id: &str, tag: &str, outcome: &str) -> Result<()> {
    conn.execute(
        "UPDATE sessions SET tag=?2, outcome=?3 WHERE session_id=?1",
        params![session_id, tag, outcome],
    )?;
    Ok(())
}

pub fn insert_insight(conn: &Connection, scope: &str, content: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO insights(scope,created_at,content) VALUES(?1,?2,?3)",
        params![scope, chrono::Utc::now().to_rfc3339(), content],
    )?;
    Ok(())
}

pub fn list_insights(conn: &Connection, limit: i64) -> Result<Value> {
    let mut stmt =
        conn.prepare("SELECT scope,created_at,content FROM insights ORDER BY id DESC LIMIT ?1")?;
    let rows = stmt.query_map(params![limit], |r| {
        Ok(json!({
            "scope": r.get::<_, Option<String>>(0)?,
            "created_at": r.get::<_, Option<String>>(1)?,
            "content": r.get::<_, Option<String>>(2)?,
        }))
    })?;
    Ok(Value::Array(rows.collect::<rusqlite::Result<_>>()?))
}

/// Hoàn tất tool khi có tool_result: set is_error + ts_result + duration (ms).
pub fn complete_tool(conn: &Connection, tool_use_id: &str, ts_result: &str, is_error: bool) -> Result<()> {
    conn.execute(
        r#"UPDATE tools
           SET ts_result = ?2,
               is_error  = ?3,
               duration_ms = CAST((julianday(?2) - julianday(ts_use)) * 86400000.0 AS INTEGER)
           WHERE tool_use_id = ?1"#,
        params![tool_use_id, ts_result, is_error as i64],
    )?;
    Ok(())
}

/// Tính lại cost cho mọi model (chạy lúc khởi động — cập nhật cả row cũ).
pub fn recompute_costs(conn: &Connection) -> Result<()> {
    let models: Vec<String> = {
        let mut stmt = conn.prepare("SELECT DISTINCT COALESCE(model,'') FROM events")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    for m in models {
        let p = crate::pricing::for_model(&m);
        conn.execute(
            r#"UPDATE events SET cost_usd =
                 (input_tokens*?2 + output_tokens*?3 + cache_read_tokens*?4 + cache_creation_tokens*?5)/1000000.0
               WHERE COALESCE(model,'') = ?1"#,
            params![m, p.input, p.output, p.cache_read, p.cache_write],
        )?;
    }
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
pub fn projects(conn: &Connection, from: Option<&str>) -> Result<Value> {
    let sql = format!(
        r#"SELECT s.project,
                  COUNT(DISTINCT s.session_id) AS sessions,
                  COALESCE(SUM(e.input_tokens),0),
                  COALESCE(SUM(e.output_tokens),0),
                  COALESCE(SUM(e.cache_read_tokens),0),
                  COALESCE(SUM(e.cache_creation_tokens),0),
                  COALESCE(SUM(e.cost_usd),0),
                  MIN(NULLIF(s.started_at,'')),
                  MAX(NULLIF(s.last_activity,''))
           FROM sessions s JOIN events e ON e.session_id = s.session_id
           WHERE 1=1{}
           GROUP BY s.project
           ORDER BY MAX(NULLIF(s.last_activity,'')) DESC"#,
        since("e.ts", from, "AND")
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |r| {
        Ok(json!({
            "project": r.get::<_, Option<String>>(0)?,
            "sessions": r.get::<_, i64>(1)?,
            "input_tokens": r.get::<_, i64>(2)?,
            "output_tokens": r.get::<_, i64>(3)?,
            "cache_read_tokens": r.get::<_, i64>(4)?,
            "cache_creation_tokens": r.get::<_, i64>(5)?,
            "cost_usd": r.get::<_, f64>(6)?,
            "started_at": r.get::<_, Option<String>>(7)?,
            "last_activity": r.get::<_, Option<String>>(8)?,
        }))
    })?;
    Ok(Value::Array(rows.collect::<rusqlite::Result<_>>()?))
}

/// Sessions (lọc theo project + thời gian nếu có) kèm tổng token mỗi session.
pub fn sessions(conn: &Connection, project: Option<&str>, from: Option<&str>) -> Result<Value> {
    let time_c = since("s.last_activity", from, "AND");
    let base = r#"SELECT s.session_id, s.project, s.git_branch, s.started_at, s.last_activity,
                         COALESCE(SUM(e.input_tokens),0),
                         COALESCE(SUM(e.output_tokens),0),
                         COALESCE(SUM(e.cache_read_tokens),0),
                         COALESCE(SUM(e.cache_creation_tokens),0),
                         COALESCE(SUM(e.cost_usd),0),
                         COUNT(e.event_id),
                         COALESCE(s.tag,''), COALESCE(s.outcome,'')
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
            "cost_usd": r.get::<_, f64>(9)?,
            "events": r.get::<_, i64>(10)?,
            "tag": r.get::<_, String>(11)?,
            "outcome": r.get::<_, String>(12)?,
        }))
    };
    let out: Vec<Value> = match project {
        Some(p) => {
            let sql = format!("{base} WHERE s.project = ?1{time_c}{tail}");
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(params![p], map)?.collect::<rusqlite::Result<_>>()?;
            rows
        }
        None => {
            let sql = format!("{base} WHERE 1=1{time_c}{tail}");
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
                  input_tokens,output_tokens,cache_read_tokens,cache_creation_tokens,prompt_id,tool_error
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
            "tool_error": r.get::<_, i64>(14)? != 0,
        }))
    })?;
    Ok(Value::Array(rows.collect::<rusqlite::Result<_>>()?))
}

/// Breakdown theo model TRONG 1 session (khi session đổi model giữa chừng).
pub fn session_models(conn: &Connection, session_id: &str) -> Result<Value> {
    let mut stmt = conn.prepare(
        r#"SELECT COALESCE(NULLIF(model,''),'(none)'),
                  COUNT(*),
                  COALESCE(SUM(input_tokens),0),
                  COALESCE(SUM(output_tokens),0),
                  COALESCE(SUM(cache_read_tokens),0),
                  COALESCE(SUM(cost_usd),0)
           FROM events WHERE session_id=?1
           GROUP BY COALESCE(NULLIF(model,''),'(none)')
           ORDER BY SUM(input_tokens)+SUM(output_tokens) DESC"#,
    )?;
    let rows = stmt.query_map(params![session_id], |r| {
        Ok(json!({
            "model": r.get::<_, String>(0)?,
            "events": r.get::<_, i64>(1)?,
            "input_tokens": r.get::<_, i64>(2)?,
            "output_tokens": r.get::<_, i64>(3)?,
            "cache_read_tokens": r.get::<_, i64>(4)?,
            "cost_usd": r.get::<_, f64>(5)?,
        }))
    })?;
    Ok(Value::Array(rows.collect::<rusqlite::Result<_>>()?))
}

/// Thống kê token in/out/cached theo nhóm: project | day | model.
pub fn summary(conn: &Connection, group_by: &str, from: Option<&str>) -> Result<Value> {
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
                  COALESCE(SUM(e.cost_usd),0)              AS cost_usd,
                  (COALESCE(SUM(e.input_tokens),0)+COALESCE(SUM(e.output_tokens),0)) AS in_out
           FROM events e {join} {where_c}{time_c}
           GROUP BY {group} ORDER BY {order}"#,
        time_c = since("e.ts", from, if where_c.is_empty() { "WHERE" } else { "AND" })
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |r| {
        Ok(json!({
            "label": r.get::<_, Option<String>>(0)?,
            "input_tokens": r.get::<_, i64>(1)?,
            "output_tokens": r.get::<_, i64>(2)?,
            "cache_read_tokens": r.get::<_, i64>(3)?,
            "cache_creation_tokens": r.get::<_, i64>(4)?,
            "cost_usd": r.get::<_, f64>(5)?,
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

/// Tổng toàn cục (header UI), lọc theo thời gian nếu có.
pub fn totals(conn: &Connection, from: Option<&str>) -> Result<Value> {
    let sql = format!(
        r#"SELECT
             COUNT(DISTINCT session_id),
             COALESCE(SUM(input_tokens),0),
             COALESCE(SUM(output_tokens),0),
             COALESCE(SUM(cache_read_tokens),0),
             COALESCE(SUM(cache_creation_tokens),0),
             COALESCE(SUM(cost_usd),0)
           FROM events WHERE 1=1{}"#,
        since("ts", from, "AND")
    );
    conn.query_row(
        &sql,
        [],
        |r| {
            Ok(json!({
                "sessions": r.get::<_, i64>(0)?,
                "input_tokens": r.get::<_, i64>(1)?,
                "output_tokens": r.get::<_, i64>(2)?,
                "cache_read_tokens": r.get::<_, i64>(3)?,
                "cache_creation_tokens": r.get::<_, i64>(4)?,
                "cost_usd": r.get::<_, f64>(5)?,
            }))
        },
    )
    .map_err(Into::into)
}

/// Phân tích tool: tần suất, lỗi, thời lượng (lọc theo project + thời gian nếu có).
pub fn tool_stats(conn: &Connection, project: Option<&str>, from: Option<&str>) -> Result<Value> {
    let time_c = since("t.ts_use", from, "AND");
    let base = r#"SELECT t.tool_name, COUNT(*), COALESCE(SUM(t.is_error),0),
                         COALESCE(AVG(t.duration_ms),0), COALESCE(MAX(t.duration_ms),0)
                  FROM tools t JOIN sessions s ON s.session_id = t.session_id"#;
    let tail = " GROUP BY t.tool_name ORDER BY COUNT(*) DESC";
    let map = |r: &rusqlite::Row| {
        let cnt: i64 = r.get(1)?;
        let err: i64 = r.get(2)?;
        Ok(json!({
            "tool": r.get::<_, Option<String>>(0)?,
            "count": cnt,
            "errors": err,
            "error_rate": if cnt > 0 { err as f64 / cnt as f64 } else { 0.0 },
            "avg_ms": r.get::<_, f64>(3)? as i64,
            "max_ms": r.get::<_, i64>(4)?,
        }))
    };
    let out: Vec<Value> = match project {
        Some(p) => {
            let mut stmt = conn.prepare(&format!("{base} WHERE s.project = ?1{time_c}{tail}"))?;
            let v = stmt.query_map(params![p], map)?.collect::<rusqlite::Result<_>>()?;
            v
        }
        None => {
            let mut stmt = conn.prepare(&format!("{base} WHERE 1=1{time_c}{tail}"))?;
            let v = stmt.query_map([], map)?.collect::<rusqlite::Result<_>>()?;
            v
        }
    };
    Ok(Value::Array(out))
}

/// Breakdown theo prompt trong 1 session: turns, token, tool, snippet.
pub fn prompt_breakdown(conn: &Connection, session_id: &str) -> Result<Value> {
    let mut stmt = conn.prepare(
        r#"SELECT ev.prompt_id,
                  COUNT(*),
                  COALESCE(SUM(ev.input_tokens),0),
                  COALESCE(SUM(ev.output_tokens),0),
                  COALESCE(SUM(ev.cache_read_tokens),0),
                  COALESCE(SUM(ev.cost_usd),0),
                  SUM(CASE WHEN ev.tool_name IS NOT NULL AND ev.tool_name<>'' THEN 1 ELSE 0 END),
                  MIN(ev.ts),
                  (SELECT e2.text FROM events e2
                     WHERE e2.session_id = ev.session_id AND e2.prompt_id = ev.prompt_id
                       AND e2.role='user' AND e2.text<>'' ORDER BY e2.ts LIMIT 1)
           FROM events ev
           WHERE ev.session_id = ?1 AND ev.prompt_id IS NOT NULL AND ev.prompt_id <> ''
           GROUP BY ev.prompt_id ORDER BY MIN(ev.ts)"#,
    )?;
    let rows = stmt.query_map(params![session_id], |r| {
        Ok(json!({
            "prompt_id": r.get::<_, Option<String>>(0)?,
            "turns": r.get::<_, i64>(1)?,
            "input_tokens": r.get::<_, i64>(2)?,
            "output_tokens": r.get::<_, i64>(3)?,
            "cache_read_tokens": r.get::<_, i64>(4)?,
            "cost_usd": r.get::<_, f64>(5)?,
            "tool_turns": r.get::<_, i64>(6)?,
            "started_at": r.get::<_, Option<String>>(7)?,
            "prompt": r.get::<_, Option<String>>(8)?,
        }))
    })?;
    Ok(Value::Array(rows.collect::<rusqlite::Result<_>>()?))
}
