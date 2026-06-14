//! Parser dung sai cho transcript JSONL của Claude Code.
//! Mỗi dòng transcript = 1 entry; ta gom các content block về 1 `Entry`.
//! Schema có thể đổi theo version Claude Code -> parser chỉ đọc field tồn tại, không panic.

use serde_json::Value;

#[derive(Debug, Clone)]
pub struct ToolUse {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct ToolResultRef {
    pub tool_use_id: String,
    pub is_error: bool,
}

#[derive(Debug, Default)]
pub struct Entry {
    pub uuid: String,
    pub session_id: String,
    pub prompt_id: Option<String>,
    pub ts: String,
    pub kind: String, // "user" | "assistant"
    pub role: Option<String>,
    pub text: String,
    pub thinking: String,
    pub tool_name: Option<String>,
    pub tool_input: Option<String>,
    pub tool_result: Option<String>,
    pub tool_error: bool, // có tool_result is_error trong dòng (badge timeline)
    pub model: Option<String>,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    pub git_branch: Option<String>,
    pub cwd: Option<String>,
    pub tool_uses: Vec<ToolUse>,            // từng tool_use (để phân tích tool)
    pub tool_results: Vec<ToolResultRef>,   // từng tool_result (is_error)
}

fn s(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(|x| x.as_str()).map(|x| x.to_string())
}

fn block_text(v: &Value) -> String {
    // tool_result content có thể là string hoặc mảng {type:text,text:..}
    match v {
        Value::String(s) => s.clone(),
        Value::Array(a) => a
            .iter()
            .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Parse 1 dòng JSONL. Trả None nếu không phải entry hội thoại (attachment/system/lỗi parse).
pub fn parse(line: &str) -> Option<Entry> {
    let v: Value = serde_json::from_str(line).ok()?;
    let kind = v.get("type")?.as_str()?.to_string();
    if kind != "user" && kind != "assistant" {
        return None; // bỏ qua attachment/system cho timeline lean
    }
    let uuid = s(&v, "uuid")?;
    let session_id = s(&v, "sessionId").unwrap_or_default();
    if session_id.is_empty() {
        return None;
    }

    let mut e = Entry {
        uuid,
        session_id,
        prompt_id: s(&v, "promptId"),
        ts: s(&v, "timestamp").unwrap_or_default(),
        kind,
        git_branch: s(&v, "gitBranch"),
        cwd: s(&v, "cwd"),
        ..Default::default()
    };

    let msg = v.get("message");
    if let Some(m) = msg {
        e.role = s(m, "role");
        e.model = s(m, "model");

        if let Some(u) = m.get("usage").filter(|u| u.is_object()) {
            e.input_tokens = u.get("input_tokens").and_then(|x| x.as_i64()).unwrap_or(0);
            e.output_tokens = u.get("output_tokens").and_then(|x| x.as_i64()).unwrap_or(0);
            e.cache_read_tokens = u
                .get("cache_read_input_tokens")
                .and_then(|x| x.as_i64())
                .unwrap_or(0);
            e.cache_creation_tokens = u
                .get("cache_creation_input_tokens")
                .and_then(|x| x.as_i64())
                .unwrap_or(0);
        }

        match m.get("content") {
            Some(Value::String(text)) => e.text = text.clone(),
            Some(Value::Array(blocks)) => {
                let mut texts = Vec::new();
                let mut thinks = Vec::new();
                let mut tools = Vec::new();
                let mut tool_inputs = Vec::new();
                let mut results = Vec::new();
                for b in blocks {
                    match b.get("type").and_then(|t| t.as_str()) {
                        Some("text") => {
                            if let Some(t) = b.get("text").and_then(|x| x.as_str()) {
                                texts.push(t.to_string());
                            }
                        }
                        Some("thinking") => {
                            if let Some(t) = b.get("thinking").and_then(|x| x.as_str()) {
                                thinks.push(t.to_string());
                            }
                        }
                        Some("tool_use") => {
                            let name = b.get("name").and_then(|x| x.as_str()).unwrap_or("");
                            if !name.is_empty() {
                                tools.push(name.to_string());
                            }
                            if let Some(inp) = b.get("input") {
                                tool_inputs.push(inp.clone());
                            }
                            if let Some(id) = b.get("id").and_then(|x| x.as_str()) {
                                e.tool_uses.push(ToolUse {
                                    id: id.to_string(),
                                    name: name.to_string(),
                                });
                            }
                        }
                        Some("tool_result") => {
                            if let Some(c) = b.get("content") {
                                results.push(block_text(c));
                            }
                            let is_err = b.get("is_error").and_then(|x| x.as_bool()).unwrap_or(false);
                            if is_err {
                                e.tool_error = true;
                            }
                            if let Some(tid) = b.get("tool_use_id").and_then(|x| x.as_str()) {
                                e.tool_results.push(ToolResultRef {
                                    tool_use_id: tid.to_string(),
                                    is_error: is_err,
                                });
                            }
                        }
                        _ => {}
                    }
                }
                e.text = texts.join("\n");
                e.thinking = thinks.join("\n\n");
                if !tools.is_empty() {
                    e.tool_name = Some(tools.join(", "));
                }
                if !tool_inputs.is_empty() {
                    e.tool_input = serde_json::to_string(&tool_inputs).ok();
                }
                if !results.is_empty() {
                    e.tool_result = Some(results.join("\n---\n"));
                }
            }
            _ => {}
        }
    }

    Some(e)
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn parses_assistant_usage_and_thinking() {
        let line = r#"{"type":"assistant","uuid":"u1","sessionId":"s1","timestamp":"2026-01-01T00:00:00Z","cwd":"/x","gitBranch":"main","message":{"role":"assistant","model":"claude-x","usage":{"input_tokens":10,"output_tokens":20,"cache_read_input_tokens":30,"cache_creation_input_tokens":5},"content":[{"type":"thinking","thinking":"nghĩ"},{"type":"text","text":"chào"},{"type":"tool_use","name":"Edit","input":{"a":1}}]}}"#;
        let e = parse(line).expect("parse");
        assert_eq!(e.input_tokens, 10);
        assert_eq!(e.output_tokens, 20);
        assert_eq!(e.cache_read_tokens, 30);
        assert_eq!(e.cache_creation_tokens, 5);
        assert_eq!(e.thinking, "nghĩ");
        assert_eq!(e.text, "chào");
        assert_eq!(e.tool_name.as_deref(), Some("Edit"));
        assert_eq!(e.git_branch.as_deref(), Some("main"));
    }

    #[test]
    fn skips_non_conversation() {
        let line = r#"{"type":"system","uuid":"u2","sessionId":"s1"}"#;
        assert!(parse(line).is_none());
    }

    #[test]
    fn parses_user_string_content() {
        let line = r#"{"type":"user","uuid":"u3","sessionId":"s1","promptId":"p1","timestamp":"t","message":{"role":"user","content":"câu hỏi"}}"#;
        let e = parse(line).unwrap();
        assert_eq!(e.text, "câu hỏi");
        assert_eq!(e.prompt_id.as_deref(), Some("p1"));
    }
}
