//! AgentLens CLI: chạy server (hook + tailer + API + UI) ở chế độ headless.
//! App desktop (Tauri) dùng chung `agentlens::run()` từ lib.

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();
    agentlens::run().await
}
