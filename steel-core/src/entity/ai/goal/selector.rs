use std::fmt;
use std::ops::BitOr;

use steel_utils::locks::CheckedMutex;

use super::reduced_tick_delay;
use crate::entity::Mob;

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

    fn can_use(&mut self, mob: &Mob<'_>) -> bool;

    fn can_continue_to_use(&mut self, mob: &Mob<'_>) -> bool {
        self.can_use(mob)
    }

    fn is_interruptable(&self) -> bool {
        true
    }

    fn is_panic_goal(&self) -> bool {
        false
    }

    fn start(&mut self, _mob: &Mob<'_>) {}

    fn stop(&mut self, _mob: &Mob<'_>) {}

    fn requires_update_every_tick(&self) -> bool {
        false
    }

    /// `Goal.adjustedTickDelay`: goals ticked every tick keep the full delay.
    fn adjusted_tick_delay(&self, ticks: i32) -> i32 {
        if self.requires_update_every_tick() {
            ticks
        } else {
            reduced_tick_delay(ticks)
        }
    }

    fn tick(&mut self, _mob: &Mob<'_>) {}
}

struct WrappedGoal {
    priority: i32,
    running: bool,
    /// Vanilla's `instanceof PanicGoal`, readable while the goal is checked out.
    panic_goal: bool,
    /// `None` while the goal runs a callback outside the selector's lock.
    goal: Option<Box<dyn Goal>>,
}

impl WrappedGoal {
    fn new(priority: i32, goal: Box<dyn Goal>) -> Self {
        Self {
            priority,
            running: false,
            panic_goal: goal.is_panic_goal(),
            goal: Some(goal),
        }
    }

    const fn is_running(&self) -> bool {
        self.running
    }

    /// Panics while the goal runs a callback: like vanilla's goal set, the
    /// selector cannot be inspected or changed from inside its own goals.
    fn goal(&self) -> &dyn Goal {
        let Some(goal) = &self.goal else {
            panic!("goal selector used from inside one of its goals");
        };
        goal.as_ref()
    }

    fn controls(&self) -> GoalControls {
        self.goal().controls()
    }

    fn can_be_replaced_by(&self, candidate_priority: i32) -> bool {
        self.goal().is_interruptable() && candidate_priority < self.priority
    }

    fn requires_update_every_tick(&self) -> bool {
        self.goal().requires_update_every_tick()
    }

    const fn is_panic_goal(&self) -> bool {
        self.panic_goal
    }
}

/// Vanilla `GoalSelector`.
///
/// Goals run their callbacks checked out of the selector, with its
/// [`CheckedMutex`] unlocked, so a goal may read its own mob's selector.
/// Adding goals from a callback works; removing them panics, as vanilla's
/// goal set would.
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

    /// Stops and removes matching goals. `mob` supplies the stop-hook context.
    pub fn remove_all_goals(
        selector: &CheckedMutex<Self>,
        mob: &Mob<'_>,
        mut predicate: impl FnMut(&dyn Goal) -> bool,
    ) {
        let count = selector.lock().available_goals.len();
        for index in 0..count {
            let Some(mut goal) = Self::check_out(selector, index, |wrapped| {
                wrapped.is_running() && predicate(wrapped.goal())
            }) else {
                continue;
            };
            goal.stop(mob);
            Self::check_in(selector, index, goal, false);
        }
        selector
            .lock()
            .available_goals
            .retain(|wrapped| !predicate(wrapped.goal()));
    }

    /// Stops the running goals that cannot continue, starts the ones that
    /// can, then ticks every running goal.
    pub fn tick(selector: &CheckedMutex<Self>, mob: &Mob<'_>) {
        let (count, disabled_controls) = {
            let selector = selector.lock();
            (selector.available_goals.len(), selector.disabled_controls)
        };

        for index in 0..count {
            let Some(mut goal) = Self::check_out(selector, index, WrappedGoal::is_running) else {
                continue;
            };
            let keep =
                !goal.controls().intersects(disabled_controls) && goal.can_continue_to_use(mob);
            if !keep {
                goal.stop(mob);
            }
            Self::check_in(selector, index, goal, keep);
        }

        for index in 0..count {
            if !selector.lock().can_start_goal(index) {
                continue;
            }
            let Some(mut goal) = Self::check_out(selector, index, |_| true) else {
                continue;
            };
            if !goal.can_use(mob) {
                Self::check_in(selector, index, goal, false);
                continue;
            }

            for control in goal.controls().iter() {
                // The checked-out goal is not running, so the search never
                // needs its controls.
                let Some(current_index) = selector.lock().running_goal_index_for(control) else {
                    continue;
                };
                let Some(mut current) =
                    Self::check_out(selector, current_index, WrappedGoal::is_running)
                else {
                    continue;
                };
                current.stop(mob);
                Self::check_in(selector, current_index, current, false);
            }
            goal.start(mob);
            Self::check_in(selector, index, goal, true);
        }

        Self::tick_running_goals(selector, mob, true);
    }

    pub fn tick_running_goals(
        selector: &CheckedMutex<Self>,
        mob: &Mob<'_>,
        force_tick_all_running_goals: bool,
    ) {
        let count = selector.lock().available_goals.len();
        for index in 0..count {
            let Some(mut goal) = Self::check_out(selector, index, |wrapped| {
                wrapped.is_running()
                    && (force_tick_all_running_goals || wrapped.requires_update_every_tick())
            }) else {
                continue;
            };
            goal.tick(mob);
            Self::check_in(selector, index, goal, true);
        }
    }

    /// Takes the goal at `index` out of the selector when `select` accepts
    /// it, so its callback can run with the selector unlocked.
    fn check_out(
        selector: &CheckedMutex<Self>,
        index: usize,
        select: impl FnOnce(&WrappedGoal) -> bool,
    ) -> Option<Box<dyn Goal>> {
        let mut selector = selector.lock();
        let wrapped = selector.available_goals.get_mut(index)?;
        if !select(wrapped) {
            return None;
        }
        let Some(goal) = wrapped.goal.take() else {
            panic!("goal selector ticked from inside one of its goals");
        };
        Some(goal)
    }

    /// Returns a checked-out goal with its new running state.
    fn check_in(selector: &CheckedMutex<Self>, index: usize, goal: Box<dyn Goal>, running: bool) {
        let mut selector = selector.lock();
        let wrapped = &mut selector.available_goals[index];
        wrapped.goal = Some(goal);
        wrapped.running = running;
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
    use crate::entity::LivingProps;
    use crate::entity::WeakSelf;
    use crate::test_support::synced_with_health;
    use std::sync::Arc;

    use std::sync::Weak;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use glam::DVec3;
    use steel_registry::entity_type::EntityTypeRef;
    use steel_registry::{init_vanilla_registry, vanilla_entities};
    use steel_utils::locks::CheckedMutex;

    use super::*;

    use crate::entity::component::Has;
    use crate::entity::component::definition::{Definition, DefinitionEntries};
    use crate::entity::prelude::*;
    use crate::entity::{Entity, EntityBase, LivingEntityBase, Mob, MobBase};
    use crate::entity::{EntityHooks, EntityProps};
    use crate::entity::{EntitySyncedData, MobProps, mob};
    use std::sync::LazyLock;
    use steel_registry::vanilla_entity_data::MobEntityData;
    use steel_utils::{DowncastType as _, DowncastTypeKey, ErasedType};

    struct TestPathfinderMob {
        base: EntityBase,
        living_base: LivingEntityBase,
        mob_base: MobBase,
        entity_data: CheckedMutex<MobEntityData>,
    }

    impl TestPathfinderMob {
        fn new() -> Self {
            init_vanilla_registry();
            Self {
                base: EntityBase::new(
                    1,
                    DVec3::ZERO,
                    vanilla_entities::PIG.dimensions,
                    Weak::new(),
                    WeakSelf::detached(),
                ),
                living_base: LivingEntityBase::without_synced_data(
                    &vanilla_entities::PIG,
                    &TestPathfinderMob::LIVING,
                ),
                mob_base: MobBase::new(&TestPathfinderMob::MOB, TestPathfinderMob::definition()),
                entity_data: CheckedMutex::new(synced_with_health(MobEntityData::new(), 10.0)),
            }
        }
    }

    crate::entity::impl_test_downcast_type!(TestPathfinderMob);

    impl TestPathfinderMob {
        const MOB: MobProps<Self> = MobProps {
            is_pathfinder: |_| true,
            ..MobProps::new()
        };

        fn definition() -> &'static Definition<Self> {
            static DEFINITION: LazyLock<Definition<TestPathfinderMob>> =
                LazyLock::new(mob::stages::definition);
            &DEFINITION
        }
    }

    impl TestPathfinderMob {
        const ENTITY: EntityProps<Self> = EntityProps {
            ..mob::entity_props()
        };
    }

    impl Entity for TestPathfinderMob {
        fn erased_component(&self, key: DowncastTypeKey) -> Option<&dyn ErasedType> {
            match key {
                MobBase::TYPE_KEY => Some(&self.mob_base),
                LivingEntityBase::TYPE_KEY => Some(&self.living_base),
                _ => None,
            }
        }

        fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
            Some(&self.entity_data)
        }

        fn base(&self) -> &EntityBase {
            &self.base
        }

        fn entity_type(&self) -> EntityTypeRef {
            &vanilla_entities::PIG
        }

        fn entity_hooks(&self) -> &'static dyn EntityHooks {
            &Self::ENTITY
        }

        fn entity_definition(&self) -> &'static dyn DefinitionEntries {
            Self::definition()
        }
    }

    impl TestPathfinderMob {
        const LIVING: LivingProps<Self> = LivingProps {
            ..LivingProps::new()
        };
    }

    impl Has<LivingEntityBase> for TestPathfinderMob {
        fn component(&self) -> &LivingEntityBase {
            &self.living_base
        }
    }

    impl Has<MobBase> for TestPathfinderMob {
        fn component(&self) -> &MobBase {
            &self.mob_base
        }
    }

    struct StaticGoal {
        controls: GoalControls,
        can_use: bool,
        can_continue: bool,
        interruptable: bool,
        requires_update_every_tick: bool,
        tick_count: Option<&'static AtomicUsize>,
        can_use_once: bool,
        panic_goal: bool,
        asserts_panicking: bool,
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
                asserts_panicking: false,
            }
        }

        const fn asserting_panicking_while_ticking(mut self) -> Self {
            self.asserts_panicking = true;
            self
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

        fn can_use(&mut self, _mob: &Mob<'_>) -> bool {
            if self.can_use_once {
                if !self.can_use {
                    return false;
                }
                self.can_use = false;
                return true;
            }
            self.can_use
        }

        fn can_continue_to_use(&mut self, _mob: &Mob<'_>) -> bool {
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

        fn tick(&mut self, mob: &Mob<'_>) {
            if let Some(tick_count) = self.tick_count {
                tick_count.fetch_add(1, Ordering::Relaxed);
            }
            if self.asserts_panicking {
                assert!(mob.is_panicking(), "a panic goal runs beside this one");
            }
        }
    }

    static RUNNING_TICK_COUNT: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn lower_priority_goal_replaces_running_goal_for_same_control() {
        let mob = Arc::new(TestPathfinderMob::new());
        let selector = CheckedMutex::new(GoalSelector::new());
        selector
            .lock()
            .add_goal(5, StaticGoal::new(GoalControls::MOVE));
        GoalSelector::tick(&selector, &mob.mob());

        selector
            .lock()
            .add_goal(3, StaticGoal::new(GoalControls::MOVE));
        GoalSelector::tick(&selector, &mob.mob());

        let selector = selector.lock();
        assert_eq!(selector.running_goal_count(), 1);
        assert!(selector.is_priority_running(3));
    }

    #[test]
    fn non_interruptable_goal_blocks_replacement() {
        let mob = Arc::new(TestPathfinderMob::new());
        let selector = CheckedMutex::new(GoalSelector::new());
        selector
            .lock()
            .add_goal(5, StaticGoal::new(GoalControls::MOVE).non_interruptable());
        GoalSelector::tick(&selector, &mob.mob());

        selector
            .lock()
            .add_goal(3, StaticGoal::new(GoalControls::MOVE));
        GoalSelector::tick(&selector, &mob.mob());

        let selector = selector.lock();
        assert_eq!(selector.running_goal_count(), 1);
        assert!(selector.is_priority_running(5));
    }

    #[test]
    fn disabled_control_stops_running_goal() {
        let mob = Arc::new(TestPathfinderMob::new());
        let selector = CheckedMutex::new(GoalSelector::new());
        selector
            .lock()
            .add_goal(5, StaticGoal::new(GoalControls::MOVE));
        GoalSelector::tick(&selector, &mob.mob());

        selector.lock().disable_control(GoalControl::Move);
        GoalSelector::tick(&selector, &mob.mob());

        assert_eq!(selector.lock().running_goal_count(), 0);
    }

    #[test]
    fn tick_running_goals_respects_requires_update_every_tick() {
        RUNNING_TICK_COUNT.store(0, Ordering::Relaxed);
        let mob = Arc::new(TestPathfinderMob::new());
        let selector = CheckedMutex::new(GoalSelector::new());
        selector.lock().add_goal(
            5,
            StaticGoal::new(GoalControls::MOVE)
                .with_update_every_tick()
                .with_tick_counter(&RUNNING_TICK_COUNT),
        );
        GoalSelector::tick(&selector, &mob.mob());

        GoalSelector::tick_running_goals(&selector, &mob.mob(), false);

        assert_eq!(RUNNING_TICK_COUNT.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn cleanup_stops_goal_that_can_no_longer_continue() {
        let mob = Arc::new(TestPathfinderMob::new());
        let selector = CheckedMutex::new(GoalSelector::new());
        selector.lock().add_goal(
            5,
            StaticGoal::new(GoalControls::MOVE)
                .with_can_continue(false)
                .with_can_use_once(),
        );

        GoalSelector::tick(&selector, &mob.mob());
        GoalSelector::tick(&selector, &mob.mob());

        assert_eq!(selector.lock().running_goal_count(), 0);
    }

    #[test]
    fn running_panic_goal_is_visible_to_pathfinder_mob() {
        let mob = Arc::new(TestPathfinderMob::new());
        let selector = mob.mob().mob_base().goal_selector();
        selector
            .lock()
            .add_goal(1, StaticGoal::new(GoalControls::MOVE).with_panic_goal());

        assert!(!mob.mob().is_panicking());

        GoalSelector::tick(selector, &mob.mob());

        assert!(selector.lock().has_running_panic_goal());
        assert!(mob.mob().is_panicking());
    }

    /// A goal reads its own mob's selector while it ticks, as vanilla goals
    /// may; the selector is unlocked around every callback.
    #[test]
    fn goal_can_read_its_own_selector_while_ticking() {
        let mob = Arc::new(TestPathfinderMob::new());
        let selector = mob.mob().mob_base().goal_selector();
        {
            let mut selector = selector.lock();
            selector.add_goal(1, StaticGoal::new(GoalControls::MOVE).with_panic_goal());
            selector.add_goal(
                2,
                StaticGoal::new(GoalControls::LOOK).asserting_panicking_while_ticking(),
            );
        }

        GoalSelector::tick(selector, &mob.mob());
        GoalSelector::tick_running_goals(selector, &mob.mob(), true);

        assert_eq!(selector.lock().running_goal_count(), 2);
    }

    #[test]
    fn remove_all_goals_stops_matching_running_goals_and_keeps_others() {
        let mob = Arc::new(TestPathfinderMob::new());
        let selector = CheckedMutex::new(GoalSelector::new());
        {
            let mut selector = selector.lock();
            selector.add_goal(3, StaticGoal::new(GoalControls::MOVE));
            selector.add_goal(7, StaticGoal::new(GoalControls::LOOK));
        }
        GoalSelector::tick(&selector, &mob.mob());
        assert_eq!(selector.lock().running_goal_count(), 2);

        GoalSelector::remove_all_goals(&selector, &mob.mob(), |goal| {
            goal.controls() == GoalControls::MOVE
        });

        {
            let selector = selector.lock();
            assert_eq!(selector.available_goal_count(), 1);
            assert!(
                selector.is_priority_running(7),
                "the non-matching goal must keep running through removal"
            );
            assert!(
                !selector.is_priority_running(3),
                "the removed goal must be gone entirely"
            );
        }

        GoalSelector::tick(&selector, &mob.mob());
        let selector = selector.lock();
        assert_eq!(selector.running_goal_count(), 1);
        assert!(selector.is_priority_running(7));
    }

    #[test]
    fn pathfinder_mob_remove_free_will_clears_all_goals() {
        let mob = Arc::new(TestPathfinderMob::new());
        let selector = mob.mob().mob_base().goal_selector();
        {
            let mut selector = selector.lock();
            selector.add_goal(1, StaticGoal::new(GoalControls::MOVE));
            selector.add_goal(2, StaticGoal::new(GoalControls::LOOK));
        }
        GoalSelector::tick(selector, &mob.mob());
        assert_eq!(selector.lock().running_goal_count(), 2);

        mob.mob().remove_free_will();

        let selector = selector.lock();
        assert_eq!(selector.available_goal_count(), 0);
        assert_eq!(selector.running_goal_count(), 0);
    }
}
