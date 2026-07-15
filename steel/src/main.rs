//! Main entry point for the Steel Minecraft server.
#![feature(thread_id_value)]

use std::backtrace::{Backtrace, BacktraceStatus};
use std::num::NonZero;
use std::panic::AssertUnwindSafe;
use std::path::Path;
use std::sync::Arc;
use std::{io, panic, thread};

use crossterm::style::Attribute::{Bold, Dim, Reset};
use crossterm::style::{Color, ResetColor, SetForegroundColor};
use futures::FutureExt;
use steel::config::{self, LogConfig};
use steel::logger::CommandLogger;
use steel::{SERVER, SteelServer, logger::LoggerLayer};
use steel_core::player::player_data::PersistentPlayerData;
use steel_core::player::player_data_storage::GlobalPlayerData;
use steel_core::server::Server;
use steel_utils::text::DisplayResolutor;
use text_components::fmt::set_display_resolutor;
use tokio::runtime::{Builder, Runtime};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
#[cfg(feature = "jaeger")]
use tracing::Subscriber;
use tracing::{Level, error};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};
#[cfg(feature = "jaeger")]
use tracing_subscriber::{Layer, registry::LookupSpan};

#[cfg(feature = "jaeger")]
fn init_jaeger<S>() -> impl Layer<S> + Send + Sync
where
    S: Subscriber + for<'span> LookupSpan<'span> + Send + Sync,
{
    use opentelemetry::KeyValue;
    use opentelemetry::global;
    use opentelemetry::trace::TracerProvider;
    use opentelemetry_sdk::Resource;
    use opentelemetry_sdk::trace::SdkTracerProvider;
    use tracing_opentelemetry::OpenTelemetryLayer;

    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .build()
        .expect("Failed to create OTLP span exporter");

    let tracer_provider = SdkTracerProvider::builder()
        .with_resource(
            Resource::builder()
                .with_attributes([
                    KeyValue::new("service.name", "steel"),
                    KeyValue::new(
                        "service.build",
                        if cfg!(debug_assertions) {
                            "debug"
                        } else {
                            "release"
                        },
                    ),
                ])
                .build(),
        )
        .with_batch_exporter(exporter)
        .build();

    let tracer = tracer_provider.tracer("steel");
    global::set_tracer_provider(tracer_provider);
    OpenTelemetryLayer::new(tracer)
        .with_filter(EnvFilter::new("trace,h2=off,hyper=off,tonic=off,tower=off"))
}

async fn init_tracing(
    cancel_token: CancellationToken,
    log_config: Option<LogConfig>,
) -> Result<Arc<CommandLogger>, String> {
    let log_level = log_config
        .as_ref()
        .map_or(Level::INFO.into(), |l| l.log_level.to_directive());

    let tracing = tracing_subscriber::registry();

    #[cfg(feature = "jaeger")]
    let tracing = tracing.with(init_jaeger());

    let layer = LoggerLayer::new(cancel_token, log_config)
        .await
        .map_err(|err| format!("failed to initialize logger: {err}"))?;
    let logger = layer.0.clone();

    let tracing = tracing.with(layer);

    let tracing = tracing.with(
        EnvFilter::builder()
            .with_default_directive(log_level)
            .from_env_lossy(),
    );

    set_display_resolutor(&DisplayResolutor);
    if let Err(err) = tracing.try_init() {
        logger.stop().await;
        return Err(format!("failed to initialize tracing subscriber: {err}"));
    }
    Ok(logger)
}

#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

#[cfg(all(feature = "mimalloc", not(feature = "dhat-heap")))]
#[global_allocator]
static ALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;

// Windows defaults to a 1 MB main thread stack, which overflows in debug
// builds due to deeply nested generated density functions.
fn main() {
    #[cfg(all(windows, debug_assertions))]
    {
        thread::Builder::new()
            .name("steel-main".to_owned())
            .stack_size(8 * 1024 * 1024)
            .spawn(steel_main)
            .expect("failed to spawn steel-main bootstrap thread")
            .join()
            .expect("steel-main thread panicked");
    }

    #[cfg(not(all(windows, debug_assertions)))]
    {
        steel_main();
    }
}

