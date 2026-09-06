//! The brain itself.

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::ptr;
use std::sync::Arc;

use steel_registry::activity::ActivityRef;
use steel_registry::vanilla_activities;

use super::behavior::{BehaviorControl, BehaviorStatus};
use super::context::BrainContext;
use super::memory::{Memories, MemoryModuleTypeRef, MemoryStatus};
use super::sensor::{Sensor, WrappedSensor};
use crate::entity::PathfinderMob;
use crate::world::World;

/// One activity's behaviors at a single priority.
struct ActivityBehaviors {
    activity: ActivityRef,
    behaviors: Vec<Box<dyn BehaviorControl>>,
}

/// One activity's behaviors, requirements, and memories to erase.
///
/// Mirrors vanilla `ActivityData`.
pub struct ActivityData {
    activity: ActivityRef,
    behaviors: Vec<(i32, Box<dyn BehaviorControl>)>,
    requirements: Vec<(MemoryModuleTypeRef, MemoryStatus)>,
    memories_to_erase_when_stopped: Vec<MemoryModuleTypeRef>,
}

impl ActivityData {
    /// Behaviors at consecutive priorities starting at `priority_of_first_behavior`.
    #[must_use]
    pub fn new(
        activity: ActivityRef,
        priority_of_first_behavior: i32,
        behaviors: Vec<Box<dyn BehaviorControl>>,
    ) -> Self {
        let mut next_priority = priority_of_first_behavior;
        let behaviors = behaviors
            .into_iter()
            .map(|behavior| {
                let prioritized = (next_priority, behavior);
                next_priority += 1;
                prioritized
            })
            .collect();

        Self::prioritized(activity, behaviors)
    }

    /// Behaviors at explicit priorities, which several behaviors may share.
    #[must_use]
    pub fn prioritized(
        activity: ActivityRef,
        behaviors: Vec<(i32, Box<dyn BehaviorControl>)>,
    ) -> Self {
        Self {
            activity,
            behaviors,
            requirements: Vec::new(),
            memories_to_erase_when_stopped: Vec::new(),
        }
    }

    /// Memory states the brain requires before it will switch to this activity.
    #[must_use]
    pub fn requiring(mut self, requirements: Vec<(MemoryModuleTypeRef, MemoryStatus)>) -> Self {
        self.requirements = requirements;
        self
    }

    /// Memories cleared when the brain switches away from this activity.
    #[must_use]
    pub fn erasing_when_stopped(mut self, memories: Vec<MemoryModuleTypeRef>) -> Self {
        self.memories_to_erase_when_stopped = memories;
        self
    }
}

/// A mob's memory-driven AI.
///
/// Mirrors vanilla `Brain`, minus the schedule
///
/// Activities are compared by the address of their registry entry, like
/// [memories](Memories).
pub struct Brain {
    memories: Memories,
    sensors: Vec<WrappedSensor>,
    behaviors_by_priority: BTreeMap<i32, Vec<ActivityBehaviors>>,
    activity_requirements: Vec<(ActivityRef, Vec<(MemoryModuleTypeRef, MemoryStatus)>)>,
    activity_memories_to_erase_when_stopped: Vec<(ActivityRef, Vec<MemoryModuleTypeRef>)>,
    core_activities: Vec<ActivityRef>,
    active_activities: Vec<ActivityRef>,
    default_activity: ActivityRef,
}

impl Brain {
    /// An empty brain running its default activity alongside the core one.
    #[must_use]
    pub fn new() -> Self {
        let mut brain = Self {
            memories: Memories::new(),
            sensors: Vec::new(),
            behaviors_by_priority: BTreeMap::new(),
            activity_requirements: Vec::new(),
            activity_memories_to_erase_when_stopped: Vec::new(),
            core_activities: vec![&vanilla_activities::CORE],
            active_activities: Vec::new(),
            default_activity: &vanilla_activities::IDLE,
        };
        brain.use_default_activity();
        brain
    }

    /// The mob's memories.
    #[must_use]
    pub const fn memories(&self) -> &Memories {
        &self.memories
    }

    /// The mob's memories, for reads and writes outside a brain tick.
    pub const fn memories_mut(&mut self) -> &mut Memories {
        &mut self.memories
    }

    /// Registers a memory this mob holds.
    pub fn register_memory(&mut self, memory: MemoryModuleTypeRef) {
        self.memories.register(memory);
    }

