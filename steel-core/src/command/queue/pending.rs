//! Start-of-tick queue for suspended command executions.

use std::{collections::VecDeque, mem};

use rustc_hash::FxHashMap;

use super::super::{
    execution::{
        CommandExecutionContext, CommandSuspensionOrder, ExecutionCommandSource, ExecutionStop,
    },
    sender::{CommandExecutionOwner, CommandSenderKey},
};

/// Maximum retained command executions polled before new command requests in one tick.
pub(crate) const COMMAND_RESUMPTIONS_PER_TICK: usize = 128;

/// Suspended command executions owned by the server tick.
pub(crate) struct PendingCommandExecutionQueue<S>
where
    S: ExecutionCommandSource,
{
    queued: VecDeque<PendingCommandExecution<S>>,
    blocked_sources: FxHashMap<CommandSenderKey, usize>,
    global_barriers: usize,
}

struct PendingCommandExecution<S>
where
    S: ExecutionCommandSource,
{
    owner: CommandExecutionOwner,
    order: CommandSuspensionOrder,
    execution: CommandExecutionContext<S>,
}

impl<S> PendingCommandExecutionQueue<S>
where
    S: ExecutionCommandSource,
{
    pub(crate) fn new() -> Self {
        Self {
            queued: VecDeque::new(),
            blocked_sources: FxHashMap::default(),
            global_barriers: 0,
        }
    }

    /// Retains an execution only when it is waiting on suspended work.
    #[must_use]
    pub(crate) fn push_suspended(
        &mut self,
        owner: CommandExecutionOwner,
        execution: CommandExecutionContext<S>,
    ) -> bool {
        let Some(order) = execution.suspension_order() else {
            return false;
        };
        let source = owner.key();
        self.retain_barrier(source, order);
        self.queued.push_back(PendingCommandExecution {
            owner,
            order,
            execution,
        });
        true
    }

    /// Returns whether a later top-level command from `source` must wait.
    pub(crate) fn blocks(&self, source: CommandSenderKey) -> bool {
        self.global_barriers > 0 || self.blocked_sources.contains_key(&source)
    }

    /// Polls each execution selected for this tick at most once, preserving FIFO order.
    pub(crate) fn tick(
        &mut self,
        limit: usize,
        mut owner_is_current: impl FnMut(&CommandExecutionOwner) -> bool,
    ) -> PendingCommandExecutionStats {
        let scheduled = self.queued.len().min(limit);
        let mut polled = 0;
        let mut finished = 0;

        for _ in 0..scheduled {
            let Some(mut pending) = self.queued.pop_front() else {
                break;
            };
            polled += 1;
            if !owner_is_current(&pending.owner) {
                pending.execution.cancel();
                self.release_barrier(pending.owner.key(), pending.order);
                finished += 1;
                continue;
            }
            match pending.execution.poll_suspension() {
                ExecutionStop::Suspended => {
                    let Some(order) = pending.execution.suspension_order() else {
                        tracing::error!("suspended command lost its active suspension");
                        self.release_barrier(pending.owner.key(), pending.order);
                        finished += 1;
                        continue;
                    };
                    if order != pending.order {
                        self.release_barrier(pending.owner.key(), pending.order);
                        self.retain_barrier(pending.owner.key(), order);
                        pending.order = order;
                    }
                    self.queued.push_back(pending);
                }
                ExecutionStop::Completed
                | ExecutionStop::CommandLimit
                | ExecutionStop::QueueOverflow => {
                    self.release_barrier(pending.owner.key(), pending.order);
                    finished += 1;
                }
            }
        }

        PendingCommandExecutionStats {
            polled,
            finished,
            pending: self.queued.len(),
        }
    }

    pub(crate) fn cancel_all(&mut self) {
        let executions = mem::take(&mut self.queued);
        self.blocked_sources.clear();
        self.global_barriers = 0;
        for mut pending in executions {
            pending.execution.cancel();
        }
    }

    fn retain_barrier(&mut self, source: CommandSenderKey, order: CommandSuspensionOrder) {
        *self.blocked_sources.entry(source).or_default() += 1;
        if order == CommandSuspensionOrder::Global {
            self.global_barriers += 1;
        }
    }

    fn release_barrier(&mut self, source: CommandSenderKey, order: CommandSuspensionOrder) {
        if let Some(count) = self.blocked_sources.get_mut(&source) {
            *count -= 1;
            if *count == 0 {
                self.blocked_sources.remove(&source);
            }
        } else {
            tracing::error!(?source, "pending command source barrier was not retained");
        }
        if order == CommandSuspensionOrder::Global {
            self.global_barriers = self.global_barriers.saturating_sub(1);
        }
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.queued.len()
    }
}

impl<S> Default for PendingCommandExecutionQueue<S>
where
    S: ExecutionCommandSource,
{
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PendingCommandExecutionStats {
    pub(crate) polled: usize,
    pub(crate) finished: usize,
    pub(crate) pending: usize,
}