/// Why 2 runtimes?
///
/// The chunk runtime is very task heavy as it sometimes spawns thousands of tasks at once. It is also very await heavy in the part where it awaits its current layer.
///
/// If we only used one runtime this would lead to the tick task being blocked by the chunk tasks.
///
/// We have to create the runtimes at this level cause tokio panics if you drop a runtime in a context where blocking is not allowed.
#[expect(
    clippy::unwrap_used,
    reason = "runtime build failures are fatal and unrecoverable at startup"
)]
fn steel_main() {
    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();

    // Load config once at startup
    let steel_config = match config::load_or_create(Path::new("config/config.toml")) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("Failed to load configuration: {error}");
            return;
        }
    };

    let main_worker_threads = configured_worker_threads(steel_config.server.threads.main_runtime);
    let chunk_worker_threads = configured_worker_threads(steel_config.server.threads.chunk_runtime);

    let chunk_runtime = Arc::new(
        Builder::new_multi_thread()
            .worker_threads(chunk_worker_threads)
            .thread_name("chunk-worker")
            .enable_all()
            .build()
            .unwrap(),
    );

    let main_runtime = Builder::new_multi_thread()
        .worker_threads(main_worker_threads)
        .thread_name("main-worker")
        .enable_all()
        .build()
        .unwrap();

    main_runtime.block_on(main_async(chunk_runtime.clone(), steel_config));

    drop(main_runtime);
    drop(chunk_runtime);
}

fn configured_worker_threads(configured_threads: Option<usize>) -> usize {
    worker_threads_for_available(configured_threads, available_worker_threads())
}

fn available_worker_threads() -> usize {
    thread::available_parallelism().map_or(4, NonZero::get)
}

fn worker_threads_for_available(
    configured_threads: Option<usize>,
    available_threads: usize,
) -> usize {
    let available_threads = available_threads.max(1);
    if let Some(configured_threads) = configured_threads.filter(|&threads| threads > 0) {
        return configured_threads.min(available_threads);
    }

    ((available_threads / 2).max(2)).min(available_threads)
}

async fn main_async(chunk_runtime: Arc<Runtime>, steel_config: config::SteelConfig) {
    let cancel_token = CancellationToken::new();

    let logger = match init_tracing(cancel_token.clone(), steel_config.log.clone()).await {
        Ok(logger) => logger,
        Err(error) => {
            eprintln!("{error}");
            return;
        }
    };
    spawn_shutdown_signal_listener(cancel_token.clone());
    let panic_token = cancel_token.clone();
    panic::set_hook(Box::new(move |panic_info| {
        let message = panic_info.payload_as_str().unwrap_or("Unknown");
        let current_thread = thread::current();
        let thread_name = current_thread.name().unwrap_or("unnamed");
        let thread_id = current_thread.id();
        if let Some(location) = panic_info.location() {
            error!(
                "{}Thread '{thread_name}' ({}) has panicked at {}:{}:{}{}",
                SetForegroundColor(Color::Red),
                thread_id.as_u64(),
                location.file(),
                location.line(),
                location.column(),
                ResetColor
            );
        } else {
            error!(
                "{}Thread '{thread_name}' ({}) has panicked at an unknown location{}",
                SetForegroundColor(Color::Red),
                thread_id.as_u64(),
                ResetColor
            );
        }
        error!(
            "{}{}[FATAL ERROR]{}{} {message}{}",
            SetForegroundColor(Color::Red),
            Bold,
            Reset,
            SetForegroundColor(Color::Red),
            ResetColor
        );

        let backtrace = Backtrace::capture();
        match backtrace.status() {
            BacktraceStatus::Captured => {
                error!("Stack Backtrace:");
                let string = backtrace.to_string();
                let traces = string.split('\n');
                for trace in traces {
                    error!("{}", trace.trim_start());
                }
            }
            BacktraceStatus::Disabled => {
                error!(
                    "{}Backtrace is disabled. Run with RUST_BACKTRACE=1 to enable it.{}",
                    Dim, Reset
                );
            }
            BacktraceStatus::Unsupported => {
                error!(
                    "{}Backtrace capability is not supported on this platform.{}",
                    Dim, Reset
                );
            }
            _ => {}
        }

        panic_token.cancel();
    }));

    let run_result = AssertUnwindSafe(run_server(chunk_runtime, cancel_token, steel_config))
        .catch_unwind()
        .await;
    let panic_payload = match run_result {
        Ok(Ok(())) => None,
        Ok(Err(error)) => {
            log::error!("Server startup failed: {error}");
            None
        }
        Err(payload) => Some(payload),
    };

    logger.stop().await;

    if let Some(payload) = panic_payload {
        panic::resume_unwind(payload);
    }
}

fn spawn_shutdown_signal_listener(cancel_token: CancellationToken) {
    let shutdown_token = cancel_token.clone();
    tokio::spawn(async move {
        tokio::select! {
            signal = wait_for_shutdown_signal() => match signal {
                Ok(signal) => {
                    log::info!("Received {signal}; shutting down gracefully");
                    shutdown_token.cancel();
                }
                Err(error) => {
                    log::error!("Failed to listen for shutdown signals: {error}");
                }
            },
            () = cancel_token.cancelled() => {}
        }
    });
}

#[cfg(unix)]
async fn wait_for_shutdown_signal() -> io::Result<&'static str> {
    use tokio::signal::unix::{SignalKind, signal};

    let mut interrupt = signal(SignalKind::interrupt())?;
    let mut terminate = signal(SignalKind::terminate())?;

    tokio::select! {
        _ = interrupt.recv() => Ok("SIGINT"),
        _ = terminate.recv() => Ok("SIGTERM"),
    }
}

