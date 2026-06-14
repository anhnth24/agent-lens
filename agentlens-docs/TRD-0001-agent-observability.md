---
id: TRD-0001
type: trd
title: "AgentLens — Technical Design Document"
status: Draft
owner: sa
parents: [PRD-0001]
version: 1
mode: create
---

# TRD-0001: AgentLens — Technical Design Document

> Cặp với **PRD-0001** (yêu cầu nghiệp vụ). Tài liệu này đủ chi tiết để bắt đầu lập trình: kiến trúc, data model, contract, config, cấu trúc dự án, thứ tự build.
> Ghi chú: các quyết định kỹ thuật được đánh dấu `[Inference]`; sự kiện sản phẩm Claude Code (hook/OTEL/JSONL) là `[Verified]` theo tài liệu chính thức (REF-1/2/3 trong PRD).

---

## 1. Mục tiêu & phạm vi

Hệ thống observability + analytics cho coding agent (Claude Code trước, agent-agnostic), org-wide, desktop (Tauri). 10 module M1–M10 theo PRD. TRD này thiết kế lõi để code Phase A→E (xem §10).

---

## 2. Kiến trúc tổng thể

```mermaid
flowchart LR
  subgraph DevMachine[Máy dev]
    cc[Claude Code]
    col[Collector - Rust]
    cc -->|hook HTTP| col
    cc -->|JSONL tail| col
    cc -->|OTLP| col
  end
  col -->|gRPC/HTTP + buffer| api[Backend API - Rust/axum]
  api --> ch[(ClickHouse: events)]
  api --> pg[(PostgreSQL: control plane)]
  api -->|WebSocket| desk[Desktop - Tauri + Web UI]
  ch --> desk
  api --> alert[Alerting]
  api --> gw[LLM Gateway provider-agnostic]
  gw --> v1[Anthropic/Bedrock]
  gw --> v2[OpenAI]
  gw --> v3[Gemini]
  gw --> v4[GLM]
  gw --> v5[MiniMax]
  alert --> notify[in-app/email/Slack/webhook]
```

**Nguyên tắc:** [Inference]
- Collector **thụ động, zero-token** — chỉ đọc dữ liệu Claude Code đã sinh (M1–M3).
- Backend là **control + query plane**; ClickHouse cho event analytics, PostgreSQL cho metadata/RBAC.
- Mọi thứ phụ thuộc vendor (agent, LLM) đi qua **adapter/gateway** để thay được bằng config.

---

## 3. Tech stack (chốt)

| Lớp | Lựa chọn | Ghi chú |
|---|---|---|
| Desktop shell | **Tauri 2.x** (Rust core) | đã chốt; nhẹ, cross-platform |
| Desktop UI | **React + TypeScript + Vite** | [Inference] hợp Tauri; có thể Svelte |
| Collector | **Rust** (tokio + axum/hyper) | [Inference] perf, chạy nền, đồng nhất Tauri core |
| Backend API | **Rust (axum)** | [Inference] perf ingest; *alternative:* .NET 8 nếu muốn tận dụng team enterprise |
| Event store | **ClickHouse** | đã chốt |
| Control plane | **PostgreSQL 16+** | đã chốt |
| LLM Gateway | provider-agnostic, **OpenAI-compatible** core | [Inference] tự viết (Rust) hoặc nhúng **LiteLLM** (Python sidecar) |
| Realtime | **WebSocket** (fallback SSE) | [Inference] |
| Auth | **OIDC/SSO** + JWT | RBAC ở PG |
| Secrets | Vault / cloud KMS | API key vendor KHÔNG lưu DB thường |
| Deploy | Docker + k8s; collector cài máy dev | |

---

## 4. Nguồn dữ liệu Claude Code (cách lấy)

### 4.1 Hooks — HTTP, realtime `[Verified]`
`.claude/settings.json` (đẩy tập trung qua `managed-settings.json`/MDM — FR-37):

