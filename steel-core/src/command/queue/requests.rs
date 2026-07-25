use std::{collections::VecDeque, sync::Arc};

use steel_utils::locks::SyncMutex;
use uuid::Uuid;

use crate::{command::sender::CommandSender, player::Player};

const DEFAULT_COMMAND_REQUEST_CAPACITY: usize = 1024;
const DEFAULT_SUGGESTION_REQUEST_CAPACITY: usize = 1024;

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

/// Returned when the relevant pending command request queue has reached its fixed capacity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandQueueFull;

enum PendingRequest<E, S> {
    Execute(E),
    Suggestions(S),
}

/// Independently bounded execution and suggestion queues.
///
/// Suggestions are coalesced by sender and the two queues are drained fairly. This keeps a client
/// producing suggestion traffic from consuming execution capacity or the entire tick budget.
struct PendingRequestQueues<K, E, S> {
    executions: VecDeque<E>,
    suggestions: VecDeque<(K, S)>,
    execution_capacity: usize,
    suggestion_capacity: usize,
    prefer_execution: bool,
}

impl<K: Eq, E, S> PendingRequestQueues<K, E, S> {
    const fn new(execution_capacity: usize, suggestion_capacity: usize) -> Self {
        Self {
            executions: VecDeque::new(),
            suggestions: VecDeque::new(),
            execution_capacity,
            suggestion_capacity,
            prefer_execution: true,
        }
    }

    fn submit_execution(&mut self, request: E) -> Result<(), CommandQueueFull> {
        if self.executions.len() >= self.execution_capacity {
            return Err(CommandQueueFull);
        }
        self.executions.push_back(request);
        Ok(())
    }

    fn submit_suggestions(&mut self, key: K, request: S) -> Result<(), CommandQueueFull> {
        if let Some((_, pending)) = self
            .suggestions
            .iter_mut()
            .find(|(pending_key, _)| pending_key == &key)
        {
            *pending = request;
            return Ok(());
        }
        if self.suggestions.len() >= self.suggestion_capacity {
            return Err(CommandQueueFull);
        }
        self.suggestions.push_back((key, request));
        Ok(())
    }

    #[cfg(test)]
    fn pop_front(&mut self) -> Option<PendingRequest<E, S>> {
        self.pop_front_where(|_| true)
    }

    fn pop_front_where(
        &mut self,
        mut execution_allowed: impl FnMut(&E) -> bool,
    ) -> Option<PendingRequest<E, S>> {
        if self.prefer_execution {
            if let Some(request) = self.pop_allowed_execution(&mut execution_allowed) {
                self.prefer_execution = false;
                return Some(PendingRequest::Execute(request));
            }
            let (_, request) = self.suggestions.pop_front()?;
            self.prefer_execution = true;
            return Some(PendingRequest::Suggestions(request));
        }

        if let Some((_, request)) = self.suggestions.pop_front() {
            self.prefer_execution = true;
            return Some(PendingRequest::Suggestions(request));
        }
        let request = self.pop_allowed_execution(&mut execution_allowed)?;
        self.prefer_execution = false;
        Some(PendingRequest::Execute(request))
    }

    fn pop_allowed_execution(
        &mut self,
        execution_allowed: &mut impl FnMut(&E) -> bool,
    ) -> Option<E> {
        let index = self.executions.iter().position(execution_allowed)?;
        self.executions.remove(index)
    }

    fn clear(&mut self) {
        self.executions.clear();
        self.suggestions.clear();
        self.prefer_execution = true;
    }
}

/// Bounded cross-task requests drained by the main game tick.
pub(crate) struct CommandRequestQueue {
    queued: SyncMutex<PendingRequestQueues<Uuid, CommandRequest, CommandRequest>>,
}

impl CommandRequestQueue {
    pub(crate) const fn new() -> Self {
        Self {
            queued: SyncMutex::new(PendingRequestQueues::new(
                DEFAULT_COMMAND_REQUEST_CAPACITY,
                DEFAULT_SUGGESTION_REQUEST_CAPACITY,
            )),
        }
    }

    pub(crate) fn submit(&self, request: CommandRequest) -> Result<(), CommandQueueFull> {
        let mut queued = self.queued.lock();
        match request {
            request @ CommandRequest::Execute { .. } => queued.submit_execution(request),
            CommandRequest::Suggestions {
                player,
                transaction_id,
                input,
            } => queued.submit_suggestions(
                player.gameprofile.id,
                CommandRequest::Suggestions {
                    player,
                    transaction_id,
                    input,
                },
            ),
        }
    }

