//! Exact damage attribution and server-owned recent damage history.

mod history;
mod source;

pub use history::DamageHistory;
pub(crate) use history::DamageHistoryBinding;
pub use source::DamageSource;

#[cfg(test)]
mod tests;
