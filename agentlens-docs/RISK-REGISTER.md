---
id: RISK-0001
type: risk-register
status: Draft
owner: pm
parents: [PRD-0001, TRD-0001]
---

# Risk Register — AgentLens

> P×I scoring (1–5). Score = P×I. Response: Mitigate / Avoid / Transfer / Accept. Nguồn: PRD-0001 §VIII, TRD-0001 §12.

| ID | Risk | Category | P | I | Score | Response | Hành động | Owner | Status |
|---|---|---|---|---|---|---|---|---|---|
| R-01 | Scope full (10 module) → thiếu nguồn lực/kéo dài | Schedule | 4 | 3 | 12 | Mitigate | Ưu tiên Must (Phase A-C) trước; giãn Should/Could; không cắt scope | PM/Lead | Open |
| R-02 | [Unverified] Thinking không đầy đủ trong JSONL theo version CC | Technical | 3 | 3 | 9 | Mitigate | Verify đầu Phase A; fallback chỉ tool/metrics nếu thiếu | SA | Open |
| R-03 | Chi phí LLM (M4) vượt ngân sách ở org | Cost | 3 | 4 | 12 | Mitigate | Cost guardrail/budget per-provider (FR-25/49); batch ngoài giờ | FinOps/Lead | Open |
| R-04 | Data nhạy cảm (code/transcript) lọt ra vendor LLM ngoài, đặc biệt GLM/MiniMax (TQ) | Security/Compliance | 3 | 5 | 15 | Accept (có mitigation) | **D-03: giữ vendor TQ trong v1** — redaction bắt buộc (FR-23); provider policy (FR-50); chấp nhận rủi ro data residency có kiểm soát | Security | Accepted |
| R-05 | Onboard hook/OTEL trên nhiều máy dev lệch/khó | Operational | 3 | 3 | 9 | Mitigate | managed-settings.json/MDM (FR-37); installer + wizard (FR-35) | Platform | Open |
| R-06 | Claude Code đổi schema hook/telemetry | Technical | 3 | 3 | 9 | Mitigate | Adapter versioned (FR-44); test theo mỗi release CC | DEV | Open |
| R-07 | Lock-in agent/LLM | Strategic | 2 | 3 | 6 | Mitigate | Agent-agnostic (FR-5/44) + provider-agnostic gateway (FR-24) | SA | Open |
| R-08 | ClickHouse query chậm ở volume org lớn | Performance | 2 | 4 | 8 | Mitigate | Partition + materialized view rollup; load test (NFR) | DEV | Open |
| R-09 | Alert noise (quá nhiều cảnh báo) | Operational | 3 | 2 | 6 | Mitigate | Dedup/throttle; policy theo team (FR-30) | Lead | Open |
| R-10 | Collector chiếm tài nguyên / chặn agent | Technical | 2 | 3 | 6 | Mitigate | Trả hook nhanh (200); xử lý async; giới hạn buffer | DEV | Open |
| R-11 | Quyết định stack chưa chốt (backend Rust vs .NET, gateway) làm chậm khởi động | Schedule | 2 | 2 | 4 | Mitigate | **Đã chốt backend Rust (D-01)**; gateway còn mở | SA/Lead | Closed |
| R-12 | Redaction tại backend (D-02) → code/transcript thô rời máy dev lên central trước khi redact | Security/Compliance | 3 | 4 | 12 | Mitigate | TLS in-transit + encrypt at-rest; RBAC chặt + audit truy cập payload; [Inference] cân nhắc redact tối thiểu tại collector cho project sensitive | Security | Open |
| R-13 | Subscription cho phạm vi dev (D-05) đụng quota/ToS nếu lạm dụng | Cost/Compliance | 2 | 2 | 4 | Mitigate | Giới hạn subscription chỉ self-analysis; org-wide bắt buộc API key/Bedrock | Lead | Open |

## Heat (ưu tiên xử lý)
Cao nhất: **R-04 (15, đã Accept có mitigation)**, R-01/R-03/**R-12** (12). Xử lý sớm trước/đầu Phase C (LLM). R-12 (redaction tại backend) cần thiết kế kỹ ở Phase A–B.

## Open questions
- Ngân sách trần LLM? Danh sách vendor được phép cho project gov?
