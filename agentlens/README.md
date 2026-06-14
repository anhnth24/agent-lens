# AgentLens

Công cụ **local** theo dõi & review session **Claude Code**: thu hook + transcript JSONL (+ OpenTelemetry tùy chọn) → lưu **SQLite** → UI web/desktop xem theo **repo** với thống kê **token in / out / cached**, **chi phí**, **tiết kiệm cache** và nhiều phân tích để cải thiện workflow agent. Việc thu thập là **zero-token** (chỉ đọc dữ liệu Claude Code đã sinh ra trên máy).

1 binary Rust gọn: hook receiver + JSONL tailer + OTLP receiver + query API + UI server. Dữ liệu **không rời máy** (chỉ gọi LLM khi bạn bấm "Tóm tắt/Insight" và đã đặt API key).

## Tính năng

- **Live** — các session đang chạy (near-realtime ~1–2s), hành động cuối, token/cost tăng dần.
- **Sessions** — danh sách theo repo + điểm **health** mỗi session; mở ra xem timeline, breakdown theo prompt/model, friction/loop, **cost burn**, lỗi.
- **Replay** — tua lại từng bước prompt → thinking → tool → result (phím ←/→/Esc).
- **Auto-follow** — bám timeline session đang chạy theo thời gian thực (chỉ append event mới, không giật).
- **Tools / Files** — phân tích tool (tần suất, lỗi, thời lượng), chuỗi tool (A→B), thao tác chậm, hot files.
- **Phân tích** — trend token theo ngày, health theo tuần, repo leaderboard, outcome correlation, heatmap hoạt động.
- **Chất lượng** — digest 7 ngày, **model right-sizing**, prompt quality theo độ dài & style, recovery (gỡ lỗi), cache advisor, skill/subagent usage, error clustering.
- **Insight (LLM)** — tóm tắt 1 session hoặc phân tích cross-session để gợi ý cải thiện (tùy chọn, cần API key, có redaction).
- **Giao diện** — pixel/retro, 2 theme **sáng/tối** (nút ☀/🌙, nhớ lựa chọn), lọc thời gian today/7d/30d/90d, tìm kiếm full-text. Áp dụng design system từ skill `ui-ux-pro-max`.

## Yêu cầu

