//! Delayed and repeating tasks run on the server tick thread.
//!
//! Enable with [`Hooks::enable_scheduler`](crate::Hooks::enable_scheduler) during pre-init.
//! Tasks may be scheduled from any thread, including event handlers, and always run at the
//! start of a server tick with borrowed [`Server`] access.

use crate::Server;
use core::sync::atomic::{AtomicU64, Ordering};

#[expect(
    clippy::disallowed_types,
    reason = "extension builds cannot link the host-only steel-utils lock wrappers"
)]
use std::sync::Mutex;

/// Identifies a scheduled task for cancellation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TaskId(u64);

type Job = Box<dyn FnMut(&Server<'_>) + Send>;

struct Task {
    id: u64,
    due: u64,
    period: Option<u64>,
    job: Job,
}

#[expect(
    clippy::disallowed_types,
    reason = "extension builds cannot link the host-only steel-utils lock wrappers"
)]
static TASKS: Mutex<Vec<Task>> = Mutex::new(Vec::new());
/// Repeating tasks cancelled while their job was running.
#[expect(
    clippy::disallowed_types,
    reason = "extension builds cannot link the host-only steel-utils lock wrappers"
)]
static CANCELLED_RUNNING: Mutex<Vec<u64>> = Mutex::new(Vec::new());
static NEXT_ID: AtomicU64 = AtomicU64::new(0);
static CURRENT_TICK: AtomicU64 = AtomicU64::new(0);

fn schedule(delay: u64, period: Option<u64>, job: Job) -> TaskId {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let due = CURRENT_TICK
        .load(Ordering::Relaxed)
        .saturating_add(delay.max(1));
    tasks().push(Task {
        id,
        due,
        period: period.map(|period| period.max(1)),
        job,
    });
    TaskId(id)
}

fn tasks() -> std::sync::MutexGuard<'static, Vec<Task>> {
    TASKS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Runs `job` once, `delay_ticks` server ticks from now (at least one).
pub fn run_later(delay_ticks: u64, job: impl FnOnce(&Server<'_>) + Send + 'static) -> TaskId {
    let mut job = Some(job);
    schedule(
        delay_ticks,
        None,
        Box::new(move |server| {
            if let Some(job) = job.take() {
                job(server);
            }
        }),
    )
}

/// Runs `job` after `delay_ticks`, then every `period_ticks`, until cancelled.
pub fn run_repeating(
    delay_ticks: u64,
    period_ticks: u64,
    job: impl FnMut(&Server<'_>) + Send + 'static,
) -> TaskId {
    schedule(delay_ticks, Some(period_ticks), Box::new(job))
}

/// Cancels a task; returns whether it was still scheduled.
pub fn cancel(task: TaskId) -> bool {
    let mut tasks = tasks();
    let before = tasks.len();
    tasks.retain(|scheduled| scheduled.id != task.0);
    if tasks.len() != before {
        return true;
    }
    drop(tasks);
    CANCELLED_RUNNING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(task.0);
    false
}

/// The last server tick seen by the scheduler.
#[must_use]
pub fn current_tick() -> u64 {
    CURRENT_TICK.load(Ordering::Relaxed)
}

/// Runs every task due at `tick`. Jobs run without holding the task lock, so they may schedule
/// or cancel tasks themselves.
pub(crate) fn run_due(server: &Server<'_>, tick: u64) {
    CURRENT_TICK.store(tick, Ordering::Relaxed);
    let due: Vec<Task> = {
        let mut tasks = tasks();
        let (due, waiting) = core::mem::take(&mut *tasks)
            .into_iter()
            .partition(|task| task.due <= tick);
        *tasks = waiting;
        due
    };
    for mut task in due {
        (task.job)(server);
        let Some(period) = task.period else {
            continue;
        };
        let cancelled = {
            let mut cancelled = CANCELLED_RUNNING
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let before = cancelled.len();
            cancelled.retain(|id| *id != task.id);
            cancelled.len() != before
        };
        if !cancelled {
            task.due = tick + period;
            tasks().push(task);
        }
    }
}