```json
{
  "hooks": {
    "SessionStart":     [{ "matcher": "*", "hooks": [{ "type": "http", "url": "http://127.0.0.1:8787/hook", "timeout": 5 }] }],
    "UserPromptSubmit": [{ "matcher": "*", "hooks": [{ "type": "http", "url": "http://127.0.0.1:8787/hook", "timeout": 5 }] }],
    "PreToolUse":       [{ "matcher": "*", "hooks": [{ "type": "http", "url": "http://127.0.0.1:8787/hook", "timeout": 5 }] }],
    "PostToolUse":      [{ "matcher": "*", "hooks": [{ "type": "http", "url": "http://127.0.0.1:8787/hook", "timeout": 5 }] }],
    "PermissionRequest":[{ "matcher": "*", "hooks": [{ "type": "http", "url": "http://127.0.0.1:8787/hook", "timeout": 5 }] }],
    "SubagentStop":     [{ "matcher": "*", "hooks": [{ "type": "http", "url": "http://127.0.0.1:8787/hook", "timeout": 5 }] }],
    "Stop":             [{ "matcher": "*", "hooks": [{ "type": "http", "url": "http://127.0.0.1:8787/hook", "timeout": 5 }] }],
    "SessionEnd":       [{ "matcher": "*", "hooks": [{ "type": "http", "url": "http://127.0.0.1:8787/hook", "timeout": 5 }] }]
  }
}
```
- Payload tới collector (POST body): `hook_event_name`, `session_id`, `cwd`, `permission_mode`, `transcript_path`, và với tool: `tool_name`, `tool_input`, `tool_response`. `[Verified]`
- Collector **trả về nhanh** (exit/200) để không chặn agent.

### 4.2 OpenTelemetry — metrics/cost `[Verified]`
Bật trên máy dev, trỏ về collector (OTLP receiver) hoặc OTEL Collector sidecar:
```bash
CLAUDE_CODE_ENABLE_TELEMETRY=1
OTEL_METRICS_EXPORTER=otlp
OTEL_LOGS_EXPORTER=otlp
OTEL_EXPORTER_OTLP_PROTOCOL=grpc
OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:4317
```
- Dùng cho: `claude_code.token.usage` (input/output/cacheRead/cacheCreation), `claude_code.cost.usage` (kèm `agent.name`, `skill.name`), tool latency, LOC.
- **Lưu ý:** OTEL **không chứa** nội dung file/prompt mặc định `[Verified]` → nội dung & "thinking" lấy từ JSONL (§4.3), không phải OTEL.

