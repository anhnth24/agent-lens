---
id: CHARTER-0001
type: charter
status: Draft
owner: pm
parents: [PRD-0001, TRD-0001]
---

# Project Charter — AgentLens

> PM coordination artifact (PMP §4.1). Nguồn: PRD-0001, TRD-0001.

## 1. Mục đích & lý do
Xây hệ thống observability + analytics cho coding agent (Claude Code trước, agent-agnostic), org-wide, để đội ngũ thấy agent làm gì/nghĩ gì, kiểm soát token/cost, và cải thiện workflow bằng dữ liệu thay vì làm mò.

## 2. Mục tiêu (SMART — [Inference], cần PO chốt)
- Coverage >95% session trong store.
- Realtime <1 phút từ hoạt động → hiển thị.
- Sau 1 quý: giảm ~15% token/task, ~20% tool-error (đo bằng trend).
- MTTD alert <5 phút.

## 3. Phạm vi
- **In:** M1–M10 (PRD §III). Agent đầu tiên: Claude Code.
- **Out:** điều khiển agent; benchmark đa agent; thay thế SIEM/OTEL org; agent không expose telemetry.

## 4. Roadmap & milestones (trả lời: "sau Phase A làm gì")

| Phase | Mục tiêu | Deliverable chính | Exit criteria |
|---|---|---|---|
| **A — Xương sống realtime** | Thấy agent realtime | Collector (hook ingest, buffer/sync) + backend ingest + ClickHouse + desktop live timeline + auth/RBAC + adapter Claude Code | Dev xem được session realtime của mình; coverage cơ bản; **verify thinking-in-JSONL** |
| **B — Analytics** | Đo & thống kê | JSONL parser + OTEL ingest + dashboard token/cost/latency + trends + filter + breakdown + PG control plane + settings/audit | Lead xem dashboard org; trends chạy; số liệu khớp OTEL |
| **C — LLM Insight** | Đề xuất cải thiện | LLM Gateway đa vendor (Anthropic/OpenAI/Gemini/GLM/MiniMax) + redaction + cost guardrail + tóm tắt/đề xuất + adapter interface | Sinh insight cho 1 scope; redaction pass; cost guardrail chặn được; provider switch bằng config |
| **D — Vận hành** | Cảnh báo & báo cáo | Alerting (rule+dedup+route) + notification + reporting/export + retention/purge + FinOps + anomaly | Alert gửi đúng kênh; report export; TTL hoạt động; budget cảnh báo |
| **E — Mở rộng** | Đa agent & tích hợp | Adapter Codex/Antigravity + REST API + webhook + onboarding wizard + collector mgmt + replay/compare + per-provider cost + provider policy | Thêm 1 agent mới qua adapter; API/webhook dùng được; onboard 1 lệnh; policy chặn vendor |

> **Tóm lại sau Phase A:** B (analytics) → C (LLM insight) → D (alerting/báo cáo/retention) → E (đa agent + tích hợp + onboarding). Mỗi phase đều ra được bản chạy, dogfood ngay.

## 5. Tổ chức & nguồn lực (đề xuất)
- Sponsor/PO: Lead. Core DEV: Rust (collector+backend), Web/TS (desktop UI). Platform/Security: RBAC, redaction, deploy. QA theo Test Strategy.
- Hạ tầng: ClickHouse + PostgreSQL + backend (docker/k8s); LLM qua Bedrock/API key.

## 6. Giả định & ràng buộc
- Timeline linh hoạt → ưu tiên Must→Should trong cùng phạm vi, không cắt scope.
- M1–M3 zero-token; M4 phát sinh chi phí token thật theo vendor.
- Cloud công cộng OK; redaction vẫn áp dụng.
- [Unverified] thinking đầy đủ trong JSONL — verify đầu Phase A.

## 7. Rủi ro mức cao
Xem `RISK-REGISTER.md`. Top: scope lớn (resourcing), LLM cost, thinking-in-JSONL, data ra vendor TQ.

## 8. Tiêu chí thành công
Đạt mục tiêu §2 + Phase A–E qua exit criteria + adoption thực tế trong team.

## Open questions
- Ngân sách & nhân lực cụ thể; ngày mốc (timeline đang linh hoạt); LLM provider mặc định; chính sách vendor TQ.
