# AgentLens — Bộ tài liệu dự án

Công cụ **local** ghi lại session Claude Code (hook · thinking · prompt · token/cost) để **review sau** và rút ra cải tiến workflow. Phạm vi hiện tại: **lean / local-first** cho cá nhân hoặc team nhỏ.

> **Trạng thái: đã implement.** Mã nguồn + hướng dẫn cài đặt/chạy local/build desktop ở **[`../agentlens/README.md`](../agentlens/README.md)**.

## Cài đặt & chạy (tóm tắt — chi tiết ở agentlens/README.md)

```bash
cd agentlens
cargo run --release                       # server: http://127.0.0.1:8787
# hoặc desktop app:
cargo run -p agentlens-desktop --release  # cửa sổ Tauri
# đóng gói cài đặt:  cd desktop/src-tauri && cargo tauri build
```

## Tài liệu hiện hành (lean)
1. **PRD-0001** (v5) — mục đích & yêu cầu: Capture → Store → Review (FR-1..FR-10).
2. **TRD-0001** (v2) — thiết kế: 1 binary Rust (hook + JSONL tail + query + UI), SQLite, web UI localhost, LLM tóm tắt tùy chọn.
3. **DECISION-LOG** — các quyết định đã chốt + pivot lean (D-12/D-13) + open questions.

## Tài liệu tham khảo (full-vision — đã hoãn)
`module-list`, `feature-catalog`, `PERSONAS`, `PROJECT-CHARTER`, `RISK-REGISTER`, `TEST-STRATEGY` mô tả tầm nhìn org-wide 10 module ban đầu; giữ lại để tham khảo, không phải phạm vi hiện tại.

## Đã làm được (so với PRD)
- **FR-1..FR-3, FR-5..FR-8**: hook ingestion · tail JSONL (thinking/prompt/usage) · dedup theo `event_id` · timeline · thống kê token in/out/cached theo session/ngày/model · lọc repo/thời gian · LLM tóm tắt + insight (tùy chọn, có redaction).
- **FR-4 OTEL**: đã có receiver OTLP/HTTP JSON (`/v1/metrics`) — cost/LOC/commit chính xác khi bật telemetry.
- **Thêm ngoài PRD**: Live + auto-follow, replay, health score, cost burn, model right-sizing, recovery, cache advisor, error clustering, repo leaderboard, heatmap; UI pixel 2 theme (skill `ui-ux-pro-max`).
- **Còn để ngỏ**: FR-9/10 retention/xóa tự động (hiện làm tay qua SQL).

## Verify đã xong
- **thinking-in-JSONL** (TRD §4.2): xác nhận version Claude Code hiện tại lưu thinking block **đã redact text** (chỉ còn signature) → đếm theo *số bước reasoning* thay vì độ dài (đã ghi ở DECISION-LOG / R-02).

## Quyết định chính (xem `DECISION-LOG.md`)
Rust 1-binary · SQLite nhúng (WAL) · UI localhost **+ đóng gói Tauri 2 desktop** · LLM 1 provider (Anthropic) tùy chọn · zero-token cho capture · bảng giá pull từ LiteLLM (refresh hàng ngày, fallback built-in).

