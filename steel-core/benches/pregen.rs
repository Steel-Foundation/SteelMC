//! End-to-end pregeneration throughput, without a server.
//!
//! The other benchmarks in this crate drive `GENERATION_PYRAMID` steps against
//! hand-built holders, which measures generation compute. This one runs the
//! production pregeneration driver against a bare `World`, so it covers the
//! whole pipeline: tickets, scheduling epochs, the generation pool, unload and
//! region saves.
//!
//! ```text
//! cargo bench -p steel-core --bench pregen --features benchmark-support -- --size 301 --reps 3
//! ```
use std::env;
use std::fmt::Display;
use std::fs;
use std::num::NonZero;
use std::path::PathBuf;
use std::process;
use std::str::FromStr;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use steel_core::bootstrap::init_globals_once;
use steel_core::config::WorldStorageConfig;
use steel_core::level_data::WorldGenerationSettings;
use steel_core::server::pregen::pregen_area_for_benchmark;
use steel_core::world::{World, WorldConfig};
use steel_core::worldgen::WorldGeneratorRegistry;
use steel_registry::vanilla_dimension_types;
use steel_utils::threading::worker_threads_for_available;
use steel_utils::types::{Difficulty, GameType};
use steel_utils::{ChunkPos, Identifier};
use tokio::runtime::{Builder, Runtime};
use tokio_util::sync::CancellationToken;
use toml::Value;
use toml::map::Map;

const DEFAULT_SEED: i64 = -9_091_483_014_810_473_238;
const DEFAULT_SIZE: i32 = 301;
const DEFAULT_REPS: usize = 3;

#[global_allocator]
static ALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;

struct Options {
    size: i32,
    reps: usize,
    seed: i64,
    generation_threads: usize,
    chunk_workers: usize,
    main_workers: usize,
    encoding_threads: usize,
    window_size: Option<i32>,
    ram_only: bool,
    keep_storage: bool,
}

impl Options {
    fn parse() -> Result<Self, String> {
        let available = thread::available_parallelism().map_or(4, NonZero::get);
        let mut options = Self {
            size: DEFAULT_SIZE,
            reps: DEFAULT_REPS,
            seed: DEFAULT_SEED,
            generation_threads: available,
            chunk_workers: worker_threads_for_available(None, available),
            main_workers: worker_threads_for_available(None, available),
            encoding_threads: available,
            window_size: None,
            ram_only: false,
            keep_storage: false,
        };

        // `cargo bench` passes libtest's own flags through.
        let mut args = env::args().skip(1).peekable();
        while let Some(arg) = args.next() {
            let mut value = || args.next().ok_or_else(|| format!("{arg} requires a value"));
            match arg.as_str() {
                "--size" => options.size = parse(&value()?, "--size")?,
                "--reps" => options.reps = parse(&value()?, "--reps")?,
                "--seed" => options.seed = parse(&value()?, "--seed")?,
                "--gen-threads" => options.generation_threads = parse(&value()?, "--gen-threads")?,
                "--chunk-workers" => options.chunk_workers = parse(&value()?, "--chunk-workers")?,
                "--main-workers" => options.main_workers = parse(&value()?, "--main-workers")?,
                "--encode-threads" => {
                    options.encoding_threads = parse(&value()?, "--encode-threads")?;
                }
                "--window" => options.window_size = Some(parse(&value()?, "--window")?),
                "--ram-only" => options.ram_only = true,
                "--keep-storage" => options.keep_storage = true,
                "--bench" | "--test" => {}
                "--help" | "-h" => {
                    print_usage();
                    process::exit(0);
                }
                other => return Err(format!("unknown argument {other} (try --help)")),
            }
        }

        if options.reps == 0 {
            return Err("--reps must be at least 1".to_owned());
        }
        Ok(options)
    }
}

fn parse<T: FromStr>(value: &str, flag: &str) -> Result<T, String>
where
    T::Err: Display,
{
    value
        .parse()
        .map_err(|error| format!("{flag} takes a number: {error}"))
}

