//! Main entry point for the Steel Minecraft server.

use std::sync::Arc;

use steel::logger::CommandLogger;
use steel::spawn_progress::generate_spawn_chunks;
use steel::{SERVER, SteelServer, logger::LoggerLayer};
use steel_utils::text::DisplayResolutor;
use text_components::fmt::set_display_resolutor;
use tokio::runtime::{Builder, Runtime};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
#[cfg(feature = "jaeger")]
use tracing::Subscriber;
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

async fn init_tracing(cancel_token: CancellationToken) -> Arc<CommandLogger> {
    let layer = LoggerLayer::new("./.tmp", cancel_token)
        .await
        .expect("Couldn't initialize the logger");
    let logger = layer.0.clone();

    let tracing = tracing_subscriber::registry().with(layer);

    #[cfg(feature = "jaeger")]
    let tracing = tracing.with(init_jaeger());

    let tracing = tracing.with(
        EnvFilter::builder()
            .with_default_directive(tracing::Level::INFO.into())
            .from_env_lossy(),
    );

    set_display_resolutor(&DisplayResolutor);
    tracing.init();
    logger
}

#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

#[cfg(all(feature = "mimalloc", not(feature = "dhat-heap")))]
#[global_allocator]
static ALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// Main entry point for the Steel Minecraft server.
///
///
/// Why 2 runtimes?
///
/// The chunk runtime is very task heavy as it sometimes spawns thousands of tasks at once. It is also very await heavy in the part where it awaits its current layer.
///
/// If we only used one runtime this would lead to the tick task being blocked by the chunk tasks.
///
/// We have to create the runtimes at this level cause tokio panics if you drop a runtime in a context where blocking is not allowed.
#[allow(clippy::unwrap_used)]
fn main() {
    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();

    let chunk_runtime = Arc::new(Builder::new_multi_thread().enable_all().build().unwrap());

    let main_runtime = Builder::new_multi_thread().enable_all().build().unwrap();

    main_runtime.block_on(main_async(chunk_runtime.clone()));

    drop(main_runtime);
    drop(chunk_runtime);
}

async fn main_async(chunk_runtime: Arc<Runtime>) {
    let cancel_token = CancellationToken::new();
    let logger = init_tracing(cancel_token.clone()).await;

    run_server(chunk_runtime, cancel_token, &logger).await;

    logger.stop().await;
}

async fn run_server(
    chunk_runtime: Arc<Runtime>,
    cancel_token: CancellationToken,
    logger: &Arc<CommandLogger>,
) {
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

    let mut steel = SteelServer::new(chunk_runtime.clone(), cancel_token.clone()).await;

    generate_spawn_chunks(&steel.server, logger).await;

    SERVER.set(steel.server.clone()).ok();
    let server = steel.server.clone();

    let task_tracker = TaskTracker::new();

    steel.start(task_tracker.clone()).await;

    log::info!("Waiting for pending tasks...");

    task_tracker.close();
    task_tracker.wait().await;

    for world in &server.worlds {
        world.chunk_map.task_tracker.close();
        world.chunk_map.task_tracker.wait().await;
    }

    // Save all dirty chunks before shutdown
    log::info!("Saving world data...");
    let mut total_saved = 0;
    for world in &server.worlds {
        world.cleanup(&mut total_saved).await;
    }
    log::info!("Saved {total_saved} chunks");

    // Save all player data before shutdown
    log::info!("Saving player data...");
    let mut players_to_save = Vec::new();
    for world in &server.worlds {
        world.players.iter_players(|_, player| {
            players_to_save.push(player.clone());
            true
        });
    }
    match server.player_data_storage.save_all(&players_to_save).await {
        Ok(count) => log::info!("Saved {count} players"),
        Err(e) => log::error!("Failed to save player data: {e}"),
    }

    log::info!("Server stopped");
}
