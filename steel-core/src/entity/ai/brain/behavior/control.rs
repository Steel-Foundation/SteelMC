//! The start/tick/stop lifecycle the brain drives.

use super::Behavior;
use crate::entity::ai::brain::context::BrainContext;
use crate::entity::ai::brain::memory::MemoryModuleTypeRef;

/// Whether a behavior is currently running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BehaviorStatus {
    /// Not running.
    Stopped,
    /// Started and not yet stopped.
    Running,
}

/// What the brain stores and drives.
///
/// Mirrors vanilla `BehaviorControl`.
pub trait BehaviorControl: Send {
    /// Current lifecycle state.
    fn status(&self) -> BehaviorStatus;

    /// Visits every memory this behavior needs, so the brain can register them.
    fn visit_required_memories(&self, visit: &mut dyn FnMut(MemoryModuleTypeRef));

    /// Starts the behavior if its conditions hold, returning whether it did.
    fn try_start(&mut self, context: &mut BrainContext<'_>) -> bool;

    /// Ticks the behavior, or stops it if it timed out or gave up.
    fn tick_or_stop(&mut self, context: &mut BrainContext<'_>);

    /// Stops the behavior unconditionally.
    fn do_stop(&mut self, context: &mut BrainContext<'_>);

    /// Name used in brain debug output.
    fn debug_name(&self) -> &'static str;
}

/// A [`Behavior`] plus the lifecycle state vanilla keeps on `Behavior` itself.
pub struct TimedBehavior<B: Behavior> {
    behavior: B,
    status: BehaviorStatus,
    end_timestamp: i64,
}

impl<B: Behavior> TimedBehavior<B> {
    /// Wraps `behavior`, stopped and not yet scheduled.
    pub const fn new(behavior: B) -> Self {
        Self {
            behavior,
            status: BehaviorStatus::Stopped,
            end_timestamp: 0,
        }
    }

    /// Vanilla `Behavior.timedOut`.
    const fn timed_out(&self, time: i64) -> bool {
        time > self.end_timestamp
    }

    fn has_required_memories(&self, context: &BrainContext<'_>) -> bool {
        self.behavior
            .entry_condition()
            .iter()
            .all(|&(memory, status)| context.memories.check(memory, status))
    }
}

impl<B: Behavior> BehaviorControl for TimedBehavior<B> {
    fn status(&self) -> BehaviorStatus {
        self.status
    }

    fn visit_required_memories(&self, visit: &mut dyn FnMut(MemoryModuleTypeRef)) {
        for &(memory, _) in self.behavior.entry_condition() {
            visit(memory);
        }
    }

    /// Vanilla `Behavior.tryStart`.
    fn try_start(&mut self, context: &mut BrainContext<'_>) -> bool {
        if !self.has_required_memories(context)
            || !self.behavior.check_extra_start_conditions(context)
        {
            return false;
        }

        self.status = BehaviorStatus::Running;
        self.end_timestamp = context.time + i64::from(self.behavior.duration().roll());
        self.behavior.start(context);
        true
    }

    fn tick_or_stop(&mut self, context: &mut BrainContext<'_>) {
        if self.timed_out(context.time) || !self.behavior.can_still_use(context) {
            self.do_stop(context);
            return;
        }

        self.behavior.tick(context);
    }

    fn do_stop(&mut self, context: &mut BrainContext<'_>) {
        self.status = BehaviorStatus::Stopped;
        self.behavior.stop(context);
    }

    fn debug_name(&self) -> &'static str {
        self.behavior.debug_name()
    }
}

/// Turns a [`Behavior`] into the [`BehaviorControl`] a brain stores.
pub trait BehaviorExt: Behavior + Sized + 'static {
    /// Wraps this behavior for registration on a brain.
    fn control(self) -> Box<dyn BehaviorControl> {
        Box::new(TimedBehavior::new(self))
    }
}

impl<B: Behavior + 'static> BehaviorExt for B {}
