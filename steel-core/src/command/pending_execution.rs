//! Start-of-tick queue for suspended command executions.

use std::{collections::VecDeque, mem};

use super::execution::{CommandExecutionContext, ExecutionCommandSource, ExecutionStop};

/// Maximum retained command executions polled before new command requests in one tick.
pub(crate) const COMMAND_RESUMPTIONS_PER_TICK: usize = 128;

/// Suspended command executions owned by the server tick.
pub(crate) struct PendingCommandExecutionQueue<S>
where
    S: ExecutionCommandSource,
{
    queued: VecDeque<CommandExecutionContext<S>>,
}

impl<S> PendingCommandExecutionQueue<S>
where
    S: ExecutionCommandSource,
{
    pub(crate) const fn new() -> Self {
        Self {
            queued: VecDeque::new(),
        }
    }

    /// Retains an execution only when it is waiting on suspended work.
    #[must_use]
    pub(crate) fn push_suspended(&mut self, execution: CommandExecutionContext<S>) -> bool {
        if !execution.is_suspended() {
            return false;
        }
        self.queued.push_back(execution);
        true
    }

    /// Polls each execution selected for this tick at most once, preserving FIFO order.
    pub(crate) fn tick(&mut self, limit: usize) -> PendingCommandExecutionStats {
        let scheduled = self.queued.len().min(limit);
        let mut polled = 0;
        let mut finished = 0;

        for _ in 0..scheduled {
            let Some(mut execution) = self.queued.pop_front() else {
                break;
            };
            polled += 1;
            match execution.poll_suspension() {
                ExecutionStop::Suspended => self.queued.push_back(execution),
                ExecutionStop::Completed
                | ExecutionStop::CommandLimit
                | ExecutionStop::QueueOverflow => finished += 1,
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
        for mut execution in executions {
            execution.cancel();
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
