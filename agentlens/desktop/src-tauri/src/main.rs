// AgentLens desktop (Tauri 2): chạy server lõi (agentlens::run) trong thread riêng,
// đợi nó sẵn sàng, rồi mở cửa sổ trỏ tới UI localhost (cùng origin với API — không CORS).
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use std::{net::TcpStream, thread, time::Duration};

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

    // 3) mở cửa sổ desktop trỏ tới UI
    tauri::Builder::default()
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
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("lỗi chạy ứng dụng Tauri");
}
