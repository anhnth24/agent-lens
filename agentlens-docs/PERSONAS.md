---
id: PERSONAS-0001
type: personas
status: Active
owner: ba
parents: [PRD-0001]
---

# PERSONAS — AgentLens

> Tên người là placeholder. Đối tượng phục vụ: org dùng coding agent (Claude Code) nội bộ.

## 1. Three Amigos (mặc định FIS)
| Vai | Tên | Lens | Sign-off |
|---|---|---|---|
| BA | Sarah | scope & business value | PRD |
| SA | Marcus | technical feasibility | TRD |
| QA | Priya | testability & risk | Test |

## 2. Primary user personas

### PER-0001 — Developer ("Hùng")
- **Role:** Dev dùng Claude Code hằng ngày. **Experience:** Mid–Senior. **Stack:** đa dạng (gồm .NET, web).
- **Goals:** thấy realtime agent đang làm gì (tool/hook), hiểu agent "nghĩ gì", debug khi agent đi sai.
- **Pain:** không biết agent làm gì trong "hộp đen"; tốn token mà không rõ ở đâu.
- **Anti-goals:** không muốn tool làm chậm agent; không muốn lộ code của mình cho người khác.
- **Tần suất:** hằng ngày.

### PER-0002 — Team Lead / Architect ("Anh")
- **Role:** Lead/Architect, tối ưu workflow & chi phí toàn team. **Experience:** Senior/Principal.
- **Goals:** dashboard token/cost theo dev·project; phát hiện điểm chậm/lỗi; nhận đề xuất cải thiện workflow; kiến trúc mở cho Codex/Antigravity.
- **Pain:** tối ưu "mò" vì thiếu dữ liệu; chi phí LLM khó kiểm soát ở org.
- **Anti-goals:** không muốn bị khóa vào 1 vendor agent/LLM.
- **Tần suất:** hằng tuần.

### PER-0003 — Platform / DevEx Admin ("Tú")
- **Role:** vận hành nền tảng. **Experience:** Senior. **Stack:** k8s/docker, IdP, MDM.
- **Goals:** triển khai collector toàn org dễ; quản RBAC; cấu hình LLM provider/retention/redaction.
- **Pain:** onboard hook/OTEL trên nhiều máy dev thủ công; cấu hình phân tán.
- **Anti-goals:** không muốn mỗi dev tự cấu hình lệch nhau.
- **Tần suất:** định kỳ.

### PER-0004 — Security / FinOps ("Linh")
- **Role:** bảo mật & chi phí. **Experience:** Senior.
- **Goals:** đảm bảo redaction; audit truy cập; kiểm soát budget LLM; chặn vendor nhạy cảm cho project gov.
- **Pain:** code/transcript có thể lọt ra LLM/cloud; chi phí vendor khó truy vết.
- **Anti-goals:** không cho data project gov gửi ra vendor TQ (GLM/MiniMax) trừ khi cho phép.
- **Tần suất:** định kỳ + khi có alert.

## 3. Business stakeholders
| Vai | Quyền quyết định | Sync/async | SLA |
|---|---|---|---|
| PO / Sponsor (Lead) | scope, ngân sách | sync | - |
| FinOps | budget LLM | async | 48h |

## 4. Technical stakeholders
- DevOps/Platform (deploy backend, ClickHouse/PG), Security (redaction/policy), IdP team (SSO), vendor LLM (Anthropic/OpenAI/Google/Zhipu/MiniMax).

## 5. Anti-personas (cố ý không phục vụ)
- External contractor/guest không thuộc org; agent không expose hook/telemetry; người dùng muốn dùng tool để **điều khiển** agent (ngoài scope — chỉ observe).

## Open questions
- Số lượng dev thực tế (định cỡ scale)? Có vai trò Security riêng hay gộp Admin?
