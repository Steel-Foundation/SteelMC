//! The Brain

use rustc_hash::{FxHashMap, FxHashSet};

use super::activity::Activity;
use super::behavior::{Behavior, BehaviorStatus};
use super::memory::{Memories, MemoryModuleType, MemoryStatus};
use super::schedule::Schedule;
use super::sensor::{Sensor, WrappedSensor};
use crate::entity::PathfinderMob;

const SCHEDULE_UPDATE_DELAY: i64 = 20;

struct PrioritizedBehavior {
    priority: i32,
    activity: Activity,
    behavior: Box<dyn Behavior>,
}

pub(crate) struct Brain {
    memories: Memories,
    sensors: Vec<WrappedSensor>,
    behaviors: Vec<PrioritizedBehavior>,
    core_activities: FxHashSet<Activity>,
    active_activities: FxHashSet<Activity>,
    default_activity: Activity,
    activity_requirements: FxHashMap<Activity, Vec<(MemoryModuleType, MemoryStatus)>>,
    schedule: Option<Schedule>,
    last_schedule_update: i64,
}

impl Brain {
    #[must_use]
    pub(crate) fn new(
        memory_types: impl IntoIterator<Item = MemoryModuleType>,
        sensors: Vec<Box<dyn Sensor>>,
    ) -> Self {
        let mut registered: Vec<MemoryModuleType> = memory_types.into_iter().collect();
        for sensor in &sensors {
            registered.extend_from_slice(sensor.requires());
        }
        Self {
            memories: Memories::new(registered),
            sensors: sensors.into_iter().map(WrappedSensor::new).collect(),
            behaviors: Vec::new(),
            core_activities: FxHashSet::default(),
            active_activities: FxHashSet::default(),
            default_activity: Activity::Idle,
            activity_requirements: FxHashMap::default(),
            schedule: None,
            last_schedule_update: -9999,
        }
    }

    #[must_use]
    pub(crate) const fn memories(&self) -> &Memories {
        &self.memories
    }

    pub(crate) const fn memories_mut(&mut self) -> &mut Memories {
        &mut self.memories
    }

    pub(crate) fn set_core_activities(&mut self, activities: impl IntoIterator<Item = Activity>) {
        self.core_activities = activities.into_iter().collect();
    }

    pub(crate) const fn set_default_activity(&mut self, activity: Activity) {
        self.default_activity = activity;
    }

    pub(crate) const fn set_schedule(&mut self, schedule: Schedule) {
        self.schedule = Some(schedule);
    }

    pub(crate) fn update_activity_from_schedule(&mut self, game_time: i64, day_time: i64) {
        if game_time - self.last_schedule_update <= SCHEDULE_UPDATE_DELAY {
            return;
        }
        self.last_schedule_update = game_time;
        let scheduled = self
            .schedule
            .map_or(Activity::Idle, |schedule| schedule.activity_at(day_time));
        if !self.active_activities.contains(&scheduled) {
            self.set_active_activity_if_possible(scheduled);
        }
    }

    pub(crate) fn add_activity(
        &mut self,
        activity: Activity,
        priority_of_first_behavior: i32,
        behaviors: Vec<Box<dyn Behavior>>,
    ) {
        self.add_activity_with_conditions(
            activity,
            priority_of_first_behavior,
            behaviors,
            Vec::new(),
        );
    }

    pub(crate) fn add_activity_with_conditions(
        &mut self,
        activity: Activity,
        priority_of_first_behavior: i32,
        behaviors: Vec<Box<dyn Behavior>>,
        conditions: Vec<(MemoryModuleType, MemoryStatus)>,
    ) {
        self.activity_requirements.insert(activity, conditions);
        for (offset, behavior) in behaviors.into_iter().enumerate() {
            self.behaviors.push(PrioritizedBehavior {
                priority: priority_of_first_behavior + offset as i32,
                activity,
                behavior,
            });
        }
        self.behaviors.sort_by_key(|entry| entry.priority);
    }

    #[must_use]
    pub(crate) fn is_active(&self, activity: Activity) -> bool {
        self.active_activities.contains(&activity)
    }

    pub(crate) fn use_default_activity(&mut self) {
        self.set_active_activity(self.default_activity);
    }

    pub(crate) fn set_active_activity_if_possible(&mut self, activity: Activity) {
        if self.activity_requirements_are_met(activity) {
            self.set_active_activity(activity);
        } else {
            self.use_default_activity();
        }
    }

    fn activity_requirements_are_met(&self, activity: Activity) -> bool {
        let Some(conditions) = self.activity_requirements.get(&activity) else {
            return false;
        };
        conditions
            .iter()
            .all(|(ty, status)| self.memories.check_memory(*ty, *status))
    }

    fn set_active_activity(&mut self, activity: Activity) {
        if self.is_active(activity) {
            return;
        }
        let Self {
            active_activities,
            core_activities,
            ..
        } = self;
        active_activities.clear();
        active_activities.extend(core_activities.iter().copied());
        active_activities.insert(activity);
    }

    pub(crate) fn tick(&mut self, mob: &dyn PathfinderMob, time: i64) {
        let Self {
            memories,
            sensors,
            behaviors,
            active_activities,
            ..
        } = self;

        memories.forget_outdated();

        for sensor in sensors {
            sensor.tick(mob, memories);
        }

        for entry in &mut *behaviors {
            if entry.behavior.status() == BehaviorStatus::Stopped
                && active_activities.contains(&entry.activity)
            {
                entry.behavior.try_start(mob, memories, time);
            }
        }

        for entry in &mut *behaviors {
            if entry.behavior.status() == BehaviorStatus::Running {
                if active_activities.contains(&entry.activity) {
                    entry.behavior.tick_or_stop(mob, memories, time);
                } else {
                    // The behavior's activity is no longer active (e.g. Rest ended
                    // at dawn) — stop it so its `stop` hook runs (e.g. wake up).
                    entry.behavior.do_stop(mob, memories, time);
                }
            }
        }
    }
}
