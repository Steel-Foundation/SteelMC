use std::fmt;
use std::mem;
use std::ops::BitOr;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

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
    /// `None` while a tick has the goal checked out.
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

    /// Panics while the goal is checked out: like vanilla's goal set, the
    /// selector cannot be ticked or have goals removed from inside its own goals.
    fn goal(&self) -> &dyn Goal {
        let Some(goal) = &self.goal else {
            panic!("goal selector used from inside one of its goals");
        };
        goal.as_ref()
    }

    const fn is_panic_goal(&self) -> bool {
        self.panic_goal
    }
}

/// A goal a tick has taken out of slot `index`, with the slot's priority and running state.
struct CheckedOutGoal {
    index: usize,
    priority: i32,
    running: bool,
    goal: Box<dyn Goal>,
}

impl CheckedOutGoal {
    fn can_be_replaced_by(&self, candidate_priority: i32) -> bool {
        self.goal.is_interruptable() && candidate_priority < self.priority
    }
}

struct GoalSet {
    goals: Vec<WrappedGoal>,
    /// Storage for the checked-out goals, kept between ticks so a tick does not allocate.
    checked_out: Vec<CheckedOutGoal>,
}

/// Vanilla `GoalSelector`.
///
/// A tick checks the goals it runs out of the selector at once and returns them
/// when it ends, so their callbacks run with the selector unlocked: a goal may
/// read its own mob's selector, change the disabled controls (the rest of the
/// tick sees the change) and add goals (they take part from the next tick).
/// Removing goals or ticking the selector from a callback panics, as vanilla's
/// goal set would. A running state changes in the selector as soon as the goal
/// has started or stopped.
pub struct GoalSelector {
    set: CheckedMutex<GoalSet>,
    /// The disabled [`GoalControls`] bits, outside the lock so a tick reads
    /// them per goal without locking.
    disabled_controls: AtomicU8,
    /// Whether the selector has any goals, so a selector without goals ticks
    /// without locking. Written only while `set` is locked.
    has_goals: AtomicBool,
}

