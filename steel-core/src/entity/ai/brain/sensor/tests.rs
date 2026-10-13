use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use super::{DEFAULT_SCAN_RATE, Sensor, WrappedSensor};
use crate::entity::ai::brain::context::BrainContext;
use crate::entity::ai::brain::memory::{MemoryModuleType, MemoryModuleTypeRef};
use crate::entity::ai::brain::test_support::{TestBrain, Ticks, test_memory};

static SEEN: MemoryModuleType<Ticks> = test_memory("seen");

const WRITES_SEEN: &[MemoryModuleTypeRef] = &[SEEN.entry()];

struct Probe {
    scans: Arc<AtomicU32>,
    scan_rate: i32,
}

impl Probe {
    fn new(scan_rate: i32) -> (Self, Arc<AtomicU32>) {
        let scans = Arc::new(AtomicU32::new(0));
        let probe = Self {
            scans: Arc::clone(&scans),
            scan_rate,
        };
        (probe, scans)
    }
}

impl Sensor for Probe {
    fn requires(&self) -> &[MemoryModuleTypeRef] {
        WRITES_SEEN
    }

    fn scan_rate(&self) -> i32 {
        self.scan_rate
    }

    fn do_tick(&mut self, context: &mut BrainContext<'_>) {
        let scan = self.scans.fetch_add(1, Ordering::Relaxed);
        context
            .memories
            .set(&SEEN, Ticks(i32::try_from(scan).expect("scan count fits")));
    }
}

/// Ticks `sensor` `ticks` times and returns the tick each scan happened on.
fn scan_ticks(sensor: &mut WrappedSensor, brain: &mut TestBrain, ticks: u32) -> Vec<u32> {
    let mut scanned_on = Vec::new();
    let mut previous = 0;
    for tick in 1..=ticks {
        sensor.tick(&mut brain.context());
        let scans = brain.memories().get(&SEEN).map_or(0, |seen| {
            u32::try_from(seen.0 + 1).expect("scan count fits")
        });
        if scans != previous {
            scanned_on.push(tick);
            previous = scans;
        }
    }
    scanned_on
}

#[test]
fn sensor_scans_once_per_scan_rate() {
    let (probe, scans) = Probe::new(5);
    let mut sensor = WrappedSensor::new(Box::new(probe));
    let mut brain = TestBrain::new();
    brain.memories().register(SEEN.entry());

    let scanned_on = scan_ticks(&mut sensor, &mut brain, 40);

    assert!(!scanned_on.is_empty(), "sensor should have scanned");
    let gaps: Vec<_> = scanned_on
        .windows(2)
        .map(|pair| pair[1] - pair[0])
        .collect();
    assert!(
        gaps.iter().all(|&gap| gap == 5),
        "scans should be one scan rate apart: {scanned_on:?}"
    );
    assert_eq!(scans.load(Ordering::Relaxed), scanned_on.len() as u32);
}

#[test]
fn sensors_start_at_a_random_offset() {
    let mut brain = TestBrain::new();
    brain.memories().register(SEEN.entry());

    let mut first_scans = Vec::new();
    for _ in 0..32 {
        let (probe, _) = Probe::new(DEFAULT_SCAN_RATE);
        let mut sensor = WrappedSensor::new(Box::new(probe));
        brain.memories().erase(SEEN.entry());
        let scanned_on = scan_ticks(&mut sensor, &mut brain, DEFAULT_SCAN_RATE as u32);
        first_scans.push(
            *scanned_on
                .first()
                .expect("sensor should scan within its rate"),
        );
    }

    assert!(
        first_scans.iter().any(|&tick| tick != first_scans[0]),
        "mobs built on the same tick must not all scan on the same tick: {first_scans:?}"
    );
}