#[cfg(not(unix))]
async fn wait_for_shutdown_signal() -> io::Result<&'static str> {
    tokio::signal::ctrl_c().await?;
    Ok("Ctrl-C")
}

async fn run_server(
    chunk_runtime: Arc<Runtime>,
    cancel_token: CancellationToken,
    steel_config: config::SteelConfig,
) -> Result<(), String> {
    #[cfg(feature = "deadlock_detection")]
    {
        // only for #[cfg]
        use parking_lot::deadlock;
        use std::thread;
        use std::time::Duration;

        // Create a background thread which checks for deadlocks every 10s
        thread::spawn(move || {
            loop {
                thread::sleep(Duration::from_secs(10));
                let deadlocks = deadlock::check_deadlock();
                if deadlocks.is_empty() {
                    continue;
                }

                log::error!("{} deadlocks detected", deadlocks.len());
                for (i, threads) in deadlocks.iter().enumerate() {
                    log::error!("Deadlock #{i}");
                    for t in threads {
                        log::error!("Thread Id {:#?}", t.thread_id());
                        log::error!("{:#?}", t.backtrace());
                    }
                }
            }
        });
    }

    let mut steel = SteelServer::new(chunk_runtime.clone(), cancel_token.clone(), steel_config)
        .await
        .map_err(|e| e.to_string())?;

    let server = steel.server.clone();

    if !server.prepare_spawn_area().await {
        shutdown_worlds(&server).await;
        return Ok(());
    }

    SERVER.set(steel.server.clone()).ok();

    let task_tracker = TaskTracker::new();

    steel.start(task_tracker.clone()).await;

    log::info!("Waiting for pending tasks...");

    task_tracker.close();
    task_tracker.wait().await;

    shutdown_worlds(&server).await;

    log::info!("Server stopped");
    Ok(())
}

async fn shutdown_worlds(server: &Arc<Server>) {
    if let Err(error) = server.flush_known_players().await {
        log::error!("Failed to flush known player cache during shutdown: {error}");
    }

    for world in server.worlds.values() {
        world.chunk_map.stop_generation_refill_loop();
        world.chunk_map.task_tracker.close();
        world.chunk_map.task_tracker.wait().await;
    }

    let mut players_to_save = Vec::new();
    for world in server.worlds.values() {
        world.players.iter_players(|_, player| {
            let domain = player.get_world().domain().to_owned();
            let data = PersistentPlayerData::from_player(player);
            player.store_ender_pearls_with_player();
            players_to_save.push((player.clone(), domain, data));
            true
        });
    }

    // Save all dirty chunks before shutdown
    log::info!("Saving world data...");
    let command_data = server.save_command_data().await;
    match command_data.scoreboards {
        Ok(saved) => log::info!("Saved {saved} domain scoreboards"),
        Err(error) => log::error!("Failed to save domain scoreboards: {error}"),
    }
    match command_data.storage {
        Ok(saved) => log::info!("Saved {saved} domain command storages"),
        Err(error) => log::error!("Failed to save domain command storage: {error}"),
    }
    let mut total_saved = 0;
    for world in server.worlds.values() {
        world.cleanup(&mut total_saved).await;
    }
    log::info!("Saved {total_saved} chunks");

    // Save all player data before shutdown
    log::info!("Saving player data...");
    let mut saved = 0;
    for (player, domain, data) in players_to_save {
        let uuid = player.gameprofile.id;
        match server
            .player_data_storage
            .save_domain_data(&domain, uuid, &data)
            .await
        {
            Ok(()) => {
                saved += 1;
            }
            Err(e) => {
                log::error!("Failed to save player {uuid} domain data during shutdown: {e}");
            }
        }
        if let Err(e) = server
            .player_data_storage
            .save_global(
                uuid,
                &GlobalPlayerData {
                    last_active_domain: domain,
                },
            )
            .await
        {
            log::error!("Failed to save player {uuid} global data during shutdown: {e}");
        }
    }
    log::info!("Saved {saved} players");
}

#[cfg(test)]
mod tests {
    use super::worker_threads_for_available;

    #[test]
    fn configured_worker_threads_are_capped_to_available_threads() {
        assert_eq!(worker_threads_for_available(Some(16), 8), 8);
        assert_eq!(worker_threads_for_available(Some(4), 8), 4);
    }

    #[test]
    fn zero_worker_threads_uses_auto_default() {
        assert_eq!(worker_threads_for_available(Some(0), 8), 4);
        assert_eq!(worker_threads_for_available(None, 8), 4);
        assert_eq!(worker_threads_for_available(None, 1), 1);
    }
}
