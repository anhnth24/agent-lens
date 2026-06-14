---
id: TEST-0001
type: test-strategy
status: Draft
owner: qa
parents: [PRD-0001, TRD-0001]
---

# Test Strategy — AgentLens

> ⚠️ **Full-vision (đã hoãn).** Lean scope chỉ cần test capture/dedup + review (timeline/cost) + (tùy chọn) redaction trước LLM. Xem `PRD-0001` v5.

> Chiến lược test (QA: Priya). Map theo module/FR. Nguồn: PRD-0001, TRD-0001.

## 1. Cấp độ test
- **Unit:** normalization, redaction rules, cost calc, alert rule eval, gateway adapter mapping.
- **Integration:** collector→backend→ClickHouse; auth/RBAC; gateway→vendor (mock); alerting→notify.
- **E2E:** session Claude Code thật → hiển thị desktop realtime; phân tích LLM → insight.
- **Performance/Load:** ingest throughput; ClickHouse query ở ≥10M event.
- **Security:** redaction coverage, RBAC bypass, secret leakage, provider policy.

## 2. Trọng tâm test theo module

| Module | Test chính |
|---|---|
| M1 Collection | Hook payload parse đúng; **idempotent/dedup** (gửi trùng không nhân đôi); buffer offline rồi sync đủ; ordering theo prompt_id; OTEL ingest khớp |
| M2 Realtime | Timeline gom đúng prompt_id; latency render <1s; replay đúng thứ tự; thinking hiển thị đúng (theo kết quả verify) |
| M3 Analytics | Token/cost/latency khớp nguồn (OTEL/JSONL `usage`); percentile đúng; filter/compare chính xác; export đúng số liệu |
| M4 LLM Insight | **Redaction chặn 100% secret/key mẫu** trước khi gửi; gateway switch provider bằng config; fallback khi vendor lỗi; cost guardrail chặn khi vượt budget; provider policy chặn vendor cấm cho project sensitive |
| M5 Alerting | Rule fire đúng điều kiện; **dedup/throttle** không spam; route đúng kênh/người; alert history ghi đủ |
| M6 Admin/RBAC | dev chỉ thấy data của mình; lead thấy team; admin full; **không leo quyền**; audit ghi mọi thao tác nhạy cảm |
| M7 Onboarding | Wizard cấu hình hook/OTEL đúng; collector health báo chính xác; config tập trung áp dụng |
| M8 Integration | API trả đúng + lọc theo RBAC; webhook bắn đúng sự kiện; forward ELK/OTEL không mất dữ liệu |
| M9 Data Mgmt | TTL/purge xóa đúng hạn; export/backup khôi phục được; delete-by-user xóa sạch |
| M10 Adapters | Adapter mới (Codex/Antigravity) tạo event đúng schema chung; không sửa core |

## 3. Test data & môi trường
- Session Claude Code mẫu (ghi lại JSONL + hook payload) làm fixture replay → test không cần chạy agent thật.
- Mock vendor LLM (trả cố định) cho test gateway/cost không tốn token.
- Env: docker-compose (PG + ClickHouse + backend + gateway mock).

## 4. Tiêu chí pass (NFR-linked)
- Ingest <1s/event; dashboard query <2s @≥10M; redaction coverage 100% trên bộ mẫu; 0 lỗi RBAC bypass; alert dedup hoạt động.

## 5. Test case generation
[Inference] Áp dụng phương pháp sinh test case của team (boundary, equivalence, negative) cho từng FR; ưu tiên FR Must (Phase A–C).

## Open questions
- Bộ secret/PII mẫu chuẩn để đo redaction coverage? Ngưỡng coverage chấp nhận?
