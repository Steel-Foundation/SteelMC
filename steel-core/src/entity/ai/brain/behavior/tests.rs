use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use super::{Behavior, BehaviorControl, BehaviorDuration, BehaviorStatus, TimedBehavior};
use crate::entity::ai::brain::context::BrainContext;
use crate::entity::ai::brain::memory::{MemoryModuleType, MemoryModuleTypeRef, MemoryStatus};
use crate::entity::ai::brain::test_support::{TestBrain, Ticks, test_memory};

static TARGET: MemoryModuleType<Ticks> = test_memory("target");

const NEEDS_TARGET: &[(MemoryModuleTypeRef, MemoryStatus)] =
    &[(TARGET.entry(), MemoryStatus::ValuePresent)];

/// Probe state the test still reaches after the behavior is wrapped.
struct Probed {
    started: AtomicU32,
    ticked: AtomicU32,
    stopped: AtomicU32,
    can_start: AtomicBool,
}

impl Probed {
    fn counts(&self) -> (u32, u32, u32) {
        (
            self.started.load(Ordering::Relaxed),
            self.ticked.load(Ordering::Relaxed),
            self.stopped.load(Ordering::Relaxed),
        )
    }

    fn set_can_start(&self, can_start: bool) {
        self.can_start.store(can_start, Ordering::Relaxed);
    }
}

struct Probe {
    probed: Arc<Probed>,
    entry_condition: &'static [(MemoryModuleTypeRef, MemoryStatus)],
    duration: BehaviorDuration,
    can_still_use: bool,
}

impl Probe {
    fn new() -> (Self, Arc<Probed>) {
        let probed = Arc::new(Probed {
            started: AtomicU32::new(0),
            ticked: AtomicU32::new(0),
            stopped: AtomicU32::new(0),
            can_start: AtomicBool::new(true),
        });
        let probe = Self {
            probed: Arc::clone(&probed),
            entry_condition: &[],
            duration: BehaviorDuration::DEFAULT,
            can_still_use: false,
        };
        (probe, probed)
    }
}

impl Behavior for Probe {
    fn entry_condition(&self) -> &[(MemoryModuleTypeRef, MemoryStatus)] {
        self.entry_condition
    }

    fn duration(&self) -> BehaviorDuration {
        self.duration
    }

    fn check_extra_start_conditions(&mut self, _context: &mut BrainContext<'_>) -> bool {
        self.probed.can_start.load(Ordering::Relaxed)
    }

    fn start(&mut self, _context: &mut BrainContext<'_>) {
        self.probed.started.fetch_add(1, Ordering::Relaxed);
    }

    fn can_still_use(&mut self, _context: &mut BrainContext<'_>) -> bool {
        self.can_still_use
    }

    fn tick(&mut self, _context: &mut BrainContext<'_>) {
        self.probed.ticked.fetch_add(1, Ordering::Relaxed);
    }

    fn stop(&mut self, _context: &mut BrainContext<'_>) {
        self.probed.stopped.fetch_add(1, Ordering::Relaxed);
    }
}

#[test]
fn behavior_starts_only_when_every_start_condition_holds() {
    let (mut probe, probed) = Probe::new();
    probe.entry_condition = NEEDS_TARGET;
    let mut control = TimedBehavior::new(probe);
    let mut brain = TestBrain::new();
    brain.memories().register(TARGET.entry());

    assert!(
        !control.try_start(&mut brain.context()),
        "missing memory should block the start"
    );

    brain.memories().set(&TARGET, Ticks(1));
    probed.set_can_start(false);

    assert!(
        !control.try_start(&mut brain.context()),
        "extra start conditions should block the start"
    );
    assert_eq!(control.status(), BehaviorStatus::Stopped);
    assert_eq!(probed.counts(), (0, 0, 0));

    probed.set_can_start(true);

    assert!(control.try_start(&mut brain.context()));
    assert_eq!(control.status(), BehaviorStatus::Running);
    assert_eq!(probed.counts(), (1, 0, 0));
}

#[test]
fn behavior_without_can_still_use_is_a_one_shot() {
    let (probe, probed) = Probe::new();
    let mut control = TimedBehavior::new(probe);
    let mut brain = TestBrain::new();

    assert!(control.try_start(&mut brain.context()));
    control.tick_or_stop(&mut brain.context());

    assert_eq!(control.status(), BehaviorStatus::Stopped);
    assert_eq!(
        probed.counts(),
        (1, 0, 1),
        "a start-only behavior should stop before it ever ticks"
    );
}

#[test]
fn running_behavior_stops_the_tick_after_its_duration() {
    let (mut probe, probed) = Probe::new();
    probe.can_still_use = true;
    probe.duration = BehaviorDuration::fixed(3);
    let mut control = TimedBehavior::new(probe);
    let mut brain = TestBrain::new();

    assert!(control.try_start(&mut brain.context()));

    for time in 1..=3 {
        brain.set_time(time);
        control.tick_or_stop(&mut brain.context());
        assert_eq!(control.status(), BehaviorStatus::Running);
    }

    brain.set_time(4);
    control.tick_or_stop(&mut brain.context());

    assert_eq!(control.status(), BehaviorStatus::Stopped);
    assert_eq!(probed.counts(), (1, 3, 1));
}

#[test]
fn duration_is_rolled_on_every_activation() {
    let (mut probe, probed) = Probe::new();
    probe.can_still_use = true;
    probe.duration = BehaviorDuration::range(1, 200);
    let mut control = TimedBehavior::new(probe);
    let mut brain = TestBrain::new();

    let mut rolled = Vec::new();
    let mut time = 0;
    for _ in 0..16 {
        brain.set_time(time);
        assert!(control.try_start(&mut brain.context()));
        let ticks_before = probed.counts().1;

        while control.status() == BehaviorStatus::Running {
            time += 1;
            brain.set_time(time);
            control.tick_or_stop(&mut brain.context());
        }

        rolled.push(probed.counts().1 - ticks_before);
    }

    assert!(
        rolled.iter().all(|&duration| (1..=200).contains(&duration)),
        "every roll should land in the declared range: {rolled:?}"
    );
    assert!(
        rolled.iter().any(|&duration| duration != rolled[0]),
        "16 activations should not all roll the same duration: {rolled:?}"
    );
}
