//! Main entry point for the Steel Minecraft server with a TUI.
use std::sync::Arc;
use steel::config::{LogConfig, LogTimeFormat};
use steel::{STEEL_CONFIG, SteelServer};
use steel_tui::{SteelApp, TuiLoggerWriter};
use steel_utils::text::DisplayResolutor;
use text_components::fmt::set_display_resolutor;
use tokio::runtime::{Builder, Runtime};
use tokio_util::sync::CancellationToken;
use tracing_subscriber::fmt::time;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, fmt};

#[cfg(feature = "mimalloc")]
#[global_allocator]
static ALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn init_logger() {
    let env_filter = EnvFilter::builder()
        .with_default_directive(tracing::Level::INFO.into())
        .from_env_lossy();

    let log = STEEL_CONFIG.log.clone().unwrap_or(LogConfig {
        time: LogTimeFormat::Uptime,
        module_path: false,
        extra: false,
    });

    let fmt_layer = fmt::layer()
        .with_writer(TuiLoggerWriter)
        .with_target(log.module_path);

    set_display_resolutor(&DisplayResolutor);
    match log.time {
        LogTimeFormat::None => {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(fmt_layer.without_time())
                .init();
        }
        LogTimeFormat::Date => {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(fmt_layer.with_timer(time::ChronoUtc::new("%T:%3f".to_string())))
                .init();
        }
        LogTimeFormat::Uptime => {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(fmt_layer.with_timer(time::uptime()))
                .init();
        }
    }
}

#[allow(clippy::unwrap_used)]
fn main() {
    let chunk_runtime = Arc::new(Builder::new_multi_thread().enable_all().build().unwrap());
    let main_runtime = Builder::new_multi_thread().enable_all().build().unwrap();

    main_runtime.block_on(main_async(chunk_runtime.clone()));

    drop(main_runtime);
    drop(chunk_runtime);
}

async fn main_async(chunk_runtime: Arc<Runtime>) {
    init_logger();
    let terminal = ratatui::init();

    let token = CancellationToken::new();
    let server_token = token.child_token();

    let steel_server = SteelServer::new(chunk_runtime, server_token.clone()).await;

    let mut steel_app = SteelApp::new(steel_server.server.clone(), token.clone(), server_token);
    let app_handle = tokio::spawn(async move {
        steel_app
            .run(terminal)
            .await
            .expect("error while running server");
    });

    SteelApp::start_server(steel_server).await;
    app_handle.await.expect("error while awaiting app");

    ratatui::restore();
}
