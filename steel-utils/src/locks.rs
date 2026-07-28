#![expect(
    clippy::disallowed_types,
    reason = "this module is the canonical definition of the allowed lock types"
)]
//! Lock wrappers for debug checks and deadlock prevention.

use std::sync::Arc;

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
