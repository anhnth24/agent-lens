//! FR-8 (tùy chọn): tóm tắt session + gợi ý cải thiện workflow bằng LLM.
//! Redaction chạy TRƯỚC khi gửi. 1 provider (Anthropic).
//!
//! Hai backend (tự chọn, hoặc ép bằng AGENTLENS_LLM_BACKEND=api|cli):
//!   • api  — gọi Messages API trực tiếp bằng ANTHROPIC_API_KEY (pay-as-you-go).
//!   • cli  — chạy `claude -p` của Claude Code; kế thừa **login subscription**
//!            (Pro/Max) trên máy → usage trừ vào credit Agent SDK hằng tháng,
//!            không tốn API key riêng. Yêu cầu đã cài Claude Code + đã /login.
//! Mặc định: có ANTHROPIC_API_KEY → api; nếu không, có `claude` trên PATH → cli.
//! Model qua AGENTLENS_MODEL (api: Haiku 4.5; cli: alias "haiku").

use anyhow::{anyhow, Result};
use regex::Regex;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::RwLock;

const ENDPOINT: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const DEFAULT_MODEL: &str = "claude-haiku-4-5";
const MAX_INPUT_CHARS: usize = 12_000;

/// Override model chọn từ UI (footer). Ưu tiên hơn AGENTLENS_MODEL env.
static MODEL_OVERRIDE: RwLock<Option<String>> = RwLock::new(None);

/// Các model cho combobox ở footer (full ID — chạy cho cả backend api & cli).
pub const MODEL_CHOICES: &[(&str, &str)] = &[
    ("claude-haiku-4-5", "Haiku 4.5 · rẻ, nhanh"),
    ("claude-sonnet-4-6", "Sonnet 4.6 · cân bằng"),
    ("claude-opus-4-8", "Opus 4.8 · mạnh nhất"),
];

/// Đặt override model (None/rỗng = xóa, quay về env/default).
pub fn set_model_override(m: Option<String>) {
    let val = m.and_then(|s| {
        let t = s.trim().to_string();
        if t.is_empty() { None } else { Some(t) }
    });
    if let Ok(mut w) = MODEL_OVERRIDE.write() {
        *w = val;
    }
}

/// Model hiệu lực: override (UI) > AGENTLENS_MODEL env > mặc định.
pub fn current_model() -> String {
    if let Ok(r) = MODEL_OVERRIDE.read() {
        if let Some(m) = r.clone() {
            return m;
        }
    }
    match std::env::var("AGENTLENS_MODEL") {
        Ok(m) if !m.is_empty() => m,
        _ => DEFAULT_MODEL.to_string(),
    }
}

