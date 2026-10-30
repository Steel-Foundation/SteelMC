//! Exact damage attribution and server-owned recent damage history.

mod combat_tracker;
mod history;
mod source;

pub use combat_tracker::FallLocation;
pub(crate) use combat_tracker::{CombatEntry, CombatTracker, death_message};
pub(crate) use history::DamageHistoryBinding;
pub use history::{DamageHistory, RecentDamageSource};
pub use source::DamageSource;

#[cfg(test)]
mod tests;
