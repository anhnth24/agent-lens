@echo off
setlocal enabledelayedexpansion
REM ============================================================================
REM  AgentLens - launcher menu cho Windows (native, double-click chay duoc)
REM  - Double-click run.bat, hoac chay trong CMD/PowerShell: run.bat
REM  - Chon nhanh khong can menu:  run.bat 2   (build release)
REM  Tuong duong run.sh (dung cho Git Bash/WSL/macOS/Linux).
REM ============================================================================

cd /d "%~dp0"
set "BIN=target\release\agentlens.exe"

REM --- che do khong tuong tac: run.bat <so> ---
if not "%~1"=="" (
  if "%~1"=="0" goto :eof
  call :do_action "%~1"
  goto :eof
)

:menu
echo.
echo  ========================================
echo             AgentLens ^(Windows^)
echo  ========================================
echo    1^) Chay server truc tiep      ^(cargo run --release^)
echo    2^) Build release Windows      -^> %BIN%
echo    3^) Chay app desktop ^(dev^)      ^(Tauri^)
echo    4^) Dong goi cai dat desktop   ^(tauri build^)
echo    5^) Chon backend LLM ^(api/cli^)
echo    6^) Dev hot reload          ^(cargo watch + UI doc tu disk^)
echo    0^) Thoat
set "choice="
set /p "choice=Chon: "
if "%choice%"=="0" goto :bye
if /i "%choice%"=="q" goto :bye
call :do_action "%choice%"
goto :menu

:bye
endlocal
goto :eof

:do_action
set "a=%~1"
if "%a%"=="1" goto run_server
if "%a%"=="2" goto build_release
if "%a%"=="3" goto run_desktop
if "%a%"=="4" goto build_desktop
if "%a%"=="5" goto choose_backend
if "%a%"=="6" goto dev_watch
echo Lua chon khong hop le: %a%
goto :eof

:need_cargo
where cargo >nul 2>nul
if errorlevel 1 (
  echo [LOI] Khong tim thay 'cargo'. Cai Rust tai https://rustup.rs roi mo lai terminal.
  echo       Windows can them 'MSVC Build Tools' ^(Desktop development with C++^) de bien dich SQLite.
  exit /b 1
)
exit /b 0

:run_server
call :need_cargo || goto pause
echo [RUN] Server: cargo run --release  -^> http://127.0.0.1:8787  ^(Ctrl+C de dung^)
cargo run --release
goto pause

:build_release
call :need_cargo || goto pause
echo [BUILD] Release cho Windows ...
cargo build --release
if errorlevel 1 ( echo [LOI] Build that bai. ) else ( echo [OK] Xong. File chay: %cd%\%BIN%  ^(co the double-click^) )
goto pause

:run_desktop
call :need_cargo || goto pause
echo [RUN] Desktop dev: cargo run -p agentlens-desktop --release ...
cargo run -p agentlens-desktop --release
goto pause

:build_desktop
call :need_cargo || goto pause
cargo tauri --version >nul 2>nul
if errorlevel 1 (
  echo Chua co cargo-tauri. Cai bang: cargo install tauri-cli
  set "yn="
  set /p "yn=Cai ngay? [y/N] "
  if /i "!yn!"=="y" ( cargo install tauri-cli ) else ( goto pause )
)
echo [BUILD] Dong goi desktop ^(tauri build^) ...
pushd desktop\src-tauri
cargo tauri build
popd
echo Luu y: Windows can WebView2 ^(thuong co san Win10/11^). Bundle o desktop\src-tauri\target\release\bundle.
goto pause

:dev_watch
call :need_cargo || goto pause
cargo watch --version >nul 2>nul
if errorlevel 1 (
  echo Chua co cargo-watch. Cai bang: cargo install cargo-watch
  set "yn="
  set /p "yn=Cai ngay? [y/N] "
  if /i "!yn!"=="y" ( cargo install cargo-watch ) else ( goto pause )
)
REM AGENTLENS_DEV_UI=1: ui.rs doc index.html tu disk -^> sua HTML chi can F5 browser.
REM cargo watch -i "ui/**": bo qua thu muc UI -^> chi rebuild+restart khi sua .rs.
set "AGENTLENS_DEV_UI=1"
echo [DEV] Hot reload: sua .rs -^> tu build+restart; sua ui\index.html -^> chi F5 browser.
echo       Server: http://127.0.0.1:8787  ^(Ctrl+C de dung^)
cargo watch -i "ui/**" -x run
goto pause

:choose_backend
echo   LLM backend cho Insight/Tom tat:
echo     a^) api  - dung ANTHROPIC_API_KEY ^(pay-as-you-go^)
echo     c^) cli  - dung 'claude -p' ^(login subscription Pro/Max^)
set "b="
set /p "b=Chon [a/c]: "
if /i "%b%"=="a" ( setx AGENTLENS_LLM_BACKEND api >nul & echo Da dat AGENTLENS_LLM_BACKEND=api ^(nho dat ANTHROPIC_API_KEY^). Mo terminal moi de co hieu luc. )
if /i "%b%"=="c" ( setx AGENTLENS_LLM_BACKEND cli >nul & echo Da dat AGENTLENS_LLM_BACKEND=cli. Can da 'claude auth login' ^(subscription^). Mo terminal moi de co hieu luc. )
goto pause

:pause
echo.
pause
goto :eof
