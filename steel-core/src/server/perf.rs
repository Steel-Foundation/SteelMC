//! Tick-phase performance recording behind `/perf`.
//!
//! Vanilla profiles JVM metrics and writes a zip. Steel instead samples the
//! per-tick and per-phase timings that the game loop already computes and
//! writes one JSON report, so the command stays cheap and portable.

use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use chrono::{DateTime, SecondsFormat, Utc};
use serde::Serialize;
use steel_utils::{Identifier, locks::SyncRwLock, translations};
use text_components::TextComponent;
use tokio::task::spawn_blocking;

use crate::chunk::{
    chunk_map::{ChunkMapGameTickTimings, ChunkMapSchedulingTimings},
    gameplay_chunk_lookup_cache::GameplayChunkLookupCacheStats,
};
use crate::command::execution::CommandSource;
use crate::world::WorldGameTickTimings;

/// Vanilla's profiling window.
const PERF_RECORDING_DURATION: Duration = Duration::from_secs(10);

/// Vanilla's profiling output directory.
const PROFILING_RESULTS_DIR: &str = "debug/profiling";

/// Tracks a `/perf` run and hands back a completed recording when it ends.
pub(crate) struct PerfRecorder {
    active: SyncRwLock<Option<ActivePerfRecording>>,
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
    pub(crate) fn start(&self, source: CommandSource) -> bool {
        self.begin(Some(source))
    }

    pub(crate) fn begin(&self, source: Option<CommandSource>) -> bool {
        let mut active = self.active.write();
        if active.is_some() {
            return false;
        }
        let results_dir = self.results_dir.read().clone();
        *active = Some(ActivePerfRecording::new(source, results_dir));
        true
    }

    /// Stops an active recording early.
    pub(crate) fn stop(&self) -> Option<CompletedPerfRecording> {
        self.active.write().take().map(CompletedPerfRecording::from)
    }

    /// Adds one tick sample, returning the finished recording once the window ends.
    pub(crate) fn record_tick<'a>(
        &self,
        total: Duration,
        worlds: impl IntoIterator<Item = (&'a Identifier, WorldGameTickTimings)>,
    ) -> Option<CompletedPerfRecording> {
        // Keep the idle path to one uncontended read lock.
        if self.active.read().is_none() {
            return None;
        }

        let mut active = self.active.write();
        let recording = active.as_mut()?;

        recording.samples.push(PerfSample {
            total,
            worlds: worlds
                .into_iter()
                .map(|(key, timings)| WorldPerfSample {
                    key: key.clone(),
                    timings,
                })
                .collect(),
        });

        if recording.started_at.elapsed() < PERF_RECORDING_DURATION {
            return None;
        }

        active.take().map(CompletedPerfRecording::from)
    }
}

struct ActivePerfRecording {
    started_at: Instant,
    started_at_utc: DateTime<Utc>,
    source: Option<CommandSource>,
    results_dir: PathBuf,
    samples: Vec<PerfSample>,
}

impl ActivePerfRecording {
    fn new(source: Option<CommandSource>, results_dir: PathBuf) -> Self {
        Self {
            started_at: Instant::now(),
            started_at_utc: Utc::now(),
            source,
            results_dir,
            samples: Vec::new(),
        }
    }
}

struct PerfSample {
    total: Duration,
    worlds: Vec<WorldPerfSample>,
}

struct WorldPerfSample {
    key: Identifier,
    timings: WorldGameTickTimings,
}

/// A recording that has ended and is ready to be summarized and persisted.
pub(crate) struct CompletedPerfRecording {
    started_at: DateTime<Utc>,
    duration: Duration,
    results_dir: PathBuf,
    samples: Vec<PerfSample>,
    source: Option<CommandSource>,
}

impl From<ActivePerfRecording> for CompletedPerfRecording {
    fn from(recording: ActivePerfRecording) -> Self {
        Self {
            started_at: recording.started_at_utc,
            duration: recording.started_at.elapsed(),
            results_dir: recording.results_dir,
            samples: recording.samples,
            source: recording.source,
        }
    }
}

