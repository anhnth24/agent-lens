# AgentLens (lean)

Công cụ **local** theo dõi & review session **Claude Code**: thu hook + transcript JSONL → lưu **SQLite** → UI web xem theo **repo** với thống kê **token in / out / cached**. Zero-token cho việc thu thập (chỉ đọc dữ liệu Claude Code đã sinh).

1 binary Rust: hook receiver + JSONL tailer + query API + UI server.

## Chạy

**Desktop app (Tauri 2)** — khuyến nghị:
```bash
cargo run -p agentlens-desktop --release
```
App tự chạy server lõi trong nền và mở cửa sổ trỏ tới UI (cùng origin với API).

**Hoặc chế độ server/headless** (mở UI bằng trình duyệt):
```bash
cargo run --release        # mặc định http://127.0.0.1:8787
```
Mở trình duyệt: <http://127.0.0.1:8787>.

> **Build desktop trên Linux** cần các lib hệ thống của Tauri:
> `libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev libjavascriptcoregtk-4.1-dev librsvg2-dev`.
> macOS/Windows: theo prerequisites của Tauri 2 (xem tauri.app).

Biến môi trường (tùy chọn):

| Env | Mặc định | Ý nghĩa |
|---|---|---|
| `AGENTLENS_ADDR` | `127.0.0.1:8787` | địa chỉ bind (API + UI + /hook) |
| `AGENTLENS_DATA_DIR` | `~/.agentlens` | nơi chứa `agentlens.db` (SQLite, WAL) |
| `AGENTLENS_PROJECTS_DIR` | `~/.claude/projects` | thư mục transcript JSONL để tail |
| `ANTHROPIC_API_KEY` | — | bật FR-8 (tóm tắt LLM). Không đặt → tính năng tóm tắt tắt |
| `AGENTLENS_MODEL` | `claude-haiku-4-5-20251001` | model cho FR-8 (chỉnh theo nhu cầu) |

> Tailer tự quét `~/.claude/projects/**/*.jsonl` nên **chạy là có dữ liệu ngay**, không bắt buộc cấu hình hook.

## Bật hook Claude Code (realtime, tùy chọn)

Cách nhanh (merge idempotent, có backup):
```bash
./scripts/install-hooks.sh                       # ~/.claude/settings.json, url mặc định :8787
./scripts/install-hooks.sh ~/.claude/settings.json http://127.0.0.1:8787/hook
```
Khởi động lại Claude Code để nạp hooks. Mẫu config: `examples/claude-settings.json`.

Hook giúp phát hiện session + `cwd`(repo) + `transcript_path` ngay khi bắt đầu. Hoặc thêm thủ công vào `~/.claude/settings.json`:

```json
{
  "hooks": {
    "SessionStart":     [{ "matcher": "*", "hooks": [{ "type": "http", "url": "http://127.0.0.1:8787/hook", "timeout": 5 }] }],
    "UserPromptSubmit": [{ "matcher": "*", "hooks": [{ "type": "http", "url": "http://127.0.0.1:8787/hook", "timeout": 5 }] }],
    "PreToolUse":       [{ "matcher": "*", "hooks": [{ "type": "http", "url": "http://127.0.0.1:8787/hook", "timeout": 5 }] }],
    "PostToolUse":      [{ "matcher": "*", "hooks": [{ "type": "http", "url": "http://127.0.0.1:8787/hook", "timeout": 5 }] }],
    "Stop":             [{ "matcher": "*", "hooks": [{ "type": "http", "url": "http://127.0.0.1:8787/hook", "timeout": 5 }] }],
    "SessionEnd":       [{ "matcher": "*", "hooks": [{ "type": "http", "url": "http://127.0.0.1:8787/hook", "timeout": 5 }] }]
  }
}
```

## API

| Method | Path | Mô tả |
|---|---|---|
| POST | `/hook` | nhận hook Claude Code (FR-1) |
| GET | `/api/totals` | tổng session + token in/out/cached |
| GET | `/api/projects` | danh sách repo + token mỗi repo |
| GET | `/api/sessions?project=` | session (lọc theo repo) + token mỗi session |
| GET | `/api/sessions/{id}/events` | timeline 1 session (prompt/thinking/tool/usage) |
| GET | `/api/summary?group_by=project\|day\|model` | thống kê token theo nhóm |
| POST | `/api/sessions/{id}/summarize` | FR-8: tóm tắt + gợi ý (redact trước khi gửi LLM) |

## Map tính năng (PRD lean)

- **FR-1** hook ingestion · **FR-2** tail JSONL (thinking/prompt/usage) · **FR-3** chuẩn hóa + dedup (event_id) · **FR-4** OTEL: *chưa làm v1, token lấy từ JSONL*.
- **FR-5** timeline · **FR-6** thống kê token in/out/cached theo session/ngày/skill/model · **FR-7** lọc theo repo/thời gian.
- **FR-8** LLM tóm tắt + gợi ý (tùy chọn) với redaction.
- **FR-9/10** retention/xóa: *chưa làm v1 (giữ tay/qua SQL)*.

## Token "cached"

Lấy từ `usage` của transcript: `cache_read_input_tokens` (hiển thị là **cached** — phần đọc lại từ prompt cache) và `cache_creation_input_tokens` (**cache write**). Token in = `input_tokens`, out = `output_tokens`.

## Test

```bash
cargo test
```
