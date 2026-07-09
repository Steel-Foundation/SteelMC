use std::{collections::VecDeque, sync::Arc};

use steel_utils::locks::SyncMutex;

use crate::{command::sender::CommandSender, player::Player};

const DEFAULT_COMMAND_REQUEST_CAPACITY: usize = 1024;

/// Maximum command requests handled before one world tick.
pub(crate) const COMMAND_REQUESTS_PER_TICK: usize = 128;

/// Work submitted from connection or console tasks for the game tick to handle.
pub(crate) enum CommandRequest {
    Execute {
        sender: CommandSender,
        command: String,
    },
    Suggestions {
        player: Arc<Player>,
        transaction_id: i32,
        input: String,
    },
}

/// Returned when the pending command request queue has reached its fixed capacity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandQueueFull;

/// Bounded cross-task FIFO drained by the main game tick.
pub(crate) struct CommandRequestQueue {
    queued: SyncMutex<VecDeque<CommandRequest>>,
    capacity: usize,
}

impl CommandRequestQueue {
    pub(crate) const fn new() -> Self {
        Self {
            queued: SyncMutex::new(VecDeque::new()),
            capacity: DEFAULT_COMMAND_REQUEST_CAPACITY,
        }
    }

    pub(crate) fn submit(&self, request: CommandRequest) -> Result<(), CommandQueueFull> {
        let mut queued = self.queued.lock();
        if queued.len() >= self.capacity {
            return Err(CommandQueueFull);
        }
        queued.push_back(request);
        Ok(())
    }

    pub(crate) fn pop_front(&self) -> Option<CommandRequest> {
        self.queued.lock().pop_front()
    }

    pub(crate) fn clear(&self) {
        self.queued.lock().clear();
    }
}

impl Default for CommandRequestQueue {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use steel_utils::locks::SyncMutex;

    use super::{CommandQueueFull, CommandRequest, CommandRequestQueue};
    use crate::command::sender::CommandSender;

    fn queue_with_capacity(capacity: usize) -> CommandRequestQueue {
        CommandRequestQueue {
            queued: SyncMutex::new(VecDeque::new()),
            capacity,
        }
    }

    fn submit(queue: &CommandRequestQueue, command: &str) -> Result<(), CommandQueueFull> {
        queue.submit(CommandRequest::Execute {
            sender: CommandSender::Console,
            command: command.to_owned(),
        })
    }

    fn pop_command(queue: &CommandRequestQueue) -> Option<String> {
        let CommandRequest::Execute { command, .. } = queue.pop_front()? else {
            return None;
        };
        Some(command)
    }

    #[test]
    fn requests_are_dequeued_in_submission_order() {
        let queue = queue_with_capacity(3);

        assert!(submit(&queue, "first").is_ok());
        assert!(submit(&queue, "second").is_ok());

        assert_eq!(pop_command(&queue).as_deref(), Some("first"));
        assert_eq!(pop_command(&queue).as_deref(), Some("second"));
        assert_eq!(pop_command(&queue), None);
    }

    #[test]
    fn full_queue_rejects_without_dropping_pending_requests() {
        let queue = queue_with_capacity(2);

        assert!(submit(&queue, "first").is_ok());
        assert!(submit(&queue, "second").is_ok());
        assert_eq!(submit(&queue, "third"), Err(CommandQueueFull));

        assert_eq!(pop_command(&queue).as_deref(), Some("first"));
        assert_eq!(pop_command(&queue).as_deref(), Some("second"));
        assert_eq!(pop_command(&queue), None);
    }

    #[test]
    fn clear_discards_pending_requests() {
        let queue = queue_with_capacity(2);

        assert!(submit(&queue, "first").is_ok());
        assert!(submit(&queue, "second").is_ok());
        queue.clear();

        assert_eq!(pop_command(&queue), None);
    }
}
