//! Serve web UI nhẹ trên localhost.
//! Mặc định nhúng index.html vào binary (giữ 1 file .exe tự chứa cho release).
//! Dev hot-reload: đặt AGENTLENS_DEV_UI=1 → đọc lại file từ ổ đĩa mỗi request,
//! sửa ui/index.html xong chỉ cần F5 trình duyệt, KHÔNG cần build/chạy lại.

use axum::response::Html;

/// Bản nhúng lúc compile — dùng cho release & làm fallback nếu đọc disk lỗi.
const EMBEDDED: &str = include_str!("../ui/index.html");
/// Đường dẫn nguồn (tuyệt đối, chốt lúc compile) để đọc lại khi dev.
const DISK_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/ui/index.html");

/// Bật đọc-từ-disk khi AGENTLENS_DEV_UI đặt và khác "0"/rỗng.
fn dev_ui() -> bool {
    std::env::var("AGENTLENS_DEV_UI")
        .map(|v| !v.is_empty() && v != "0")
        .unwrap_or(false)
}

pub async fn index() -> Html<String> {
    if dev_ui() {
        if let Ok(s) = std::fs::read_to_string(DISK_PATH) {
            return Html(s);
        }
    }
    Html(EMBEDDED.to_string())
}
