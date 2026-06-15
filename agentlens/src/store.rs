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
  cache_savings_usd     REAL DEFAULT 0,
  tool_error            INTEGER DEFAULT 0,
  thinking_blocks       INTEGER DEFAULT 0,
  git_branch            TEXT,
  cwd                   TEXT
);
CREATE TABLE IF NOT EXISTS session_metrics (
  session_id   TEXT PRIMARY KEY,
  otel_cost    REAL DEFAULT 0,
  loc_added    INTEGER DEFAULT 0,
  loc_removed  INTEGER DEFAULT 0,
  commits      INTEGER DEFAULT 0,
  prs          INTEGER DEFAULT 0,
  updated_at   TEXT DEFAULT ''
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
CREATE TABLE IF NOT EXISTS settings (
  key   TEXT PRIMARY KEY,
  value TEXT
);
CREATE INDEX IF NOT EXISTS idx_events_session ON events(session_id, ts);
CREATE INDEX IF NOT EXISTS idx_events_prompt  ON events(prompt_id);
CREATE INDEX IF NOT EXISTS idx_sessions_proj  ON sessions(project);
"#;

/// Đọc 1 setting kv (None nếu chưa có).
pub fn get_setting(conn: &Connection, key: &str) -> Result<Option<String>> {
    let v = conn
        .query_row("SELECT value FROM settings WHERE key=?1", params![key], |r| {
            r.get::<_, String>(0)
        })
        .ok();
    Ok(v)
}

/// Ghi 1 setting kv (upsert).
pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO settings(key,value) VALUES(?1,?2)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, value],
    )?;
    Ok(())
}

/// Tổng cost_usd (ước tính theo bảng giá) của các event có ts >= `from_iso`.
/// Dùng cho readout "chi tiêu tháng này" ở footer.
pub fn cost_since(conn: &Connection, from_iso: &str) -> Result<f64> {
    let c: f64 = conn.query_row(
        "SELECT COALESCE(SUM(cost_usd),0) FROM events WHERE ts >= ?1",
        params![from_iso],
        |r| r.get(0),
    )?;
    Ok(c)
}

/// Mệnh đề thời gian an toàn (from do server sinh từ keyword range, không phải input thô).
fn since(col: &str, from: Option<&str>, kw: &str) -> String {
    match from {
        Some(f) if !f.is_empty() => format!(" {kw} {col} >= '{f}'"),
        _ => String::new(),
    }
}

/// Điểm sức khỏe session 0..100 (dùng chung cho sessions/leaderboard/trend).
fn health_score(input: i64, cache_read: i64, cache_creation: i64, tool_total: i64, tool_err: i64, rework: i64) -> i64 {
    let err_rate = if tool_total > 0 { tool_err as f64 / tool_total as f64 } else { 0.0 };
    let denom = (input + cache_read + cache_creation).max(1) as f64;
    let cache_hit = cache_read as f64 / denom;
    let mut h = 100.0 - err_rate * 40.0 - (rework as f64) * 5.0;
    if cache_hit < 0.5 { h -= (0.5 - cache_hit) * 20.0; }
    h.clamp(0.0, 100.0) as i64
}

/// Khóa tuần ISO (year-Wweek) từ timestamp; fallback ngày nếu parse lỗi.
fn week_key(ts: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(ts)
        .map(|d| d.format("%G-W%V").to_string())
        .unwrap_or_else(|_| ts.chars().take(10).collect())
}

/// Thành phần thô của 1 session (để tính health/trend/leaderboard mà không lặp SQL).
struct SessComp {
    session_id: String,
    project: String,
    last_activity: String,
    input: i64,
    cache_read: i64,
    cache_creation: i64,
    cost: f64,
    tool_total: i64,
    tool_err: i64,
    rework: i64,
}

