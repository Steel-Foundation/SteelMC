//! Tick-phase performance recording behind `/perf`.
//!
//! Vanilla profiles JVM metrics and writes a zip. Steel instead samples the
//! per-tick and per-phase timings that the game loop already computes and
//! writes them as raw per-tick JSON samples, leaving statistics to external
//! tools. The report also carries the server, system and per-world context that
//! vanilla writes to `system.txt`, `server.properties.txt` and `gamerules.txt`.

use std::{
    collections::BTreeMap,
    env::consts,
    fs,
    io::{self, Write},
    num::NonZero,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

use chrono::{DateTime, Local, SecondsFormat};
use serde::Serialize;
use steel_registry::REGISTRY;
use steel_utils::{Identifier, locks::SyncRwLock, translations};
use text_components::{Modifier, TextComponent, format::Color};
use tokio::task::spawn_blocking;

use super::Server;
use crate::chunk::{
    chunk_map::{ChunkMapGameTickTimings, ChunkMapSchedulingTimings},
    gameplay_chunk_lookup_cache::GameplayChunkLookupCacheStats,
};
use crate::command::sender::CommandSender;
use crate::world::{World, WorldGameTickTimings};

/// Vanilla's profiling window.
const PERF_RECORDING_DURATION: Duration = Duration::from_secs(10);

/// Vanilla's profiling output directory.
const PROFILING_RESULTS_DIR: &str = "debug/profiling";

/// Chunk-map phase timings recorded for every world tick.
const CHUNK_MAP_PHASE_COUNT: usize = 14;

/// Chunk-map counters recorded for every world tick.
const CHUNK_MAP_COUNT_COUNT: usize = 17;

/// Tracks a `/perf` run and hands back a completed recording when it ends.
pub(crate) struct PerfRecorder {
    active: SyncRwLock<Option<PerfRecording>>,
    results_dir: SyncRwLock<PathBuf>,
}

impl PerfRecorder {
    pub(crate) fn new() -> Self {
        Self {
            active: SyncRwLock::new(None),
            results_dir: SyncRwLock::new(PathBuf::from(PROFILING_RESULTS_DIR)),
        }
    }

    /// Redirects report output so tests never write into the repository.
    #[cfg(test)]
    pub(crate) fn set_results_dir(&self, directory: PathBuf) {
        *self.results_dir.write() = directory;
    }

    /// Starts a recording, returning `false` when one is already active.
    pub(crate) fn start(&self, sender: CommandSender) -> bool {
        self.begin(Some(sender))
    }

    /// Starts a recording without command feedback; the result is only logged.
    pub(crate) fn begin(&self, sender: Option<CommandSender>) -> bool {
        let mut active = self.active.write();
        if active.is_some() {
            return false;
        }
        *active = Some(PerfRecording {
            started_at: Instant::now(),
            started_at_local: Local::now(),
            sender,
            samples: Vec::new(),
        });
        true
    }

    /// Stops an active recording early.
    pub(crate) fn stop(&self) -> Option<PerfRecording> {
        self.active.write().take()
    }

    /// Adds one tick sample, returning the finished recording once the window ends.
    pub(crate) fn record_tick<'a>(
        &self,
        total: Duration,
        worlds: impl IntoIterator<Item = (&'a Identifier, WorldGameTickTimings)>,
    ) -> Option<PerfRecording> {
        // Keep the idle path to one uncontended read lock.
        if self.active.read().is_none() {
            return None;
        }

        let mut active = self.active.write();
        let recording = active.as_mut()?;
        recording.samples.push(TickSample {
            total,
            worlds: worlds
                .into_iter()
                .map(|(key, timings)| (key.clone(), WorldTickSample::capture(timings)))
                .collect(),
        });

        if recording.started_at.elapsed() < PERF_RECORDING_DURATION {
            return None;
        }
        active.take()
    }
}

/// A `/perf` run, keeping one raw sample per tick.
pub(crate) struct PerfRecording {
    started_at: Instant,
    started_at_local: DateTime<Local>,
    sender: Option<CommandSender>,
    samples: Vec<TickSample>,
}

/// One tick's measured timings, before the report converts them to milliseconds.
struct TickSample {
    total: Duration,
    worlds: BTreeMap<Identifier, WorldTickSample>,
}

/// One world's measured timings within a single tick.
struct WorldTickSample {
    tick_time: Duration,
    entity_tick: Duration,
    chunk_map: [(&'static str, Duration); CHUNK_MAP_PHASE_COUNT],
    counts: [(&'static str, usize); CHUNK_MAP_COUNT_COUNT],
}

impl WorldTickSample {
    const fn capture(timings: WorldGameTickTimings) -> Self {
        let metrics = ChunkMapMetrics::extract(&timings.chunk_map);
        Self {
            tick_time: timings.elapsed,
            entity_tick: timings.entity_tick,
            chunk_map: metrics.phases,
            counts: metrics.counts,
        }
    }
}

impl PerfRecording {
    /// Sends the vanilla stop summary, then builds and writes the JSON report
    /// off the tick thread. Only the metadata snapshot needs the server.
    pub(crate) fn finish(self, server: &Server) {
        let Self {
            started_at,
            started_at_local,
            sender,
            samples,
        } = self;
        let duration = started_at.elapsed();
        let ticks = samples.len();
        let seconds = duration.as_secs_f64();

        if let Some(sender) = &sender {
            send_success(
                sender,
                &translations::COMMANDS_PERF_STOPPED
                    .message([
                        format!("{seconds:.2}"),
                        ticks.to_string(),
                        format!("{:.2}", PerfReport::ticks_per_second(ticks, seconds)),
                    ])
                    .component(),
            );
        }

        let metadata = PerfMetadata::capture(server);
        let results_dir = server.perf.results_dir.read().clone();
        tokio::spawn(async move {
            let result = spawn_blocking(move || {
                PerfReport::new(started_at_local, duration, samples, metadata).save(&results_dir)
            })
            .await
            .unwrap_or_else(|error| Err(io::Error::other(error)));
            match (result, sender) {
                (Ok(path), Some(sender)) => send_success(
                    &sender,
                    &translations::COMMANDS_PERF_REPORT_SAVED
                        .message([path.display().to_string()])
                        .component(),
                ),
                (Ok(path), None) => {
                    log::info!("Created performance report at {}", path.display());
                }
                (Err(error), sender) => {
                    log::error!("Failed to create performance report: {error}");
                    if let Some(sender) = sender {
                        sender.send_message(
                            &TextComponent::from(&translations::COMMANDS_PERF_REPORT_FAILED)
                                .color(Color::Red),
                        );
                    }
                }
            }
        });
    }
}

/// Mirrors [`crate::command::execution::CommandSource::send_success`], since the
/// sender outlives the command execution that started the recording.
fn send_success(sender: &CommandSender, message: &TextComponent) {
    if sender.accepts_command_feedback() {
        sender.send_message(message);
    }
}

/// Server, system and per-world context captured when a recording finishes.
#[derive(Debug, Default)]
struct PerfMetadata {
    system: PerfSystemReport,
    server: PerfServerReport,
    worlds: BTreeMap<Identifier, WorldMetadataReport>,
}

impl PerfMetadata {
    fn capture(server: &Server) -> Self {
        Self {
            system: PerfSystemReport {
                operating_system: consts::OS,
                architecture: consts::ARCH,
                available_parallelism: thread::available_parallelism().map_or(0, NonZero::get),
            },
            server: PerfServerReport {
                players: server.player_count(),
                max_players: server.config.max_players,
                view_distance: server.config.view_distance,
                simulation_distance: server.config.simulation_distance,
                max_chained_neighbor_updates: server.config.max_chained_neighbor_updates,
                online_mode: server.config.online_mode,
                encryption: server.config.encryption,
                player_info_forwarding: server.config.proxy.forwarding.mode_name(),
                proxy_online_mode: server.config.proxy.online_mode,
                enforce_secure_chat: server.config.enforce_secure_chat,
                allow_flight: server.config.allow_flight,
                compression_threshold: server
                    .config
                    .compression
                    .map(|compression| compression.threshold.get()),
                compression_level: server
                    .config
                    .compression
                    .map(|compression| compression.level),
                packet_workers: server.config.packet_workers,
                chunk_generation_threads: server.config.chunk_generation_threads,
                chunk_encoding_threads: server.config.chunk_encoding_threads,
                current_tick: server.current_tick(),
                pending_jobs: server.jobs.len(),
            },
            worlds: server
                .worlds
                .values()
                .map(|world| (world.key.clone(), WorldMetadataReport::capture(world)))
                .collect(),
        }
    }
}

#[derive(Debug, Default, Serialize)]
struct PerfSystemReport {
    operating_system: &'static str,
    architecture: &'static str,
    available_parallelism: usize,
}

#[derive(Debug, Default, Serialize)]
struct PerfServerReport {
    players: usize,
    max_players: u32,
    view_distance: u8,
    simulation_distance: u8,
    max_chained_neighbor_updates: i32,
    online_mode: bool,
    encryption: bool,
    player_info_forwarding: &'static str,
    proxy_online_mode: bool,
    enforce_secure_chat: bool,
    allow_flight: bool,
    compression_threshold: Option<u32>,
    compression_level: Option<i32>,
    packet_workers: Option<usize>,
    chunk_generation_threads: Option<usize>,
    chunk_encoding_threads: Option<usize>,
    current_tick: u64,
    pending_jobs: usize,
}

#[derive(Debug, Default, Serialize)]
struct WorldMetadataReport {
    seed: i64,
    difficulty: &'static str,
    difficulty_locked: bool,
    default_gamemode: &'static str,
    gamerules: BTreeMap<String, serde_json::Value>,
}

impl WorldMetadataReport {
    fn capture(world: &World) -> Self {
        let level_data = world.level_data.read();
        let data = level_data.data();
        let gamerules = REGISTRY
            .game_rules
            .iter()
            .map(|(_, rule)| {
                let name = if rule.key().namespace == Identifier::VANILLA_NAMESPACE {
                    rule.key().path.to_string()
                } else {
                    rule.key().to_string()
                };
                let value = rule.serialize_erased_value(
                    data.game_rules_values
                        .get_erased(rule, &REGISTRY.game_rules),
                );
                (name, value)
            })
            .collect();

        Self {
            seed: data.seed,
            difficulty: data.difficulty.name(),
            difficulty_locked: data.difficulty_locked,
            default_gamemode: world.default_gamemode().name(),
            gamerules,
        }
    }
}

#[derive(Debug, Serialize)]
struct PerfReport {
    /// Vanilla's report name: `yyyy-MM-dd_HH.mm.ss-<level>-<version>`, minus the
    /// level name since a Steel report covers every world.
    #[serde(skip)]
    file_stem: String,
    steel_version: &'static str,
    minecraft_version: &'static str,
    started_at: String,
    duration_seconds: f64,
    tick_count: usize,
    ticks_per_second: f64,
    system: PerfSystemReport,
    server: PerfServerReport,
    worlds: BTreeMap<Identifier, WorldMetadataReport>,
    /// One entry per recorded tick, in recording order.
    samples: Vec<TickSampleReport>,
}

impl PerfReport {
    #[expect(
        clippy::cast_precision_loss,
        reason = "ticks per second is a summary floating-point value"
    )]
    fn ticks_per_second(ticks: usize, seconds: f64) -> f64 {
        if seconds > 0.0 {
            ticks as f64 / seconds
        } else {
            0.0
        }
    }

    fn new(
        started_at: DateTime<Local>,
        duration: Duration,
        samples: Vec<TickSample>,
        metadata: PerfMetadata,
    ) -> Self {
        let tick_count = samples.len();
        let seconds = duration.as_secs_f64();
        let PerfMetadata {
            system,
            server,
            worlds,
        } = metadata;

        Self {
            file_stem: format!(
                "{}-{}",
                started_at.format("%Y-%m-%d_%H.%M.%S"),
                steel_utils::MC_VERSION
            ),
            steel_version: env!("CARGO_PKG_VERSION"),
            minecraft_version: steel_utils::MC_VERSION,
            started_at: started_at.to_rfc3339_opts(SecondsFormat::Millis, true),
            duration_seconds: seconds,
            tick_count,
            ticks_per_second: Self::ticks_per_second(tick_count, seconds),
            system,
            server,
            worlds,
            samples: samples.into_iter().map(TickSampleReport::from).collect(),
        }
    }

    fn save(self, directory: &Path) -> io::Result<PathBuf> {
        fs::create_dir_all(directory)?;
        let json = serde_json::to_vec_pretty(&self).map_err(io::Error::other)?;
        let stem = &self.file_stem;
        let mut suffix = 0;
        loop {
            let path = directory.join(match suffix {
                0 => format!("{stem}.json"),
                suffix => format!("{stem}-{suffix}.json"),
            });
            // `create_new` claims the name atomically, so overlapping report
            // writes cannot both pick the same file.
            let mut file = match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    suffix += 1;
                    continue;
                }
                Err(error) => return Err(error),
            };
            if let Err(error) = file.write_all(&json) {
                drop(file);
                if let Err(remove_error) = fs::remove_file(&path) {
                    log::warn!(
                        "Failed to remove incomplete performance report {}: {remove_error}",
                        path.display()
                    );
                }
                return Err(error);
            }
            return Ok(path);
        }
    }
}

/// One recorded tick: the full server tick time plus every world ticked in it.
#[derive(Debug, Serialize)]
struct TickSampleReport {
    tick_ms: f64,
    worlds: BTreeMap<Identifier, WorldTickSampleReport>,
}

impl From<TickSample> for TickSampleReport {
    fn from(sample: TickSample) -> Self {
        Self {
            tick_ms: millis(sample.total),
            worlds: sample
                .worlds
                .into_iter()
                .map(|(key, sample)| (key, WorldTickSampleReport::from(sample)))
                .collect(),
        }
    }
}

#[derive(Debug, Serialize)]
struct WorldTickSampleReport {
    tick_ms: f64,
    entity_tick_ms: f64,
    chunk_map: BTreeMap<&'static str, f64>,
    counts: BTreeMap<&'static str, usize>,
}

impl From<WorldTickSample> for WorldTickSampleReport {
    fn from(sample: WorldTickSample) -> Self {
        Self {
            tick_ms: millis(sample.tick_time),
            entity_tick_ms: millis(sample.entity_tick),
            chunk_map: sample
                .chunk_map
                .into_iter()
                .map(|(name, duration)| (name, millis(duration)))
                .collect(),
            counts: sample.counts.into_iter().collect(),
        }
    }
}

/// Converts a measured duration to the millisecond unit used throughout the report.
fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}

