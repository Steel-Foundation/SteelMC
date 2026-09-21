use crate::player::Player;
use rustc_hash::FxHashSet;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use steel_protocol::packets::game::c_update_advancement::CUpdateAdvancements;
use steel_registry::advancement::registry::AdvancementNode;
use steel_registry::advancement::{
    Advancement, AdvancementProgressData, AdvancementRequirement, Criteria,
};
use steel_utils::Identifier;

/// Manages a player's collection of advancements.
///
/// This handles saving, loading, and tracking the state of granted / revoked advancements.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct PlayerAdvancement {
    pub progress: AdvancementProgressMap,
    pub is_first_packet: bool,
    pub roots_to_update: FxHashSet<&'static AdvancementNode>,
    pub visible: FxHashSet<&'static Advancement>,
    pub progress_changed: FxHashSet<&'static Advancement>,
    pub last_selected_tab: Option<&'static Advancement>,
}

impl PlayerAdvancement {
    pub fn flush_dirty(&mut self, player: &Player, show_advancement: bool) {
        if self.is_first_packet || !self.roots_to_update.is_empty() {
            let mut progress: HashMap<Identifier, &AdvancementProgress> = HashMap::new();
            let mut added: Vec<&Advancement> = Vec::new();
            let mut removed: Vec<Identifier> = Vec::new();
            for root in self.roots_to_update.clone() {
                self.update_tree_visibility(root, &mut added, &mut removed);
            }
            self.roots_to_update.clear();
            for advancement in &self.progress_changed {
                if self.visible.contains(advancement) {
                    progress.insert(advancement.key.clone(), &self.progress.map[advancement]);
                }
            }
            self.progress_changed.clear();
            if !progress.is_empty() || !added.is_empty() || !removed.is_empty() {
                let parsed_progress: Vec<AdvancementProgressData> = progress
                    .into_iter()
                    .map(|(key, val)| AdvancementProgressData {
                        id: key,
                        progress: val
                            .criteria
                            .iter()
                            .map(|(key, val)| Criteria {
                                criterion_id: key.clone(),
                                achieve_date: val.0.map(|time| {
                                    time.duration_since(UNIX_EPOCH)
                                        .map_or(0, |d| d.as_millis() as i64)
                                }),
                            })
                            .collect(),
                    })
                    .collect();
                player.try_send_client_packet(&CUpdateAdvancements::new(
                    self.is_first_packet,
                    added,
                    parsed_progress,
                    removed,
                    show_advancement,
                ));
            }
        }
        self.is_first_packet = false;
    }
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct AdvancementProgressMap {
    pub map: BTreeMap<&'static Advancement, AdvancementProgress>,
}

/// Represents the progress of a given advancement for a player.
///
/// Tracks whether the advancement has been fully completed. In the future,
/// this will also track specific criteria progress.
#[derive(Debug, Clone, Default)]
pub struct AdvancementProgress {
    /// Indicates the different progress of all criteria currently only a boolean
    pub criteria: HashMap<Arc<str>, CriterionProgress>,
    /// The Requirement for the Advancement to be mark as complete
    pub requirements: AdvancementRequirement,
}

impl AdvancementProgress {
    /// Returns `true` if the advancement is done.
    #[must_use]
    pub fn is_done(&self) -> bool {
        self.requirements.test(|s| self.is_criterion_done(s))
    }

    /// Check if a criterion his mark has complete
    fn is_criterion_done(&self, criterion: &str) -> bool {
        self.criteria
            .get(criterion)
            .is_some_and(CriterionProgress::is_done)
    }

    /// Returns `true` if the advancement has any progress. Currently just returns if it is fully complete.
    #[must_use]
    pub fn has_progress(&self) -> bool {
        for value in self.criteria.values() {
            if value.is_done() {
                return true;
            }
        }
        false
    }

    pub fn grant_progress(&mut self, name: &str) -> bool {
        if let Some(value) = self.criteria.get_mut(name)
            && !value.is_done()
        {
            value.grant();
            true
        } else {
            false
        }
    }

    pub fn revoke_progress(&mut self, name: &str) -> bool {
        if let Some(value) = self.criteria.get_mut(name)
            && value.is_done()
        {
            value.revoke();
            true
        } else {
            false
        }
    }

    pub fn update(&mut self, requirements: AdvancementRequirement) {
        let names = requirements.names();
        self.criteria.retain(|key, _criterion| names.contains(key));
        for name in names {
            self.criteria.entry(name).or_default();
        }
        self.requirements = requirements;
    }

    #[inline]
    pub fn get_remaining_criteria(&self) -> impl Iterator<Item = Arc<str>> {
        self.criteria
            .iter()
            .filter(|&(_id, criterion)| !criterion.is_done())
            .map(|(id, _criterion)| id.clone())
    }

    #[inline]
    pub fn get_completed_criteria(&self) -> impl Iterator<Item = Arc<str>> {
        self.criteria
            .iter()
            .filter(|&(_id, criterion)| criterion.is_done())
            .map(|(id, _criterion)| id.clone())
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
pub struct CriterionProgress(pub Option<SystemTime>);

impl CriterionProgress {
    pub fn grant(&mut self) {
        self.0 = Some(SystemTime::now());
    }

    pub const fn revoke(&mut self) {
        self.0 = None;
    }

    #[must_use]
    pub const fn is_done(&self) -> bool {
        self.0.is_some()
    }
}