fn print_usage() {
    println!(
        "\
Headless pregeneration benchmark.

  --size N           chunk side length, odd (default {DEFAULT_SIZE})
  --reps N           runs to perform (default {DEFAULT_REPS})
  --seed N           world seed (default {DEFAULT_SEED})
  --gen-threads N    generation pool threads (default: the server's)
  --chunk-workers N  chunk runtime workers (default: the server's)
  --main-workers N   main runtime workers (default: the server's)
  --encode-threads N encoding pool threads (default: the server's)
  --window N         pregen window side length (default: the server's)
  --ram-only         skip region-file persistence entirely
  --keep-storage     do not delete the per-run storage directory
"
    );
}

/// Current and peak resident set size in MiB.
///
/// Peak is process-lifetime and does not reset between reps, so use `--reps 1`
/// when comparing it. Linux only; elsewhere the rate is printed on its own.
#[cfg(target_os = "linux")]
fn rss_mib() -> Option<(u64, u64)> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    let field = |name: &str| {
        let line = status.lines().find(|line| line.starts_with(name))?;
        let kib: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
        Some(kib / 1024)
    };
    Some((field("VmRSS:")?, field("VmHWM:")?))
}

#[cfg(not(target_os = "linux"))]
fn rss_mib() -> Option<(u64, u64)> {
    None
}

/// Unique per run and removed afterwards. Under `target/` because a 601x601 run
/// writes gigabytes of region files.
fn storage_root(rep: usize) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../target/bench-pregen")
        .join(format!("{}-{rep}-{unique}", process::id()))
}

struct Harness {
    world: Arc<World>,
    main_runtime: Runtime,
    /// Held so the world's runtime outlives it; dropped last.
    _chunk_runtime: Arc<Runtime>,
    storage: Option<PathBuf>,
}

fn build_harness(options: &Options, rep: usize) -> Result<Harness, String> {
    let chunk_runtime = Arc::new(
        Builder::new_multi_thread()
            .worker_threads(options.chunk_workers)
            .thread_name("chunk-worker")
            .enable_all()
            .build()
            .map_err(|error| format!("chunk runtime should start: {error}"))?,
    );
    let main_runtime = Builder::new_multi_thread()
        .worker_threads(options.main_workers)
        .thread_name("main-worker")
        .enable_all()
        .build()
        .map_err(|error| format!("main runtime should start: {error}"))?;

    let generation_pool = Arc::new(
        rayon::ThreadPoolBuilder::new()
            .num_threads(options.generation_threads)
            .thread_name(|index| format!("rayon-gen-{index}"))
            .build()
            .map_err(|error| format!("generation pool should start: {error}"))?,
    );
    let encoding_pool = Arc::new(
        rayon::ThreadPoolBuilder::new()
            .num_threads(options.encoding_threads)
            .thread_name(|index| format!("rayon-chunk-enc-{index}"))
            .build()
            .map_err(|error| format!("encoding pool should start: {error}"))?,
    );

    let generator_key = Identifier::vanilla_static("overworld");
    let generator_registry = WorldGeneratorRegistry::new_with_builtins()
        .map_err(|error| format!("built-in generators should register: {error}"))?;
    let generator_config = generator_registry
        .validate_config(&generator_key, &Value::Table(Map::new()))
        .map_err(|error| format!("overworld generator config should validate: {error}"))?;
    let generator_output = generator_registry
        .create(
            None,
            &generator_config,
            options.seed,
            generation_pool.clone(),
        )
        .map_err(|error| format!("overworld generator should build: {error}"))?;

    let (storage, storage_config) = if options.ram_only {
        (None, WorldStorageConfig::RamOnly)
    } else {
        let root = storage_root(rep);
        fs::create_dir_all(&root)
            .map_err(|error| format!("bench storage directory should be creatable: {error}"))?;
        let path = root.to_string_lossy().into_owned();
        (Some(root), WorldStorageConfig::Disk { path })
    };

    let generation_settings = WorldGenerationSettings::from_generator_config(
        generator_key,
        &generator_output.config,
        generator_output.dimension_type.key.clone(),
        generator_output.dimension_type.min_y,
        generator_output.dimension_type.height,
    );
    let is_flat = generator_output.is_flat;
    let sea_level = generator_output.sea_level;
    let dimension_type = generator_output.dimension_type;

    let world = main_runtime
        .block_on(World::new_with_config_and_encoding_pool(
            Arc::clone(&chunk_runtime),
            vanilla_dimension_types::OVERWORLD.key.clone(),
            dimension_type,
            options.seed,
            WorldConfig {
                storage: storage_config,
                level_data_path: None,
                generator: Arc::new(generator_output.generator),
                generation_settings,
                view_distance: 10,
                simulation_distance: 10,
                max_chained_neighbor_updates: 1_000_000,
                compression: None,
                is_flat,
                sea_level,
                default_gamemode: GameType::Survival,
                difficulty: Difficulty::Normal,
            },
            generation_pool,
            encoding_pool,
        ))
        .map_err(|error| format!("bench world should initialize: {error}"))?;

    Ok(Harness {
        world,
        main_runtime,
        _chunk_runtime: chunk_runtime,
        storage,
    })
}

impl Harness {
    fn run(&self, options: &Options) -> Result<Duration, String> {
        let cancel_token = CancellationToken::new();
        self.main_runtime
            .block_on(pregen_area_for_benchmark(
                &self.world,
                ChunkPos::new(0, 0),
                options.size,
                options.window_size,
                &cancel_token,
            ))?
            .ok_or_else(|| "pregeneration was cancelled".to_owned())
    }

    /// Quiesces the world the way server shutdown does, so the next rep does not
    /// start while this one is still generating and saving.
    fn shutdown(self, keep_storage: bool) {
        let Self {
            world,
            main_runtime,
            _chunk_runtime,
            storage,
        } = self;
        main_runtime.block_on(async {
            world.chunk_map.stop_generation_refill_loop();
            world.chunk_map.task_tracker.close();
            world.chunk_map.task_tracker.wait().await;
        });
        drop(world);
        match storage {
            Some(storage) if keep_storage => {
                println!("  storage kept at {}", storage.display());
            }
            Some(storage) => {
                let _ = fs::remove_dir_all(storage);
            }
            None => {}
        }
    }
}

fn main() {
    let options = match Options::parse() {
        Ok(options) => options,
        Err(error) => {
            eprintln!("error: {error}");
            process::exit(2);
        }
    };

    init_globals_once();

    let total_chunks = f64::from(options.size) * f64::from(options.size);
    println!(
        "pregen {size}x{size} ({total} chunks), seed {seed}, {gen} generation threads, \
    {chunk} chunk workers, {main} main workers, {store}",
        size = options.size,
        total = total_chunks as u64,
        seed = options.seed,
        gen = options.generation_threads,
        chunk = options.chunk_workers,
        main = options.main_workers,
        store = if options.ram_only { "ram-only" } else { "disk" },
    );

    let mut rates = Vec::with_capacity(options.reps);
    for rep in 0..options.reps {
        let harness = match build_harness(&options, rep) {
            Ok(harness) => harness,
            Err(error) => {
                eprintln!("error: {error}");
                process::exit(1);
            }
        };
        let elapsed = match harness.run(&options) {
            Ok(elapsed) => elapsed,
            Err(error) => {
                eprintln!("error: {error}");
                process::exit(1);
            }
        };
        harness.shutdown(options.keep_storage);

        let rate = total_chunks / elapsed.as_secs_f64();
        rates.push(rate);
        let rss = match rss_mib() {
            Some((now, peak)) => format!("  RSS {now} MiB (peak {peak})"),
            None => String::new(),
        };
        println!(
            "  rep {}: {:.2}s  {rate:.1} chunks/s{rss}",
            rep + 1,
            elapsed.as_secs_f64(),
        );
    }

    let mean = rates.iter().sum::<f64>() / rates.len() as f64;
    let min = rates.iter().copied().fold(f64::INFINITY, f64::min);
    let max = rates.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    println!(
        "mean {mean:.1} chunks/s  (min {min:.1}, max {max:.1}, spread {:.2}%)",
        if mean > 0.0 {
            (max - min) / mean * 100.0
        } else {
            0.0
        },
    );
}
