#![expect(
    clippy::disallowed_types,
    reason = "this module is the canonical definition of the allowed lock types"
)]
//! Lock wrappers for debug checks and deadlock prevention.

use tokio::sync::{Mutex, RwLock};

/// A synchronous mutex.
pub type SyncMutex<T> = parking_lot::Mutex<T>;
/// A synchronous read-write lock.
pub type SyncRwLock<T> = parking_lot::RwLock<T>;

/// An asynchronous mutex.
pub type AsyncMutex<T> = Mutex<T>;
/// An asynchronous read-write lock.
pub type AsyncRwLock<T> = RwLock<T>;
