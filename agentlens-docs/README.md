# AgentLens — Bộ tài liệu dự án

Hệ thống quan sát & phân tích workflow coding agent (Claude Code → agent-agnostic), org-wide, desktop (Tauri).

## Thứ tự đọc
1. **PRD-0001** — yêu cầu nghiệp vụ (cái gì / tại sao), 10 module, ~50 FR.
2. **TRD-0001** — thiết kế kỹ thuật (xây thế nào): kiến trúc, DDL ClickHouse+PostgreSQL, hook/OTEL config, LLM gateway, cấu trúc monorepo, thứ tự build A→E, docker-compose.
3. **module-list** — phân rã 10 module + boundary + phụ thuộc.
4. **feature-catalog** — 44 feature, map FR + phase + status (granularity để giao việc).
5. **PERSONAS** — Dev / Lead / Admin / Security + Three Amigos.
6. **PROJECT-CHARTER** — mục tiêu, scope, roadmap Phase A→E.
7. **RISK-REGISTER** — risk chấm P×I (R-04 đã Accept; thêm R-12/R-13).
8. **TEST-STRATEGY** — test theo module/FR.
9. **DECISION-LOG** — các quyết định đã chốt (D-01..D-11) + open questions còn lại.

## Bắt đầu code
Phase A (xương sống realtime): collector → backend ingest → ClickHouse → desktop live timeline → auth/RBAC.
Việc cần verify đầu tiên: thinking-in-JSONL theo version Claude Code (TRD §4.3, §12).

## Quyết định
Đã chốt (xem `DECISION-LOG.md`): backend **Rust**, redaction **tại backend**, **giữ vendor TQ**, default **Anthropic**, subscription **chỉ cho dev**, retention **180 ngày**, **desktop-only**, notify **in-app/email/webhook**.
Còn mở: gateway tự viết vs LiteLLM; realtime WS vs SSE; verify thinking-in-JSONL.
