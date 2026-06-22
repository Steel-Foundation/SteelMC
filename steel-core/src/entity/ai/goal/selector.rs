use std::fmt;
use std::ops::BitOr;

use crate::entity::PathfinderMob;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoalControl {
    Move,
    Look,
    Jump,
    Target,
}

impl GoalControl {
    const ALL: [Self; 4] = [Self::Move, Self::Look, Self::Jump, Self::Target];
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct GoalControls(u8);

impl GoalControls {
    pub const EMPTY: Self = Self(0);
    pub const MOVE: Self = Self(1 << 0);
    pub const LOOK: Self = Self(1 << 1);
    pub const JUMP: Self = Self(1 << 2);
    pub const TARGET: Self = Self(1 << 3);

    #[must_use]
    pub const fn from_control(control: GoalControl) -> Self {
        match control {
            GoalControl::Move => Self::MOVE,
            GoalControl::Look => Self::LOOK,
            GoalControl::Jump => Self::JUMP,
            GoalControl::Target => Self::TARGET,
        }
    }

    #[must_use]
    pub const fn contains(self, control: GoalControl) -> bool {
        self.0 & Self::from_control(control).0 != 0
    }

    #[must_use]
    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    pub const fn insert(&mut self, control: GoalControl) {
        self.0 |= Self::from_control(control).0;
    }

    pub const fn remove(&mut self, control: GoalControl) {
        self.0 &= !Self::from_control(control).0;
    }

    pub fn iter(self) -> impl Iterator<Item = GoalControl> {
        GoalControl::ALL
            .into_iter()
            .filter(move |control| self.contains(*control))
    }
}

impl fmt::Debug for GoalControls {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.iter()).finish()
    }
}

impl BitOr for GoalControls {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

pub trait Goal: Send {
    fn controls(&self) -> GoalControls;

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool;

    fn can_continue_to_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        self.can_use(mob)
    }

    fn is_interruptable(&self) -> bool {
        true
    }

    fn is_panic_goal(&self) -> bool {
        false
    }

    fn start(&mut self, _mob: &dyn PathfinderMob) {}

    fn stop(&mut self, _mob: &dyn PathfinderMob) {}

    fn requires_update_every_tick(&self) -> bool {
        false
    }

    fn tick(&mut self, _mob: &dyn PathfinderMob) {}
}

struct WrappedGoal {
    priority: i32,
    goal: Box<dyn Goal>,
    running: bool,
}

impl WrappedGoal {
    fn new(priority: i32, goal: Box<dyn Goal>) -> Self {
        Self {
            priority,
            goal,
            running: false,
        }
    }

    const fn is_running(&self) -> bool {
        self.running
    }

    fn controls(&self) -> GoalControls {
        self.goal.controls()
    }

    fn can_be_replaced_by(&self, candidate_priority: i32) -> bool {
        self.goal.is_interruptable() && candidate_priority < self.priority
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        self.goal.can_use(mob)
    }

    fn can_continue_to_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        self.goal.can_continue_to_use(mob)
    }

    fn start(&mut self, mob: &dyn PathfinderMob) {
        if self.running {
            return;
        }
        self.running = true;
        self.goal.start(mob);
    }

    fn stop(&mut self, mob: &dyn PathfinderMob) {
        if !self.running {
            return;
        }
        self.running = false;
        self.goal.stop(mob);
    }

    fn tick(&mut self, mob: &dyn PathfinderMob) {
        self.goal.tick(mob);
    }

    fn requires_update_every_tick(&self) -> bool {
        self.goal.requires_update_every_tick()
    }

    fn is_panic_goal(&self) -> bool {
        self.goal.is_panic_goal()
    }
}

pub struct GoalSelector {
    available_goals: Vec<WrappedGoal>,
    disabled_controls: GoalControls,
}

