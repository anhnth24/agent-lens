---
id: DECISION-0001
type: decision-log
status: Active
owner: sa
parents: [PRD-0001, TRD-0001, RISK-0001]
---

# Decision Log — AgentLens

> Ghi nhận các quyết định đã chốt (kèm hệ quả) để gỡ "Open questions" trong PRD/TRD/CHARTER.
> Nguồn: phiên review tài liệu 2026-06-14. Các nhận định suy luận gắn `[Inference]`.

## 1. Quyết định đã chốt

| # | Vấn đề | Quyết định | Ngày | Hệ quả / ghi chú |
|---|---|---|---|---|
| D-01 | Backend + ingest API stack | **Rust (axum)** | 2026-06-14 | Gỡ R-11. Đồng nhất với collector + Tauri core; cần nhân lực Rust. |
| D-02 | Vị trí redaction (FR-23) | **Tại backend** (trước khi ghi `payload` / trước khi gửi LLM) | 2026-06-14 | Dữ liệu thô (code/transcript/thinking) **rời máy dev lên central** rồi mới redact → tăng phơi nhiễm, xem R-12. [Inference] cần TLS in-transit + RBAC chặt + audit. |
| D-03 | Vendor LLM Trung Quốc (GLM/MiniMax) | **Giữ đầy đủ trong v1** | 2026-06-14 | R-04 chuyển sang **Accept** (có mitigation). Vẫn bật provider policy (FR-50) + redaction (FR-23); ghi nhận chấp nhận rủi ro data residency. |
| D-04 | LLM provider mặc định | **Anthropic (Claude)** | 2026-06-14 | Default toàn hệ thống; fallback theo FR-48. |
| D-05 | Subscription vs Non-Goal #3 | **Lai: subscription cho phạm vi dev (tự phân tích session của mình); API key/Bedrock cho phân tích org-wide** | 2026-06-14 | PRD Non-Goal #3 được làm rõ thành "không dùng subscription cho phân tích **org-wide**". Org-wide đi qua API key/Bedrock để hợp ToS + có quota thương mại. |
| D-06 | Retention event (FR-41) | **180 ngày** (TTL ClickHouse) | 2026-06-14 | Khớp DDL hiện tại (TRD §5.2). Có thể cấu hình lại sau. |
| D-07 | Mặt tiền UI v1 | **Desktop-only (Tauri)** | 2026-06-14 | Web read-only cho Lead/Security hoãn sau v1. |
| D-08 | Kênh notify v1 (FR-29) | **In-app + Email + Webhook outbound** | 2026-06-14 | Slack/Teams hoãn (có thể đạt qua webhook outbound). |

## 2. Quyết định thiết kế kỹ thuật (SA chốt — xử lý các khoảng trống phát hiện khi review)

| # | Khoảng trống | Phương án | Tham chiếu |
|---|---|---|---|
| D-09 | FR-6 yêu cầu dedup nhưng DDL dùng `MergeTree` (không tự khử trùng) + `event_id` sinh ở DB | Đổi sang **`ReplacingMergeTree`**; `event_id` là **hash ổn định** sinh ở collector (theo nội dung event), không để DB tự sinh UUID | TRD §5.2, §6.1 |
| D-10 | `prompt_id` lấy từ đâu ở Phase A (chưa có OTEL) | Phase A **suy ra prompt_id từ chuỗi hook** theo `session_id` + thứ tự (UserPromptSubmit→…→Stop); Phase B đối chiếu với OTEL `prompt.id` | TRD §4.1, §5.1 |
| D-11 | Token/cost trùng giữa OTEL và JSONL | **OTEL là nguồn sự thật cho cost/token**; JSONL chỉ bổ sung nội dung + thinking → tránh đếm trùng. Cần bảng giá model cho trường hợp thiếu OTEL cost | TRD §4.2, §4.3 |

## 2b. Pivot LEAN (2026-06-14) — supersede phần lớn mục 1

| # | Quyết định | Hệ quả |
|---|---|---|
| **D-12** | **Thu hẹp về lean / local-first** cho cá nhân–team nhỏ: chỉ **Capture → Store → Review** (+ LLM tóm tắt tùy chọn). Mục đích thật: ghi session Claude Code (hook/thinking/prompt/cost) để review & cải tiến workflow. | Hoãn toàn bộ org-wide: backend tập trung, RBAC/SSO, gateway đa vendor, vendor TQ, alerting, API/webhook, onboarding/MDM, FinOps, multi-agent adapter. |
| **D-13** | Stack lean: **1 binary Rust** (hook + JSONL tail + query + UI server), **DuckDB** nhúng, **web UI localhost** (không Tauri v1), **LLM 1 provider (Anthropic) tùy chọn**. | Thay cho ClickHouse+Postgres+Tauri+gateway. |

**Quyết định cũ bị thay/không còn áp dụng do D-12:** D-02 (redaction-at-backend — local nên chỉ redact khi gửi LLM), D-03 (vendor TQ — bỏ), D-07 (Tauri → web localhost), D-08 (notify — bỏ). Vẫn giữ: D-01 (Rust), D-04 (Anthropic), D-06 (retention 180), D-09/D-10/D-11 (dedup hash, prompt_id, OTEL cost — áp dụng bản local). Chi tiết: `PRD-0001` v5, `TRD-0001` v2.

## 3. Open questions còn lại (cần verify)

- **[Action — chặn FR-5]** Verify thinking-in-JSONL theo version Claude Code (đầu bước 1, dogfood ngay).
- **Store:** DuckDB vs SQLite (TRD §3).
- **FR-8 (LLM gợi ý):** làm ngay hay review thủ công trước rồi thêm sau.
