//! The rayon pools the server runs chunk work on.

use std::sync::Arc;

use rayon::{ThreadPool, ThreadPoolBuilder};

use super::{available_worker_threads, cap_positive_thread_count};

/// Builds the chunk generation pool.
///
/// `configured_threads` is the operator's count; `None` leaves rayon's default.
///
/// # Errors
/// Returns an error if the pool cannot be started.
pub fn build_generation_pool(configured_threads: Option<usize>) -> Result<Arc<ThreadPool>, String> {
    let mut builder = ThreadPoolBuilder::new().thread_name(|i| format!("rayon-gen-{i}"));
    if let Some(threads) = cap_positive_thread_count(configured_threads, available_worker_threads())
    {
        builder = builder.num_threads(threads);
    }
    // Debug builds have deep call chains in density functions that overflow the
    // default 2 MB stack.
    if cfg!(debug_assertions) {
        builder = builder.stack_size(8 * 1024 * 1024);
    }
    builder
        .build()
        .map(Arc::new)
        .map_err(|error| format!("failed to create generation thread pool: {error}"))
}

/// Builds the chunk encoding pool.
///
/// # Errors
/// Returns an error if the pool cannot be started.
pub fn build_chunk_encoding_pool(
    configured_threads: Option<usize>,
) -> Result<Arc<ThreadPool>, String> {
    let mut builder = ThreadPoolBuilder::new().thread_name(|i| format!("rayon-chunk-encode-{i}"));
    if let Some(threads) = cap_positive_thread_count(configured_threads, available_worker_threads())
    {
        builder = builder.num_threads(threads);
    }
    builder
        .build()
        .map(Arc::new)
        .map_err(|error| format!("failed to create chunk encoding thread pool: {error}"))
}