/// Ẩn secret/key/token/password trước khi gửi ra LLM.
pub fn redact(input: &str) -> String {
    // các pattern thường gặp; thay giá trị bằng [REDACTED]
    let patterns = [
        r"sk-[A-Za-z0-9_\-]{16,}",
        r"ghp_[A-Za-z0-9]{20,}",
        r"AKIA[0-9A-Z]{16}",
        r"(?i)(api[_-]?key|secret|token|password|passwd|authorization|bearer)\s*[:=]\s*\S+",
        r"eyJ[A-Za-z0-9_\-]{10,}\.[A-Za-z0-9_\-]{10,}\.[A-Za-z0-9_\-]{10,}", // JWT
    ];
    let mut out = input.to_string();
    for p in patterns {
        if let Ok(re) = Regex::new(p) {
            out = re.replace_all(&out, "[REDACTED]").to_string();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::redact;
    #[test]
    fn redacts_common_secrets() {
        let r = redact("export API_KEY=abcd1234 and key sk-ABCDEFGHIJKLMNOP1234 done");
        assert!(!r.contains("abcd1234"), "api_key value phải bị ẩn: {r}");
        assert!(!r.contains("sk-ABCDEFGHIJKLMNOP1234"), "sk- key phải bị ẩn: {r}");
        assert!(r.contains("[REDACTED]"));
    }
    #[test]
    fn keeps_plain_text() {
        let r = redact("chạy tool Edit trên file main.rs");
        assert_eq!(r, "chạy tool Edit trên file main.rs");
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Backend {
    Api,
    Cli,
}

fn api_key() -> Option<String> {
    std::env::var("ANTHROPIC_API_KEY").ok().filter(|k| !k.is_empty())
}

/// Tìm launcher `claude`. Ưu tiên AGENTLENS_CLAUDE_BIN, rồi quét PATH.
/// Trên Windows npm cài shim `claude.cmd` (không phải .exe) nên phải xét .exe/.cmd/.bat;
/// file `claude` không đuôi (script bash) bị bỏ qua trên Windows vì không exec trực tiếp được.
fn resolve_claude() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("AGENTLENS_CLAUDE_BIN") {
        if !p.is_empty() {
            let pb = PathBuf::from(p);
            if pb.is_file() {
                return Some(pb);
            }
        }
    }
    #[cfg(windows)]
    let names: &[&str] = &["claude.exe", "claude.cmd", "claude.bat"];
    #[cfg(not(windows))]
    let names: &[&str] = &["claude"];

    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            for n in names {
                let cand = dir.join(n);
                if cand.is_file() {
                    return Some(cand);
                }
            }
        }
    }
    // Fallback: app GUI trên macOS/Linux KHÔNG kế thừa PATH shell -> quét thư mục cài phổ biến.
    #[cfg(not(windows))]
    for dir in unix_bin_dirs() {
        let cand = dir.join("claude");
        if cand.is_file() {
            return Some(cand);
        }
    }
    None
}

/// Thư mục bin cài CLI thường gặp trên macOS/Linux (app GUI thường thiếu trong PATH).
#[cfg(not(windows))]
fn unix_bin_dirs() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"]
        .iter()
        .map(PathBuf::from)
        .collect();
    if let Some(h) = dirs::home_dir() {
        for sub in [".local/bin", ".npm-global/bin", ".bun/bin", ".deno/bin", ".claude/local"] {
            v.push(h.join(sub));
        }
    }
    v
}

/// Có `claude` (Claude Code CLI) để dùng backend cli không?
fn cli_available() -> bool {
    resolve_claude().is_some()
}

/// Tạo Command cho `claude`. Trên Windows, shim `.cmd`/`.bat` không exec trực tiếp
/// qua CreateProcess được nên phải gọi qua `cmd /C`; đồng thời đặt CREATE_NO_WINDOW
/// để app desktop (GUI) KHÔNG bật cửa sổ console đen mỗi lần spawn `claude`.
fn claude_command(bin: &Path) -> tokio::process::Command {
    #[cfg(windows)]
    {
        let ext = bin
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_ascii_lowercase());
        let mut c = if matches!(ext.as_deref(), Some("cmd") | Some("bat")) {
            let mut c = tokio::process::Command::new("cmd");
            c.arg("/C").arg(bin);
            c
        } else {
            tokio::process::Command::new(bin)
        };
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        c
    }
    #[cfg(not(windows))]
    {
        let mut c = tokio::process::Command::new(bin);
        // Bơm thư mục bin phổ biến vào PATH để `claude` (script node) tìm thấy `node`
        // khi AgentLens chạy như app GUI (macOS PATH tối thiểu, thiếu homebrew/npm).
        let extra: Vec<String> = unix_bin_dirs()
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        let cur = std::env::var("PATH").unwrap_or_default();
        c.env("PATH", format!("{}:{}", extra.join(":"), cur));
        c
    }
}

/// Backend đang hiệu lực (env ép > auto-detect).
fn backend() -> Backend {
    match std::env::var("AGENTLENS_LLM_BACKEND").ok().as_deref() {
        Some("cli") | Some("subscription") => Backend::Cli,
        Some("api") => Backend::Api,
        _ => {
            if api_key().is_some() {
                Backend::Api
            } else {
                Backend::Cli
            }
        }
    }
}

