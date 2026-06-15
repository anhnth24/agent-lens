// AgentLens desktop (Tauri 2): chạy server lõi (agentlens::run) trong thread riêng,
// đợi nó sẵn sàng, rồi mở cửa sổ trỏ tới UI localhost (cùng origin với API — không CORS).
// Khi mở app, kiểm tra bản mới qua GitHub Releases; nếu có thì hỏi cập nhật.
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use std::{net::TcpStream, thread, time::Duration};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
use tauri_plugin_updater::UpdaterExt;

/// File ghi version đã "Bỏ qua" để không hỏi lại bản đó (vẫn hỏi khi có bản mới hơn).
fn skip_file() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(".agentlens")
        .join("skipped-update")
}

/// Kiểm tra + xử lý cập nhật. Lỗi mạng / không có bản mới -> im lặng bỏ qua.
async fn check_update(app: tauri::AppHandle) {
    let updater = match app.updater() {
        Ok(u) => u,
        Err(_) => return,
    };
    let update = match updater.check().await {
        Ok(Some(u)) => u,
        _ => return,
    };
    let ver = update.version.clone();

    // Đã bấm "Bỏ qua" đúng version này -> không hỏi nữa.
    if std::fs::read_to_string(skip_file())
        .map(|s| s.trim() == ver)
        .unwrap_or(false)
    {
        return;
    }

    // Hỏi: Cập nhật ngay? (dialog blocking chạy trên thread riêng để khỏi chặn event loop)
    let want = {
        let app = app.clone();
        let msg = format!("Có bản mới {ver}. Cập nhật ngay không?");
        tokio::task::spawn_blocking(move || {
            app.dialog()
                .message(msg)
                .title("AgentLens — cập nhật")
                .buttons(MessageDialogButtons::OkCancelCustom(
                    "Cập nhật ngay".into(),
                    "Để sau".into(),
                ))
                .blocking_show()
        })
        .await
        .unwrap_or(false)
    };

    if want {
        // Tải + cài; xong khởi động lại app.
        match update.download_and_install(|_chunk, _total| {}, || {}).await {
            Ok(_) => app.restart(),
            Err(e) => {
                let app = app.clone();
                let msg = format!("Cập nhật thất bại: {e}");
                let _ = tokio::task::spawn_blocking(move || {
                    app.dialog().message(msg).title("Lỗi cập nhật").blocking_show()
                })
                .await;
            }
        }
    } else {
        // Không cập nhật ngay -> hỏi có "Bỏ qua bản này" không.
        let skip = {
            let app = app.clone();
            let msg = format!(
                "Bỏ qua bản {ver}? Sẽ không nhắc lại bản này (vẫn nhắc khi có bản mới hơn)."
            );
            tokio::task::spawn_blocking(move || {
                app.dialog()
                    .message(msg)
                    .title("Bỏ qua bản này?")
                    .buttons(MessageDialogButtons::OkCancelCustom(
                        "Bỏ qua bản này".into(),
                        "Không".into(),
                    ))
                    .blocking_show()
            })
            .await
            .unwrap_or(false)
        };
        if skip {
            let p = skip_file();
            if let Some(dir) = p.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(p, &ver);
        }
    }
}

fn main() {
    // 1) server lõi (hook + tailer + API + UI) trong tokio runtime riêng
    thread::spawn(|| {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("tạo tokio runtime");
        rt.block_on(async {
            if let Err(e) = agentlens::run().await {
                eprintln!("agentlens server lỗi: {e}");
            }
        });
    });

    // 2) đợi server bind xong (tối đa ~10s)
    let addr = agentlens::default_addr();
    for _ in 0..100 {
        if TcpStream::connect(&addr).is_ok() {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    let url = format!("http://{addr}/");

    // 3) mở cửa sổ desktop trỏ tới UI + kiểm tra cập nhật
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::External(url.parse().expect("url hợp lệ")),
            )
            .title("AgentLens — Claude Code sessions")
            .inner_size(1280.0, 840.0)
            .min_inner_size(900.0, 600.0)
            .build()?;

            // kiểm tra bản mới trong nền, không chặn mở cửa sổ
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move { check_update(handle).await });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("lỗi chạy ứng dụng Tauri");
}