    /// Adds a sensor, registering every memory it writes.
    pub fn add_sensor(&mut self, sensor: Box<dyn Sensor>) {
        let sensor = WrappedSensor::new(sensor);
        for &memory in sensor.requires() {
            self.memories.register(memory);
        }
        self.sensors.push(sensor);
    }

    /// Adds an activity's behaviors, registering every memory they need.
    ///
    /// Mirrors vanilla `Brain.addActivity`. Re-adding an activity replaces its
    /// requirements and appends its behaviors.
    pub fn add_activity(&mut self, data: ActivityData) {
        let ActivityData {
            activity,
            behaviors,
            requirements,
            memories_to_erase_when_stopped,
        } = data;

        self.set_activity_requirements(activity, requirements);
        if !memories_to_erase_when_stopped.is_empty() {
            self.activity_memories_to_erase_when_stopped
                .push((activity, memories_to_erase_when_stopped));
        }

        for (priority, behavior) in behaviors {
            let memories = &mut self.memories;
            behavior.visit_required_memories(&mut |memory| memories.register(memory));

            let groups = self.behaviors_by_priority.entry(priority).or_default();
            match groups
                .iter_mut()
                .find(|group| ptr::eq(group.activity, activity))
            {
                Some(group) => group.behaviors.push(behavior),
                None => groups.push(ActivityBehaviors {
                    activity,
                    behaviors: vec![behavior],
                }),
            }
        }
    }

    /// Sets the activities that stay active alongside the current one.
    pub fn set_core_activities(&mut self, activities: Vec<ActivityRef>) {
        self.core_activities = activities;
    }

    /// Sets the activity the brain falls back to.
    pub const fn set_default_activity(&mut self, activity: ActivityRef) {
        self.default_activity = activity;
    }

    /// Switches to the default activity.
    pub fn use_default_activity(&mut self) {
        self.set_active_activity(self.default_activity);
    }

    /// Switches to `activity`, or to the default when its requirements fail.
    pub fn set_active_activity_if_possible(&mut self, activity: ActivityRef) {
        if self.activity_requirements_are_met(activity) {
            self.set_active_activity(activity);
        } else {
            self.use_default_activity();
        }
    }

    /// Switches to the first activity whose requirements are met, if any.
    pub fn set_active_activity_to_first_valid(&mut self, activities: &[ActivityRef]) {
        for &activity in activities {
            if self.activity_requirements_are_met(activity) {
                self.set_active_activity(activity);
                return;
            }
        }
    }

    /// Whether `activity` is currently active.
    #[must_use]
    pub fn is_active(&self, activity: ActivityRef) -> bool {
        self.active_activities
            .iter()
            .any(|&active| ptr::eq(active, activity))
    }

    /// The active activity that is not a core one.
    #[must_use]
    pub fn active_non_core_activity(&self) -> Option<ActivityRef> {
        self.active_activities.iter().copied().find(|&active| {
            !self
                .core_activities
                .iter()
                .any(|&core| ptr::eq(core, active))
        })
    }

    /// Runs one brain tick.
    ///
    /// Vanilla `Brain.tick`, in the order that makes the rest work: expired
    /// memories are dropped before anything reads them, sensors then write what
    /// the mob perceives, stopped behaviors get their chance to start, and
    /// running ones tick or stop.
    pub fn tick(&mut self, mob: &dyn PathfinderMob, level: &Arc<World>, time: i64) {
        self.memories.forget_outdated();
        self.tick_sensors(mob, level, time);
        self.start_each_non_running_behavior(mob, level, time);
        self.tick_each_running_behavior(mob, level, time);
    }

    /// Stops every running behavior.
    ///
    /// Mirrors vanilla `Brain.stopAll`.
    pub fn stop_all(&mut self, mob: &dyn PathfinderMob, level: &Arc<World>, time: i64) {
        let Self {
            memories,
            behaviors_by_priority,
            ..
        } = self;
        let mut context = BrainContext {
            level,
            mob,
            memories,
            time,
        };

        for behavior in running_behaviors(behaviors_by_priority) {
            behavior.do_stop(&mut context);
        }
    }

