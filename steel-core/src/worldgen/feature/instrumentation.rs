use std::{
    cell::RefCell,
    env,
    sync::{
        LazyLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use rustc_hash::FxHashSet;

static ORE_PROFILE_ENABLED: LazyLock<bool> =
    LazyLock::new(|| env::var_os("STEEL_ORE_PROFILE").is_some());
static ORE_PROFILE_LOG_EVERY: LazyLock<u64> = LazyLock::new(|| {
    env::var("STEEL_ORE_PROFILE_LOG_EVERY")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(10_000)
});

static ORE_TOTALS: OreProfileTotals = OreProfileTotals::new();

type SectionKey = (i32, i32, usize);

pub(crate) fn ore_profile_enabled() -> bool {
    *ORE_PROFILE_ENABLED
}

pub(crate) struct OreFeatureProfile {
    stats: Option<RefCell<OreFeatureStats>>,
}

impl OreFeatureProfile {
    pub(crate) fn new(config_size: i32) -> Self {
        Self {
            stats: ore_profile_enabled().then(|| RefCell::new(OreFeatureStats::new(config_size))),
        }
    }

    pub(crate) const fn stats(&self) -> Option<&RefCell<OreFeatureStats>> {
        self.stats.as_ref()
    }

    pub(crate) fn finish(self, placed: u64) {
        if let Some(stats) = self.stats {
            ORE_TOTALS.publish(stats.into_inner(), placed);
        }
    }
}

pub(crate) struct OreFeatureStats {
    started_at: Instant,
    config_size: i32,
    candidate_positions: u64,
    unique_positions: u64,
    write_allowed_positions: u64,
    target_reads: u64,
    neighbor_reads: u64,
    section_read_attempts: u64,
    section_write_attempts: u64,
    section_read_contentions: u64,
    section_write_contentions: u64,
    chunk_cache_misses: u64,
    chunk_status_upgrades: u64,
    writes: u64,
    candidate_time: Duration,
    batch_apply_time: Duration,
    read_time: Duration,
    write_time: Duration,
    read_contention_wait_time: Duration,
    write_contention_wait_time: Duration,
    read_sections: FxHashSet<SectionKey>,
    write_sections: FxHashSet<SectionKey>,
}

impl OreFeatureStats {
    fn new(config_size: i32) -> Self {
        Self {
            started_at: Instant::now(),
            config_size,
            candidate_positions: 0,
            unique_positions: 0,
            write_allowed_positions: 0,
            target_reads: 0,
            neighbor_reads: 0,
            section_read_attempts: 0,
            section_write_attempts: 0,
            section_read_contentions: 0,
            section_write_contentions: 0,
            chunk_cache_misses: 0,
            chunk_status_upgrades: 0,
            writes: 0,
            candidate_time: Duration::ZERO,
            batch_apply_time: Duration::ZERO,
            read_time: Duration::ZERO,
            write_time: Duration::ZERO,
            read_contention_wait_time: Duration::ZERO,
            write_contention_wait_time: Duration::ZERO,
            read_sections: FxHashSet::default(),
            write_sections: FxHashSet::default(),
        }
    }

    pub(crate) const fn record_candidate_position(&mut self) {
        self.candidate_positions += 1;
    }

    pub(crate) const fn record_unique_position(&mut self) {
        self.unique_positions += 1;
    }

    pub(crate) const fn record_write_allowed_position(&mut self) {
        self.write_allowed_positions += 1;
    }

    pub(crate) const fn record_write_allowed_positions(&mut self, count: u64) {
        self.write_allowed_positions += count;
    }

    pub(crate) const fn record_target_read(&mut self) {
        self.target_reads += 1;
    }

    pub(crate) const fn record_neighbor_read(&mut self) {
        self.neighbor_reads += 1;
    }

    pub(crate) fn record_section_read_attempt(
        &mut self,
        chunk_x: i32,
        chunk_z: i32,
        section: usize,
    ) {
        self.section_read_attempts += 1;
        self.read_sections.insert((chunk_x, chunk_z, section));
    }

    pub(crate) const fn record_section_read_contention(&mut self) {
        self.section_read_contentions += 1;
    }

    pub(crate) fn record_section_write_attempt(
        &mut self,
        chunk_x: i32,
        chunk_z: i32,
        section: usize,
    ) {
        self.section_write_attempts += 1;
        self.write_sections.insert((chunk_x, chunk_z, section));
    }

    pub(crate) const fn record_section_write_contention(&mut self) {
        self.section_write_contentions += 1;
    }

    pub(crate) const fn record_chunk_cache_miss(&mut self) {
        self.chunk_cache_misses += 1;
    }

    pub(crate) const fn record_chunk_status_upgrade(&mut self) {
        self.chunk_status_upgrades += 1;
    }

    pub(crate) const fn record_write(&mut self) {
        self.writes += 1;
    }

    pub(crate) fn record_candidate_time(&mut self, elapsed: Duration) {
        self.candidate_time += elapsed;
    }

    pub(crate) fn record_batch_apply_time(&mut self, elapsed: Duration) {
        self.batch_apply_time += elapsed;
    }

    pub(crate) fn record_read_time(&mut self, elapsed: Duration) {
        self.read_time += elapsed;
    }

    pub(crate) fn record_write_time(&mut self, elapsed: Duration) {
        self.write_time += elapsed;
    }

    pub(crate) fn record_read_contention_wait_time(&mut self, elapsed: Duration) {
        self.read_contention_wait_time += elapsed;
    }

    pub(crate) fn record_write_contention_wait_time(&mut self, elapsed: Duration) {
        self.write_contention_wait_time += elapsed;
    }
}

struct OreProfileTotals {
    veins: AtomicU64,
    placed_veins: AtomicU64,
    placed_blocks: AtomicU64,
    config_size_total: AtomicU64,
    candidate_positions: AtomicU64,
    unique_positions: AtomicU64,
    write_allowed_positions: AtomicU64,
    target_reads: AtomicU64,
    neighbor_reads: AtomicU64,
    section_read_attempts: AtomicU64,
    section_write_attempts: AtomicU64,
    section_read_contentions: AtomicU64,
    section_write_contentions: AtomicU64,
    chunk_cache_misses: AtomicU64,
    chunk_status_upgrades: AtomicU64,
    writes: AtomicU64,
    candidate_time_nanos: AtomicU64,
    batch_apply_time_nanos: AtomicU64,
    read_time_nanos: AtomicU64,
    write_time_nanos: AtomicU64,
    read_contention_wait_time_nanos: AtomicU64,
    write_contention_wait_time_nanos: AtomicU64,
    elapsed_nanos: AtomicU64,
    unique_read_sections: AtomicU64,
    unique_write_sections: AtomicU64,
    max_unique_read_sections: AtomicU64,
    max_unique_write_sections: AtomicU64,
}

impl OreProfileTotals {
    const fn new() -> Self {
        Self {
            veins: AtomicU64::new(0),
            placed_veins: AtomicU64::new(0),
            placed_blocks: AtomicU64::new(0),
            config_size_total: AtomicU64::new(0),
            candidate_positions: AtomicU64::new(0),
            unique_positions: AtomicU64::new(0),
            write_allowed_positions: AtomicU64::new(0),
            target_reads: AtomicU64::new(0),
            neighbor_reads: AtomicU64::new(0),
            section_read_attempts: AtomicU64::new(0),
            section_write_attempts: AtomicU64::new(0),
            section_read_contentions: AtomicU64::new(0),
            section_write_contentions: AtomicU64::new(0),
            chunk_cache_misses: AtomicU64::new(0),
            chunk_status_upgrades: AtomicU64::new(0),
            writes: AtomicU64::new(0),
            candidate_time_nanos: AtomicU64::new(0),
            batch_apply_time_nanos: AtomicU64::new(0),
            read_time_nanos: AtomicU64::new(0),
            write_time_nanos: AtomicU64::new(0),
            read_contention_wait_time_nanos: AtomicU64::new(0),
            write_contention_wait_time_nanos: AtomicU64::new(0),
            elapsed_nanos: AtomicU64::new(0),
            unique_read_sections: AtomicU64::new(0),
            unique_write_sections: AtomicU64::new(0),
            max_unique_read_sections: AtomicU64::new(0),
            max_unique_write_sections: AtomicU64::new(0),
        }
    }

    fn publish(&self, stats: OreFeatureStats, placed: u64) {
        let vein_index = self.veins.fetch_add(1, Ordering::Relaxed) + 1;
        if placed > 0 {
            self.placed_veins.fetch_add(1, Ordering::Relaxed);
            self.placed_blocks.fetch_add(placed, Ordering::Relaxed);
        }

        self.config_size_total
            .fetch_add(stats.config_size.max(0) as u64, Ordering::Relaxed);
        self.candidate_positions
            .fetch_add(stats.candidate_positions, Ordering::Relaxed);
        self.unique_positions
            .fetch_add(stats.unique_positions, Ordering::Relaxed);
        self.write_allowed_positions
            .fetch_add(stats.write_allowed_positions, Ordering::Relaxed);
        self.target_reads
            .fetch_add(stats.target_reads, Ordering::Relaxed);
        self.neighbor_reads
            .fetch_add(stats.neighbor_reads, Ordering::Relaxed);
        self.section_read_attempts
            .fetch_add(stats.section_read_attempts, Ordering::Relaxed);
        self.section_write_attempts
            .fetch_add(stats.section_write_attempts, Ordering::Relaxed);
        self.section_read_contentions
            .fetch_add(stats.section_read_contentions, Ordering::Relaxed);
        self.section_write_contentions
            .fetch_add(stats.section_write_contentions, Ordering::Relaxed);
        self.chunk_cache_misses
            .fetch_add(stats.chunk_cache_misses, Ordering::Relaxed);
        self.chunk_status_upgrades
            .fetch_add(stats.chunk_status_upgrades, Ordering::Relaxed);
        self.writes.fetch_add(stats.writes, Ordering::Relaxed);
        self.candidate_time_nanos
            .fetch_add(duration_nanos(stats.candidate_time), Ordering::Relaxed);
        self.batch_apply_time_nanos
            .fetch_add(duration_nanos(stats.batch_apply_time), Ordering::Relaxed);
        self.read_time_nanos
            .fetch_add(duration_nanos(stats.read_time), Ordering::Relaxed);
        self.write_time_nanos
            .fetch_add(duration_nanos(stats.write_time), Ordering::Relaxed);
        self.read_contention_wait_time_nanos.fetch_add(
            duration_nanos(stats.read_contention_wait_time),
            Ordering::Relaxed,
        );
        self.write_contention_wait_time_nanos.fetch_add(
            duration_nanos(stats.write_contention_wait_time),
            Ordering::Relaxed,
        );
        self.elapsed_nanos.fetch_add(
            duration_nanos(stats.started_at.elapsed()),
            Ordering::Relaxed,
        );

        let read_section_count = stats.read_sections.len() as u64;
        let write_section_count = stats.write_sections.len() as u64;
        self.unique_read_sections
            .fetch_add(read_section_count, Ordering::Relaxed);
        self.unique_write_sections
            .fetch_add(write_section_count, Ordering::Relaxed);
        atomic_max(&self.max_unique_read_sections, read_section_count);
        atomic_max(&self.max_unique_write_sections, write_section_count);

        let log_every = *ORE_PROFILE_LOG_EVERY;
        if log_every != 0 && vein_index.is_multiple_of(log_every) {
            self.log_snapshot(vein_index);
        }
    }

    fn log_snapshot(&self, veins: u64) {
        let placed_veins = self.placed_veins.load(Ordering::Relaxed);
        let placed_blocks = self.placed_blocks.load(Ordering::Relaxed);
        let candidate_positions = self.candidate_positions.load(Ordering::Relaxed);
        let unique_positions = self.unique_positions.load(Ordering::Relaxed);
        let write_allowed_positions = self.write_allowed_positions.load(Ordering::Relaxed);
        let target_reads = self.target_reads.load(Ordering::Relaxed);
        let neighbor_reads = self.neighbor_reads.load(Ordering::Relaxed);
        let section_read_attempts = self.section_read_attempts.load(Ordering::Relaxed);
        let section_write_attempts = self.section_write_attempts.load(Ordering::Relaxed);
        let section_read_contentions = self.section_read_contentions.load(Ordering::Relaxed);
        let section_write_contentions = self.section_write_contentions.load(Ordering::Relaxed);
        let chunk_cache_misses = self.chunk_cache_misses.load(Ordering::Relaxed);
        let chunk_status_upgrades = self.chunk_status_upgrades.load(Ordering::Relaxed);
        let writes = self.writes.load(Ordering::Relaxed);
        let candidate_time_ms = nanos_to_ms(self.candidate_time_nanos.load(Ordering::Relaxed));
        let batch_apply_time_ms = nanos_to_ms(self.batch_apply_time_nanos.load(Ordering::Relaxed));
        let read_time_ms = nanos_to_ms(self.read_time_nanos.load(Ordering::Relaxed));
        let write_time_ms = nanos_to_ms(self.write_time_nanos.load(Ordering::Relaxed));
        let read_wait_ms =
            nanos_to_ms(self.read_contention_wait_time_nanos.load(Ordering::Relaxed));
        let write_wait_ms = nanos_to_ms(
            self.write_contention_wait_time_nanos
                .load(Ordering::Relaxed),
        );
        let elapsed_ms = nanos_to_ms(self.elapsed_nanos.load(Ordering::Relaxed));
        let avg_config_size = ratio(self.config_size_total.load(Ordering::Relaxed), veins);
        let avg_read_sections = ratio(self.unique_read_sections.load(Ordering::Relaxed), veins);
        let avg_write_sections = ratio(self.unique_write_sections.load(Ordering::Relaxed), veins);
        let max_read_sections = self.max_unique_read_sections.load(Ordering::Relaxed);
        let max_write_sections = self.max_unique_write_sections.load(Ordering::Relaxed);

        let message = format!(
            "ore profile veins={veins} placed_veins={placed_veins} placed_blocks={placed_blocks} \
             avg_size={avg_config_size:.2} candidates={candidate_positions} unique={unique_positions} \
             write_allowed={write_allowed_positions} writes={writes} target_reads={target_reads} \
             neighbor_reads={neighbor_reads} read_locks={section_read_attempts} \
             read_contentions={section_read_contentions} write_locks={section_write_attempts} \
             write_contentions={section_write_contentions} chunk_cache_misses={chunk_cache_misses} \
             chunk_status_upgrades={chunk_status_upgrades} avg_read_sections={avg_read_sections:.2} \
             avg_write_sections={avg_write_sections:.2} max_read_sections={max_read_sections} \
             max_write_sections={max_write_sections} candidate_ms={candidate_time_ms:.2} \
             batch_apply_ms={batch_apply_time_ms:.2} read_ms={read_time_ms:.2} \
             write_ms={write_time_ms:.2} read_wait_ms={read_wait_ms:.2} \
             write_wait_ms={write_wait_ms:.2} elapsed_ms={elapsed_ms:.2}"
        );
        if log::log_enabled!(log::Level::Info) {
            log::info!("{message}");
        } else {
            eprintln!("{message}");
        }
    }
}

fn duration_nanos(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

fn nanos_to_ms(nanos: u64) -> f64 {
    nanos as f64 / 1_000_000.0
}

fn ratio(total: u64, count: u64) -> f64 {
    if count == 0 {
        0.0
    } else {
        total as f64 / count as f64
    }
}

fn atomic_max(target: &AtomicU64, value: u64) {
    let mut current = target.load(Ordering::Relaxed);
    while value > current {
        match target.compare_exchange_weak(current, value, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return,
            Err(next) => current = next,
        }
    }
}