/// LLM có khả dụng không? (có API key, hoặc có `claude` CLI để dùng subscription)
pub fn is_enabled() -> bool {
    match std::env::var("AGENTLENS_LLM_BACKEND").ok().as_deref() {
        Some("cli") | Some("subscription") => cli_available(),
        Some("api") => api_key().is_some(),
        _ => api_key().is_some() || cli_available(),
    }
}

/// Tên backend để hiển thị trên UI/log.
pub fn backend_label() -> &'static str {
    match backend() {
        Backend::Api => "api-key (pay-as-you-go)",
        Backend::Cli => "claude -p (subscription)",
    }
}

/// Trạng thái auth của `claude` CLI: chạy `claude auth status --json`.
/// Trả về JSON {logged_in, auth_method, provider, subscription_type} hoặc null nếu không có CLI/lỗi.
/// LƯU Ý: Claude Code **không** expose số dư credit/quota subscription còn lại.
/// Login bằng tài khoản Claude.ai (Pro/Max) → authMethod="claude.ai", apiProvider="firstParty",
/// có subscriptionType (vd "max"); login bằng API key → authMethod khác và không có subscriptionType.
/// Phải resolve binary + bọc shim như `ask_cli`: trên Windows `claude` là `.cmd`,
/// `Command::new("claude")` (CreateProcess chỉ thử `.exe`) sẽ không spawn được → Null sai.
pub async fn cli_auth_status() -> Value {
    let bin = match resolve_claude() {
        Some(b) => b,
        None => return Value::Null,
    };
    let out = claude_command(&bin)
        .arg("auth")
        .arg("status")
        .arg("--json")
        .output()
        .await;
    match out {
        Ok(o) if o.status.success() => match serde_json::from_slice::<Value>(&o.stdout) {
            Ok(v) => json!({
                "logged_in": v.get("loggedIn").and_then(|x| x.as_bool()),
                "auth_method": v.get("authMethod").and_then(|x| x.as_str()),
                "provider": v.get("apiProvider").and_then(|x| x.as_str()),
                "subscription_type": v.get("subscriptionType").and_then(|x| x.as_str()),
            }),
            Err(_) => Value::Null,
        },
        _ => Value::Null,
    }
}

/// Backend đang chọn dưới dạng chuỗi ngắn cho API/UI.
pub fn backend_kind() -> &'static str {
    match backend() {
        Backend::Api => "api",
        Backend::Cli => "cli",
    }
}

/// Tóm tắt 1 session + gợi ý cải thiện workflow.
pub async fn summarize(session_brief: &str) -> Result<String> {
    let prompt = format!(
        "Đây là log rút gọn của một session Claude Code (đã ẩn secret). \
Hãy trả lời NGẮN GỌN bằng tiếng Việt:\n\
1) Session này làm gì (mục tiêu + kết quả).\n\
2) Các tool/hành động dùng nhiều nhất.\n\
3) 3–5 gợi ý cải thiện workflow/prompt/skill/hook.\n\n=== LOG ===\n{session_brief}"
    );
    ask(&prompt).await
}

/// Gọi LLM với 1 prompt (redact + cắt bớt trước khi gửi), tự chọn backend.
pub async fn ask(prompt: &str) -> Result<String> {
    let mut content = redact(prompt);
    if content.len() > MAX_INPUT_CHARS {
        content.truncate(MAX_INPUT_CHARS);
        content.push_str("\n…(đã cắt bớt)");
    }
    match backend() {
        Backend::Api => ask_api(&content).await,
        Backend::Cli => ask_cli(&content).await,
    }
}

