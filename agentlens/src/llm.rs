//! FR-8 (tùy chọn): tóm tắt session + gợi ý cải thiện workflow bằng LLM.
//! Redaction chạy TRƯỚC khi gửi. 1 provider (Anthropic Messages API).
//! Bật bằng env ANTHROPIC_API_KEY; model qua AGENTLENS_MODEL (mặc định Haiku 4.5).

use anyhow::{anyhow, Result};
use regex::Regex;
use serde_json::{json, Value};

const ENDPOINT: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const DEFAULT_MODEL: &str = "claude-haiku-4-5-20251001";
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

pub fn is_enabled() -> bool {
    std::env::var("ANTHROPIC_API_KEY").map(|k| !k.is_empty()).unwrap_or(false)
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

/// Gọi Anthropic Messages API với 1 prompt (redact + cắt bớt trước khi gửi).
pub async fn ask(prompt: &str) -> Result<String> {
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| anyhow!("chưa đặt ANTHROPIC_API_KEY — LLM tắt"))?;
    let model = std::env::var("AGENTLENS_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_string());

    let mut content = redact(prompt);
    if content.len() > MAX_INPUT_CHARS {
        content.truncate(MAX_INPUT_CHARS);
        content.push_str("\n…(đã cắt bớt)");
    }

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
