---
id: FEATCAT-0001
type: feature-catalog
status: Draft
owner: ba
parents: [MODLIST-0001, PRD-0001]
---

# Feature Catalog — AgentLens

> Module → Feature inventory. Mỗi dòng là **Feature** — granularity cho `/fis:plan`. KHÔNG decompose thành User Story ở đây.
> Phase = thứ tự build (TRD §10). Status: Planned (mặc định).

| Feature | Module | Mô tả | FR | Priority | Phase | Status |
|---|---|---|---|---|---|---|
| F-01 Hook ingestion | M1 | Nhận hook HTTP realtime | FR-1 | Must | A | Planned |
| F-02 JSONL parsing | M1 | Tail transcript (thinking, tool, usage) | FR-2 | Must | B | Planned |
| F-03 OTEL ingestion | M1 | Nhận metrics/events OTLP | FR-3 | Must | B | Planned |
| F-04 Collector buffer/sync | M1 | Chạy nền, offline buffer, sync, idempotent | FR-4,6 | Must | A | Planned |
| F-05 Event normalization | M1 | Schema agent-agnostic (agent_type) | FR-5 | Must | A | Planned |
| F-06 Live session timeline | M2 | Gom prompt_id, realtime | FR-7,8 | Must | A | Planned |
| F-07 Event detail viewer | M2 | tool_input/response/thinking | FR-9 | Must | A | Planned |
| F-08 Session replay | M2 | Tua lại theo timeline | FR-10 | Should | E | Planned |
| F-09 Multi-session live | M2 | Theo dõi nhiều session/dev | FR-11 | Should | E | Planned |
| F-10 Metrics dashboard | M3 | token/cost/latency/deny/hook-fail | FR-12,13 | Must | B | Planned |
| F-11 Trends | M3 | Xu hướng theo thời gian | FR-14 | Must | B | Planned |
| F-12 Filter/search | M3 | Đa chiều | FR-15 | Must | B | Planned |
| F-13 Comparison | M3 | dev/team/project/time | FR-16 | Should | E | Planned |
| F-14 Skill/hook/subagent breakdown | M3 | Theo skill.name, Task | FR-17 | Should | B | Planned |
| F-15 Reporting/export | M3 | PDF/CSV/Excel + scheduled | FR-18 | Should | D | Planned |
| F-16 Cost attribution/FinOps | M3 | Budget per team/project | FR-19 | Should | D | Planned |
| F-17 Annotation/tagging | M3 | Tag session (task/success) | FR-20 | Could | E | Planned |
| F-18 LLM summarize | M4 | Tóm tắt session | FR-21 | Must | C | Planned |
| F-19 LLM recommendations | M4 | Đề xuất cải thiện workflow | FR-22 | Must | C | Planned |
| F-20 Redaction | M4 | Lọc code/secret/PII | FR-23 | Must | C | Planned |
| F-21 LLM Gateway multi-provider | M4 | Anthropic/OpenAI/Gemini/GLM/MiniMax + select/fallback | FR-24,48 | Must | C | Planned |
| F-22 Cost guardrail/budget | M4 | Per-provider budget, chặn vượt | FR-25,49 | Must/Should | C | Planned |
| F-23 Insight feedback | M4 | Đo actionability | FR-26 | Should | C | Planned |
| F-24 Anomaly detection | M4 | latency/cost/error bất thường | FR-27 | Should | D | Planned |
| F-25 Provider policy | M4 | Chặn vendor theo project nhạy cảm | FR-50 | Should | E | Planned |
| F-26 Alert rules engine | M5 | Rule + dedup + route | FR-28,30 | Must | D | Planned |
| F-27 Notification channels | M5 | in-app/email/Slack/webhook | FR-29 | Should | D | Planned |
| F-28 Auth SSO + RBAC | M6 | OIDC + role scope | FR-31 | Must | A | Planned |
| F-29 Settings UI | M6 | provider/retention/redaction | FR-32 | Must | B | Planned |
| F-30 Audit log | M6 | Ai xem/đổi gì | FR-33 | Must | B | Planned |
| F-31 Org/team/project mgmt | M6 | Quản lý cấu trúc | FR-34 | Should | B | Planned |
| F-32 Onboarding wizard | M7 | Cài collector + hook/OTEL | FR-35 | Should | E | Planned |
| F-33 Collector mgmt/health | M7 | Health/version/lag | FR-36 | Should | E | Planned |
| F-34 Central config distribution | M7 | managed-settings/MDM | FR-37 | Should | E | Planned |
| F-35 REST API | M8 | Đọc metrics/events/insight | FR-38 | Should | E | Planned |
| F-36 Outbound webhook | M8 | Khi alert/insight | FR-39 | Should | E | Planned |
| F-37 ELK/OTEL forward | M8 | Dual-write | FR-40 | Could | E | Planned |
| F-38 Retention/purge | M9 | TTL cấu hình + auto-purge | FR-41 | Must | D | Planned |
| F-39 Export/backup | M9 | Backup dữ liệu | FR-42 | Should | E | Planned |
| F-40 Delete by user/project | M9 | Privacy/compliance | FR-43 | Should | E | Planned |
| F-41 Adapter interface | M10 | Contract chung | FR-44 | Must | C/E | Planned |
| F-42 Claude Code adapter | M10 | Đầy đủ | FR-45 | Must | A | Planned |
| F-43 Codex adapter | M10 | Mở rộng | FR-46 | Should | E | Planned |
| F-44 Antigravity adapter | M10 | Mở rộng | FR-47 | Should | E | Planned |

## Tổng hợp theo phase
- **A:** F-01,04,05,06,07,28,42 (xương sống realtime).
- **B:** F-02,03,10,11,12,14,29,30,31.
- **C:** F-18,19,20,21,22,23,41.
- **D:** F-15,16,24,26,27,38.
- **E:** F-08,09,13,17,25,32,33,34,35,36,37,39,40,43,44.

## Open questions
- F-21/F-25 (multi-provider + policy) phụ thuộc chính sách vendor TQ — chốt trước Phase C/E.
