use std::path::PathBuf;

use clap::Parser;
use ukeela_cli::{Backend, DEFAULT_CHANELLS_SIZE, StabilizationMode, run};

#[derive(clap::Parser, Clone)]
pub struct Config {
    #[arg(short, long, default_value = "/dev/video0")]
    pub camera_device: String,

    #[arg(long, default_value_t = 1920)]
    pub width: u32,

    #[arg(long, default_value_t = 1080)]
    pub height: u32,

    #[arg(long, default_value_t = 30)]
    pub fps: u32,

    #[arg(long)]
    pub use_stabilization: bool,

    #[arg(long, default_value_t = StabilizationMode::L1Optimal)]
    pub mode: StabilizationMode,

    #[arg(long)]
    pub gpu_enabled: bool,

    #[arg(long, default_value_t = Backend::Auto)]
    pub backend: Backend,

    #[arg(long, default_value_t = DEFAULT_CHANELLS_SIZE)]
    pub channel_size: usize,

    #[arg(long, global = true, required = false, default_value_t = default_log_level())]
    pub log_level: tracing::Level,
    /// Directory where all logs will be placed
    #[arg(long, required = false, default_value_os_t = dirs::home_dir().unwrap_or_default().join("logs") )]
    pub log_output_dir: PathBuf,

    /// Format logs as JSON
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

fn default_log_level() -> tracing::Level {
    if cfg!(debug_assertions) {
        tracing::Level::DEBUG
    } else {
        tracing::Level::INFO
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let Config {
        camera_device,
        width,
        height,
        fps,
        use_stabilization,
        mode,
        gpu_enabled,
        backend,
        channel_size,
        log_level,
        log_output_dir,
        json,
    } = Config::parse();

    let _guard = setup_logger(log_level, log_output_dir, "ukeela", json);

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    runtime.block_on(run(use_stabilization, gpu_enabled, backend, channel_size))?;

    Ok(())
}

pub fn setup_logger(
    log_level: tracing::Level,
    log_output_dir: PathBuf,
    name: &str,
    json: bool,
) -> tracing_appender::non_blocking::WorkerGuard {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    // Set up daily rolling file appender (creates logs/app.log.YYYY-MM-DD)
    let file_appender = tracing_appender::rolling::hourly(log_output_dir, name);
    let (non_blocking_appender, guard) = tracing_appender::non_blocking(file_appender);

    let filter = tracing_subscriber::EnvFilter::builder()
        .with_default_directive(log_level.into())
        .from_env()
        .expect("default level is set")
        .add_directive("reqwest=warn".parse().unwrap())
        .add_directive("hyper_util=warn".parse().unwrap());

    let is_show_file = cfg!(debug_assertions);
    if json {
        let file_layer = tracing_subscriber::fmt::layer()
            .json()
            .with_ansi(false)
            .with_writer(non_blocking_appender);
        let fmt_layer = tracing_subscriber::fmt::layer()
            .json()
            .with_ansi(false)
            .with_file(is_show_file)
            .with_line_number(is_show_file)
            .with_target(is_show_file);
        tracing_subscriber::registry()
            .with(fmt_layer)
            .with(filter)
            .with(file_layer)
            .init();
    } else {
        let file_layer = tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .with_writer(non_blocking_appender);
        let fmt_layer = tracing_subscriber::fmt::layer()
            .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stdout()))
            .with_file(is_show_file)
            .with_line_number(is_show_file)
            .with_target(is_show_file);
        tracing_subscriber::registry()
            .with(fmt_layer)
            .with(filter)
            .with(file_layer)
            .init();
    }

    guard
}