/// Every timing and count reported for one chunk-map tick sample.
struct ChunkMapMetrics {
    phases: [(&'static str, Duration); CHUNK_MAP_PHASE_COUNT],
    counts: [(&'static str, usize); CHUNK_MAP_COUNT_COUNT],
}

impl ChunkMapMetrics {
    /// Destructures both timing structs exhaustively, so adding a field is a
    /// compile error until it is classified as a phase or a count here.
    const fn extract(chunk_map: &ChunkMapGameTickTimings) -> Self {
        let ChunkMapGameTickTimings {
            scheduling,
            broadcast_changes,
            collect_tickable,
            tick_chunks,
            tick_block_entities,
            tickable_count,
            total_chunks,
            lookup_cache,
        } = chunk_map;

        let ChunkMapSchedulingTimings {
            ticket_updates,
            block_entity_unloads,
            readiness_demotions,
            lifecycle_commit,
            readiness_reconcile,
            post_process_generation,
            post_process_chunk_count,
            post_process_position_count,
            readiness_candidate_count,
            ticking_snapshot_rebuild,
            rebuilt_ticking_chunk_count,
            lookup_cache: readiness_lookup_cache,
            schedule_generation,
            scheduled_count,
            run_generation,
            process_unloads,
        } = scheduling;

        let GameplayChunkLookupCacheStats {
            holder_hits,
            missing_hits,
            scc_lookups,
            foreign_map_bypasses,
            evictions,
        } = lookup_cache;

        let GameplayChunkLookupCacheStats {
            holder_hits: readiness_holder_hits,
            missing_hits: readiness_missing_hits,
            scc_lookups: readiness_scc_lookups,
            foreign_map_bypasses: readiness_foreign_map_bypasses,
            evictions: readiness_evictions,
        } = readiness_lookup_cache;

        Self {
            phases: [
                ("ticket_updates", *ticket_updates),
                ("block_entity_unloads", *block_entity_unloads),
                ("readiness_demotions", *readiness_demotions),
                ("lifecycle_commit", *lifecycle_commit),
                ("readiness_reconcile", *readiness_reconcile),
                ("post_process_generation", *post_process_generation),
                ("ticking_snapshot_rebuild", *ticking_snapshot_rebuild),
                ("schedule_generation", *schedule_generation),
                ("run_generation", *run_generation),
                ("process_unloads", *process_unloads),
                ("broadcast_changes", *broadcast_changes),
                ("collect_tickable", *collect_tickable),
                ("tick_chunks", *tick_chunks),
                ("tick_block_entities", *tick_block_entities),
            ],
            counts: [
                ("tickable_chunks", *tickable_count),
                ("total_chunks", *total_chunks),
                ("post_process_chunks", *post_process_chunk_count),
                ("post_process_positions", *post_process_position_count),
                ("readiness_candidates", *readiness_candidate_count),
                ("rebuilt_ticking_chunks", *rebuilt_ticking_chunk_count),
                ("scheduled_chunks", *scheduled_count),
                ("lookup_holder_hits", *holder_hits),
                ("lookup_missing_hits", *missing_hits),
                ("lookup_scc_lookups", *scc_lookups),
                ("lookup_foreign_map_bypasses", *foreign_map_bypasses),
                ("lookup_evictions", *evictions),
                ("readiness_lookup_holder_hits", *readiness_holder_hits),
                ("readiness_lookup_missing_hits", *readiness_missing_hits),
                ("readiness_lookup_scc_lookups", *readiness_scc_lookups),
                (
                    "readiness_lookup_foreign_map_bypasses",
                    *readiness_foreign_map_bypasses,
                ),
                ("readiness_lookup_evictions", *readiness_evictions),
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{env, process};

    use super::*;

    fn timings(elapsed_ms: u64, entity_ms: u64, tick_chunks_ms: u64) -> WorldGameTickTimings {
        let chunk_map = ChunkMapGameTickTimings {
            tick_chunks: Duration::from_millis(tick_chunks_ms),
            tickable_count: 3,
            total_chunks: 10,
            ..Default::default()
        };
        WorldGameTickTimings {
            elapsed: Duration::from_millis(elapsed_ms),
            chunk_map,
            entity_tick: Duration::from_millis(entity_ms),
        }
    }

    #[test]
    fn recording_keeps_raw_tick_and_world_phase_samples() {
        let recorder = PerfRecorder::new();
        let overworld = Identifier::new_static("minecraft", "overworld");

        assert!(recorder.begin(None));
        assert!(!recorder.begin(None));
        assert!(
            recorder
                .record_tick(Duration::from_millis(40), [(&overworld, timings(20, 5, 8))])
                .is_none()
        );
        assert!(
            recorder
                .record_tick(
                    Duration::from_millis(60),
                    [(&overworld, timings(40, 15, 12))],
                )
                .is_none()
        );

        let completed = recorder.stop().expect("recording should be active");
        let report = PerfReport::new(
            completed.started_at_local,
            completed.started_at.elapsed(),
            completed.samples,
            PerfMetadata::default(),
        );

        assert_eq!(report.tick_count, 2);
        assert_eq!(report.samples.len(), 2);
        assert!((report.samples[0].tick_ms - 40.0).abs() < f64::EPSILON);
        assert!((report.samples[1].tick_ms - 60.0).abs() < f64::EPSILON);

        let world = report.samples[1]
            .worlds
            .get(&overworld)
            .expect("overworld should be sampled");
        assert!((world.tick_ms - 40.0).abs() < f64::EPSILON);
        assert!((world.entity_tick_ms - 15.0).abs() < f64::EPSILON);
        assert!((world.chunk_map["tick_chunks"] - 12.0).abs() < f64::EPSILON);
        assert_eq!(world.counts["tickable_chunks"], 3);
        assert_eq!(world.counts["total_chunks"], 10);
    }

    #[test]
    fn saving_reports_with_the_same_start_time_keeps_both() {
        let directory = env::temp_dir().join(format!("steel-perf-save-{}", process::id()));
        if let Err(error) = fs::remove_dir_all(&directory) {
            assert_eq!(error.kind(), io::ErrorKind::NotFound);
        }

        let started_at = Local::now();
        let save = || {
            PerfReport::new(
                started_at,
                Duration::from_secs(1),
                Vec::new(),
                PerfMetadata::default(),
            )
            .save(&directory)
        };
        let first = save().expect("the first report should be written");
        let second = save().expect("the second report should be written");

        assert_ne!(first, second);
        for path in [&first, &second] {
            let json = fs::read(path).expect("the report should be readable");
            serde_json::from_slice::<serde_json::Value>(&json).expect("the report should be valid");
        }

        fs::remove_dir_all(&directory).expect("the test directory should be removable");
    }

    #[test]
    fn recording_auto_stops_after_the_profiling_window() {
        let recorder = PerfRecorder::new();
        let overworld = Identifier::new_static("minecraft", "overworld");
        assert!(recorder.begin(None));
        if let Some(recording) = recorder.active.write().as_mut() {
            recording.started_at = Instant::now()
                .checked_sub(PERF_RECORDING_DURATION)
                .expect("the profiling window should fit before now");
        }

        let completed =
            recorder.record_tick(Duration::from_millis(20), [(&overworld, timings(20, 5, 8))]);

        assert!(completed.is_some());
        assert!(recorder.active.read().is_none());
    }
}