impl GoalSelector {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            set: CheckedMutex::new(GoalSet {
                goals: Vec::new(),
                checked_out: Vec::new(),
            }),
            disabled_controls: AtomicU8::new(GoalControls::EMPTY.0),
            has_goals: AtomicBool::new(false),
        }
    }

    pub fn add_goal<G>(&self, priority: i32, goal: G)
    where
        G: Goal + 'static,
    {
        let mut set = self.set.lock();
        set.goals.push(WrappedGoal::new(priority, Box::new(goal)));
        self.has_goals.store(true, Ordering::Release);
    }

    /// Stops and removes matching goals. `mob` supplies the stop-hook context.
    pub fn remove_all_goals(&self, mob: &Mob<'_>, mut predicate: impl FnMut(&dyn Goal) -> bool) {
        if !self.has_goals() {
            return;
        }
        let mut goals = self.check_out(|_| true);
        for goal in &mut goals {
            if !goal.running || !predicate(goal.goal.as_ref()) {
                continue;
            }
            goal.goal.stop(mob);
            self.set_running(goal, false);
        }
        self.check_in(goals);

        let mut set = self.set.lock();
        set.goals.retain(|wrapped| !predicate(wrapped.goal()));
        self.has_goals
            .store(!set.goals.is_empty(), Ordering::Release);
    }

    /// Stops the running goals that cannot continue, starts the ones that
    /// can, then ticks every running goal.
    pub fn tick(&self, mob: &Mob<'_>) {
        if !self.has_goals() {
            return;
        }
        let mut goals = self.check_out(|_| true);

        let disabled_controls = self.disabled_controls();
        for goal in &mut goals {
            if !goal.running {
                continue;
            }
            if !goal.goal.controls().intersects(disabled_controls)
                && goal.goal.can_continue_to_use(mob)
            {
                continue;
            }
            goal.goal.stop(mob);
            self.set_running(goal, false);
        }

        for index in 0..goals.len() {
            // Read per candidate: the callbacks before it may have changed them.
            let disabled_controls = self.disabled_controls();
            if !Self::can_start_goal(&goals, index, disabled_controls)
                || !goals[index].goal.can_use(mob)
            {
                continue;
            }

            for control in goals[index].goal.controls().iter() {
                // The candidate is not running, so the search never finds it.
                let Some(current_index) = Self::running_goal_index_for(&goals, control) else {
                    continue;
                };
                let current = &mut goals[current_index];
                current.goal.stop(mob);
                self.set_running(current, false);
            }
            let candidate = &mut goals[index];
            candidate.goal.start(mob);
            self.set_running(candidate, true);
        }

        for goal in &mut goals {
            if goal.running {
                goal.goal.tick(mob);
            }
        }
        self.check_in(goals);
    }

    pub fn tick_running_goals(&self, mob: &Mob<'_>, force_tick_all_running_goals: bool) {
        if !self.has_goals() {
            return;
        }
        let mut goals = self.check_out(|wrapped| {
            wrapped.is_running()
                && (force_tick_all_running_goals || wrapped.goal().requires_update_every_tick())
        });
        if goals.is_empty() {
            return;
        }
        for goal in &mut goals {
            goal.goal.tick(mob);
        }
        self.check_in(goals);
    }

    /// Takes the goals `select` accepts out of the selector, in slot order, so
    /// their callbacks can run with the selector unlocked.
    fn check_out(&self, mut select: impl FnMut(&WrappedGoal) -> bool) -> Vec<CheckedOutGoal> {
        let mut set = self.set.lock();
        let set = &mut *set;
        let mut checked_out = mem::take(&mut set.checked_out);
        for (index, wrapped) in set.goals.iter_mut().enumerate() {
            if !select(wrapped) {
                continue;
            }
            let Some(goal) = wrapped.goal.take() else {
                panic!("goal selector used from inside one of its goals");
            };
            checked_out.push(CheckedOutGoal {
                index,
                priority: wrapped.priority,
                running: wrapped.running,
                goal,
            });
        }
        if checked_out.is_empty() {
            // Keeps the storage for the next tick.
            set.checked_out = checked_out;
            return Vec::new();
        }
        checked_out
    }

    /// Returns checked-out goals to their slots. Slots never move while goals
    /// are checked out: goals added meanwhile go after them and removal panics.
    fn check_in(&self, mut checked_out: Vec<CheckedOutGoal>) {
        let mut set = self.set.lock();
        while let Some(goal) = checked_out.pop() {
            set.goals[goal.index].goal = Some(goal.goal);
        }
        set.checked_out = checked_out;
    }

    /// Sets a checked-out goal's running state, in the selector too so the
    /// goals' callbacks see it.
    fn set_running(&self, goal: &mut CheckedOutGoal, running: bool) {
        goal.running = running;
        self.set.lock().goals[goal.index].running = running;
    }

    fn has_goals(&self) -> bool {
        self.has_goals.load(Ordering::Acquire)
    }

    fn disabled_controls(&self) -> GoalControls {
        GoalControls(self.disabled_controls.load(Ordering::Acquire))
    }

    pub fn disable_control(&self, control: GoalControl) {
        self.disabled_controls
            .fetch_or(GoalControls::from_control(control).0, Ordering::AcqRel);
    }

    pub fn enable_control(&self, control: GoalControl) {
        self.disabled_controls
            .fetch_and(!GoalControls::from_control(control).0, Ordering::AcqRel);
    }

    pub fn set_control(&self, control: GoalControl, enabled: bool) {
        if enabled {
            self.enable_control(control);
        } else {
            self.disable_control(control);
        }
    }

    #[must_use]
    pub fn running_goal_count(&self) -> usize {
        self.set
            .lock()
            .goals
            .iter()
            .filter(|goal| goal.is_running())
            .count()
    }

    #[must_use]
    pub fn available_goal_count(&self) -> usize {
        self.set.lock().goals.len()
    }

    #[must_use]
    pub(crate) fn has_running_panic_goal(&self) -> bool {
        self.set
            .lock()
            .goals
            .iter()
            .any(|goal| goal.is_running() && goal.is_panic_goal())
    }

    #[cfg(test)]
    #[must_use]
    pub(crate) fn available_goal_priorities(&self) -> Vec<i32> {
        self.set
            .lock()
            .goals
            .iter()
            .map(|goal| goal.priority)
            .collect()
    }

    #[cfg(test)]
    #[must_use]
    pub(crate) fn is_control_disabled(&self, control: GoalControl) -> bool {
        self.disabled_controls().contains(control)
    }

    fn can_start_goal(
        goals: &[CheckedOutGoal],
        index: usize,
        disabled_controls: GoalControls,
    ) -> bool {
        let goal = &goals[index];
        !goal.running
            && !goal.goal.controls().intersects(disabled_controls)
            && Self::goal_can_be_replaced_for_all_controls(goals, goal)
    }

    fn goal_can_be_replaced_for_all_controls(
        goals: &[CheckedOutGoal],
        candidate: &CheckedOutGoal,
    ) -> bool {
        for control in candidate.goal.controls().iter() {
            if let Some(current_index) = Self::running_goal_index_for(goals, control)
                && !goals[current_index].can_be_replaced_by(candidate.priority)
            {
                return false;
            }
        }
        true
    }

    fn running_goal_index_for(goals: &[CheckedOutGoal], control: GoalControl) -> Option<usize> {
        goals
            .iter()
            .position(|goal| goal.running && goal.goal.controls().contains(control))
    }

    #[cfg(test)]
    fn is_priority_running(&self, priority: i32) -> bool {
        self.set
            .lock()
            .goals
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
            .field("available_goals", &self.available_goal_count())
            .field("running_goals", &self.running_goal_count())
            .field("disabled_controls", &self.disabled_controls())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::EntityTable;
    use crate::entity::LivingProps;
    use crate::entity::LivingTable;
    use crate::entity::MobTable;
    use crate::entity::WeakSelf;
    use crate::test_support::synced_with_health;
    use std::sync::Arc;

    use std::sync::Weak;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use glam::DVec3;
    use steel_registry::entity_type::EntityTypeRef;
    use steel_registry::{init_vanilla_registry, vanilla_entities};
    use steel_utils::locks::{CheckedMutex, SyncMutex};

    use super::*;

    use crate::entity::EntityProps;
    use crate::entity::component::Has;
    use crate::entity::prelude::*;
    use crate::entity::{Entity, EntityBase, LivingEntityBase, Mob, MobBase};
    use crate::entity::{EntitySyncedData, MobProps, mob};
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
                living_base: LivingEntityBase::without_synced_data(&vanilla_entities::PIG),
                mob_base: MobBase::new(|_| {}),
                entity_data: CheckedMutex::new(synced_with_health(MobEntityData::new(), 10.0)),
            }
        }
    }

    crate::entity::impl_test_downcast_type!(TestPathfinderMob);

    impl MobTable for TestPathfinderMob {
        const MOB: MobProps<Self> = MobProps {
            is_pathfinder: |_| true,
            ..MobProps::new()
        };
    }

    impl EntityTable for TestPathfinderMob {
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
    }

    impl LivingTable for TestPathfinderMob {
        const LIVING: LivingProps<Self> = LivingProps {
            // Tests drive the goals and navigation themselves.
            server_ai_step: |_| {},
            ..mob::living_props()
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
        let selector = GoalSelector::new();
        selector.add_goal(5, StaticGoal::new(GoalControls::MOVE));
        selector.tick(&mob.mob());

        selector.add_goal(3, StaticGoal::new(GoalControls::MOVE));
        selector.tick(&mob.mob());
        assert_eq!(selector.running_goal_count(), 1);
        assert!(selector.is_priority_running(3));
    }

    #[test]
    fn non_interruptable_goal_blocks_replacement() {
        let mob = Arc::new(TestPathfinderMob::new());
        let selector = GoalSelector::new();
        selector.add_goal(5, StaticGoal::new(GoalControls::MOVE).non_interruptable());
        selector.tick(&mob.mob());

        selector.add_goal(3, StaticGoal::new(GoalControls::MOVE));
        selector.tick(&mob.mob());
        assert_eq!(selector.running_goal_count(), 1);
        assert!(selector.is_priority_running(5));
    }

    #[test]
    fn disabled_control_stops_running_goal() {
        let mob = Arc::new(TestPathfinderMob::new());
        let selector = GoalSelector::new();
        selector.add_goal(5, StaticGoal::new(GoalControls::MOVE));
        selector.tick(&mob.mob());

        selector.disable_control(GoalControl::Move);
        selector.tick(&mob.mob());

        assert_eq!(selector.running_goal_count(), 0);
    }

    #[test]
    fn tick_running_goals_respects_requires_update_every_tick() {
        RUNNING_TICK_COUNT.store(0, Ordering::Relaxed);
        let mob = Arc::new(TestPathfinderMob::new());
        let selector = GoalSelector::new();
        selector.add_goal(
            5,
            StaticGoal::new(GoalControls::MOVE)
                .with_update_every_tick()
                .with_tick_counter(&RUNNING_TICK_COUNT),
        );
        selector.tick(&mob.mob());

        selector.tick_running_goals(&mob.mob(), false);

        assert_eq!(RUNNING_TICK_COUNT.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn cleanup_stops_goal_that_can_no_longer_continue() {
        let mob = Arc::new(TestPathfinderMob::new());
        let selector = GoalSelector::new();
        selector.add_goal(
            5,
            StaticGoal::new(GoalControls::MOVE)
                .with_can_continue(false)
                .with_can_use_once(),
        );

        selector.tick(&mob.mob());
        selector.tick(&mob.mob());

        assert_eq!(selector.running_goal_count(), 0);
    }

    #[test]
    fn running_panic_goal_is_visible_to_pathfinder_mob() {
        let mob = Arc::new(TestPathfinderMob::new());
        let selector = mob.mob().mob_base().goal_selector();
        selector.add_goal(1, StaticGoal::new(GoalControls::MOVE).with_panic_goal());

        assert!(!mob.mob().is_panicking());

        selector.tick(&mob.mob());

        assert!(selector.has_running_panic_goal());
        assert!(mob.mob().is_panicking());
    }

    /// A goal reads its own mob's selector while it ticks, as vanilla goals
    /// may; the selector is unlocked around every callback.
    #[test]
    fn goal_can_read_its_own_selector_while_ticking() {
        let mob = Arc::new(TestPathfinderMob::new());
        let selector = mob.mob().mob_base().goal_selector();
        selector.add_goal(1, StaticGoal::new(GoalControls::MOVE).with_panic_goal());
        selector.add_goal(
            2,
            StaticGoal::new(GoalControls::LOOK).asserting_panicking_while_ticking(),
        );

        selector.tick(&mob.mob());
        selector.tick_running_goals(&mob.mob(), true);

        assert_eq!(selector.running_goal_count(), 2);
    }

    type CallbackLog = Arc<SyncMutex<Vec<(&'static str, &'static str, usize)>>>;

    /// Logs its own selector's running goal count from each callback.
    struct RunningCountProbe {
        name: &'static str,
        controls: GoalControls,
        log: CallbackLog,
    }

    impl RunningCountProbe {
        fn record(&self, mob: &Mob<'_>, callback: &'static str) {
            let running = mob.mob_base().goal_selector().running_goal_count();
            self.log.lock().push((self.name, callback, running));
        }
    }

    impl Goal for RunningCountProbe {
        fn controls(&self) -> GoalControls {
            self.controls
        }

        fn can_use(&mut self, mob: &Mob<'_>) -> bool {
            self.record(mob, "can_use");
            true
        }

        fn can_continue_to_use(&mut self, mob: &Mob<'_>) -> bool {
            self.record(mob, "can_continue_to_use");
            true
        }

        fn start(&mut self, mob: &Mob<'_>) {
            self.record(mob, "start");
        }

        fn stop(&mut self, mob: &Mob<'_>) {
            self.record(mob, "stop");
        }

        fn tick(&mut self, mob: &Mob<'_>) {
            self.record(mob, "tick");
        }
    }

    /// A goal is running from the end of its `start` to the end of its `stop`,
    /// as each callback sees it.
    #[test]
    fn callbacks_see_running_state_change_as_goals_start_and_stop() {
        let mob = Arc::new(TestPathfinderMob::new());
        let selector = mob.mob().mob_base().goal_selector();
        let log = CallbackLog::default();
        selector.add_goal(
            5,
            RunningCountProbe {
                name: "low",
                controls: GoalControls::MOVE,
                log: Arc::clone(&log),
            },
        );
        selector.tick(&mob.mob());
        log.lock().clear();

        selector.add_goal(
            3,
            RunningCountProbe {
                name: "high",
                controls: GoalControls::MOVE,
                log: Arc::clone(&log),
            },
        );
        selector.tick(&mob.mob());

        assert_eq!(
            *log.lock(),
            [
                ("low", "can_continue_to_use", 1),
                ("high", "can_use", 1),
                ("low", "stop", 1),
                ("high", "start", 0),
                ("high", "tick", 1),
            ]
        );
    }

    /// Disables MOVE on its own selector from `can_use`, then declines to start.
    struct DisablesMove;

    impl Goal for DisablesMove {
        fn controls(&self) -> GoalControls {
            GoalControls::EMPTY
        }

        fn can_use(&mut self, mob: &Mob<'_>) -> bool {
            mob.mob_base()
                .goal_selector()
                .disable_control(GoalControl::Move);
            false
        }
    }

    #[test]
    fn control_disabled_from_a_callback_applies_to_later_goals_in_the_same_tick() {
        let mob = Arc::new(TestPathfinderMob::new());
        let selector = mob.mob().mob_base().goal_selector();
        selector.add_goal(1, DisablesMove);
        selector.add_goal(2, StaticGoal::new(GoalControls::MOVE));

        selector.tick(&mob.mob());

        assert_eq!(selector.running_goal_count(), 0);
    }

    /// Adds a LOOK goal to its own selector when it starts.
    struct AddsGoalOnStart;

    impl Goal for AddsGoalOnStart {
        fn controls(&self) -> GoalControls {
            GoalControls::EMPTY
        }

        fn can_use(&mut self, _mob: &Mob<'_>) -> bool {
            true
        }

        fn start(&mut self, mob: &Mob<'_>) {
            mob.mob_base()
                .goal_selector()
                .add_goal(9, StaticGoal::new(GoalControls::LOOK));
        }
    }

    #[test]
    fn goal_added_from_a_callback_takes_part_from_the_next_tick() {
        let mob = Arc::new(TestPathfinderMob::new());
        let selector = mob.mob().mob_base().goal_selector();
        selector.add_goal(1, AddsGoalOnStart);
        selector.add_goal(2, StaticGoal::new(GoalControls::MOVE));

        selector.tick(&mob.mob());

        assert_eq!(selector.available_goal_priorities(), [1, 2, 9]);
        assert!(!selector.is_priority_running(9));
        assert_eq!(selector.running_goal_count(), 2);

        selector.tick(&mob.mob());

        assert!(selector.is_priority_running(9));
        assert_eq!(selector.running_goal_count(), 3);
    }

    /// Removes every goal of its own selector while it ticks.
    struct RemovesGoalsOnTick;

    impl Goal for RemovesGoalsOnTick {
        fn controls(&self) -> GoalControls {
            GoalControls::EMPTY
        }

        fn can_use(&mut self, _mob: &Mob<'_>) -> bool {
            true
        }

        fn tick(&mut self, mob: &Mob<'_>) {
            mob.mob_base()
                .goal_selector()
                .remove_all_goals(mob, |_| true);
        }
    }

    #[test]
    #[should_panic(expected = "goal selector used from inside one of its goals")]
    fn removing_goals_from_a_callback_panics() {
        let mob = Arc::new(TestPathfinderMob::new());
        let selector = mob.mob().mob_base().goal_selector();
        selector.add_goal(1, RemovesGoalsOnTick);

        selector.tick(&mob.mob());
    }

    #[test]
    fn remove_all_goals_stops_matching_running_goals_and_keeps_others() {
        let mob = Arc::new(TestPathfinderMob::new());
        let selector = GoalSelector::new();
        selector.add_goal(3, StaticGoal::new(GoalControls::MOVE));
        selector.add_goal(7, StaticGoal::new(GoalControls::LOOK));
        selector.tick(&mob.mob());
        assert_eq!(selector.running_goal_count(), 2);

        selector.remove_all_goals(&mob.mob(), |goal| goal.controls() == GoalControls::MOVE);

        {
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

        selector.tick(&mob.mob());
        assert_eq!(selector.running_goal_count(), 1);
        assert!(selector.is_priority_running(7));
    }

    #[test]
    fn pathfinder_mob_remove_free_will_clears_all_goals() {
        let mob = Arc::new(TestPathfinderMob::new());
        let selector = mob.mob().mob_base().goal_selector();
        selector.add_goal(1, StaticGoal::new(GoalControls::MOVE));
        selector.add_goal(2, StaticGoal::new(GoalControls::LOOK));
        selector.tick(&mob.mob());
        assert_eq!(selector.running_goal_count(), 2);

        mob.mob().remove_free_will();
        assert_eq!(selector.available_goal_count(), 0);
        assert_eq!(selector.running_goal_count(), 0);
    }
}
