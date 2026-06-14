# AgentLens — Bộ tài liệu dự án

Công cụ **local** ghi lại session Claude Code (hook · thinking · prompt · token/cost) để **review sau** và rút ra cải tiến workflow. Phạm vi hiện tại: **lean / local-first** cho cá nhân hoặc team nhỏ.

## Tài liệu hiện hành (lean)
1. **PRD-0001** (v5) — mục đích & yêu cầu: Capture → Store → Review (FR-1..FR-10).
2. **TRD-0001** (v2) — thiết kế: 1 binary Rust (hook + JSONL tail + query + UI), DuckDB, web UI localhost, LLM tóm tắt tùy chọn.
3. **DECISION-LOG** — các quyết định đã chốt + pivot lean (D-12/D-13) + open questions.

## Tài liệu tham khảo (full-vision — đã hoãn)
`module-list`, `feature-catalog`, `PERSONAS`, `PROJECT-CHARTER`, `RISK-REGISTER`, `TEST-STRATEGY` mô tả tầm nhìn org-wide 10 module ban đầu; giữ lại để tham khảo, không phải phạm vi hiện tại.

## Bắt đầu code
1. **Capture + Store:** hook receiver (HTTP `:8787`) + JSONL tailer → DuckDB (TRD §9 bước 1).
2. **Review UI:** timeline session + dashboard token/cost + filter.
3. (Tùy chọn) LLM tóm tắt/gợi ý, OTEL cost, retention.

**Verify đầu tiên:** thinking-in-JSONL theo version Claude Code (TRD §4.2) — dogfood ngay bằng session đang chạy.

## Quyết định chính (xem `DECISION-LOG.md`)
Rust 1-binary · DuckDB nhúng · web UI localhost (không Tauri v1) · LLM 1 provider (Anthropic) tùy chọn · zero-token cho capture · retention 180 ngày.
Còn mở: DuckDB vs SQLite; làm LLM-gợi-ý ngay hay sau; verify thinking-in-JSONL.