    fn tick_sensors(&mut self, mob: &dyn PathfinderMob, level: &Arc<World>, time: i64) {
        let Self {
            memories, sensors, ..
        } = self;
        let mut context = BrainContext {
            level,
            mob,
            memories,
            time,
        };

        for sensor in sensors {
            sensor.tick(&mut context);
        }
    }

    /// Starts every stopped behavior of an active activity, lowest priority first.
    fn start_each_non_running_behavior(
        &mut self,
        mob: &dyn PathfinderMob,
        level: &Arc<World>,
        time: i64,
    ) {
        let Self {
            memories,
            behaviors_by_priority,
            active_activities,
            ..
        } = self;
        let mut context = BrainContext {
            level,
            mob,
            memories,
            time,
        };

        for groups in behaviors_by_priority.values_mut() {
            for group in groups {
                if !active_activities
                    .iter()
                    .any(|&active| ptr::eq(active, group.activity))
                {
                    continue;
                }

                for behavior in &mut group.behaviors {
                    if behavior.status() == BehaviorStatus::Stopped {
                        behavior.try_start(&mut context);
                    }
                }
            }
        }
    }

    /// Ticks every running behavior, whatever activity it belongs to.
    ///
    /// A behavior that started under an activity the brain has since left keeps
    /// running until it times out or gives up, exactly as in vanilla. Switching
    /// activity erases that activity's memories instead of cutting its behaviors
    /// off mid-run.
    fn tick_each_running_behavior(
        &mut self,
        mob: &dyn PathfinderMob,
        level: &Arc<World>,
        time: i64,
    ) {
        let Self {
            memories,
            behaviors_by_priority,
            ..
        } = self;
        let mut context = BrainContext {
            level,
            mob,
            memories,
            time,
        };

        for behavior in running_behaviors(behaviors_by_priority) {
            behavior.tick_or_stop(&mut context);
        }
    }

    fn set_active_activity(&mut self, activity: ActivityRef) {
        if self.is_active(activity) {
            return;
        }

        self.erase_memories_for_other_activities_than(activity);

        let Self {
            core_activities,
            active_activities,
            ..
        } = self;
        active_activities.clear();
        active_activities.extend(core_activities.iter().copied());
        active_activities.push(activity);
    }

    /// What vanilla does when switching activity, instead of stopping the
    /// leaving activity's behaviors.
    fn erase_memories_for_other_activities_than(&mut self, activity: ActivityRef) {
        let Self {
            memories,
            active_activities,
            activity_memories_to_erase_when_stopped,
            ..
        } = self;

        for &stopped in active_activities.iter() {
            if ptr::eq(stopped, activity) {
                continue;
            }

            for (registered, to_erase) in activity_memories_to_erase_when_stopped.iter() {
                if ptr::eq(*registered, stopped) {
                    for &memory in to_erase {
                        memories.erase(memory);
                    }
                }
            }
        }
    }

    fn set_activity_requirements(
        &mut self,
        activity: ActivityRef,
        requirements: Vec<(MemoryModuleTypeRef, MemoryStatus)>,
    ) {
        match self
            .activity_requirements
            .iter_mut()
            .find(|(registered, _)| ptr::eq(*registered, activity))
        {
            Some((_, registered)) => *registered = requirements,
            None => self.activity_requirements.push((activity, requirements)),
        }
    }

    /// Whether the brain may switch to `activity`.
    ///
    /// An activity with no registered behaviors has no requirements entry and is
    /// unreachable, so an unknown activity falls back to the default rather than
    /// becoming silently available.
    fn activity_requirements_are_met(&self, activity: ActivityRef) -> bool {
        let Some((_, requirements)) = self
            .activity_requirements
            .iter()
            .find(|(registered, _)| ptr::eq(*registered, activity))
        else {
            return false;
        };

        requirements
            .iter()
            .all(|&(memory, status)| self.memories.check(memory, status))
    }
}

impl Default for Brain {
    fn default() -> Self {
        Self::new()
    }
}

/// Every running behavior, in priority order.
fn running_behaviors(
    behaviors_by_priority: &mut BTreeMap<i32, Vec<ActivityBehaviors>>,
) -> impl Iterator<Item = &mut Box<dyn BehaviorControl>> {
    behaviors_by_priority
        .values_mut()
        .flatten()
        .flat_map(|group| group.behaviors.iter_mut())
        .filter(|behavior| behavior.status() == BehaviorStatus::Running)
}
