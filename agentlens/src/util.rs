use std::path::Path;

/// Tên repo/project của một session.
/// Ưu tiên **git root** (đi ngược lên tìm `.git`) để gom "session theo repo" cho đúng
/// (vd cwd `/home/user/workflow-agent/agentlens` -> repo `workflow-agent`).
/// Nếu không thấy `.git` (hoặc path không tồn tại trên máy này) -> dùng thư mục cuối của cwd.
pub fn repo_name(cwd: &str) -> String {
    if cwd.is_empty() {
        return String::new();
    }
    let mut dir = Path::new(cwd);
    loop {
        if dir.join(".git").exists() {
            return basename(dir, cwd);
        }
        match dir.parent() {
            Some(p) => dir = p,
            None => break,
        }
    }
    basename(Path::new(cwd), cwd)
}

fn basename(p: &Path, fallback: &str) -> String {
    p.file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| fallback.to_string())
}
