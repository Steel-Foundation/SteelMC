#![expect(
    clippy::disallowed_types,
    reason = "this module is the canonical definition of the allowed lock types"
)]
//! Lock wrappers for debug checks and deadlock prevention.

use std::any::type_name;
use std::fmt::{self, Debug, Formatter};
use std::ops::{Deref, DerefMut};
use std::ptr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use parking_lot::{MappedMutexGuard, MutexGuard};
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
/// mutex; this turns it into a panic naming the locked type and the second
/// `lock` call site, which is how re-entrant
/// game logic (an entity hurting another that hurts it back) surfaces a guard
/// held across a call out.
pub struct CheckedMutex<T: ?Sized> {
    owner: AtomicUsize,
    inner: SyncMutex<T>,
}

impl<T> CheckedMutex<T> {
    /// Creates an unlocked mutex.
    pub const fn new(value: T) -> Self {
        Self {
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
            type_name::<T>()
        );
        let guard = self.inner.lock();
        self.owner.store(token, Ordering::Relaxed);
        CheckedMutexGuard {
            owner: OwnerReset(&self.owner),
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
            .field("type", &type_name::<T>())
            .field("inner", &self.inner)
            .finish_non_exhaustive()
    }
}

/// Clears a [`CheckedMutex`]'s owning thread when dropped.
struct OwnerReset<'a>(&'a AtomicUsize);

impl Drop for OwnerReset<'_> {
    fn drop(&mut self) {
        self.0.store(0, Ordering::Relaxed);
    }
}

/// Guard of a [`CheckedMutex`]; unlocking clears the owning thread.
pub struct CheckedMutexGuard<'a, T: ?Sized> {
    // Declared first so it drops first: the owner is cleared before `guard`
    // unlocks, so the next owner's store wins.
    owner: OwnerReset<'a>,
    guard: MutexGuard<'a, T>,
}

impl<'a, T: ?Sized> CheckedMutexGuard<'a, T> {
    /// Narrows the guard to a part of the locked value, keeping the lock held.
    pub fn map<U: ?Sized>(
        this: Self,
        f: impl FnOnce(&mut T) -> &mut U,
    ) -> MappedCheckedMutexGuard<'a, U> {
        let Self { owner, guard } = this;
        MappedCheckedMutexGuard {
            _owner: owner,
            guard: MutexGuard::map(guard, f),
        }
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

/// A [`CheckedMutexGuard`] narrowed with [`CheckedMutexGuard::map`].
pub struct MappedCheckedMutexGuard<'a, T: ?Sized> {
    // Only dropped; declared first for the same unlock order as the guard.
    _owner: OwnerReset<'a>,
    guard: MappedMutexGuard<'a, T>,
}

impl<T: ?Sized> Deref for MappedCheckedMutexGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.guard
    }
}

impl<T: ?Sized> DerefMut for MappedCheckedMutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.guard
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::thread;

    use super::{CheckedMutex, CheckedMutexGuard};

    #[test]
    #[should_panic(expected = "`i32` is already locked by this thread")]
    fn relocking_on_the_same_thread_panics_instead_of_deadlocking() {
        let mutex = CheckedMutex::new(0);
        let _held = mutex.lock();
        let _again = mutex.lock();
    }

    #[test]
    fn other_threads_wait_for_the_holder() {
        let mutex = Arc::new(CheckedMutex::new(0));
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
    fn dropping_a_mapped_guard_unlocks() {
        let mutex = CheckedMutex::new((1, 2));
        *CheckedMutexGuard::map(mutex.lock(), |pair| &mut pair.1) += 1;
        assert_eq!(*mutex.lock(), (1, 3));
    }

    #[test]
    fn unlocking_allows_the_same_thread_to_lock_again() {
        let mutex = CheckedMutex::new(1);
        *mutex.lock() += 1;
        assert_eq!(*mutex.lock(), 2);
    }
}
