use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use steel_registry::activity::ActivityRef;
use steel_registry::vanilla_activities;
use steel_utils::locks::SyncMutex;

use super::{ActivityData, Brain};
use crate::entity::ai::brain::behavior::{
    Behavior, BehaviorControl, BehaviorDuration, BehaviorExt,
};
use crate::entity::ai::brain::context::BrainContext;
use crate::entity::ai::brain::memory::{
    MemoryModuleType, MemoryModuleTypeRef, MemoryStatus, RememberedEntity,
};
use crate::entity::ai::brain::sensor::Sensor;
use crate::entity::ai::brain::test_support::{TestBrain, Ticks, test_memory};

static TARGET: MemoryModuleType<Ticks> = test_memory("target");
static SIGHTING: MemoryModuleType<Ticks> = test_memory("sighting");
static SCRATCH: MemoryModuleType<RememberedEntity> = test_memory("scratch");

const NEEDS_TARGET: &[(MemoryModuleTypeRef, MemoryStatus)] =
    &[(TARGET.entry(), MemoryStatus::ValuePresent)];
const NEEDS_SIGHTING: &[(MemoryModuleTypeRef, MemoryStatus)] =
    &[(SIGHTING.entry(), MemoryStatus::ValuePresent)];

/// Records the order behaviors start in, shared across every probe in a brain.
#[derive(Default)]
struct Journal {
    started: SyncMutex<Vec<&'static str>>,
    ticks: AtomicU32,
    long_running: AtomicBool,
}

impl Journal {
    fn started(&self) -> Vec<&'static str> {
        self.started.lock().clone()
    }
}

struct Probe {
    name: &'static str,
    journal: Arc<Journal>,
    entry_condition: &'static [(MemoryModuleTypeRef, MemoryStatus)],
}

impl Probe {
    fn new(name: &'static str, journal: &Arc<Journal>) -> Self {
        Self {
            name,
            journal: Arc::clone(journal),
            entry_condition: &[],
        }
    }

    fn requiring(
        mut self,
        entry_condition: &'static [(MemoryModuleTypeRef, MemoryStatus)],
    ) -> Self {
        self.entry_condition = entry_condition;
        self
    }
}

impl Behavior for Probe {
    fn entry_condition(&self) -> &[(MemoryModuleTypeRef, MemoryStatus)] {
        self.entry_condition
    }

    fn duration(&self) -> BehaviorDuration {
        BehaviorDuration::fixed(1_000)
    }

    fn start(&mut self, _context: &mut BrainContext<'_>) {
        self.journal.started.lock().push(self.name);
    }

    fn can_still_use(&mut self, _context: &mut BrainContext<'_>) -> bool {
        self.journal.long_running.load(Ordering::Relaxed)
    }

    fn tick(&mut self, _context: &mut BrainContext<'_>) {
        self.journal.ticks.fetch_add(1, Ordering::Relaxed);
    }
}

/// Writes `SIGHTING` on every scan.
struct SightingSensor;

impl Sensor for SightingSensor {
    fn requires(&self) -> &[MemoryModuleTypeRef] {
        const WRITES: &[MemoryModuleTypeRef] = &[SIGHTING.entry()];
        WRITES
    }

    fn scan_rate(&self) -> i32 {
        1
    }

    fn do_tick(&mut self, context: &mut BrainContext<'_>) {
        context.memories.set(&SIGHTING, Ticks(1));
    }
}

