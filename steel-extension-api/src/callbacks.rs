//! Extension-side closure tables.
//!
//! Native callbacks are plain `extern "C"` functions plus a `u64` token. Each extension compiles its
//! own copy of this crate, so these statics are private to that extension: a closure is leaked into a
//! table at subscription time and its index becomes the token passed back by the host.

#[expect(
    clippy::disallowed_types,
    reason = "extension builds cannot link the host-only steel-utils lock wrappers"
)]
use std::sync::RwLock;

/// Append-only table of process-lifetime closures.
pub(crate) struct Table<F: ?Sized + 'static> {
    #[expect(
        clippy::disallowed_types,
        reason = "extension builds cannot link the host-only steel-utils lock wrappers"
    )]
    entries: RwLock<Vec<&'static F>>,
}

impl<F: ?Sized + 'static> Table<F> {
    #[expect(
        clippy::disallowed_types,
        reason = "extension builds cannot link the host-only steel-utils lock wrappers"
    )]
    pub(crate) const fn new() -> Self {
        Self {
            entries: RwLock::new(Vec::new()),
        }
    }

    /// Retains `entry` for the process lifetime and returns its token.
    pub(crate) fn insert(&self, entry: Box<F>) -> u64 {
        let mut entries = self
            .entries
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        entries.push(Box::leak(entry));
        (entries.len() - 1) as u64
    }

    /// Returns the closure registered under `token`.
    pub(crate) fn get(&self, token: u64) -> Option<&'static F> {
        let entries = self
            .entries
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        usize::try_from(token)
            .ok()
            .and_then(|index| entries.get(index).copied())
    }
}