fn session_components(conn: &Connection, project: Option<&str>, from: Option<&str>) -> Result<Vec<SessComp>> {
    let time_c = since("s.last_activity", from, "AND");
    let base = r#"SELECT s.project, COALESCE(s.last_activity,''),
                    COALESCE(SUM(e.input_tokens),0),
                    COALESCE(SUM(e.cache_read_tokens),0),
                    COALESCE(SUM(e.cache_creation_tokens),0),
                    COALESCE(SUM(e.cost_usd),0),
                    (SELECT COUNT(*) FROM tools t WHERE t.session_id=s.session_id),
                    (SELECT COALESCE(SUM(is_error),0) FROM tools t WHERE t.session_id=s.session_id),
                    (SELECT COUNT(*) FROM (SELECT 1 FROM tools t WHERE t.session_id=s.session_id
                       AND t.tool_name IN ('Edit','Write','MultiEdit') AND t.target<>''
                       GROUP BY t.target HAVING COUNT(*)>=4)),
                    s.session_id
                  FROM sessions s LEFT JOIN events e ON e.session_id=s.session_id"#;
    let tail = " GROUP BY s.session_id";
    let map = |r: &rusqlite::Row| {
        Ok(SessComp {
            project: r.get::<_, Option<String>>(0)?.unwrap_or_default(),
            last_activity: r.get::<_, String>(1)?,
            input: r.get(2)?,
            cache_read: r.get(3)?,
            cache_creation: r.get(4)?,
            cost: r.get(5)?,
            tool_total: r.get(6)?,
            tool_err: r.get(7)?,
            rework: r.get(8)?,
            session_id: r.get(9)?,
        })
    };
    let out: Vec<SessComp> = match project {
        Some(p) => {
            let mut st = conn.prepare(&format!("{base} WHERE s.project=?1{time_c}{tail}"))?;
            let rows = st.query_map(params![p], map)?.collect::<rusqlite::Result<_>>()?;
            rows
        }
        None => {
            let mut st = conn.prepare(&format!("{base} WHERE 1=1{time_c}{tail}"))?;
            let rows = st.query_map([], map)?.collect::<rusqlite::Result<_>>()?;
            rows
        }
    };
    Ok(out)
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
        "ALTER TABLE events ADD COLUMN thinking_blocks INTEGER DEFAULT 0",
        "ALTER TABLE events ADD COLUMN cache_savings_usd REAL DEFAULT 0",
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
    let model = e.model.as_deref().unwrap_or("");
    let cost = crate::pricing::cost(
        model,
        e.input_tokens,
        e.output_tokens,
        e.cache_read_tokens,
        e.cache_creation_tokens,
    );
    let savings = crate::pricing::cache_savings(model, e.cache_read_tokens);
    let n = conn.execute(
        r#"INSERT OR IGNORE INTO events
           (event_id,session_id,prompt_id,ts,kind,role,text,thinking,tool_name,tool_input,tool_result,
            model,input_tokens,output_tokens,cache_read_tokens,cache_creation_tokens,cost_usd,cache_savings_usd,tool_error,thinking_blocks,git_branch,cwd)
           VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22)"#,
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
            savings,
            e.tool_error as i64,
            e.thinking_blocks,
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

/// Outcome correlation: gộp metrics theo tag outcome (success/partial/fail/chưa tag).
pub fn outcomes(conn: &Connection, project: Option<&str>, from: Option<&str>) -> Result<Value> {
    use std::collections::BTreeMap;
    // (sessions, cost, tokens, events, tools, errs)
    let mut m: BTreeMap<String, (i64, f64, i64, i64, i64, i64)> = BTreeMap::new();
    let key = |s: String| if s.is_empty() { "(chưa tag)".to_string() } else { s };

    let sql1 = format!(
        r#"SELECT COALESCE(s.outcome,''), COUNT(DISTINCT s.session_id),
                  COALESCE(SUM(e.cost_usd),0),
                  COALESCE(SUM(e.input_tokens)+SUM(e.output_tokens),0),
                  COUNT(e.event_id)
           FROM sessions s LEFT JOIN events e ON e.session_id=s.session_id
           WHERE 1=1{proj}{time} GROUP BY COALESCE(s.outcome,'')"#,
        proj = if project.is_some() { " AND s.project=?1" } else { "" },
        time = since("s.last_activity", from, "AND"),
    );
    {
        let mut st = conn.prepare(&sql1)?;
        let f = |r: &rusqlite::Row| Ok((r.get::<_,String>(0)?, r.get::<_,i64>(1)?, r.get::<_,f64>(2)?, r.get::<_,i64>(3)?, r.get::<_,i64>(4)?));
        let rows: Vec<(String, i64, f64, i64, i64)> = match project {
            Some(p) => st.query_map(params![p], f)?.collect::<rusqlite::Result<_>>()?,
            None => st.query_map([], f)?.collect::<rusqlite::Result<_>>()?,
        };
        for (o, sess, cost, tok, ev) in rows {
            let e = m.entry(key(o)).or_default();
            e.0 = sess; e.1 = cost; e.2 = tok; e.3 = ev;
        }
    }

    let sql2 = format!(
        r#"SELECT COALESCE(s.outcome,''), COUNT(*), COALESCE(SUM(t.is_error),0)
           FROM sessions s JOIN tools t ON t.session_id=s.session_id
           WHERE 1=1{proj}{time} GROUP BY COALESCE(s.outcome,'')"#,
        proj = if project.is_some() { " AND s.project=?1" } else { "" },
        time = since("t.ts_use", from, "AND"),
    );
    {
        let mut st = conn.prepare(&sql2)?;
        let f = |r: &rusqlite::Row| Ok((r.get::<_,String>(0)?, r.get::<_,i64>(1)?, r.get::<_,i64>(2)?));
        let rows: Vec<(String, i64, i64)> = match project {
            Some(p) => st.query_map(params![p], f)?.collect::<rusqlite::Result<_>>()?,
            None => st.query_map([], f)?.collect::<rusqlite::Result<_>>()?,
        };
        for (o, tools, errs) in rows {
            let e = m.entry(key(o)).or_default();
            e.4 = tools; e.5 = errs;
        }
    }

    let out: Vec<Value> = m.into_iter().map(|(o, (sess, cost, tok, ev, tools, errs))| {
        let s = sess.max(1) as f64;
        json!({
            "outcome": o,
            "sessions": sess,
            "cost_usd": cost,
            "avg_cost": cost / s,
            "tokens": tok,
            "avg_tokens": (tok as f64 / s) as i64,
            "events": ev,
            "tools": tools,
            "tool_errors": errs,
            "error_rate": if tools > 0 { errs as f64 / tools as f64 } else { 0.0 },
        })
    }).collect();
    Ok(Value::Array(out))
}

/// Heatmap hoạt động theo (thứ trong tuần, giờ) — UTC.
pub fn heatmap(conn: &Connection, project: Option<&str>, from: Option<&str>) -> Result<Value> {
    let sql = format!(
        r#"SELECT CAST(strftime('%w', e.ts) AS INTEGER), CAST(strftime('%H', e.ts) AS INTEGER), COUNT(*)
           FROM events e JOIN sessions s ON s.session_id=e.session_id
           WHERE e.ts<>''{proj}{time}
           GROUP BY 1,2"#,
        proj = if project.is_some() { " AND s.project=?1" } else { "" },
        time = since("e.ts", from, "AND"),
    );
    let map = |r: &rusqlite::Row| {
        Ok(json!({
            "dow": r.get::<_, i64>(0)?,
            "hour": r.get::<_, i64>(1)?,
            "count": r.get::<_, i64>(2)?,
        }))
    };
    let out: Vec<Value> = match project {
        Some(p) => { let mut s=conn.prepare(&sql)?; let v=s.query_map(params![p],map)?.collect::<rusqlite::Result<_>>()?; v }
        None => { let mut s=conn.prepare(&sql)?; let v=s.query_map([],map)?.collect::<rusqlite::Result<_>>()?; v }
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
    // 4) retry: cùng 1 lệnh Bash chạy lặp nhiều lần
    {
        let mut stmt = conn.prepare(
            r#"SELECT target, COUNT(*) c FROM tools
               WHERE session_id=?1 AND tool_name='Bash' AND target<>''
               GROUP BY target HAVING c>=3 ORDER BY c DESC LIMIT 5"#,
        )?;
        let rows = stmt.query_map(params![session_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })?;
        for row in rows {
            let (target, c) = row?;
            findings.push(json!({
                "kind": "retry",
                "severity": if c >= 5 { "high" } else { "med" },
                "text": format!("Lệnh chạy lại {} lần: {}", c, target),
            }));
        }
    }
    // 5) context bloat: cửa sổ context (cache_read 1 turn) lớn
    {
        let peak: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(cache_read_tokens),0) FROM events WHERE session_id=?1",
                params![session_id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        if peak > 150_000 {
            findings.push(json!({
                "kind": "context",
                "severity": if peak > 300_000 { "high" } else { "med" },
                "text": format!("Context lớn: tới {} token/turn (cân nhắc /compact hoặc chia nhỏ task)", peak),
            }));
        }
    }
    Ok(Value::Array(findings))
}

/// Các bước lỗi trong 1 session (kèm nội dung lỗi) — để debug nhanh.
pub fn errors(conn: &Connection, session_id: &str) -> Result<Value> {
    let mut stmt = conn.prepare(
        r#"SELECT ts, COALESCE(tool_result,'') FROM events
           WHERE session_id=?1 AND tool_error=1 ORDER BY ts"#,
    )?;
    let rows = stmt.query_map(params![session_id], |r| {
        let res: String = r.get(1)?;
        Ok(json!({
            "ts": r.get::<_, Option<String>>(0)?,
            "snippet": res.chars().take(300).collect::<String>(),
        }))
    })?;
    Ok(Value::Array(rows.collect::<rusqlite::Result<_>>()?))
}

/// File agent động nhiều nhất (Edit/Write/Read) — lọc theo project + thời gian.
pub fn hot_files(conn: &Connection, project: Option<&str>, from: Option<&str>) -> Result<Value> {
    let sql = format!(
        r#"SELECT t.target,
                  SUM(CASE WHEN t.tool_name IN ('Edit','Write','MultiEdit') THEN 1 ELSE 0 END) AS edits,
                  SUM(CASE WHEN t.tool_name='Read' THEN 1 ELSE 0 END) AS reads,
                  COUNT(*) AS total
           FROM tools t JOIN sessions s ON s.session_id=t.session_id
           WHERE t.tool_name IN ('Edit','Write','Read','MultiEdit') AND t.target<>''
                 {proj}{time}
           GROUP BY t.target ORDER BY total DESC LIMIT 20"#,
        proj = if project.is_some() { " AND s.project=?1" } else { "" },
        time = since("t.ts_use", from, "AND"),
    );
    let map = |r: &rusqlite::Row| {
        Ok(json!({
            "target": r.get::<_, String>(0)?,
            "edits": r.get::<_, i64>(1)?,
            "reads": r.get::<_, i64>(2)?,
            "total": r.get::<_, i64>(3)?,
        }))
    };
    let out: Vec<Value> = match project {
        Some(p) => { let mut s=conn.prepare(&sql)?; let v=s.query_map(params![p],map)?.collect::<rusqlite::Result<_>>()?; v }
        None => { let mut s=conn.prepare(&sql)?; let v=s.query_map([],map)?.collect::<rusqlite::Result<_>>()?; v }
    };
    Ok(Value::Array(out))
}

/// Thao tác tool chậm nhất — lọc theo project + thời gian.
pub fn slowest(conn: &Connection, project: Option<&str>, from: Option<&str>) -> Result<Value> {
    let sql = format!(
        r#"SELECT t.tool_name, t.target, t.duration_ms, t.session_id, t.is_error
           FROM tools t JOIN sessions s ON s.session_id=t.session_id
           WHERE t.duration_ms IS NOT NULL {proj}{time}
           ORDER BY t.duration_ms DESC LIMIT 15"#,
        proj = if project.is_some() { " AND s.project=?1" } else { "" },
        time = since("t.ts_use", from, "AND"),
    );
    let map = |r: &rusqlite::Row| {
        Ok(json!({
            "tool": r.get::<_, Option<String>>(0)?,
            "target": r.get::<_, Option<String>>(1)?,
            "duration_ms": r.get::<_, Option<i64>>(2)?,
            "session_id": r.get::<_, String>(3)?,
            "is_error": r.get::<_, i64>(4)? != 0,
        }))
    };
    let out: Vec<Value> = match project {
        Some(p) => { let mut s=conn.prepare(&sql)?; let v=s.query_map(params![p],map)?.collect::<rusqlite::Result<_>>()?; v }
        None => { let mut s=conn.prepare(&sql)?; let v=s.query_map([],map)?.collect::<rusqlite::Result<_>>()?; v }
    };
    Ok(Value::Array(out))
}

/// Tool-sequence patterns: cặp tool liên tiếp (A→B) phổ biến nhất.
pub fn sequences(conn: &Connection, project: Option<&str>, from: Option<&str>) -> Result<Value> {
    use std::collections::HashMap;
    let sql = format!(
        r#"SELECT t.session_id, t.tool_name FROM tools t JOIN sessions s ON s.session_id=t.session_id
           WHERE t.tool_name<>''{proj}{time} ORDER BY t.session_id, t.ts_use"#,
        proj = if project.is_some() { " AND s.project=?1" } else { "" },
        time = since("t.ts_use", from, "AND"),
    );
    let f = |r: &rusqlite::Row| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?));
    let rows: Vec<(String, String)> = {
        let mut st = conn.prepare(&sql)?;
        match project {
            Some(p) => st.query_map(params![p], f)?.collect::<rusqlite::Result<_>>()?,
            None => st.query_map([], f)?.collect::<rusqlite::Result<_>>()?,
        }
    };
    let mut pairs: HashMap<(String, String), i64> = HashMap::new();
    let mut prev: Option<(String, String)> = None; // (session, tool)
    for (sess, tool) in rows {
        if let Some((ps, pt)) = &prev {
            if *ps == sess {
                *pairs.entry((pt.clone(), tool.clone())).or_default() += 1;
            }
        }
        prev = Some((sess, tool));
    }
    let mut v: Vec<((String, String), i64)> = pairs.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    let out: Vec<Value> = v
        .into_iter()
        .take(15)
        .map(|((a, b), c)| json!({ "from": a, "to": b, "count": c }))
        .collect();
    Ok(Value::Array(out))
}

