//! Bảng giá token (USD / 1 triệu token) để quy đổi token → chi phí.
//! Thứ tự ưu tiên: (1) bảng động nạp từ nguồn ngoài (AGENTLENS_PRICING_FILE / _URL),
//! (2) bảng built-in theo bậc model ([Unverified] ước tính).
//! [Unverified] Tôi không biết API giá real-time chính thức của Anthropic; nguồn động
//! thường dùng là file JSON cộng đồng (vd LiteLLM model_prices) — bật qua env.

use anyhow::anyhow;
use std::sync::{OnceLock, RwLock};

#[derive(Clone, Copy, Debug)]
pub struct Prices {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
}

/// Bảng giá nạp từ nguồn ngoài: (model_substring_lowercase, Prices per-Mtok).
static OVERRIDES: OnceLock<RwLock<Vec<(String, Prices)>>> = OnceLock::new();

/// Parse JSON giá. Hỗ trợ 2 format:
/// - per-Mtok: { "model": { "input":3, "output":15, "cache_read":0.3, "cache_write":3.75 } }
/// - LiteLLM (per-token): { "model": { "input_cost_per_token":..., "output_cost_per_token":...,
///   "cache_read_input_token_cost":..., "cache_creation_input_token_cost":... } }
pub fn load_json(text: &str) -> anyhow::Result<usize> {
    let v: serde_json::Value = serde_json::from_str(text)?;
    let obj = v.as_object().ok_or_else(|| anyhow!("pricing JSON phải là object"))?;
    let mut list = Vec::new();
    for (name, val) in obj {
        let g = |k: &str| val.get(k).and_then(|x| x.as_f64());
        let prices = if g("input").is_some() {
            Prices {
                input: g("input").unwrap_or(0.0),
                output: g("output").unwrap_or(0.0),
                cache_read: g("cache_read").unwrap_or(0.0),
                cache_write: g("cache_write").unwrap_or(0.0),
            }
        } else if g("input_cost_per_token").is_some() {
            Prices {
                input: g("input_cost_per_token").unwrap_or(0.0) * 1e6,
                output: g("output_cost_per_token").unwrap_or(0.0) * 1e6,
                cache_read: g("cache_read_input_token_cost").unwrap_or(0.0) * 1e6,
                cache_write: g("cache_creation_input_token_cost").unwrap_or(0.0) * 1e6,
            }
        } else {
            continue;
        };
        list.push((name.to_lowercase(), prices));
    }
    let n = list.len();
    *OVERRIDES.get_or_init(|| RwLock::new(Vec::new())).write().unwrap() = list;
    Ok(n)
}

/// Nạp bảng giá từ nguồn ngoài nếu có cấu hình env. Trả số model nạp được.
pub async fn refresh_from_source() -> anyhow::Result<usize> {
    if let Ok(path) = std::env::var("AGENTLENS_PRICING_FILE") {
        if !path.is_empty() {
            return load_json(&std::fs::read_to_string(&path)?);
        }
    }
    if let Ok(url) = std::env::var("AGENTLENS_PRICING_URL") {
        if !url.is_empty() {
            let text = reqwest::get(&url).await?.text().await?;
            return load_json(&text);
        }
    }
    Ok(0)
}

/// Chọn giá theo tên model (substring). Ưu tiên bảng động (match dài nhất), rồi built-in.
pub fn for_model(model: &str) -> Prices {
    let m = model.to_lowercase();
    if let Some(lock) = OVERRIDES.get() {
        if let Ok(list) = lock.read() {
            let mut best: Option<(usize, Prices)> = None;
            for (key, p) in list.iter() {
                if !key.is_empty() && (m.contains(key.as_str()) || key.contains(&m)) {
                    if best.map(|(l, _)| key.len() > l).unwrap_or(true) {
                        best = Some((key.len(), *p));
                    }
                }
            }
            if let Some((_, p)) = best {
                return p;
            }
        }
    }
    if m.contains("opus") {
        Prices { input: 15.0, output: 75.0, cache_read: 1.5, cache_write: 18.75 }
    } else if m.contains("haiku") {
        Prices { input: 0.8, output: 4.0, cache_read: 0.08, cache_write: 1.0 }
    } else if m.contains("sonnet") || m.contains("claude") {
        Prices { input: 3.0, output: 15.0, cache_read: 0.3, cache_write: 3.75 }
    } else {
        Prices { input: 0.0, output: 0.0, cache_read: 0.0, cache_write: 0.0 }
    }
}

/// Chi phí USD cho một bản ghi usage.
pub fn cost(model: &str, input: i64, output: i64, cache_read: i64, cache_write: i64) -> f64 {
    let p = for_model(model);
    (input as f64 * p.input
        + output as f64 * p.output
        + cache_read as f64 * p.cache_read
        + cache_write as f64 * p.cache_write)
        / 1_000_000.0
}
