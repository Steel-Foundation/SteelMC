use std::{
    fmt,
    future::Future,
    mem,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Waker},
};

use steel_registry::item_stack::ItemStack;
use steel_utils::locks::SyncMutex;

use super::{
    GeneratedLoot, LootFuture, LootParams, LootTableRef, OperationError, OperationResult,
    OwnedLootRandom,
};
use crate::{
    inventory::container::Container,
    server::jobs::{JobPoll, ServerJob, ServerJobContext},
    world::World,
};

/// Complete output and its still-reserved RNG, retained through container fill.
pub struct PreparedLoot {
    items: Vec<ItemStack>,
    random: OwnedLootRandom,
}

impl PreparedLoot {
    /// Releases RNG ownership and returns generated stacks in output order.
    #[must_use]
    pub fn into_items(self) -> Vec<ItemStack> {
        self.items
    }

    /// Places stacks using the same RNG that generated them, then releases it.
    pub fn fill(mut self, container: &mut dyn Container) {
        GeneratedLoot::new(self.items, &mut self.random).fill(container);
    }
}

#[derive(Default)]
struct CompletionState {
    result: Option<OperationResult<()>>,
    waiting: Vec<Waker>,
    continuations: Vec<Box<dyn FnOnce(OperationResult<()>) + Send>>,
}

/// Completion shared by container access and interaction continuations.
#[derive(Clone, Default)]
pub struct LootCompletion {
    state: Arc<SyncMutex<CompletionState>>,
}

impl LootCompletion {
    fn finish(&self, result: OperationResult<()>) {
        let (waiting, continuations) = {
            let mut state = self.state.lock();
            assert!(state.result.is_none(), "loot operation completed twice");
            state.result = Some(result.clone());
            (
                mem::take(&mut state.waiting),
                mem::take(&mut state.continuations),
            )
        };
        for continuation in continuations {
            continuation(result.clone());
        }
        for waker in waiting {
            waker.wake();
        }
    }

    /// Checks readiness once without waiting or consuming the shared result.
    pub fn poll_result(&self) -> Poll<OperationResult<()>> {
        self.state
            .lock()
            .result
            .as_ref()
            .map_or(Poll::Pending, |result| Poll::Ready(result.clone()))
    }

    /// Allows synchronous entry points to return their continuation to the caller.
    pub fn try_result(&self) -> OperationResult<()> {
        match self.poll_result() {
            Poll::Ready(result) => result,
            Poll::Pending => Err(OperationError::Pending(self.clone())),
        }
    }

    /// Runs a dependent tick action before this operation releases its save permit.
    /// No game-data locks are held while invoking the action.
    pub fn on_complete(&self, action: impl FnOnce(OperationResult<()>) + Send + 'static) {
        let result = {
            let mut state = self.state.lock();
            if state.result.is_none() {
                state.continuations.push(Box::new(action));
                return;
            }
            state.result.clone()
        };
        if let Some(result) = result {
            action(result);
        }
    }
}

impl fmt::Debug for LootCompletion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LootCompletion")
            .field("finished", &self.poll_result().is_ready())
            .finish()
    }
}

impl Future for LootCompletion {
    type Output = OperationResult<()>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut state = self.state.lock();
        if let Some(result) = &state.result {
            return Poll::Ready(result.clone());
        }
        if !state
            .waiting
            .iter()
            .any(|waiting| waiting.will_wake(cx.waker()))
        {
            state.waiting.push(cx.waker().clone());
        }
        Poll::Pending
    }
}

struct LootOperation {
    future: LootFuture<'static>,
    completion: LootCompletion,
}

impl LootOperation {
    fn poll_operation(&mut self) -> JobPoll {
        match self
            .future
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
        {
            Poll::Pending => JobPoll::Pending,
            Poll::Ready(result) => {
                if let Err(error) = &result {
                    log::error!("Loot operation failed: {error}");
                }
                JobPoll::Finished
            }
        }
    }
}

impl ServerJob for LootOperation {
    fn poll(&mut self, _context: &mut ServerJobContext) -> JobPoll {
        self.poll_operation()
    }
    fn must_complete_before_shutdown(&self) -> bool {
        true
    }
}

impl Drop for LootOperation {
    fn drop(&mut self) {
        if self.completion.poll_result().is_pending() {
            self.completion.finish(Err(OperationError::Cancelled));
        }
    }
}

impl World {
    /// Prepares an owned evaluation to be driven as part of a server loot operation.
    pub fn prepare_loot(
        self: &Arc<Self>,
        table: Option<LootTableRef>,
        seed: i64,
        mut params: LootParams,
    ) -> LootFuture<'static, PreparedLoot> {
        let world = Arc::clone(self);
        params.world = Some(Arc::clone(self));
        Box::pin(async move {
            let mut random = world.request_loot_random(table, seed).await?;
            let items = if let Some(table) = table {
                let mut context = params.context(&mut random);
                table.get_random_items_async(&mut context).await?
            } else {
                Vec::new()
            };
            Ok(PreparedLoot { items, random })
        })
    }

    /// Starts at the caller's tick stage and retains unfinished work in server
    /// jobs. Event snapshots, RNG ownership, and placement live until completion.
    pub fn run_loot<T: Send + 'static>(
        self: &Arc<Self>,
        work: LootFuture<'static, T>,
        complete: impl FnOnce(OperationResult<T>) -> OperationResult<()> + Send + 'static,
    ) -> OperationResult<LootCompletion> {
        let completion = LootCompletion::default();
        self.run_loot_with_completion(work, completion.clone(), complete)?;
        Ok(completion)
    }

    pub(crate) fn run_loot_with_completion<T: Send + 'static>(
        self: &Arc<Self>,
        work: LootFuture<'static, T>,
        completion: LootCompletion,
        complete: impl FnOnce(OperationResult<T>) -> OperationResult<()> + Send + 'static,
    ) -> OperationResult<()> {
        let lifecycle = Arc::clone(&self.loot_lifecycle.read());
        let signal = completion.clone();
        let future = Box::pin(async move {
            let _admission = lifecycle.enter().await;
            let result = complete(work.await);
            signal.finish(result.clone());
            result
        });
        let mut operation = LootOperation { future, completion };
        if operation.poll_operation() == JobPoll::Finished {
            return Ok(());
        }
        let Some(jobs) = self.loot_jobs.read().upgrade() else {
            return Err(OperationError::SchedulerUnavailable);
        };
        jobs.spawn(operation);
        Ok(())
    }
}