/// Chuẩn hoá 1 thông điệp lỗi thành "chữ ký" để gom cụm.
fn error_signature(s: &str) -> String {
    let first = s.lines().find(|l| !l.trim().is_empty()).unwrap_or("").to_lowercase();
    let mut out = String::new();
    let mut prev_digit = false;
    for ch in first.chars() {
        if ch.is_ascii_digit() {
            if !prev_digit { out.push('#'); }
            prev_digit = true;
        } else {
            prev_digit = false;
            out.push(if ch.is_whitespace() { ' ' } else { ch });
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(90).collect()
}

/// #2 Error clustering: gom lỗi giống nhau khắp các session → failure mode hay lặp.
pub fn error_clusters(conn: &Connection, project: Option<&str>, from: Option<&str>) -> Result<Value> {
    use std::collections::HashMap;
    let sql = format!(
        r#"SELECT COALESCE(e.tool_result,'') FROM events e JOIN sessions s ON s.session_id=e.session_id
           WHERE e.tool_error=1{proj}{time}"#,
        proj = if project.is_some() { " AND s.project=?1" } else { "" },
        time = since("e.ts", from, "AND"),
    );
    let f = |r: &rusqlite::Row| r.get::<_, String>(0);
    let rows: Vec<String> = {
        let mut st = conn.prepare(&sql)?;
        match project {
            Some(p) => st.query_map(params![p], f)?.collect::<rusqlite::Result<_>>()?,
            None => st.query_map([], f)?.collect::<rusqlite::Result<_>>()?,
        }
    };
    let mut groups: HashMap<String, (i64, String)> = HashMap::new();
    for r in rows {
        if r.trim().is_empty() { continue; }
        let sig = error_signature(&r);
        if sig.is_empty() { continue; }
        let e = groups.entry(sig).or_insert((0, r.chars().take(160).collect()));
        e.0 += 1;
    }
    let mut v: Vec<(String, (i64, String))> = groups.into_iter().collect();
    v.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
    let out: Vec<Value> = v.into_iter().take(15)
        .map(|(sig, (c, sample))| json!({ "signature": sig, "count": c, "sample": sample }))
        .collect();
    Ok(Value::Array(out))
}

/// #3 Skill & subagent usage: Task/Skill/SlashCommand dùng nhiều, cost/lỗi.
pub fn agents(conn: &Connection, project: Option<&str>, from: Option<&str>) -> Result<Value> {
    let sql = format!(
        r#"SELECT t.tool_name, COALESCE(NULLIF(t.target,''),'(không rõ)'),
                  COUNT(*), COALESCE(SUM(t.is_error),0), COALESCE(AVG(t.duration_ms),0)
           FROM tools t JOIN sessions s ON s.session_id=t.session_id
           WHERE t.tool_name IN ('Task','Skill','SlashCommand'){proj}{time}
           GROUP BY t.tool_name, t.target ORDER BY COUNT(*) DESC LIMIT 25"#,
        proj = if project.is_some() { " AND s.project=?1" } else { "" },
        time = since("t.ts_use", from, "AND"),
    );
    let map = |r: &rusqlite::Row| {
        Ok(json!({
            "kind": r.get::<_, String>(0)?,
            "name": r.get::<_, String>(1)?,
            "count": r.get::<_, i64>(2)?,
            "errors": r.get::<_, i64>(3)?,
            "avg_ms": r.get::<_, f64>(4)? as i64,
        }))
    };
    let out: Vec<Value> = match project {
        Some(p) => { let mut st=conn.prepare(&sql)?; let v=st.query_map(params![p],map)?.collect::<rusqlite::Result<_>>()?; v }
        None => { let mut st=conn.prepare(&sql)?; let v=st.query_map([],map)?.collect::<rusqlite::Result<_>>()?; v }
    };
    Ok(Value::Array(out))
}

/// #6+#7 Prompt quality (bucket theo độ dài) + gợi ý dùng model rẻ hơn ([Inference] heuristic).
pub fn prompt_insights(conn: &Connection, project: Option<&str>, from: Option<&str>) -> Result<Value> {
    let sql = format!(
        r#"SELECT
             (SELECT LENGTH(e2.text) FROM events e2 WHERE e2.session_id=ev.session_id
                AND e2.prompt_id=ev.prompt_id AND e2.role='user' AND e2.text<>'' ORDER BY e2.ts LIMIT 1),
             COUNT(*),
             SUM(CASE WHEN ev.tool_name IS NOT NULL AND ev.tool_name<>'' THEN 1 ELSE 0 END),
             COALESCE(SUM(ev.output_tokens),0),
             COALESCE(SUM(ev.cost_usd),0),
             MAX(COALESCE(ev.model,''))
           FROM events ev JOIN sessions s ON s.session_id=ev.session_id
           WHERE ev.prompt_id IS NOT NULL AND ev.prompt_id<>''{proj}{time}
           GROUP BY ev.session_id, ev.prompt_id"#,
        proj = if project.is_some() { " AND s.project=?1" } else { "" },
        time = since("ev.ts", from, "AND"),
    );
    let f = |r: &rusqlite::Row| Ok((
        r.get::<_, Option<i64>>(0)?.unwrap_or(0),
        r.get::<_, i64>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?,
        r.get::<_, f64>(4)?, r.get::<_, String>(5)?,
    ));
    let rows: Vec<(i64, i64, i64, i64, f64, String)> = {
        let mut st = conn.prepare(&sql)?;
        match project {
            Some(p) => st.query_map(params![p], f)?.collect::<rusqlite::Result<_>>()?,
            None => st.query_map([], f)?.collect::<rusqlite::Result<_>>()?,
        }
    };
    // 3 bucket độ dài prompt
    let labels = ["ngắn (<200)", "vừa (200–1000)", "dài (>1000)"];
    let mut b = [(0i64, 0i64, 0f64); 3]; // (count, turns_sum, cost_sum)
    let mut cand = 0i64;
    let mut cand_cost = 0f64;
    for (plen, turns, tool_turns, output, cost, model) in &rows {
        let idx = if *plen < 200 { 0 } else if *plen <= 1000 { 1 } else { 2 };
        b[idx].0 += 1; b[idx].1 += turns; b[idx].2 += cost;
        // candidate dùng model rẻ hơn: opus + ít tool + output nhỏ
        if model.to_lowercase().contains("opus") && *tool_turns <= 2 && *output < 3000 {
            cand += 1; cand_cost += cost;
        }
    }
    let buckets: Vec<Value> = labels.iter().enumerate().map(|(i, l)| {
        let (c, t, cost) = b[i];
        let cf = c.max(1) as f64;
        json!({ "bucket": l, "prompts": c, "avg_turns": (t as f64 / cf), "avg_cost": cost / cf })
    }).collect();
    Ok(json!({
        "buckets": buckets,
        "recommendation": {
            "candidates": cand,
            "candidate_cost": cand_cost,
            "note": "[Inference] prompt dùng Opus nhưng ít tool & output nhỏ → có thể dùng model rẻ hơn (Haiku/Sonnet)."
        }
    }))
}

/// #8 Weekly digest: 7 ngày gần nhất vs 7 ngày trước đó.
pub fn digest(conn: &Connection, project: Option<&str>) -> Result<Value> {
    use chrono::{Duration, Utc};
    let d7 = (Utc::now() - Duration::days(7)).to_rfc3339();
    let d14 = (Utc::now() - Duration::days(14)).to_rfc3339();
    let proj = if project.is_some() { " AND s.project=?1" } else { "" };
    let window = |lo: &str, hi: Option<&str>| -> Result<(i64, f64, f64)> {
        let hi_c = hi.map(|h| format!(" AND e.ts < '{h}'")).unwrap_or_default();
        let sql = format!(
            r#"SELECT COUNT(DISTINCT e.session_id), COALESCE(SUM(e.cost_usd),0), COALESCE(SUM(e.cache_savings_usd),0)
               FROM events e JOIN sessions s ON s.session_id=e.session_id
               WHERE e.ts >= '{lo}'{hi_c}{proj}"#
        );
        let g = |r: &rusqlite::Row| Ok((r.get::<_, i64>(0)?, r.get::<_, f64>(1)?, r.get::<_, f64>(2)?));
        match project {
            Some(p) => conn.query_row(&sql, params![p], g).map_err(Into::into),
            None => conn.query_row(&sql, [], g).map_err(Into::into),
        }
    };
    let (s_now, c_now, sav_now) = window(&d7, None)?;
    let (s_prev, c_prev, _) = window(&d14, Some(&d7))?;
    Ok(json!({
        "sessions": s_now, "cost": c_now, "savings": sav_now,
        "prev_sessions": s_prev, "prev_cost": c_prev,
        "cost_delta_pct": if c_prev > 0.0 { (c_now - c_prev) / c_prev * 100.0 } else { 0.0 }
    }))
}

/// #1 Recovery path: với mỗi tool, lỗi liên tiếp dài nhất + TB số lần lỗi tới khi gỡ được.
pub fn recovery(conn: &Connection, project: Option<&str>, from: Option<&str>) -> Result<Value> {
    use std::collections::HashMap;
    let sql = format!(
        r#"SELECT t.session_id, t.tool_name, t.is_error
           FROM tools t JOIN sessions s ON s.session_id=t.session_id
           WHERE t.tool_name<>''{proj}{time} ORDER BY t.session_id, t.ts_use"#,
        proj = if project.is_some() { " AND s.project=?1" } else { "" },
        time = since("t.ts_use", from, "AND"),
    );
    let f = |r: &rusqlite::Row| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?));
    let rows: Vec<(String, String, i64)> = {
        let mut st = conn.prepare(&sql)?;
        match project {
            Some(p) => st.query_map(params![p], f)?.collect::<rusqlite::Result<_>>()?,
            None => st.query_map([], f)?.collect::<rusqlite::Result<_>>()?,
        }
    };
    // per tool: tổng lỗi, số chuỗi đã gỡ, tổng độ dài chuỗi đã gỡ, chuỗi dài nhất
    #[derive(Default)]
    struct Acc { errors: i64, recovered: i64, len_sum: i64, max_streak: i64 }
    let mut acc: HashMap<String, Acc> = HashMap::new();
    let mut cur: HashMap<(String, String), i64> = HashMap::new(); // (session,tool) -> streak hiện tại
    for (sess, tool, is_err) in &rows {
        let k = (sess.clone(), tool.clone());
        let a = acc.entry(tool.clone()).or_default();
        if *is_err == 1 {
            a.errors += 1;
            let c = cur.entry(k).or_insert(0);
            *c += 1;
            if *c > a.max_streak { a.max_streak = *c; }
        } else if let Some(c) = cur.get_mut(&k) {
            if *c > 0 { a.recovered += 1; a.len_sum += *c; *c = 0; }
        }
    }
    let mut v: Vec<Value> = acc.into_iter().filter(|(_, a)| a.errors > 0).map(|(tool, a)| {
        json!({
            "tool": tool,
            "errors": a.errors,
            "recovered": a.recovered,
            "avg_to_recover": if a.recovered > 0 { a.len_sum as f64 / a.recovered as f64 } else { 0.0 },
            "max_streak": a.max_streak,
        })
    }).collect();
    v.sort_by(|a, b| b["errors"].as_i64().unwrap_or(0).cmp(&a["errors"].as_i64().unwrap_or(0)));
    Ok(Value::Array(v))
}