impl CompletedPerfRecording {
    /// Sends the vanilla stop summary, then writes and reports the JSON report off-thread.
    pub(crate) fn finish(self) {
        let Self {
            started_at,
            duration,
            results_dir,
            samples,
            source,
        } = self;

        let ticks = samples.len();
        let seconds = duration.as_secs_f64();
        let ticks_per_second = if seconds > 0.0 {
            ticks as f64 / seconds
        } else {
            0.0
        };

        if let Some(source) = &source {
            let message = translations::COMMANDS_PERF_STOPPED
                .message([
                    format!("{seconds:.2}"),
                    ticks.to_string(),
                    format!("{ticks_per_second:.2}"),
                ])
                .component();
            source.send_success(&message, false);
        }

        let report = PerfReport::from_recording(started_at, duration, samples);
        tokio::spawn(async move {
            let result = spawn_blocking(move || report.save(&results_dir)).await;
            match result {
                Ok(Ok(path)) => {
                    if let Some(source) = source {
                        let message = translations::COMMANDS_PERF_REPORT_SAVED
                            .message([path.display().to_string()])
                            .component();
                        source.send_success(&message, false);
                    } else {
                        log::info!("Created performance report at {}", path.display());
                    }
                }
                Ok(Err(error)) => {
                    log::error!("Failed to create performance report: {error}");
                    if let Some(source) = source {
                        source.send_failure(TextComponent::from(
                            &translations::COMMANDS_PERF_REPORT_FAILED,
                        ));
                    }
                }
                Err(error) => {
                    log::error!("Performance report task failed: {error}");
                    if let Some(source) = source {
                        source.send_failure(TextComponent::from(
                            &translations::COMMANDS_PERF_REPORT_FAILED,
                        ));
                    }
                }
            }
        });
    }
}

#[derive(Debug, Serialize)]
struct PerfReport {
    steel_version: &'static str,
    minecraft_version: &'static str,
    started_at: String,
    duration_seconds: f64,
    ticks: usize,
    ticks_per_second: f64,
    tick_time: TimingStats,
    worlds: BTreeMap<String, WorldPerfReport>,
}

impl PerfReport {
    #[expect(
        clippy::cast_precision_loss,
        reason = "report fields are intentionally summary floating-point values"
    )]
    fn from_recording(
        started_at: DateTime<Utc>,
        duration: Duration,
        samples: Vec<PerfSample>,
    ) -> Self {
        let ticks = samples.len();
        let seconds = duration.as_secs_f64();
        let mut tick_times = Vec::with_capacity(ticks);
        let mut worlds = BTreeMap::<String, WorldTimingAccumulator>::new();

        for sample in samples {
            tick_times.push(sample.total);
            for world in sample.worlds {
                worlds
                    .entry(world.key.to_string())
                    .or_default()
                    .push(world.timings);
            }
        }

        Self {
            steel_version: env!("CARGO_PKG_VERSION"),
            minecraft_version: steel_utils::MC_VERSION,
            started_at: started_at.to_rfc3339_opts(SecondsFormat::Millis, true),
            duration_seconds: seconds,
            ticks,
            ticks_per_second: if seconds > 0.0 {
                ticks as f64 / seconds
            } else {
                0.0
            },
            tick_time: TimingStats::from(tick_times),
            worlds: worlds
                .into_iter()
                .map(|(key, accumulator)| (key, accumulator.finish()))
                .collect(),
        }
    }

    fn save(self, directory: &Path) -> io::Result<PathBuf> {
        fs::create_dir_all(directory)?;
        let path = self.available_path(directory);
        let json = serde_json::to_vec_pretty(&self).map_err(io::Error::other)?;
        fs::write(&path, json)?;
        Ok(path)
    }

    fn available_path(&self, directory: &Path) -> PathBuf {
        let stem = self.report_stem();
        let mut path = directory.join(format!("{stem}.json"));
        let mut suffix = 1;
        while path.exists() {
            path = directory.join(format!("{stem}-{suffix}.json"));
            suffix += 1;
        }
        path
    }

    fn report_stem(&self) -> String {
        self.started_at
            .chars()
            .map(|value| {
                if value.is_ascii_alphanumeric() || matches!(value, '-' | '_' | '.') {
                    value
                } else {
                    '_'
                }
            })
            .collect()
    }
}

#[derive(Debug, Default, Serialize)]
struct WorldPerfReport {
    tick_time: TimingStats,
    entity_tick: TimingStats,
    chunk_map: BTreeMap<&'static str, TimingStats>,
    counts: BTreeMap<&'static str, CountStats>,
}

#[derive(Debug, Default, Serialize)]
struct TimingStats {
    mean_ms: f64,
    p50_ms: f64,
    p95_ms: f64,
    p99_ms: f64,
    max_ms: f64,
}

impl From<Vec<Duration>> for TimingStats {
    #[expect(
        clippy::cast_precision_loss,
        reason = "duration distributions are summarized as floating-point milliseconds"
    )]
    fn from(mut samples: Vec<Duration>) -> Self {
        if samples.is_empty() {
            return Self::default();
        }

        samples.sort_unstable();
        let len = samples.len();
        let total_nanos: u128 = samples.iter().map(Duration::as_nanos).sum();

        Self {
            mean_ms: Self::to_millis(total_nanos as f64 / len as f64),
            p50_ms: Self::to_millis(Self::percentile_nanos(&samples, 50)),
            p95_ms: Self::to_millis(Self::percentile_nanos(&samples, 95)),
            p99_ms: Self::to_millis(Self::percentile_nanos(&samples, 99)),
            max_ms: Self::to_millis(samples[len - 1].as_nanos() as f64),
        }
    }
}

