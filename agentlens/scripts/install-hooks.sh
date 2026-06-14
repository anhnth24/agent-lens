#!/usr/bin/env bash
# Merge AgentLens HTTP hooks vào settings.json của Claude Code (idempotent, có backup).
# Dùng: scripts/install-hooks.sh [SETTINGS_PATH] [HOOK_URL]
#   SETTINGS_PATH mặc định: ~/.claude/settings.json
#   HOOK_URL      mặc định: http://127.0.0.1:8787/hook
set -euo pipefail

SETTINGS="${1:-$HOME/.claude/settings.json}"
URL="${2:-http://127.0.0.1:8787/hook}"

mkdir -p "$(dirname "$SETTINGS")"
[ -f "$SETTINGS" ] && cp "$SETTINGS" "$SETTINGS.bak.$(date +%s)" && echo "Backup: $SETTINGS.bak.*"

SETTINGS="$SETTINGS" URL="$URL" python3 - <<'PY'
import json, os
path = os.environ["SETTINGS"]
url  = os.environ["URL"]
events = ["SessionStart","UserPromptSubmit","PreToolUse","PostToolUse","SubagentStop","Stop","SessionEnd"]

data = {}
if os.path.exists(path):
    with open(path) as f:
        try: data = json.load(f)
        except Exception: data = {}

hooks = data.setdefault("hooks", {})
for ev in events:
    entry = {"matcher": "*", "hooks": [{"type": "http", "url": url, "timeout": 5}]}
    hooks[ev] = [entry]   # ghi đè block hook của AgentLens cho event này

with open(path, "w") as f:
    json.dump(data, f, indent=2)
    f.write("\n")
print("Đã ghi hooks vào", path, "->", url)
PY

echo "Xong. Khởi động lại Claude Code để nạp hooks (settings đọc lúc start session)."