/// #2 Prompt style → kết quả: phân loại prompt (có ví dụ/đường dẫn/độ dài) đối chiếu turns/cost/lỗi.
pub fn prompt_styles(conn: &Connection, project: Option<&str>, from: Option<&str>) -> Result<Value> {
    let sql = format!(
        r#"SELECT
             (SELECT e2.text FROM events e2 WHERE e2.session_id=ev.session_id
                AND e2.prompt_id=ev.prompt_id AND e2.role='user' AND e2.text<>'' ORDER BY e2.ts LIMIT 1),
             COUNT(*),
             COALESCE(SUM(ev.cost_usd),0),
             COALESCE(SUM(CASE WHEN ev.tool_error=1 THEN 1 ELSE 0 END),0)
           FROM events ev JOIN sessions s ON s.session_id=ev.session_id
           WHERE ev.prompt_id IS NOT NULL AND ev.prompt_id<>''{proj}{time}
           GROUP BY ev.session_id, ev.prompt_id"#,
        proj = if project.is_some() { " AND s.project=?1" } else { "" },
        time = since("ev.ts", from, "AND"),
    );
    let f = |r: &rusqlite::Row| Ok((
        r.get::<_, Option<String>>(0)?.unwrap_or_default(),
        r.get::<_, i64>(1)?, r.get::<_, f64>(2)?, r.get::<_, i64>(3)?,
    ));
    let rows: Vec<(String, i64, f64, i64)> = {
        let mut st = conn.prepare(&sql)?;
        match project {
            Some(p) => st.query_map(params![p], f)?.collect::<rusqlite::Result<_>>()?,
            None => st.query_map([], f)?.collect::<rusqlite::Result<_>>()?,
        }
    };
    // 4 style (ưu tiên theo thứ tự): có ví dụ/code → có đường dẫn → ngắn/mơ hồ → mô tả thường
    let labels = ["có ví dụ/code", "có đường dẫn file", "ngắn/mơ hồ (<80)", "mô tả thường"];
    let path_re = regex::Regex::new(r"[\w./-]+\.[a-zA-Z]{1,5}\b").unwrap();
    let mut b = [(0i64, 0i64, 0f64, 0i64); 4]; // (prompts, turns_sum, cost_sum, errors_sum)
    for (text, turns, cost, errs) in &rows {
        let idx = if text.contains('`') {
            0
        } else if path_re.is_match(text) {
            1
        } else if text.chars().count() < 80 {
            2
        } else {
            3
        };
        b[idx].0 += 1; b[idx].1 += turns; b[idx].2 += cost; b[idx].3 += errs;
    }
    let out: Vec<Value> = labels.iter().enumerate().map(|(i, l)| {
        let (c, t, cost, errs) = b[i];
        let cf = c.max(1) as f64;
        json!({
            "style": l, "prompts": c,
            "avg_turns": t as f64 / cf, "avg_cost": cost / cf,
            "avg_errors": errs as f64 / cf,
        })
    }).collect();
    Ok(Value::Array(out))
}