impl TimingStats {
    #[expect(
        clippy::cast_precision_loss,
        reason = "percentile values are converted to floating-point milliseconds"
    )]
    fn percentile_nanos(samples: &[Duration], percent: usize) -> f64 {
        let index = (samples.len() * percent / 100).min(samples.len() - 1);
        samples[index].as_nanos() as f64
    }

    fn to_millis(nanos: f64) -> f64 {
        nanos / 1_000_000.0
    }
}

#[derive(Debug, Default, Serialize)]
struct CountStats {
    mean: f64,
    min: usize,
    max: usize,
}

impl From<Vec<usize>> for CountStats {
    #[expect(
        clippy::cast_precision_loss,
        reason = "count distributions are summarized as floating-point means"
    )]
    fn from(samples: Vec<usize>) -> Self {
        if samples.is_empty() {
            return Self::default();
        }

        let total: u64 = samples.iter().map(|&value| value as u64).sum();
        Self {
            mean: total as f64 / samples.len() as f64,
            min: samples.iter().copied().min().unwrap_or_default(),
            max: samples.iter().copied().max().unwrap_or_default(),
        }
    }
}

#[derive(Default)]
struct WorldTimingAccumulator {
    tick_time: Vec<Duration>,
    entity_tick: Vec<Duration>,
    chunk_map: BTreeMap<&'static str, Vec<Duration>>,
    counts: BTreeMap<&'static str, Vec<usize>>,
}

impl WorldTimingAccumulator {
    fn push(&mut self, timings: WorldGameTickTimings) {
        self.tick_time.push(timings.elapsed);
        self.entity_tick.push(timings.entity_tick);

        let metrics = ChunkMapMetrics::extract(&timings.chunk_map);
        for (name, duration) in metrics.phases {
            self.chunk_map.entry(name).or_default().push(duration);
        }
        for (name, count) in metrics.counts {
            self.counts.entry(name).or_default().push(count);
        }
    }

    fn finish(self) -> WorldPerfReport {
        WorldPerfReport {
            tick_time: TimingStats::from(self.tick_time),
            entity_tick: TimingStats::from(self.entity_tick),
            chunk_map: self
                .chunk_map
                .into_iter()
                .map(|(name, samples)| (name, TimingStats::from(samples)))
                .collect(),
            counts: self
                .counts
                .into_iter()
                .map(|(name, samples)| (name, CountStats::from(samples)))
                .collect(),
        }
    }
}

/// Every timing and count reported for one chunk-map tick sample.
struct ChunkMapMetrics {
    phases: [(&'static str, Duration); 14],
    counts: [(&'static str, usize); 17],
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
    fn recording_aggregates_tick_and_world_phase_timings() {
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
        let report =
            PerfReport::from_recording(completed.started_at, completed.duration, completed.samples);

        assert_eq!(report.ticks, 2);
        assert!((report.tick_time.mean_ms - 50.0).abs() < f64::EPSILON);

        let world = report
            .worlds
            .get("minecraft:overworld")
            .expect("overworld should be reported");
        assert!((world.tick_time.mean_ms - 30.0).abs() < f64::EPSILON);
        assert!((world.entity_tick.mean_ms - 10.0).abs() < f64::EPSILON);
        assert!((world.chunk_map["tick_chunks"].mean_ms - 10.0).abs() < f64::EPSILON);
        assert!((world.counts["tickable_chunks"].mean - 3.0).abs() < f64::EPSILON);
        assert!((world.counts["total_chunks"].mean - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn timing_stats_use_millisecond_percentiles_and_max() {
        let samples: Vec<Duration> = (1..=100).map(Duration::from_millis).collect();
        let stats = TimingStats::from(samples);

        assert!((stats.mean_ms - 50.5).abs() < f64::EPSILON);
        assert!((stats.p50_ms - 51.0).abs() < f64::EPSILON);
        assert!((stats.p95_ms - 96.0).abs() < f64::EPSILON);
        assert!((stats.p99_ms - 100.0).abs() < f64::EPSILON);
        assert!((stats.max_ms - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn recording_auto_stops_after_the_profiling_window() {
        let recorder = PerfRecorder::new();
        let overworld = Identifier::new_static("minecraft", "overworld");
        let mut recording = ActivePerfRecording::new(None, PathBuf::from(PROFILING_RESULTS_DIR));
        recording.started_at = Instant::now()
            .checked_sub(PERF_RECORDING_DURATION)
            .expect("the profiling window should fit before now");
        *recorder.active.write() = Some(recording);

        let completed =
            recorder.record_tick(Duration::from_millis(20), [(&overworld, timings(20, 5, 8))]);

        assert!(completed.is_some());
        assert!(recorder.active.read().is_none());
    }
}
