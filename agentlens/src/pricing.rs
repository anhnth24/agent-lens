//! Bảng giá token (USD / 1 triệu token) để quy đổi token → chi phí.
//! [Unverified] Giá ƯỚC TÍNH theo bậc model — chỉnh tại đây nếu cần con số chính xác.

#[derive(Clone, Copy, Debug)]
pub struct Prices {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
}

/// Chọn giá theo tên model (substring). Model lạ → 0 (cost = 0).
pub fn for_model(model: &str) -> Prices {
    let m = model.to_lowercase();
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