/// #4 Cache efficiency advisor: session cache-hit thấp + cost đáng kể → cảnh báo & lý do.
pub fn cache_advisor(conn: &Connection, project: Option<&str>, from: Option<&str>) -> Result<Value> {
    let comps = session_components(conn, project, from)?;
    let mut v: Vec<(f64, Value)> = Vec::new();
    for c in &comps {
        let denom = (c.input + c.cache_read + c.cache_creation).max(1) as f64;
        let hit = c.cache_read as f64 / denom;
        if c.cost >= 0.5 && hit < 0.4 {
            let note = if c.cache_creation > c.cache_read * 2 {
                "Tạo cache nhiều hơn đọc lại — context thay đổi liên tục / session ngắn."
            } else if c.input > c.cache_read {
                "Input chưa cache lớn — giữ ngữ cảnh ổn định, tránh sửa lại file lớn nhiều lần."
            } else {
                "Cache-hit thấp — xem lại cấu trúc prompt/đính kèm để tái dùng cache."
            };
            v.push((c.cost, json!({
                "session_id": c.session_id, "project": c.project,
                "cache_hit": hit, "cost": c.cost,
                "input_tokens": c.input, "cache_read_tokens": c.cache_read,
                "note": note,
            })));
        }
    }
    v.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    Ok(Value::Array(v.into_iter().take(15).map(|(_, x)| x).collect()))
}

