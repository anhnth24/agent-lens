//! Tail transcript JSONL: quét ~/.claude/projects + các transcript_path do hook báo,
//! đọc phần mới theo byte-offset, parse và ghi events (idempotent qua event_id).

use crate::{jsonl, store, util::repo_name, AppState};
use std::{
    collections::HashMap,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

pub async fn run(state: AppState) {
    let mut offsets: HashMap<PathBuf, u64> = HashMap::new();
    // prompt_id gần nhất theo session (chỉ dòng user có promptId) -> gán cho dòng assistant kế tiếp
    let mut last_prompt: HashMap<String, String> = HashMap::new();
    loop {
        let mut paths: Vec<PathBuf> = Vec::new();
        if state.projects_dir.exists() {
            for entry in walkdir::WalkDir::new(&state.projects_dir)
                .into_iter()
                .filter_map(|x| x.ok())
            {
                if entry.file_type().is_file()
                    && entry.path().extension().map(|x| x == "jsonl").unwrap_or(false)
                {
                    paths.push(entry.path().to_path_buf());
                }
            }
        }
        if let Ok(extra) = state.extra_paths.lock() {
            for p in extra.iter() {
                if !paths.contains(p) {
                    paths.push(p.clone());
                }
            }
        }
        let mut changed = false;
        for path in paths {
            match process_file(&state, &path, &mut offsets, &mut last_prompt) {
                Ok(n) if n > 0 => changed = true,
                Ok(_) => {}
                Err(err) => tracing::warn!("tail {:?}: {err}", path),
            }
        }
        if changed {
            let _ = state.events_tx.send(()); // báo WS refresh (bỏ qua nếu không có subscriber)
        }
        tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
    }
}

fn process_file(
    state: &AppState,
    path: &Path,
    offsets: &mut HashMap<PathBuf, u64>,
    last_prompt: &mut HashMap<String, String>,
) -> anyhow::Result<usize> {
    let mut f = std::fs::File::open(path)?;
    let len = f.metadata()?.len();
    let mut off = *offsets.get(path).unwrap_or(&0);
    if len < off {
        off = 0; // file bị rotate/truncate -> đọc lại từ đầu (dedup lo phần trùng)
    }
    if len == off {
        return Ok(0);
    }
    f.seek(SeekFrom::Start(off))?;
    let mut buf = String::new();
    f.read_to_string(&mut buf)?;

    // chỉ xử lý tới newline cuối; phần dòng dở để vòng sau
    let consume_to = match buf.rfind('\n') {
        Some(i) => i + 1,
        None => return Ok(0), // chưa có dòng hoàn chỉnh
    };
    let chunk = &buf[..consume_to];
    let transcript_path = path.to_string_lossy().to_string();
    let mut inserted = 0usize;

    {
        let conn = state.db.lock().unwrap();
        for line in chunk.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Some(mut entry) = jsonl::parse(line) {
                // propagate prompt_id: user có promptId -> nhớ; dòng sau (assistant) kế thừa
                match &entry.prompt_id {
                    Some(pid) => {
                        last_prompt.insert(entry.session_id.clone(), pid.clone());
                    }
                    None => {
                        if let Some(pid) = last_prompt.get(&entry.session_id) {
                            entry.prompt_id = Some(pid.clone());
                        }
                    }
                }
                let project = repo_name(entry.cwd.as_deref().unwrap_or(""));
                let _ = store::upsert_session_from_entry(&conn, &entry, &project, &transcript_path);
                if let Ok(n) = store::insert_event(&conn, &entry, &project) {
                    inserted += n;
                }
                for tu in &entry.tool_uses {
                    let _ = store::insert_tool_use(
                        &conn,
                        &entry.session_id,
                        entry.prompt_id.as_deref(),
                        &tu.id,
                        &tu.name,
                        &entry.ts,
                    );
                }
                for tr in &entry.tool_results {
                    let _ = store::complete_tool(&conn, &tr.tool_use_id, &entry.ts, tr.is_error);
                }
            }
        }
    }

    offsets.insert(path.to_path_buf(), off + consume_to as u64);
    Ok(inserted)
}
