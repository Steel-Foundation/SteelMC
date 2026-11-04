//! Exact damage attribution and server-owned recent damage history.

mod history;
mod source;

pub(crate) use history::DamageHistoryBinding;
pub use history::{DamageHistory, RecentDamageSource};
pub use source::DamageSource;

#[cfg(test)]
mod tests;