fn probes(names: &[&'static str], journal: &Arc<Journal>) -> Vec<Box<dyn BehaviorControl>> {
    names
        .iter()
        .map(|&name| Probe::new(name, journal).control())
        .collect()
}

fn tick(brain: &mut Brain, mob: &mut TestBrain, time: i64) {
    mob.set_time(time);
    let context = mob.context();
    brain.tick(context.mob, context.level, time);
}

#[test]
fn behaviors_and_sensors_register_the_memories_they_declare() {
    let journal = Arc::new(Journal::default());
    let mut brain = Brain::new();
    brain.add_sensor(Box::new(SightingSensor));
    brain.add_activity(ActivityData::new(
        &vanilla_activities::IDLE,
        0,
        vec![
            Probe::new("idle", &journal)
                .requiring(NEEDS_TARGET)
                .control(),
        ],
    ));

    assert!(
        brain.memories().is_registered(TARGET.entry()),
        "a behavior's entry condition should register its memory"
    );
    assert!(
        brain.memories().is_registered(SIGHTING.entry()),
        "a sensor's output memory should register itself"
    );
    assert!(!brain.memories().is_registered(SCRATCH.entry()));
}

#[test]
fn behaviors_start_in_priority_order_and_share_priorities() {
    let journal = Arc::new(Journal::default());
    let mut mob = TestBrain::new();
    let mut brain = Brain::new();
    brain.add_activity(ActivityData::prioritized(
        &vanilla_activities::IDLE,
        vec![
            (5, Probe::new("late", &journal).control()),
            (0, Probe::new("early", &journal).control()),
            (5, Probe::new("late_twin", &journal).control()),
        ],
    ));

    tick(&mut brain, &mut mob, 1);

    assert_eq!(
        journal.started(),
        vec!["early", "late", "late_twin"],
        "lower priorities start first, and a shared priority starts both"
    );
}

#[test]
fn leaving_an_activity_does_not_stop_its_running_behaviors() {
    let journal = Arc::new(Journal::default());
    journal.long_running.store(true, Ordering::Relaxed);
    let mut mob = TestBrain::new();
    let mut brain = Brain::new();
    brain.add_activity(ActivityData::new(
        &vanilla_activities::IDLE,
        0,
        probes(&["idle"], &journal),
    ));
    brain.add_activity(ActivityData::new(
        &vanilla_activities::PANIC,
        0,
        probes(&["panic"], &journal),
    ));

    tick(&mut brain, &mut mob, 1);
    let ticks_while_idle = journal.ticks.load(Ordering::Relaxed);

    brain.set_active_activity_if_possible(&vanilla_activities::PANIC);
    tick(&mut brain, &mut mob, 2);

    assert!(
        journal.ticks.load(Ordering::Relaxed) > ticks_while_idle + 1,
        "the idle behavior should keep ticking alongside the panic one"
    );
    assert_eq!(journal.started(), vec!["idle", "panic"]);
}

#[test]
fn leaving_an_activity_erases_the_memories_it_declared() {
    let journal = Arc::new(Journal::default());
    let mut brain = Brain::new();
    brain.register_memory(TARGET.entry());
    brain.add_activity(ActivityData::new(
        &vanilla_activities::IDLE,
        0,
        probes(&["idle"], &journal),
    ));
    brain.add_activity(
        ActivityData::new(&vanilla_activities::PANIC, 0, probes(&["panic"], &journal))
            .requiring(NEEDS_TARGET.to_vec())
            .erasing_when_stopped(vec![TARGET.entry()]),
    );
    brain.memories_mut().set(&TARGET, Ticks(1));

    brain.set_active_activity_if_possible(&vanilla_activities::PANIC);
    assert!(brain.is_active(&vanilla_activities::PANIC));
    assert!(brain.memories().has_value(TARGET.entry()));

    brain.use_default_activity();

    assert!(
        !brain.memories().has_value(TARGET.entry()),
        "switching away should erase the activity's declared memories"
    );
}

#[test]
fn unmet_requirements_fall_back_to_the_default_activity() {
    let journal = Arc::new(Journal::default());
    let mut brain = Brain::new();
    brain.add_activity(ActivityData::new(
        &vanilla_activities::IDLE,
        0,
        probes(&["idle"], &journal),
    ));
    brain.add_activity(
        ActivityData::new(&vanilla_activities::PANIC, 0, probes(&["panic"], &journal))
            .requiring(NEEDS_TARGET.to_vec()),
    );
    brain.register_memory(TARGET.entry());

    brain.set_active_activity_if_possible(&vanilla_activities::PANIC);
    assert!(brain.is_active(&vanilla_activities::IDLE));

    brain.memories_mut().set(&TARGET, Ticks(1));
    brain.set_active_activity_if_possible(&vanilla_activities::PANIC);
    assert!(brain.is_active(&vanilla_activities::PANIC));

    brain.set_active_activity_if_possible(&vanilla_activities::MEET);
    assert!(
        brain.is_active(&vanilla_activities::IDLE),
        "an activity with no behaviors is unreachable, not silently available"
    );
}

#[test]
fn core_activities_stay_active_across_switches() {
    let journal = Arc::new(Journal::default());
    let mut brain = Brain::new();
    let cores: Vec<ActivityRef> = vec![&vanilla_activities::CORE, &vanilla_activities::SWIM];
    brain.set_core_activities(cores);
    brain.add_activity(ActivityData::new(
        &vanilla_activities::PANIC,
        0,
        probes(&["panic"], &journal),
    ));

    brain.set_active_activity_if_possible(&vanilla_activities::PANIC);

    assert!(brain.is_active(&vanilla_activities::CORE));
    assert!(brain.is_active(&vanilla_activities::SWIM));
    assert_eq!(
        brain.active_non_core_activity(),
        Some(&vanilla_activities::PANIC as ActivityRef)
    );
}

#[test]
fn memories_expire_before_behaviors_read_them() {
    let journal = Arc::new(Journal::default());
    let mut mob = TestBrain::new();
    let mut brain = Brain::new();
    brain.add_activity(ActivityData::new(
        &vanilla_activities::IDLE,
        0,
        vec![
            Probe::new("idle", &journal)
                .requiring(NEEDS_TARGET)
                .control(),
        ],
    ));
    brain.memories_mut().set_with_expiry(&TARGET, Ticks(1), 0);

    tick(&mut brain, &mut mob, 1);

    assert!(
        journal.started().is_empty(),
        "a memory expiring this tick must not let a behavior start"
    );
}

#[test]
fn sensors_write_memories_that_behaviors_start_on_the_same_tick() {
    let journal = Arc::new(Journal::default());
    let mut mob = TestBrain::new();
    let mut brain = Brain::new();
    brain.add_sensor(Box::new(SightingSensor));
    brain.add_activity(ActivityData::new(
        &vanilla_activities::IDLE,
        0,
        vec![
            Probe::new("idle", &journal)
                .requiring(NEEDS_SIGHTING)
                .control(),
        ],
    ));

    tick(&mut brain, &mut mob, 1);

    assert_eq!(
        journal.started(),
        vec!["idle"],
        "a behavior should see the memory its sensor wrote this tick"
    );
}