/// #5 Model right-sizing: theo nhóm công việc, cost hiện tại vs nếu chạy Sonnet/Haiku.
pub fn model_rightsizing(conn: &Connection, project: Option<&str>, from: Option<&str>) -> Result<Value> {
    use std::collections::BTreeMap;
    let sql = format!(
        r#"SELECT
             CASE
               WHEN e.tool_name IN ('Read','Grep','Glob','LS') THEN 'đọc/tìm'
               WHEN e.tool_name IN ('Edit','Write','MultiEdit','NotebookEdit') THEN 'sửa file'
               WHEN e.tool_name='Bash' THEN 'bash'
               WHEN e.tool_name IN ('Task','Skill') THEN 'subagent'
               ELSE 'chat/khác' END AS cat,
             COALESCE(e.model,''),
             COALESCE(SUM(e.input_tokens),0), COALESCE(SUM(e.output_tokens),0),
             COALESCE(SUM(e.cache_read_tokens),0), COALESCE(SUM(e.cache_creation_tokens),0),
             COALESCE(SUM(e.cost_usd),0), COUNT(*)
           FROM events e JOIN sessions s ON s.session_id=e.session_id
           WHERE COALESCE(e.model,'')<>''{proj}{time}
           GROUP BY cat, COALESCE(e.model,'')"#,
        proj = if project.is_some() { " AND s.project=?1" } else { "" },
        time = since("e.ts", from, "AND"),
    );
    let f = |r: &rusqlite::Row| Ok((
        r.get::<_, String>(0)?, r.get::<_, String>(1)?,
        r.get::<_, i64>(2)?, r.get::<_, i64>(3)?, r.get::<_, i64>(4)?, r.get::<_, i64>(5)?,
        r.get::<_, f64>(6)?, r.get::<_, i64>(7)?,
    ));
    let rows: Vec<(String, String, i64, i64, i64, i64, f64, i64)> = {
        let mut st = conn.prepare(&sql)?;
        match project {
            Some(p) => st.query_map(params![p], f)?.collect::<rusqlite::Result<_>>()?,
            None => st.query_map([], f)?.collect::<rusqlite::Result<_>>()?,
        }
    };
    // gộp theo nhóm: (cost, in, out, cache_read, cache_creation, events, has_opus)
    let mut m: BTreeMap<String, (f64, i64, i64, i64, i64, i64, bool)> = BTreeMap::new();
    for (cat, model, in_, out, cr, cc, cost, n) in rows {
        let e = m.entry(cat).or_default();
        e.0 += cost; e.1 += in_; e.2 += out; e.3 += cr; e.4 += cc; e.5 += n;
        if model.to_lowercase().contains("opus") { e.6 = true; }
    }
    let mut out: Vec<Value> = m.into_iter().map(|(cat, (cost, in_, out, cr, cc, n, has_opus))| {
        let as_sonnet = crate::pricing::cost("claude-sonnet", in_, out, cr, cc);
        let as_haiku = crate::pricing::cost("claude-haiku", in_, out, cr, cc);
        // chỉ tác vụ máy móc mới hợp lý để hạ Opus -> Haiku (đọc/tìm, bash)
        let simple = cat == "đọc/tìm" || cat == "bash";
        json!({
            "category": cat, "events": n, "current_cost": cost,
            "as_sonnet": as_sonnet, "as_haiku": as_haiku,
            "save_sonnet": (cost - as_sonnet).max(0.0),
            "save_haiku": (cost - as_haiku).max(0.0),
            "has_opus": has_opus,
            "simple": simple,
        })
    }).collect();
    out.sort_by(|a, b| b["current_cost"].as_f64().unwrap_or(0.0)
        .partial_cmp(&a["current_cost"].as_f64().unwrap_or(0.0)).unwrap_or(std::cmp::Ordering::Equal));
    Ok(Value::Array(out))
}