- **Rust** (stable, kèm `cargo`).
- Chạy **server/headless**: không cần gì thêm.
- Build **desktop (Tauri 2)** — cần lib hệ thống:
  - **Linux:** `libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev libjavascriptcoregtk-4.1-dev librsvg2-dev`
  - **macOS:** Xcode Command Line Tools · **Windows:** WebView2 + MSVC build tools
  - (Theo prerequisites Tauri 2: <https://tauri.app>)

## Cách 1 — Chạy local (server/headless)

Đơn giản nhất, mở bằng trình duyệt:

```bash
cd agentlens
cargo run --release          # mặc định http://127.0.0.1:8787
```

Mở <http://127.0.0.1:8787>. Tailer tự quét `~/.claude/projects/**/*.jsonl` nên **chạy là có dữ liệu ngay**, không bắt buộc cấu hình hook.

## Cách 2 — App desktop (Tauri 2)

**Chạy thử (dev):** app tự chạy server lõi trong nền và mở cửa sổ trỏ tới UI (cùng origin với API, không CORS):

```bash
cd agentlens
cargo run -p agentlens-desktop --release
```

**Đóng gói cài đặt (production bundle):**

```bash
cargo install tauri-cli --version "^2"      # cài 1 lần
cd agentlens/desktop/src-tauri
cargo tauri build                            # output ở target/release/bundle/
```

Kết quả: `.deb`/`.AppImage` (Linux), `.dmg`/`.app` (macOS), `.msi`/`.exe` (Windows) trong `target/release/bundle/`.

## Biến môi trường (tùy chọn)

| Env | Mặc định | Ý nghĩa |
|---|---|---|
| `AGENTLENS_ADDR` | `127.0.0.1:8787` | địa chỉ bind (API + UI + `/hook` + OTLP) |
| `AGENTLENS_DATA_DIR` | `~/.agentlens` | nơi chứa `agentlens.db` (SQLite, WAL) |
| `AGENTLENS_PROJECTS_DIR` | `~/.claude/projects` | thư mục transcript JSONL để tail |
| `AGENTLENS_PRICING_URL` | LiteLLM (community) | nguồn bảng giá JSON, refresh hàng ngày. Đặt rỗng (`""`) để chỉ dùng bảng built-in |
| `AGENTLENS_PRICING_FILE` | — | file JSON giá local (ưu tiên hơn URL) |
| `ANTHROPIC_API_KEY` | — | bật tính năng Insight/Tóm tắt (LLM). Không đặt → tắt |
| `AGENTLENS_MODEL` | `claude-haiku-4-5-20251001` | model cho tính năng LLM |

> **Chi phí là ước tính** theo bảng giá (built-in hoặc LiteLLM) — phụ thuộc nguồn giá, không phải hóa đơn chính thức.

## Bật hook Claude Code (realtime, tùy chọn)

Tailer đã đủ để có dữ liệu. Bật hook giúp nhận diện session + `cwd`(repo) + `transcript_path` **ngay khi bắt đầu** và đẩy update realtime.

Cách nhanh (merge idempotent, có backup):

```bash
./scripts/install-hooks.sh                                              # ~/.claude/settings.json, url :8787
./scripts/install-hooks.sh ~/.claude/settings.json http://127.0.0.1:8787/hook
```

Khởi động lại Claude Code để nạp hooks. Mẫu config: `examples/claude-settings.json`. Hoặc thêm thủ công vào `~/.claude/settings.json`:

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

## Bật OpenTelemetry (cost/LOC/commit chính xác, tùy chọn)

Đặt biến trên máy chạy Claude Code để đẩy metric OTLP/HTTP JSON về AgentLens:

```bash
export CLAUDE_CODE_ENABLE_TELEMETRY=1
export OTEL_METRICS_EXPORTER=otlp
export OTEL_EXPORTER_OTLP_PROTOCOL=http/json
export OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:8787
```

AgentLens trích `cost.usage`, `lines_of_code.count`, `commit.count`, `pull_request.count` cho từng session (hiển thị trong chi tiết session).

## API

| Method | Path | Mô tả |
|---|---|---|
| POST | `/hook` | nhận hook Claude Code |
| POST | `/v1/metrics` | OTLP/HTTP JSON metrics (logs/traces được nhận & bỏ qua) |
| GET | `/api/totals` | tổng session + token + cost + tiết kiệm cache |
| GET | `/api/projects` | danh sách repo + token mỗi repo |
| GET | `/api/live?mins=10` | session đang hoạt động |
| GET | `/api/sessions?project=&range=` | session (lọc repo/thời gian) + health |
| GET | `/api/sessions/{id}/events?after=` | timeline 1 session (hỗ trợ auto-follow) |
| GET | `/api/sessions/{id}/prompts\|models\|friction\|errors\|otel` | breakdown chi tiết |
| POST | `/api/sessions/{id}/tag` · `/summarize` | gắn tag/outcome · tóm tắt LLM |
| GET | `/api/summary?group_by=project\|day\|model` | thống kê token theo nhóm |
| GET | `/api/tools\|files\|slowest\|sequences\|outcomes\|heatmap` | phân tích tool/file/hoạt động |
| GET | `/api/health-trend\|leaderboard\|digest` | sức khỏe theo tuần · repo leaderboard · digest |
| GET | `/api/recovery\|prompt-styles\|prompt-insights\|cache-advisor\|model-rightsizing\|agents\|error-clusters` | phân tích chất lượng |
| GET | `/api/search?q=` · `/api/insights` · POST `/api/insights/analyze` | tìm kiếm · insight đã lưu · phân tích cross-session |
| GET | `/ws` | WebSocket báo update (live) |

## Dữ liệu & quyền riêng tư

- Mọi thứ chạy **local**; DB ở `~/.agentlens/agentlens.db` (SQLite/WAL) — sao lưu/xóa thủ công.
- Chỉ đọc transcript Claude Code đã có; **không gửi gì ra ngoài** trừ khi bạn bấm Insight/Tóm tắt (khi đó nội dung được **redact** trước khi gửi tới Anthropic API).
- Dedup idempotent theo `event_id` (uuid dòng JSONL) — chạy lại an toàn.

## Skill thiết kế

Giao diện áp dụng design system từ skill `ui-ux-pro-max` (đã cài tại `.claude/skills/ui-ux-pro-max`, MIT). Có thể gọi lại trong Claude Code để chỉnh sửa UI về sau.

## Test

```bash
cargo test
```