/// Backend API: Anthropic Messages API bằng x-api-key (pay-as-you-go).
async fn ask_api(content: &str) -> Result<String> {
    let api_key = api_key().ok_or_else(|| anyhow!("chưa đặt ANTHROPIC_API_KEY — LLM (api) tắt"))?;
    let model = current_model();

    let body = json!({
        "model": model,
        "max_tokens": 1024,
        "messages": [{ "role": "user", "content": content }]
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(ENDPOINT)
        .header("x-api-key", api_key)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await?;

    let status = resp.status();
    let v: Value = resp.json().await?;
    if !status.is_success() {
        return Err(anyhow!("LLM API {}: {}", status, v));
    }

    let text = v
        .get("content")
        .and_then(|c| c.as_array())
        .map(|blocks| {
            blocks
                .iter()
                .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();

    if text.is_empty() {
        Err(anyhow!("LLM trả về rỗng: {}", v))
    } else {
        Ok(text)
    }
}

/// Backend CLI: `claude -p` của Claude Code — kế thừa login subscription của máy.
/// Chạy trong thư mục tạm để KHÔNG nạp CLAUDE.md/hook/skill của repo hiện tại
/// (tránh nhiễu + tránh hook AgentLens tự ghi lại chính lần gọi này).
async fn ask_cli(content: &str) -> Result<String> {
    let model = current_model();

    let bin = resolve_claude().ok_or_else(|| {
        anyhow!(
            "không tìm thấy `claude` trên PATH. Cài Claude Code + `claude auth login` (subscription); \
             hoặc chỉ đường dẫn bằng AGENTLENS_CLAUDE_BIN; hoặc dùng AGENTLENS_LLM_BACKEND=api với ANTHROPIC_API_KEY."
        )
    })?;

    let mut cmd = claude_command(&bin);
    cmd.arg("-p")
        .arg("--model")
        .arg(&model)
        // json để lấy kèm total_cost_usd + usage (giá quy đổi theo API, KHÔNG phải
        // tiền thật bị trừ khi dùng subscription — chỉ để biết mức tiêu thụ mỗi lần gọi).
        .arg("--output-format")
        .arg("json")
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| {
        anyhow!(
            "không chạy được `claude` ({}) ({e}). Thử đặt AGENTLENS_CLAUDE_BIN tới file claude, \
             hoặc dùng AGENTLENS_LLM_BACKEND=api với ANTHROPIC_API_KEY.",
            bin.display()
        )
    })?;

    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        stdin.write_all(content.as_bytes()).await?;
        stdin.shutdown().await?;
    }

    let out = child.wait_with_output().await?;
    if !out.status.success() {
        return Err(anyhow!(
            "`claude -p` lỗi (mã {:?}): {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    let raw = stdout.trim();

    // Parse JSON envelope: lấy `result` (text trả lời) + log cost/usage để theo dõi tiêu thụ.
    // Nếu không phải JSON (CLI cũ/format khác) thì coi cả stdout là text trả lời.
    let text = match serde_json::from_str::<Value>(raw) {
        Ok(v) => {
            let cost = v.get("total_cost_usd").and_then(|x| x.as_f64()).unwrap_or(0.0);
            let u = v.get("usage");
            let tok = |k: &str| u.and_then(|x| x.get(k)).and_then(|x| x.as_i64()).unwrap_or(0);
            tracing::info!(
                "claude -p [{}] ~${:.4} (API-equiv, subscription không trừ tiền thật) — \
                 in {} / out {} / cache_read {} / cache_write {} tok",
                model,
                cost,
                tok("input_tokens"),
                tok("output_tokens"),
                tok("cache_read_input_tokens"),
                tok("cache_creation_input_tokens"),
            );
            v.get("result")
                .and_then(|x| x.as_str())
                .map(|s| s.trim().to_string())
                .unwrap_or_default()
        }
        Err(_) => raw.to_string(),
    };

    if text.is_empty() {
        Err(anyhow!(
            "`claude -p` trả về rỗng — kiểm tra đã `/login` subscription chưa: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    } else {
        Ok(text)
    }
}