/// #7 Health trend theo tuần (ISO year-week): điểm sức khỏe TB + cost.
pub fn health_trend(conn: &Connection, project: Option<&str>, from: Option<&str>) -> Result<Value> {
    use std::collections::BTreeMap;
    let comps = session_components(conn, project, from)?;
    let mut weeks: BTreeMap<String, (f64, i64, f64)> = BTreeMap::new(); // (health_sum, sessions, cost_sum)
    for c in &comps {
        if c.last_activity.is_empty() { continue; }
        let wk = week_key(&c.last_activity);
        let h = health_score(c.input, c.cache_read, c.cache_creation, c.tool_total, c.tool_err, c.rework);
        let e = weeks.entry(wk).or_default();
        e.0 += h as f64; e.1 += 1; e.2 += c.cost;
    }
    let out: Vec<Value> = weeks.into_iter().map(|(wk, (hs, n, cost))| {
        json!({ "week": wk, "sessions": n, "avg_health": if n > 0 { hs / n as f64 } else { 0.0 }, "cost": cost })
    }).collect();
    Ok(Value::Array(out))
}

/// #8 Repo leaderboard: xếp hạng repo theo cost/health/rework/error.
pub fn leaderboard(conn: &Connection, from: Option<&str>) -> Result<Value> {
    use std::collections::BTreeMap;
    let comps = session_components(conn, None, from)?;
    // (sessions, cost, health_sum, rework, tool_total, tool_err)
    let mut m: BTreeMap<String, (i64, f64, f64, i64, i64, i64)> = BTreeMap::new();
    for c in &comps {
        let key = if c.project.is_empty() { "(không rõ)".to_string() } else { c.project.clone() };
        let h = health_score(c.input, c.cache_read, c.cache_creation, c.tool_total, c.tool_err, c.rework);
        let e = m.entry(key).or_default();
        e.0 += 1; e.1 += c.cost; e.2 += h as f64; e.3 += c.rework; e.4 += c.tool_total; e.5 += c.tool_err;
    }
    let mut out: Vec<Value> = m.into_iter().map(|(p, (n, cost, hs, rw, tt, te))| {
        json!({
            "project": p, "sessions": n, "cost": cost,
            "avg_health": if n > 0 { hs / n as f64 } else { 0.0 },
            "rework": rw,
            "error_rate": if tt > 0 { te as f64 / tt as f64 } else { 0.0 },
        })
    }).collect();
    out.sort_by(|a, b| b["cost"].as_f64().unwrap_or(0.0)
        .partial_cmp(&a["cost"].as_f64().unwrap_or(0.0)).unwrap_or(std::cmp::Ordering::Equal));
    Ok(Value::Array(out))
}

pub fn set_tag(conn: &Connection, session_id: &str, tag: &str, outcome: &str) -> Result<()> {
    conn.execute(
        "UPDATE sessions SET tag=?2, outcome=?3 WHERE session_id=?1",
        params![session_id, tag, outcome],
    )?;
    Ok(())
}

/// Cộng dồn metrics OTEL cho 1 session (giá trị coi như delta — [Unverified] temporality).
pub fn upsert_otel(
    conn: &Connection,
    session_id: &str,
    cost: f64,
    loc_added: i64,
    loc_removed: i64,
    commits: i64,
    prs: i64,
) -> Result<()> {
    conn.execute(
        r#"INSERT INTO session_metrics(session_id,otel_cost,loc_added,loc_removed,commits,prs,updated_at)
           VALUES(?1,?2,?3,?4,?5,?6,?7)
           ON CONFLICT(session_id) DO UPDATE SET
             otel_cost   = otel_cost   + excluded.otel_cost,
             loc_added   = loc_added   + excluded.loc_added,
             loc_removed = loc_removed + excluded.loc_removed,
             commits     = commits     + excluded.commits,
             prs         = prs         + excluded.prs,
             updated_at  = excluded.updated_at"#,
        params![session_id, cost, loc_added, loc_removed, commits, prs, chrono::Utc::now().to_rfc3339()],
    )?;
    Ok(())
}

