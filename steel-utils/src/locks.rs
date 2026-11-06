#![expect(
    clippy::disallowed_types,
    reason = "this module is the canonical definition of the allowed lock types"
)]
//! Lock wrappers for debug checks and deadlock prevention.

use std::fmt::{self, Debug, Formatter};
use std::ops::{Deref, DerefMut};
use std::ptr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use parking_lot::MutexGuard;
use tokio::sync::{Mutex, RwLock};

/// A synchronous mutex.
pub type SyncMutex<T> = parking_lot::Mutex<T>;
/// A synchronous read-write lock.
pub type SyncRwLock<T> = parking_lot::RwLock<T>;

/// An asynchronous mutex.
pub type AsyncMutex<T> = Mutex<T>;
/// An asynchronous read-write lock.
pub type AsyncRwLock<T> = RwLock<T>;

/// A value shared across threads behind the crate's standard lock.
pub type Shared<T> = Arc<SyncMutex<T>>;

/// Wraps `value` in the standard shared lock handle.
pub fn shared<T>(value: T) -> Shared<T> {
    Arc::new(SyncMutex::new(value))
}

/// Wraps this value in an `Arc<SyncMutex<>>`
pub trait IntoShared: Sized {
    /// Wraps this value in an `Arc<SyncMutex<>>`
    fn into_shared(self) -> Shared<Self> {
        Arc::new(SyncMutex::new(self))
    }
}

impl<T> IntoShared for T {}

thread_local! {
    static THREAD_TOKEN: u8 = const { 0 };
}

/// A non-zero value unique to the current thread while it runs.
fn thread_token() -> usize {
    THREAD_TOKEN.with(|token| ptr::from_ref(token).addr())
}

/// A [`SyncMutex`] that panics when the thread holding it locks it again.
///
/// Other threads block as usual. A same-thread re-lock would deadlock a plain
/// mutex; this turns it into a panic naming the lock, which is how re-entrant
/// game logic (an entity hurting another that hurts it back) surfaces a guard
/// held across a call out.
pub struct CheckedMutex<T: ?Sized> {
    name: &'static str,
    owner: AtomicUsize,
    inner: SyncMutex<T>,
}

impl<T> CheckedMutex<T> {
    /// Creates an unlocked mutex reported as `name` in re-entrancy panics.
    pub const fn new(name: &'static str, value: T) -> Self {
        Self {
            name,
            owner: AtomicUsize::new(0),
            inner: SyncMutex::new(value),
        }
    }

    /// Consumes the mutex, returning the value.
    pub fn into_inner(self) -> T {
        self.inner.into_inner()
    }
}

impl<T: ?Sized> CheckedMutex<T> {
    /// Locks the mutex, blocking while another thread holds it.
    ///
    /// # Panics
    ///
    /// Panics if the current thread already holds this mutex.
    #[track_caller]
    pub fn lock(&self) -> CheckedMutexGuard<'_, T> {
        let token = thread_token();
        // Only this thread ever stores its own token, so a match means this
        // thread holds the lock; other threads' writes can never equal it.
        assert!(
            self.owner.load(Ordering::Relaxed) != token,
            "`{}` is already locked by this thread (re-entrant access)",
            self.name
        );
        let guard = self.inner.lock();
        self.owner.store(token, Ordering::Relaxed);
        CheckedMutexGuard {
            owner: &self.owner,
            guard,
        }
    }

    /// Returns the value through exclusive access, without locking.
    pub const fn get_mut(&mut self) -> &mut T {
        self.inner.get_mut()
    }
}

impl<T: Debug> Debug for CheckedMutex<T> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CheckedMutex")
            .field("name", &self.name)
            .field("inner", &self.inner)
            .finish_non_exhaustive()
    }
}

/// Guard of a [`CheckedMutex`]; unlocking clears the owning thread.
pub struct CheckedMutexGuard<'a, T: ?Sized> {
    owner: &'a AtomicUsize,
    guard: MutexGuard<'a, T>,
}

impl<T: ?Sized> Drop for CheckedMutexGuard<'_, T> {
    fn drop(&mut self) {
        // Cleared before `guard` unlocks, so the next owner's store wins.
        self.owner.store(0, Ordering::Relaxed);
    }
}

impl<T: ?Sized> Deref for CheckedMutexGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.guard
    }
}

impl<T: ?Sized> DerefMut for CheckedMutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.guard
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::thread;

    use super::CheckedMutex;

    #[test]
    #[should_panic(expected = "`age` is already locked by this thread")]
    fn relocking_on_the_same_thread_panics_instead_of_deadlocking() {
        let mutex = CheckedMutex::new("age", 0);
        let _held = mutex.lock();
        let _again = mutex.lock();
    }

    #[test]
    fn other_threads_wait_for_the_holder() {
        let mutex = Arc::new(CheckedMutex::new("count", 0));
        let workers: Vec<_> = (0..4)
            .map(|_| {
                let mutex = Arc::clone(&mutex);
                thread::spawn(move || {
                    for _ in 0..1000 {
                        *mutex.lock() += 1;
                    }
                })
            })
            .collect();
        for worker in workers {
            worker.join().expect("worker thread should not panic");
        }
        assert_eq!(*mutex.lock(), 4000);
    }

    #[test]
    fn unlocking_allows_the_same_thread_to_lock_again() {
        let mutex = CheckedMutex::new("flap", 1);
        *mutex.lock() += 1;
        assert_eq!(*mutex.lock(), 2);
    }
}