impl GoalSelector {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            available_goals: Vec::new(),
            disabled_controls: GoalControls::EMPTY,
        }
    }

    pub fn add_goal<G>(&mut self, priority: i32, goal: G)
    where
        G: Goal + 'static,
    {
        self.available_goals
            .push(WrappedGoal::new(priority, Box::new(goal)));
    }

    pub fn tick(&mut self, mob: &dyn PathfinderMob) {
        for index in 0..self.available_goals.len() {
            let should_stop = {
                let disabled_controls = self.disabled_controls;
                let goal = &mut self.available_goals[index];
                goal.is_running()
                    && (goal.controls().intersects(disabled_controls)
                        || !goal.can_continue_to_use(mob))
            };
            if should_stop {
                self.available_goals[index].stop(mob);
            }
        }

        for index in 0..self.available_goals.len() {
            if !self.can_start_goal(index) {
                continue;
            }
            if !self.available_goals[index].can_use(mob) {
                continue;
            }

            let controls = self.available_goals[index].controls();
            for control in controls.iter() {
                if let Some(current_index) = self.running_goal_index_for(control) {
                    self.available_goals[current_index].stop(mob);
                }
            }
            self.available_goals[index].start(mob);
        }

        self.tick_running_goals(mob, true);
    }

    pub fn tick_running_goals(
        &mut self,
        mob: &dyn PathfinderMob,
        force_tick_all_running_goals: bool,
    ) {
        for goal in &mut self.available_goals {
            if goal.is_running()
                && (force_tick_all_running_goals || goal.requires_update_every_tick())
            {
                goal.tick(mob);
            }
        }
    }

    pub const fn disable_control(&mut self, control: GoalControl) {
        self.disabled_controls.insert(control);
    }

    pub const fn enable_control(&mut self, control: GoalControl) {
        self.disabled_controls.remove(control);
    }

    pub const fn set_control(&mut self, control: GoalControl, enabled: bool) {
        if enabled {
            self.enable_control(control);
        } else {
            self.disable_control(control);
        }
    }

    #[must_use]
    pub fn running_goal_count(&self) -> usize {
        self.available_goals
            .iter()
            .filter(|goal| goal.is_running())
            .count()
    }

    #[must_use]
    pub const fn available_goal_count(&self) -> usize {
        self.available_goals.len()
    }

    #[must_use]
    pub(crate) fn has_running_panic_goal(&self) -> bool {
        self.available_goals
            .iter()
            .any(|goal| goal.is_running() && goal.is_panic_goal())
    }

    #[cfg(test)]
    #[must_use]
    pub(crate) fn available_goal_priorities(&self) -> Vec<i32> {
        self.available_goals
            .iter()
            .map(|goal| goal.priority)
            .collect()
    }

    #[cfg(test)]
    #[must_use]
    pub(crate) const fn is_control_disabled(&self, control: GoalControl) -> bool {
        self.disabled_controls.contains(control)
    }

    fn can_start_goal(&self, index: usize) -> bool {
        let goal = &self.available_goals[index];
        !goal.is_running()
            && !goal.controls().intersects(self.disabled_controls)
            && self.goal_can_be_replaced_for_all_controls(index)
    }

    fn goal_can_be_replaced_for_all_controls(&self, candidate_index: usize) -> bool {
        let candidate = &self.available_goals[candidate_index];
        for control in candidate.controls().iter() {
            if let Some(current_index) = self.running_goal_index_for(control)
                && !self.available_goals[current_index].can_be_replaced_by(candidate.priority)
            {
                return false;
            }
        }
        true
    }

    fn running_goal_index_for(&self, control: GoalControl) -> Option<usize> {
        self.available_goals
            .iter()
            .position(|goal| goal.is_running() && goal.controls().contains(control))
    }

    #[cfg(test)]
    fn is_priority_running(&self, priority: i32) -> bool {
        self.available_goals
            .iter()
            .any(|goal| goal.priority == priority && goal.is_running())
    }
}

impl Default for GoalSelector {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for GoalSelector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GoalSelector")
            .field("available_goals", &self.available_goals.len())
            .field("running_goals", &self.running_goal_count())
            .field("disabled_controls", &self.disabled_controls)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Weak;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use glam::DVec3;
    use steel_registry::entity_type::EntityTypeRef;
    use steel_registry::{test_support::init_test_registry, vanilla_entities};
    use steel_utils::locks::SyncMutex;

    use super::*;
    use crate::entity::{
        Entity, EntityBase, LivingEntity, LivingEntityBase, Mob, MobBase, PathfinderMob,
    };

    struct TestPathfinderMob {
        base: EntityBase,
        living_base: LivingEntityBase,
        mob_base: MobBase,
        mob_flags: SyncMutex<i8>,
        health: SyncMutex<f32>,
    }

    impl TestPathfinderMob {
        fn new() -> Self {
            init_test_registry();
            Self {
                base: EntityBase::new(
                    1,
                    DVec3::ZERO,
                    vanilla_entities::PIG.dimensions,
                    Weak::new(),
                ),
                living_base: LivingEntityBase::new(&vanilla_entities::PIG),
                mob_base: MobBase::new(),
                mob_flags: SyncMutex::new(0),
                health: SyncMutex::new(10.0),
            }
        }
    }

    impl Entity for TestPathfinderMob {
        fn base(&self) -> &EntityBase {
            &self.base
        }

        fn entity_type(&self) -> EntityTypeRef {
            &vanilla_entities::PIG
        }
    }

    impl LivingEntity for TestPathfinderMob {
        fn living_base(&self) -> &LivingEntityBase {
            &self.living_base
        }

        fn get_health(&self) -> f32 {
            *self.health.lock()
        }

        fn set_health(&self, health: f32) {
            *self.health.lock() = health;
        }
    }

    impl Mob for TestPathfinderMob {
        fn mob_base(&self) -> &MobBase {
            &self.mob_base
        }

        fn mob_flags(&self) -> i8 {
            *self.mob_flags.lock()
        }

        fn set_mob_flags(&self, flags: i8) {
            *self.mob_flags.lock() = flags;
        }
    }

    impl PathfinderMob for TestPathfinderMob {}

