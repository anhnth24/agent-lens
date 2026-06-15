#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────────────
# AgentLens — launcher có menu (Windows qua Git Bash/WSL, cũng chạy macOS/Linux)
#
# Cách chạy trên Windows:
#   • Git Bash : chuột phải trong thư mục agentlens → "Git Bash Here" → ./run.sh
#   • WSL      : bash run.sh
#   • PowerShell/CMD: bash run.sh   (cần đã cài Git for Windows hoặc WSL)
#
# Có thể chọn item trực tiếp không cần menu:  ./run.sh 2   (build release)
# ─────────────────────────────────────────────────────────────────────────────
set -u

# về đúng thư mục chứa script (để cargo chạy đúng crate)
cd "$(dirname "$0")" || exit 1

# màu (tắt nếu không phải terminal)
if [ -t 1 ]; then B=$'\033[1m'; C=$'\033[36m'; Y=$'\033[33m'; R=$'\033[31m'; G=$'\033[32m'; N=$'\033[0m'; else B=; C=; Y=; R=; G=; N=; fi

# phát hiện OS để hiển thị đúng tên file nhị phân
case "$(uname -s 2>/dev/null)" in
  MINGW*|MSYS*|CYGWIN*) OSLBL="Windows"; EXE=".exe" ;;
  Darwin)               OSLBL="macOS";   EXE="" ;;
  *)                    OSLBL="Linux";   EXE="" ;;
esac
BIN="target/release/agentlens${EXE}"

need_cargo() {
  if ! command -v cargo >/dev/null 2>&1; then
    echo "${R}Không tìm thấy 'cargo'.${N} Cài Rust tại https://rustup.rs rồi mở lại terminal."
    [ "$OSLBL" = "Windows" ] && echo "Windows: cần thêm 'MSVC Build Tools' (Desktop development with C++) để biên dịch SQLite."
    return 1
  fi
}

pause() { echo; read -r -p "Enter để về menu..." _ || true; }

run_server() {            # 1) chạy server trực tiếp
  need_cargo || return
  echo "${G}▶ Chạy server (cargo run --release) → http://127.0.0.1:8787${N}"
  echo "  (Ctrl+C để dừng)"
  cargo run --release
}

build_release() {         # 2) build release cho OS hiện tại
  need_cargo || return
  echo "${G}▶ Build release cho ${OSLBL} …${N}"
  if cargo build --release; then
    echo "${G}✔ Xong.${N} File chạy: ${B}$(pwd)/${BIN}${N}"
    [ "$OSLBL" = "Windows" ] && echo "  Có thể copy '${BIN}' đi nơi khác và double-click để chạy."
  else
    echo "${R}✘ Build lỗi.${N}"
  fi
}

run_desktop() {           # 3) chạy app desktop (Tauri) ở chế độ dev
  need_cargo || return
  echo "${G}▶ Chạy desktop app (cargo run -p agentlens-desktop --release) …${N}"
  cargo run -p agentlens-desktop --release
}

build_desktop() {         # 4) đóng gói cài đặt desktop (Tauri)
  need_cargo || return
  if ! cargo tauri --version >/dev/null 2>&1; then
    echo "${Y}Chưa có cargo-tauri.${N} Cài bằng: ${B}cargo install tauri-cli${N}"
    read -r -p "Cài ngay? [y/N] " a
    case "$a" in y|Y) cargo install tauri-cli || return ;; *) return ;; esac
  fi
  echo "${G}▶ Đóng gói desktop (tauri build) …${N}"
  ( cd desktop/src-tauri && cargo tauri build )
  echo "${Y}Lưu ý:${N} Linux cần WebKitGTK; Windows cần WebView2; bundle nằm trong desktop/src-tauri/target/release/bundle."
}

dev_watch() {             # 6) dev hot reload: cargo watch + UI đọc từ disk
  need_cargo || return
  if ! cargo watch --version >/dev/null 2>&1; then
    echo "${Y}Chưa có cargo-watch.${N} Cài bằng: ${B}cargo install cargo-watch${N}"
    read -r -p "Cài ngay? [y/N] " a
    case "$a" in y|Y) cargo install cargo-watch || return ;; *) return ;; esac
  fi
  # AGENTLENS_DEV_UI=1: ui.rs đọc index.html từ disk → sửa HTML chỉ cần F5 browser.
  # cargo watch -i "ui/**": bỏ qua thư mục UI → chỉ rebuild+restart khi sửa .rs.
  export AGENTLENS_DEV_UI=1
  echo "${G}▶ Dev hot reload${N} → http://127.0.0.1:8787  (Ctrl+C để dừng)"
  echo "  sửa .rs → tự build+restart; sửa ui/index.html → chỉ F5 browser."
  cargo watch -i "ui/**" -x run
}

choose_backend() {        # 5) chọn backend LLM cho Insight (api key / subscription)
  echo "  LLM backend cho tính năng Insight/Tóm tắt:"
  echo "    a) api  — dùng ANTHROPIC_API_KEY (pay-as-you-go)"
  echo "    c) cli  — dùng 'claude -p' (login subscription Pro/Max)"
  read -r -p "  Chọn [a/c]: " b
  case "$b" in
    a|A) export AGENTLENS_LLM_BACKEND=api
         echo "  Đặt AGENTLENS_LLM_BACKEND=api (nhớ export ANTHROPIC_API_KEY)." ;;
    c|C) export AGENTLENS_LLM_BACKEND=cli
         echo "  Đặt AGENTLENS_LLM_BACKEND=cli. Cần đã 'claude auth login' (subscription)."
         command -v claude >/dev/null 2>&1 && claude auth status 2>/dev/null | head -3 || echo "  (chưa thấy 'claude' trên PATH)" ;;
    *)   echo "  Bỏ qua." ;;
  esac
  echo "  ${Y}(Biến môi trường chỉ áp dụng cho các lựa chọn chạy trong phiên menu này.)${N}"
}

do_action() {
  case "$1" in
    1) run_server ;;
    2) build_release ;;
    3) run_desktop ;;
    4) build_desktop ;;
    5) choose_backend ;;
    6) dev_watch ;;
    0|q|Q) exit 0 ;;
    *) echo "${R}Lựa chọn không hợp lệ: $1${N}" ;;
  esac
}

# chế độ không-tương-tác: ./run.sh <số>
if [ "$#" -ge 1 ]; then do_action "$1"; exit $?; fi

# menu vòng lặp
while true; do
  echo
  echo "${B}╔══════════════════════════════════════╗${N}"
  echo "${B}║            AgentLens (${OSLBL})$(printf '%*s' $((13-${#OSLBL})) '')║${N}"
  echo "${B}╚══════════════════════════════════════╝${N}"
  echo "  ${C}1${N}) Chạy server trực tiếp        (cargo run --release)"
  echo "  ${C}2${N}) Build release ${OSLBL}        → ${BIN}"
  echo "  ${C}3${N}) Chạy app desktop (dev)       (Tauri)"
  echo "  ${C}4${N}) Đóng gói cài đặt desktop     (tauri build)"
  echo "  ${C}5${N}) Chọn backend LLM (api/cli)"
  echo "  ${C}6${N}) Dev hot reload          (cargo watch + UI đọc từ disk)"
  echo "  ${C}0${N}) Thoát"
  read -r -p "Chọn: " choice || exit 0
  do_action "$choice"
  case "$choice" in 1|3|6) ;; *) pause ;; esac   # 1/3/6 chạy lâu, không pause
done
