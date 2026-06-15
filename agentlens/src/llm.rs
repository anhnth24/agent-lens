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
use std::process::Stdio;

const ENDPOINT: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const DEFAULT_MODEL_API: &str = "claude-haiku-4-5-20251001";
const DEFAULT_MODEL_CLI: &str = "haiku";
const MAX_INPUT_CHARS: usize = 12_000;

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

/// Có `claude` (Claude Code CLI) trên PATH không? (không spawn process)
fn cli_available() -> bool {
    if let Ok(path) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path) {
            if dir.join("claude").is_file() {
                return true;
            }
            #[cfg(windows)]
            if dir.join("claude.exe").is_file() {
                return true;
            }
        }
    }
    false
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
/// Trả về JSON {logged_in, auth_method, provider} hoặc null nếu không có CLI/lỗi.
/// LƯU Ý: Claude Code **không** expose số dư credit/quota subscription còn lại —
/// chỉ có auth_method (`oauth_token` = subscription, `api_key` = pay-as-you-go).
pub async fn cli_auth_status() -> Value {
    if !cli_available() {
        return Value::Null;
    }
    let out = tokio::process::Command::new("claude")
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
    let model = std::env::var("AGENTLENS_MODEL").unwrap_or_else(|_| DEFAULT_MODEL_API.to_string());

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
    let model = std::env::var("AGENTLENS_MODEL").unwrap_or_else(|_| DEFAULT_MODEL_CLI.to_string());

    let mut child = tokio::process::Command::new("claude")
        .arg("-p")
        .arg("--model")
        .arg(&model)
        .arg("--output-format")
        .arg("text")
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            anyhow!(
                "không chạy được `claude` CLI ({e}). Cài Claude Code và `claude` + `/login` \
                 (subscription), hoặc đặt ANTHROPIC_API_KEY và AGENTLENS_LLM_BACKEND=api."
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

    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if text.is_empty() {
        Err(anyhow!(
            "`claude -p` trả về rỗng — kiểm tra đã `/login` subscription chưa: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    } else {
        Ok(text)
    }
}