    pub(crate) fn pop_front_runnable(
        &self,
        mut execution_allowed: impl FnMut(&CommandSender) -> bool,
    ) -> Option<CommandRequest> {
        let request = self.queued.lock().pop_front_where(|request| {
            let CommandRequest::Execute { sender, .. } = request else {
                return false;
            };
            execution_allowed(sender)
        })?;
        match request {
            PendingRequest::Execute(request) | PendingRequest::Suggestions(request) => {
                Some(request)
            }
        }
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
    use super::{CommandQueueFull, PendingRequest, PendingRequestQueues};

    fn queue_with_capacity(
        execution_capacity: usize,
        suggestion_capacity: usize,
    ) -> PendingRequestQueues<u8, &'static str, &'static str> {
        PendingRequestQueues::new(execution_capacity, suggestion_capacity)
    }

    #[test]
    fn executions_are_dequeued_in_submission_order() {
        let mut queue = queue_with_capacity(3, 3);

        assert!(queue.submit_execution("first").is_ok());
        assert!(queue.submit_execution("second").is_ok());

        assert!(matches!(
            queue.pop_front(),
            Some(PendingRequest::Execute("first"))
        ));
        assert!(matches!(
            queue.pop_front(),
            Some(PendingRequest::Execute("second"))
        ));
        assert!(queue.pop_front().is_none());
    }

    #[test]
    fn blocked_execution_sources_are_skipped_without_reordering_their_requests() {
        let mut queue = queue_with_capacity(3, 1);
        assert!(queue.submit_execution("blocked first").is_ok());
        assert!(queue.submit_execution("ready").is_ok());
        assert!(queue.submit_execution("blocked second").is_ok());

        assert!(matches!(
            queue.pop_front_where(|request| !request.starts_with("blocked")),
            Some(PendingRequest::Execute("ready"))
        ));
        assert!(matches!(
            queue.pop_front(),
            Some(PendingRequest::Execute("blocked first"))
        ));
        assert!(matches!(
            queue.pop_front(),
            Some(PendingRequest::Execute("blocked second"))
        ));
    }

    #[test]
    fn full_execution_queue_rejects_without_dropping_pending_requests() {
        let mut queue = queue_with_capacity(2, 2);

        assert!(queue.submit_execution("first").is_ok());
        assert!(queue.submit_execution("second").is_ok());
        assert_eq!(queue.submit_execution("third"), Err(CommandQueueFull));

        assert!(matches!(
            queue.pop_front(),
            Some(PendingRequest::Execute("first"))
        ));
        assert!(matches!(
            queue.pop_front(),
            Some(PendingRequest::Execute("second"))
        ));
        assert!(queue.pop_front().is_none());
    }

    #[test]
    fn suggestion_capacity_cannot_starve_execution_capacity() {
        let mut queue = queue_with_capacity(2, 2);

        assert!(queue.submit_suggestions(1, "first suggestion").is_ok());
        assert!(queue.submit_suggestions(2, "second suggestion").is_ok());
        assert_eq!(
            queue.submit_suggestions(3, "rejected suggestion"),
            Err(CommandQueueFull)
        );

        assert!(queue.submit_execution("command").is_ok());
        assert!(matches!(
            queue.pop_front(),
            Some(PendingRequest::Execute("command"))
        ));
    }

    #[test]
    fn suggestions_from_one_sender_are_coalesced() {
        let mut queue = queue_with_capacity(1, 1);

        assert!(queue.submit_suggestions(1, "old").is_ok());
        assert!(queue.submit_suggestions(1, "latest").is_ok());
        assert!(matches!(
            queue.pop_front(),
            Some(PendingRequest::Suggestions("latest"))
        ));
        assert!(queue.pop_front().is_none());
    }

    #[test]
    fn busy_queues_are_drained_fairly() {
        let mut queue = queue_with_capacity(2, 2);
        assert!(queue.submit_execution("first command").is_ok());
        assert!(queue.submit_execution("second command").is_ok());
        assert!(queue.submit_suggestions(1, "first suggestion").is_ok());
        assert!(queue.submit_suggestions(2, "second suggestion").is_ok());

        assert!(matches!(
            queue.pop_front(),
            Some(PendingRequest::Execute(_))
        ));
        assert!(matches!(
            queue.pop_front(),
            Some(PendingRequest::Suggestions(_))
        ));
        assert!(matches!(
            queue.pop_front(),
            Some(PendingRequest::Execute(_))
        ));
        assert!(matches!(
            queue.pop_front(),
            Some(PendingRequest::Suggestions(_))
        ));
    }

    #[test]
    fn clear_discards_all_pending_requests() {
        let mut queue = queue_with_capacity(2, 2);

        assert!(queue.submit_execution("command").is_ok());
        assert!(queue.submit_suggestions(1, "suggestion").is_ok());
        queue.clear();

        assert!(queue.pop_front().is_none());
    }
}