/// Metrics OTEL của 1 session (LOC/commits/cost chính xác nếu bật OTEL).
pub fn session_metric(conn: &Connection, session_id: &str) -> Result<Value> {
    conn.query_row(
        "SELECT otel_cost,loc_added,loc_removed,commits,prs FROM session_metrics WHERE session_id=?1",
        params![session_id],
        |r| Ok(json!({
            "otel_cost": r.get::<_, f64>(0)?,
            "loc_added": r.get::<_, i64>(1)?,
            "loc_removed": r.get::<_, i64>(2)?,
            "commits": r.get::<_, i64>(3)?,
            "prs": r.get::<_, i64>(4)?,
        })),
    )
    .or_else(|_| Ok(json!({"otel_cost":0.0,"loc_added":0,"loc_removed":0,"commits":0,"prs":0})))
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
            r#"UPDATE events SET
                 cost_usd = (input_tokens*?2 + output_tokens*?3 + cache_read_tokens*?4 + cache_creation_tokens*?5)/1000000.0,
                 cache_savings_usd = cache_read_tokens * (CASE WHEN ?2>?4 THEN ?2-?4 ELSE 0 END)/1000000.0
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
                         COALESCE(s.tag,''), COALESCE(s.outcome,''),
                         (SELECT COUNT(*) FROM tools t WHERE t.session_id=s.session_id),
                         (SELECT COALESCE(SUM(is_error),0) FROM tools t WHERE t.session_id=s.session_id),
                         (SELECT COUNT(*) FROM (SELECT 1 FROM tools t WHERE t.session_id=s.session_id
                            AND t.tool_name IN ('Edit','Write','MultiEdit') AND t.target<>''
                            GROUP BY t.target HAVING COUNT(*)>=4))
                  FROM sessions s LEFT JOIN events e ON e.session_id = s.session_id"#;
    let tail = " GROUP BY s.session_id ORDER BY s.last_activity DESC";
    let map = |r: &rusqlite::Row| {
        let input: i64 = r.get(5)?;
        let cache_read: i64 = r.get(7)?;
        let cache_creation: i64 = r.get(8)?;
        let tool_total: i64 = r.get(13)?;
        let tool_err: i64 = r.get(14)?;
        let rework: i64 = r.get(15)?;
        // health score 0..100 (đơn giản, có thể tinh chỉnh)
        let err_rate = if tool_total > 0 { tool_err as f64 / tool_total as f64 } else { 0.0 };
        let denom = (input + cache_read + cache_creation).max(1) as f64;
        let cache_hit = cache_read as f64 / denom;
        let mut h = 100.0 - err_rate * 40.0 - (rework as f64) * 5.0;
        if cache_hit < 0.5 { h -= (0.5 - cache_hit) * 20.0; }
        let health = h.clamp(0.0, 100.0) as i64;
        Ok(json!({
            "session_id": r.get::<_, String>(0)?,
            "project": r.get::<_, Option<String>>(1)?,
            "git_branch": r.get::<_, Option<String>>(2)?,
            "started_at": r.get::<_, Option<String>>(3)?,
            "last_activity": r.get::<_, Option<String>>(4)?,
            "input_tokens": input,
            "output_tokens": r.get::<_, i64>(6)?,
            "cache_read_tokens": cache_read,
            "cache_creation_tokens": cache_creation,
            "cost_usd": r.get::<_, f64>(9)?,
            "events": r.get::<_, i64>(10)?,
            "tag": r.get::<_, String>(11)?,
            "outcome": r.get::<_, String>(12)?,
            "tool_total": tool_total,
            "tool_errors": tool_err,
            "rework_files": rework,
            "health": health,
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

/// Timeline 1 session. `after` (ISO ts) chỉ lấy event mới hơn — dùng cho auto-follow.
pub fn session_events(conn: &Connection, session_id: &str, after: Option<&str>) -> Result<Value> {
    let extra = match after {
        Some(a) if !a.is_empty() => " AND ts > ?2",
        _ => "",
    };
    let sql = format!(
        r#"SELECT ts,kind,role,text,thinking,tool_name,tool_input,tool_result,model,
                  input_tokens,output_tokens,cache_read_tokens,cache_creation_tokens,prompt_id,tool_error,cost_usd
           FROM events WHERE session_id = ?1{extra} ORDER BY ts, rowid"#
    );
    let mut stmt = conn.prepare(&sql)?;
    let map = |r: &rusqlite::Row| {
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
            "cost_usd": r.get::<_, f64>(15)?,
        }))
    };
    let out: Vec<Value> = match after {
        Some(a) if !a.is_empty() => {
            let rows = stmt.query_map(params![session_id, a], map)?.collect::<rusqlite::Result<_>>()?;
            rows
        }
        _ => {
            let rows = stmt.query_map(params![session_id], map)?.collect::<rusqlite::Result<_>>()?;
            rows
        }
    };
    Ok(Value::Array(out))
}

/// Các session đang/mới hoạt động (cho view Live): kèm hành động cuối + cost/token hiện tại.
pub fn live_sessions(conn: &Connection, project: Option<&str>, since_ts: &str) -> Result<Value> {
    let base = r#"SELECT s.session_id, s.project, s.git_branch, s.last_activity,
                    COALESCE(SUM(e.cost_usd),0),
                    COALESCE(SUM(e.input_tokens),0),
                    COALESCE(SUM(e.output_tokens),0),
                    COALESCE(SUM(e.cache_read_tokens),0),
                    COUNT(e.event_id),
                    (SELECT COALESCE(NULLIF(e2.tool_name,''), substr(e2.text,1,90))
                       FROM events e2 WHERE e2.session_id=s.session_id
                         AND (COALESCE(e2.tool_name,'')<>'' OR COALESCE(e2.text,'')<>'')
                       ORDER BY e2.ts DESC, e2.rowid DESC LIMIT 1),
                    (SELECT COALESCE(e3.tool_name,'') FROM events e3 WHERE e3.session_id=s.session_id
                       AND COALESCE(e3.tool_name,'')<>'' ORDER BY e3.ts DESC, e3.rowid DESC LIMIT 1),
                    (SELECT COALESCE(e4.model,'') FROM events e4 WHERE e4.session_id=s.session_id
                       AND COALESCE(e4.model,'')<>'' ORDER BY e4.ts DESC, e4.rowid DESC LIMIT 1)
                  FROM sessions s LEFT JOIN events e ON e.session_id=s.session_id
                  WHERE s.last_activity >= ?1"#;
    let tail = " GROUP BY s.session_id ORDER BY s.last_activity DESC";
    let map = |r: &rusqlite::Row| {
        Ok(json!({
            "session_id": r.get::<_, String>(0)?,
            "project": r.get::<_, Option<String>>(1)?,
            "git_branch": r.get::<_, Option<String>>(2)?,
            "last_activity": r.get::<_, Option<String>>(3)?,
            "cost_usd": r.get::<_, f64>(4)?,
            "input_tokens": r.get::<_, i64>(5)?,
            "output_tokens": r.get::<_, i64>(6)?,
            "cache_read_tokens": r.get::<_, i64>(7)?,
            "events": r.get::<_, i64>(8)?,
            "last_action": r.get::<_, Option<String>>(9)?,
            "last_tool": r.get::<_, Option<String>>(10)?,
            "model": r.get::<_, Option<String>>(11)?,
        }))
    };
    let out: Vec<Value> = match project {
        Some(p) => {
            let mut stmt = conn.prepare(&format!("{base} AND s.project=?2{tail}"))?;
            let rows = stmt.query_map(params![since_ts, p], map)?.collect::<rusqlite::Result<_>>()?;
            rows
        }
        None => {
            let mut stmt = conn.prepare(&format!("{base}{tail}"))?;
            let rows = stmt.query_map(params![since_ts], map)?.collect::<rusqlite::Result<_>>()?;
            rows
        }
    };
    Ok(Value::Array(out))
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
             COALESCE(SUM(cost_usd),0),
             COALESCE(SUM(cache_savings_usd),0),
             (SELECT COALESCE(SUM(loc_added),0) FROM session_metrics),
             (SELECT COALESCE(SUM(loc_removed),0) FROM session_metrics),
             (SELECT COALESCE(SUM(commits),0) FROM session_metrics),
             (SELECT COALESCE(SUM(otel_cost),0) FROM session_metrics)
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
                "cache_savings_usd": r.get::<_, f64>(6)?,
                "loc_added": r.get::<_, i64>(7)?,
                "loc_removed": r.get::<_, i64>(8)?,
                "commits": r.get::<_, i64>(9)?,
                "otel_cost": r.get::<_, f64>(10)?,
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
                  COALESCE(SUM(LENGTH(ev.thinking)),0),
                  COALESCE(SUM(ev.thinking_blocks),0),
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
            "thinking_chars": r.get::<_, i64>(7)?,
            "thinking_blocks": r.get::<_, i64>(8)?,
            "started_at": r.get::<_, Option<String>>(9)?,
            "prompt": r.get::<_, Option<String>>(10)?,
        }))
    })?;
    Ok(Value::Array(rows.collect::<rusqlite::Result<_>>()?))
}