    struct StaticGoal {
        controls: GoalControls,
        can_use: bool,
        can_continue: bool,
        interruptable: bool,
        requires_update_every_tick: bool,
        tick_count: Option<&'static AtomicUsize>,
        can_use_once: bool,
        panic_goal: bool,
    }

    impl StaticGoal {
        const fn new(controls: GoalControls) -> Self {
            Self {
                controls,
                can_use: true,
                can_continue: true,
                interruptable: true,
                requires_update_every_tick: false,
                tick_count: None,
                can_use_once: false,
                panic_goal: false,
            }
        }

        const fn non_interruptable(mut self) -> Self {
            self.interruptable = false;
            self
        }

        const fn with_can_continue(mut self, can_continue: bool) -> Self {
            self.can_continue = can_continue;
            self
        }

        const fn with_can_use_once(mut self) -> Self {
            self.can_use_once = true;
            self
        }

        const fn with_update_every_tick(mut self) -> Self {
            self.requires_update_every_tick = true;
            self
        }

        const fn with_tick_counter(mut self, tick_count: &'static AtomicUsize) -> Self {
            self.tick_count = Some(tick_count);
            self
        }

        const fn with_panic_goal(mut self) -> Self {
            self.panic_goal = true;
            self
        }
    }

    impl Goal for StaticGoal {
        fn controls(&self) -> GoalControls {
            self.controls
        }

        fn can_use(&mut self, _mob: &dyn PathfinderMob) -> bool {
            if self.can_use_once {
                if !self.can_use {
                    return false;
                }
                self.can_use = false;
                return true;
            }
            self.can_use
        }

        fn can_continue_to_use(&mut self, _mob: &dyn PathfinderMob) -> bool {
            self.can_continue
        }

        fn is_interruptable(&self) -> bool {
            self.interruptable
        }

        fn is_panic_goal(&self) -> bool {
            self.panic_goal
        }

        fn requires_update_every_tick(&self) -> bool {
            self.requires_update_every_tick
        }

        fn tick(&mut self, _mob: &dyn PathfinderMob) {
            if let Some(tick_count) = self.tick_count {
                tick_count.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    static RUNNING_TICK_COUNT: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn lower_priority_goal_replaces_running_goal_for_same_control() {
        let mob = TestPathfinderMob::new();
        let mut selector = GoalSelector::new();
        selector.add_goal(5, StaticGoal::new(GoalControls::MOVE));
        selector.tick(&mob);

        selector.add_goal(3, StaticGoal::new(GoalControls::MOVE));
        selector.tick(&mob);

        assert_eq!(selector.running_goal_count(), 1);
        assert!(selector.is_priority_running(3));
    }

    #[test]
    fn non_interruptable_goal_blocks_replacement() {
        let mob = TestPathfinderMob::new();
        let mut selector = GoalSelector::new();
        selector.add_goal(5, StaticGoal::new(GoalControls::MOVE).non_interruptable());
        selector.tick(&mob);

        selector.add_goal(3, StaticGoal::new(GoalControls::MOVE));
        selector.tick(&mob);

        assert_eq!(selector.running_goal_count(), 1);
        assert!(selector.is_priority_running(5));
    }

    #[test]
    fn disabled_control_stops_running_goal() {
        let mob = TestPathfinderMob::new();
        let mut selector = GoalSelector::new();
        selector.add_goal(5, StaticGoal::new(GoalControls::MOVE));
        selector.tick(&mob);

        selector.disable_control(GoalControl::Move);
        selector.tick(&mob);

        assert_eq!(selector.running_goal_count(), 0);
    }

    #[test]
    fn tick_running_goals_respects_requires_update_every_tick() {
        RUNNING_TICK_COUNT.store(0, Ordering::Relaxed);
        let mob = TestPathfinderMob::new();
        let mut selector = GoalSelector::new();
        selector.add_goal(
            5,
            StaticGoal::new(GoalControls::MOVE)
                .with_update_every_tick()
                .with_tick_counter(&RUNNING_TICK_COUNT),
        );
        selector.tick(&mob);

        selector.tick_running_goals(&mob, false);

        assert_eq!(RUNNING_TICK_COUNT.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn cleanup_stops_goal_that_can_no_longer_continue() {
        let mob = TestPathfinderMob::new();
        let mut selector = GoalSelector::new();
        selector.add_goal(
            5,
            StaticGoal::new(GoalControls::MOVE)
                .with_can_continue(false)
                .with_can_use_once(),
        );

        selector.tick(&mob);
        selector.tick(&mob);

        assert_eq!(selector.running_goal_count(), 0);
    }

    #[test]
    fn running_panic_goal_is_visible_to_pathfinder_mob() {
        let mob = TestPathfinderMob::new();
        mob.mob_base()
            .goal_selector()
            .lock()
            .add_goal(1, StaticGoal::new(GoalControls::MOVE).with_panic_goal());

        assert!(!mob.is_panicking());

        mob.mob_base().goal_selector().lock().tick(&mob);

        assert!(
            mob.mob_base()
                .goal_selector()
                .lock()
                .has_running_panic_goal()
        );
        assert!(mob.is_panicking());
    }
}