### 4.3 Transcript JSONL — "agent nghĩ gì" + token `[Verified]`
- Đường dẫn lấy từ `transcript_path` trong mọi hook payload: `~/.claude/projects/<proj>/<session>.jsonl`.
- Collector **tail** file; mỗi dòng có `message` (role, content gồm text/tool_use/tool_result/**thinking**) và `message.usage` (input/output/cache_read tokens).
- `[Unverified]` Mức độ JSONL lưu **đầy đủ raw thinking** hay rút gọn tùy version Claude Code → **verify thực nghiệm trước khi build M2/FR-9**; nếu thiếu, fallback chỉ tool/metrics.
- **Nguồn sự thật token/cost (D-11):** dùng **OTEL** (§4.2) làm nguồn chính cho cost/token; JSONL `message.usage` chỉ bổ sung nội dung + thinking → tránh đếm trùng. Khi thiếu OTEL cost, quy đổi từ token qua **bảng giá model** (per provider).

---

## 5. Data model

### 5.1 Normalized Event (schema chung — agent-agnostic, FR-5)
```jsonc
{
  "schema_version": 1,
  "agent_type": "claude_code",          // claude_code | codex | antigravity
  "org_id": "uuid", "project": "string", "dev_id": "uuid",
  "session_id": "string",
  "prompt_id": "string",                // gom event theo prompt. Phase A: suy ra từ chuỗi hook (session_id + thứ tự UserPromptSubmit→…→Stop). Phase B: đối chiếu OTEL prompt.id (D-10)
  "event_type": "post_tool",            // session_start|user_prompt|pre_tool|post_tool|permission|subagent_stop|stop|session_end
  "ts": "2026-06-15T10:00:00.000Z",
  "tool": { "name": "Edit", "input": {}, "response": {}, "duration_ms": 120, "success": true },
  "permission": "allow",                // allow|deny|ask|null
  "usage": { "model": "string", "input_tokens": 0, "output_tokens": 0, "cache_read_tokens": 0, "cost_usd": 0.0 },
  "thinking": "string|null",            // từ JSONL ([Unverified] theo version)
  "skill_name": "string|null",
  "meta": { "cwd": "string", "transcript_path": "string" }
}
```

### 5.2 ClickHouse — bảng events (FR-1/3/12/13/14)
```sql
CREATE TABLE events
(
    event_id          UUID,                          -- hash ổn định sinh ở collector (KHÔNG để DB tự sinh) → dedup idempotent (D-09)
    version           UInt64 DEFAULT 0,               -- ReplacingMergeTree dùng để giữ bản mới nhất khi trùng event_id
    ts                DateTime64(3),
    agent_type        LowCardinality(String),
    org_id            UUID,
    project           LowCardinality(String),
    dev_id            UUID,
    session_id        String,
    prompt_id         String,
    event_type        LowCardinality(String),
    tool_name         LowCardinality(String) DEFAULT '',
    duration_ms       UInt32 DEFAULT 0,
    success           UInt8  DEFAULT 1,
    permission        LowCardinality(String) DEFAULT '',
    model             LowCardinality(String) DEFAULT '',
    input_tokens      UInt32 DEFAULT 0,
    output_tokens     UInt32 DEFAULT 0,
    cache_read_tokens UInt32 DEFAULT 0,
    cost_usd          Float64 DEFAULT 0,
    skill_name        LowCardinality(String) DEFAULT '',
    payload           String DEFAULT '',     -- JSON tool_input/response/thinking (đã redact nếu policy yêu cầu)
    transcript_path   String DEFAULT ''
)
ENGINE = ReplacingMergeTree(version)          -- D-09: khử trùng theo ORDER BY key (gửi trùng/retry không nhân đôi, FR-6)
PARTITION BY toYYYYMM(ts)
ORDER BY (org_id, project, ts, session_id, event_id)
TTL toDateTime(ts) + INTERVAL 180 DAY;       -- retention 180 ngày (D-06, FR-41); dùng FINAL hoặc rollup MV khi đọc để bỏ bản trùng chưa merge
```
Gợi ý: materialized view cho rollup theo ngày/dev/skill (token/cost/latency p50/p95) để dashboard nhanh.

### 5.3 PostgreSQL — control plane (FR-31/32/34/41/50)
```sql
CREATE TABLE orgs            (id uuid PRIMARY KEY, name text, created_at timestamptz DEFAULT now());
CREATE TABLE teams           (id uuid PRIMARY KEY, org_id uuid REFERENCES orgs, name text);
CREATE TABLE projects        (id uuid PRIMARY KEY, org_id uuid REFERENCES orgs, code text, name text, sensitive bool DEFAULT false);
CREATE TABLE users           (id uuid PRIMARY KEY, org_id uuid, email text UNIQUE, name text, idp_sub text);
CREATE TABLE roles           (id smallint PRIMARY KEY, name text);           -- dev|lead|admin|security
CREATE TABLE user_roles      (user_id uuid, role_id smallint, scope_team uuid NULL, PRIMARY KEY(user_id, role_id, scope_team));
CREATE TABLE collectors      (id uuid PRIMARY KEY, dev_id uuid, hostname text, version text, last_seen timestamptz, status text);
CREATE TABLE llm_providers   (id uuid PRIMARY KEY, name text, vendor text, endpoint text, model text, region text,
                              enabled bool DEFAULT true, secret_ref text);   -- secret_ref -> Vault/KMS, KHÔNG lưu key ở đây
CREATE TABLE provider_policies(project_id uuid, allowed_vendors text[], blocked_vendors text[]); -- FR-50
CREATE TABLE redaction_rules (id uuid PRIMARY KEY, org_id uuid, pattern text, action text);      -- FR-23
CREATE TABLE alert_rules     (id uuid PRIMARY KEY, org_id uuid, name text, condition jsonb, channels jsonb, scope jsonb); -- FR-28/30
CREATE TABLE alert_history   (id uuid PRIMARY KEY, rule_id uuid, ts timestamptz, payload jsonb, delivered bool);
CREATE TABLE insights        (id uuid PRIMARY KEY, org_id uuid, scope jsonb, provider text, summary text,
                              recommendations jsonb, cost_usd numeric, status text, feedback text, created_at timestamptz); -- FR-22/26
CREATE TABLE budgets         (id uuid PRIMARY KEY, scope jsonb, provider text, limit_usd numeric, period text);  -- FR-25/49
CREATE TABLE audit_log       (id bigserial PRIMARY KEY, user_id uuid, action text, target text, ts timestamptz DEFAULT now()); -- FR-33
```

---

## 6. Components

### 6.1 Collector (Rust) — M1
Endpoints/đầu vào: `POST /hook` (HTTP hook), OTLP receiver `:4317`, JSONL tailer.
Chức năng: chuẩn hóa → Normalized Event (§5.1); buffer cục bộ (SQLite/embedded) khi offline; sync batch lên backend; idempotent (event_id/hash), dedup, gom theo prompt_id (FR-4/5/6).
Báo health về backend định kỳ (FR-36).

### 6.2 Backend API (Rust/axum) — M1/M3/M6
- `POST /ingest/batch` nhận event từ collector → ghi ClickHouse.
- Query API (đọc ClickHouse) cho dashboard/analytics.
- Auth/RBAC middleware (JWT + role scope).
- Orchestrate alerting + gọi gateway.

### 6.3 Realtime (WebSocket) — M2
`WS /realtime?session=...|project=...`: backend push event mới (sau ingest) tới desktop subscriber. Server-side filter theo RBAC.

### 6.4 LLM Gateway (provider-agnostic) — M4
- Interface chung kiểu OpenAI: `chat(provider, messages, opts) -> {text, usage, cost}`.
- Adapter per vendor: anthropic(bedrock/vertex/api), openai, gemini, zhipu(GLM), minimax. `[Inference]` vendor không OpenAI-compatible → shim riêng; cân nhắc **LiteLLM** sidecar để rút gọn.
- **Redaction** chạy TRƯỚC khi gọi (FR-23). **Cost guardrail** + budget per provider (FR-25/49). **Provider policy** chặn vendor theo `projects.sensitive`/`provider_policies` (FR-50). Default + fallback (FR-48).
- Config mẫu:
```yaml
default: claude-bedrock
fallback: [openai, gemini]
providers:
  - { name: claude-bedrock, vendor: anthropic, via: bedrock, region: ap-southeast-1, model: "anthropic.claude-..." }
  - { name: openai,        vendor: openai,    endpoint: "https://api.openai.com/v1", model: "gpt-..." }
  - { name: gemini,        vendor: google,    model: "gemini-..." }
  - { name: glm,           vendor: zhipu,     endpoint: "...", model: "glm-..." }      # TQ — kiểm tra policy
  - { name: minimax,       vendor: minimax,   endpoint: "...", model: "..." }          # TQ — kiểm tra policy
```

### 6.5 Alerting — M5
Rule engine eval theo batch ingest + anomaly (FR-27/28); dedup/throttle; route theo policy team; gửi in-app/email/Slack/webhook (FR-29/30).

### 6.6 Desktop (Tauri + React/TS) — M2/M3
Views: Live timeline (gom prompt_id), Event detail (tool_input/response/thinking), Dashboard (token/cost/latency p50-p95/deny/hook-fail), Trends, Compare, Reports/export, Insights, Settings/Admin.

### 6.7 Auth/RBAC — M6
OIDC login → JWT. Ma trận quyền:

| Role | Xem session | Analytics team/org | Cấu hình/RBAC | Audit/cost/redaction |
|---|---|---|---|---|
| dev | của mình | - | - | - |
| lead | team | ✓ | - | xem cost |
| admin | ✓ | ✓ | ✓ | ✓ |
| security | ✓ | ✓ | policy bảo mật | ✓ |

---

## 7. API contracts (rút gọn)

```
POST /ingest/batch        # collector -> backend; body: Event[]; 200/ack
GET  /sessions            # filter: dev,project,from,to,limit
GET  /sessions/{id}/events
GET  /metrics/summary     # group_by: dev|project|skill|model|tool; range
GET  /metrics/latency     # percentile tool latency
WS   /realtime
POST /insights/analyze    # body: {scope, provider?}; -> job
GET  /insights/{id}
POST /insights/{id}/feedback
GET  /providers           # admin
POST /alert-rules         # admin
GET  /collectors          # health
POST /export/report       # csv/pdf/xlsx
```
Auth: `Authorization: Bearer <jwt>`; mọi query lọc theo org/role scope.

---

## 8. Security
- **Redaction** (FR-23): regex secret/API key/token + tùy chọn strip code/PII, áp dụng trước khi ghi `payload` (nếu policy) và trước khi gửi LLM.
- TLS in-transit; encrypt at-rest (PG + ClickHouse); API key vendor ở Vault/KMS (`secret_ref`).
- Audit log mọi thao tác đọc nhạy cảm/đổi config (FR-33).
- `[Inference]` Project `sensitive=true` → mặc định **chặn vendor ngoài** (đặc biệt GLM/MiniMax — TQ) trừ khi allow tường minh (FR-50).

---

## 9. Cấu trúc dự án (monorepo)

```
agentlens/
├─ collector/            # Rust: hook receiver + OTLP + JSONL tailer + buffer/sync
├─ backend/              # Rust(axum): ingest, query, realtime WS, auth, alerting
│  └─ src/{ingest,query,realtime,auth,alerting,gateway_client}/
├─ gateway/              # LLM gateway (Rust hoặc Python/LiteLLM sidecar)
├─ desktop/
│  ├─ src-tauri/         # Rust (Tauri core)
│  └─ src/               # React + TS (UI)
├─ shared/               # schema Event + types (codegen sang TS/Rust)
├─ migrations/{postgres,clickhouse}/
├─ deploy/docker-compose.yml
└─ docs/{PRD-0001,TRD-0001}.md
```

---

## 10. Thứ tự build (mapping FR — không cắt scope, chỉ là build order)

| Phase | Nội dung | FR |
|---|---|---|
| **A. Xương sống realtime** | collector hook ingest → backend ingest + ClickHouse → desktop live timeline → auth/RBAC | FR-1,4,5,6,7,8,9,31 |
| **B. Analytics** | JSONL parser + dashboard token/cost/latency + PG control plane + filter | FR-2,3,12,13,14,15 |
| **C. LLM insight** | gateway provider-agnostic + redaction + cost guardrail + tóm tắt/đề xuất | FR-21,22,23,24,25,48 |
| **D. Vận hành** | alerting + reporting/export + retention + FinOps | FR-18,19,28,29,30,41 |
| **E. Mở rộng** | API/webhook + onboarding + replay/compare + per-provider cost/policy + adapter Codex/Antigravity | FR-10,16,35,36,37,38,39,44,45,46,47,49,50 |

> Gợi ý: hoàn tất **Phase A** trước = đã có "xem agent đang làm gì realtime" chạy được — dùng làm dogfood để verify `[Unverified]` thinking-in-JSONL.

---

## 11. Local dev (docker-compose)

```yaml
services:
  postgres:
    image: postgres:16
    environment: { POSTGRES_PASSWORD: dev, POSTGRES_DB: agentlens }
    ports: ["5432:5432"]
  clickhouse:
    image: clickhouse/clickhouse-server:latest
    ports: ["8123:8123", "9000:9000"]
  backend:
    build: ./backend
    depends_on: [postgres, clickhouse]
    ports: ["8080:8080"]   # API + WS
    environment:
      DATABASE_URL: postgres://postgres:dev@postgres/agentlens
      CLICKHOUSE_URL: http://clickhouse:8123
  gateway:
    build: ./gateway
    ports: ["8090:8090"]
# collector: chạy trên máy dev (ngoài compose), trỏ về backend:8080
```
Migrations: chạy `migrations/postgres` (vd sqlx/atlas) + `migrations/clickhouse` lúc khởi tạo.

---

## 12. Quyết định kỹ thuật (xem `DECISION-LOG.md`)

**Đã chốt (2026-06-14):**
- ✅ Backend **Rust (axum)** — D-01.
- ✅ Redaction **tại backend** — D-02 (xem rủi ro R-12).
- ✅ Vendor TQ (GLM/MiniMax) **giữ đầy đủ** v1 — D-03 (R-04 → Accept có mitigation).
- ✅ LLM provider mặc định **Anthropic** — D-04.
- ✅ Subscription chỉ cho phạm vi dev; org-wide dùng API key/Bedrock — D-05.
- ✅ Retention **180 ngày** — D-06.
- ✅ **Desktop-only** v1 (chưa làm web read-only) — D-07.
- ✅ Notify **in-app/email/webhook** — D-08.
- ✅ Dedup: **ReplacingMergeTree** + event_id hash ở collector — D-09 (§5.2).
- ✅ `prompt_id` Phase A suy ra từ chuỗi hook — D-10 (§4.1/§5.1).
- ✅ Token/cost: **OTEL là nguồn sự thật** — D-11 (§4.3).

**Còn mở:**
- `[Unverified]` Thinking đầy đủ trong JSONL? → verify ngay đầu Phase A.
- Gateway **tự viết Rust** vs **LiteLLM sidecar (Python)**.
- Realtime **WebSocket** vs **SSE**.
