//! advancement linked to the player.
//!
//! Groups the advancementPlayer, advancement progress and visibility evaluator

pub mod progress;
pub mod rewards;
mod saving;
mod visibility_evaluator;

pub(crate) use saving::{PersistedAdvancements, PersistedAdvancementsRef};

use crate::entity::Entity;
use crate::player::Player;
use crate::player::advancement::rewards::grant_reward;
use progress::{AdvancementProgress, AdvancementProgressMap};
use rustc_hash::{FxHashMap, FxHashSet};
use std::mem::take;
use std::sync::Arc;
use std::time::UNIX_EPOCH;
use steel_protocol::packets::game::CSelectAdvancementsTab;
use steel_protocol::packets::game::CUpdateAdvancements;
use steel_registry::REGISTRY;
use steel_registry::advancement::registry::{
    AdvancementIdx, AdvancementNode, AdvancementRef, FrozenAdvancementRegistry,
};
use steel_registry::advancement::{AdvancementProgressData, Criteria};
use steel_registry::vanilla_game_rules::SHOW_ADVANCEMENT_MESSAGES;
use steel_utils::Identifier;

/// Manages a player's collection of advancements.
///
/// This handles saving, loading, and tracking the state of granted / revoked advancements.
#[derive(Debug, Clone)]
pub struct PlayerAdvancement {
    /// the progress of the player for each advancement
    pub(crate) progress: AdvancementProgressMap,
    is_first_packet: bool,
    roots_to_update: FxHashSet<AdvancementIdx>,
    visible: FxHashSet<AdvancementIdx>,
    progress_changed: FxHashSet<AdvancementIdx>,
    /// represent the last selected advancement tab
    last_selected_tab: Option<Identifier>,
    /// the registry used to evaluate the visibility of the advancements
    registry: Arc<FrozenAdvancementRegistry>,
}

impl Default for PlayerAdvancement {
    fn default() -> Self {
        Self {
            is_first_packet: true,
            progress: AdvancementProgressMap::default(),
            roots_to_update: FxHashSet::default(),
            visible: FxHashSet::default(),
            progress_changed: FxHashSet::default(),
            last_selected_tab: Option::default(),
            registry: REGISTRY.advancements.snapshot(),
        }
    }
}

impl PlayerAdvancement {
    /// Returns a reference to the current advancement registry.
    #[must_use]
    #[inline]
    pub const fn registry(&self) -> &Arc<FrozenAdvancementRegistry> {
        &self.registry
    }

    /// Synchronizes the player's advancement data with the current advancement registry.
    ///
    /// Progress is kept by advancement identifier because indices belong to the old tree. It is
    /// reapplied to the new registry so an advancement reload does not drop live progress.
    pub fn sync(&mut self, current: Arc<FrozenAdvancementRegistry>) {
        if Arc::ptr_eq(&self.registry, &current) {
            return;
        }
        // TODO: clear advancement listeners once listener registration is implemented.
        let progress = self.persisted_progress();
        self.registry = current;
        self.apply_persisted_progress(progress);
    }

    /// Award a criterion to a player. If this completes the advancement,
    /// grant the advancement along with its rewards and display the corresponding chat message.
    pub(crate) fn award(
        &mut self,
        player: &Player,
        advancement_key: &Identifier,
        criterion: &str,
    ) -> bool {
        let mut result = false;
        let Some(advancement_idx) = self.registry.get_idx_by_key(advancement_key) else {
            return false;
        };
        let advancement = self.registry.value_by_idx(advancement_idx);
        let progress = self
            .progress
            .get_mut_or_start_progress(&self.registry, advancement_idx);
        let was_done = progress.is_done();
        if progress.grant_progress(criterion) {
            //TODO: register advancement listener
            self.progress_changed.insert(advancement_idx);
            result = true;
            if !was_done && progress.is_done() {
                grant_reward(player, advancement.rewards());
                if let Some(display) = advancement.display()
                    && display.announce_chat
                    && player
                        .level()
                        .is_some_and(|level| level.get_game_rule(&SHOW_ADVANCEMENT_MESSAGES))
                {
                    player.server().broadcast_system_chat(
                        &display
                            .frame_type
                            .create_announcement(advancement, player.display_name()),
                        None,
                    );
                }
            }
        }

        if !was_done && progress.is_done() {
            self.mark_for_visibility_update(advancement_idx);
        }
        result
    }

    /// revoke the specified criterion for a player.
    pub fn revoke(&mut self, advancement_key: &Identifier, criterion: &str) -> bool {
        let mut result = false;
        let Some(advancement) = self.registry.get_idx_by_key(advancement_key) else {
            return false;
        };
        let progress = self
            .progress
            .get_mut_or_start_progress(&self.registry, advancement);
        let was_done = progress.is_done();
        if progress.revoke_progress(criterion) {
            //TODO: unregister advancement listener
            self.progress_changed.insert(advancement);
            result = true;
        }

        if was_done && !progress.is_done() {
            self.mark_for_visibility_update(advancement);
        }

        result
    }

    /// mark the advancement to be sent to the client next tick
    fn mark_for_visibility_update(&mut self, advancement: AdvancementIdx) {
        self.roots_to_update.insert(self.registry.root(advancement));
    }

    /// modify the added and the removed from the available tree status
    fn update_tree_visibility(
        &mut self,
        root: AdvancementIdx,
        added: &mut Vec<AdvancementNode>,
        removed: &mut Vec<Identifier>,
    ) {
        visibility_evaluator::evaluate_visibility(
            root,
            self,
            &mut |player_advancement, node| {
                player_advancement
                    .progress
                    .get_mut_or_start_progress(&player_advancement.registry, node)
                    .is_done()
            },
            &mut move |player_advancement, node_idx, should_be_visible| {
                let node = player_advancement.registry.get_by_idx(node_idx);
                if should_be_visible {
                    if player_advancement.visible.insert(node_idx) {
                        added.push(node.clone());
                        if player_advancement.progress.has_progress(node_idx) {
                            player_advancement.progress_changed.insert(node_idx);
                        }
                    }
                } else if player_advancement.visible.remove(&node_idx) {
                    removed.push(node.value().key().clone());
                }
            },
        );
    }

    /// send the advancement update packet to a player with the updated tree
    pub fn flush_dirty(&mut self, player: &Player, show_advancement: bool) {
        if self.is_first_packet
            || !self.roots_to_update.is_empty()
            || !self.progress_changed.is_empty()
        {
            let mut progress: FxHashMap<Identifier, &AdvancementProgress> = FxHashMap::default();
            let mut added: Vec<AdvancementNode> = Vec::new();
            let mut removed: Vec<Identifier> = Vec::new();
            let is_first_packet = self.is_first_packet;
            let roots = take(&mut self.roots_to_update);
            for root in roots {
                self.update_tree_visibility(root, &mut added, &mut removed);
            }
            for advancement_idx in &self.progress_changed {
                if self.visible.contains(advancement_idx) {
                    progress.insert(
                        self.registry.value_by_idx(*advancement_idx).key().clone(),
                        self.progress
                            .get_progress(*advancement_idx)
                            .unwrap_or_else(|| {
                                unreachable!(
                                    "progress changed should only contains valid advancement"
                                )
                            }),
                    );
                }
            }
            self.progress_changed.clear();
            // even if it's empty it must be updated when switching domain
            if self.is_first_packet
                || !progress.is_empty()
                || !added.is_empty()
                || !removed.is_empty()
            {
                let parsed_progress: Vec<AdvancementProgressData> = progress
                    .into_iter()
                    .map(|(key, val)| AdvancementProgressData {
                        id: key,
                        progress: val
                            .criteria()
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
                player.send_packet(CUpdateAdvancements::new(
                    is_first_packet,
                    added,
                    parsed_progress,
                    removed,
                    show_advancement,
                ));
            }
        }
        self.is_first_packet = false;
    }

    /// change the selected tab and update the client with it
    pub fn set_selected_tab(&mut self, player: &Player, advancement: Option<AdvancementRef>) {
        let new = advancement
            .filter(|a| a.is_root() && a.display().is_some())
            .map(|a| a.key().clone());

        if self.last_selected_tab != new {
            player.send_packet(CSelectAdvancementsTab::new(new.clone()));
            self.last_selected_tab = new;
        }
    }

    /// Snapshots the advancements that still have progress for persistence.
    pub(crate) fn persisted_progress(&self) -> Vec<(Identifier, AdvancementProgress)> {
        self.progress
            .iter()
            .filter(|(_, progress)| progress.has_progress())
            .map(|(advancement, progress)| {
                (
                    self.registry.value_by_idx(*advancement).key().clone(),
                    progress.clone(),
                )
            })
            .collect()
    }

    /// Replaces the current progress with entries loaded from storage.
    ///
    /// The first flush after this call sends the full tree to the client, so a
    /// domain switch also clears progress that belonged to the previous domain.
    pub(crate) fn apply_persisted_progress(
        &mut self,
        entries: impl IntoIterator<Item = (Identifier, AdvancementProgress)>,
    ) {
        self.progress.clear();
        self.visible.clear();
        self.roots_to_update.clear();
        self.progress_changed.clear();
        self.last_selected_tab = None;
        self.is_first_packet = true;

        for (advancement_id, progress) in entries {
            let Some(advancement_idx) = self.registry.get_idx_by_key(&advancement_id) else {
                tracing::warn!(
                    "Ignored advancement '{}' in saved progress - it doesn't exist anymore?",
                    advancement_id
                );
                continue;
            };
            self.progress
                .start_progress(&self.registry, advancement_idx, progress);
            self.progress_changed.insert(advancement_idx);
            self.mark_for_visibility_update(advancement_idx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::borrow::Cow;
    use steel_registry::init_vanilla_registry;

    fn advancement_with_criteria() -> (Identifier, Cow<'static, str>) {
        let registry = REGISTRY.advancements.snapshot();
        let advancement = registry
            .advancements()
            .find(|advancement| !advancement.requirements().names().is_empty())
            .expect("vanilla registry should contain an advancement with criteria");
        let criterion = advancement
            .requirements()
            .names()
            .into_iter()
            .next()
            .expect("advancement should have a criterion");
        (advancement.key().clone(), criterion)
    }

    #[test]
    fn persisted_progress_skips_advancements_without_progress() {
        init_vanilla_registry();

        let (advancement_id, criterion) = advancement_with_criteria();
        let mut player_advancement = PlayerAdvancement::default();
        let advancement_idx = player_advancement
            .registry
            .get_idx_by_key(&advancement_id)
            .expect("advancement should exist in the registry");
        let untouched_id = player_advancement
            .registry
            .advancements()
            .find(|candidate| candidate.key() != &advancement_id)
            .expect("vanilla registry should contain more than one advancement")
            .key()
            .clone();
        let untouched_idx = player_advancement
            .registry
            .get_idx_by_key(&untouched_id)
            .expect("advancement should exist in the registry");

        player_advancement
            .progress
            .get_mut_or_start_progress(&player_advancement.registry, advancement_idx)
            .grant_progress(&criterion);
        player_advancement
            .progress
            .get_mut_or_start_progress(&player_advancement.registry, untouched_idx)
            .grant_progress("steel_never_completed");

        let persisted = player_advancement.persisted_progress();

        assert_eq!(persisted.len(), 1);
        assert_eq!(persisted[0].0, advancement_id);
    }

    #[test]
    fn apply_persisted_progress_restores_progress_and_resets_client_state() {
        init_vanilla_registry();

        let (advancement_id, criterion) = advancement_with_criteria();
        let mut source = PlayerAdvancement::default();
        let advancement_idx = source
            .registry
            .get_idx_by_key(&advancement_id)
            .expect("advancement should exist in the registry");
        source
            .progress
            .get_mut_or_start_progress(&source.registry, advancement_idx)
            .grant_progress(&criterion);
        let persisted = source.persisted_progress();

        let mut restored = PlayerAdvancement {
            is_first_packet: false,
            ..PlayerAdvancement::default()
        };
        restored.apply_persisted_progress(persisted);

        assert!(restored.is_first_packet, "client state must be reset");
        assert_eq!(restored.progress.len(), 1);
        let progress = restored
            .progress
            .get_progress(advancement_idx)
            .expect("progress should be restored");
        assert_eq!(
            progress.get_completed_criteria().collect::<Vec<_>>(),
            vec![&*criterion]
        );
    }

    #[test]
    fn sync_reapplies_progress_to_the_replacement_registry() {
        init_vanilla_registry();

        let (key, criterion) = advancement_with_criteria();
        let mut player_advancement = PlayerAdvancement::default();
        let advancement_idx = player_advancement
            .registry
            .get_idx_by_key(&key)
            .expect("advancement should exist in the registry");
        player_advancement
            .progress
            .get_mut_or_start_progress(&player_advancement.registry, advancement_idx)
            .grant_progress(&criterion);

        let replacement = Arc::new(REGISTRY.advancements.prepare_tree());
        player_advancement.sync(Arc::clone(&replacement));

        assert!(Arc::ptr_eq(&player_advancement.registry, &replacement));
        assert!(
            player_advancement.is_first_packet,
            "client state must be reset"
        );
        let idx = player_advancement
            .registry
            .get_idx_by_key(&key)
            .expect("advancement should exist in the replacement registry");
        let progress = player_advancement
            .progress
            .get_progress(idx)
            .expect("progress should survive the registry swap");
        assert_eq!(
            progress.get_completed_criteria().collect::<Vec<_>>(),
            vec![&*criterion]
        );
    }

    #[test]
    fn apply_persisted_progress_ignores_unknown_advancements() {
        init_vanilla_registry();

        let unknown = Identifier::vanilla_static("steel_unknown_advancement");
        let mut player_advancement = PlayerAdvancement::default();

        player_advancement.apply_persisted_progress([(unknown, AdvancementProgress::default())]);

        assert!(player_advancement.progress.is_empty());
    }
}
