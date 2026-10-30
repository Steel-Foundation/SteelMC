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
use std::time::UNIX_EPOCH;
use steel_protocol::packets::game::CSelectAdvancementsTab;
use steel_protocol::packets::game::CUpdateAdvancements;
use steel_registry::REGISTRY;
use steel_registry::advancement::registry::{AdvancementNodeRef, AdvancementRef};
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
    roots_to_update: FxHashSet<AdvancementNodeRef>,
    visible: FxHashSet<AdvancementRef>,
    progress_changed: FxHashSet<AdvancementRef>,
    /// represent the las selected advancement tab
    pub last_selected_tab: Option<AdvancementRef>,
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
        }
    }
}

impl PlayerAdvancement {
    /// Award a criterion to a player. If this completes the advancement,
    /// grant the advancement along with its rewards and display the corresponding chat message.
    pub(crate) fn award(
        &mut self,
        player: &Player,
        advancement: AdvancementRef,
        criterion: &str,
    ) -> bool {
        let mut result = false;
        let progress = self.progress.get_mut_or_start_progress(advancement);
        let was_done = progress.is_done();
        if progress.grant_progress(criterion) {
            //TODO: register advancement listener
            self.progress_changed.insert(advancement);
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
            self.mark_for_visibility_update(advancement);
        }
        result
    }

    /// revoke the specified criterion for a player.
    pub fn revoke(&mut self, advancement: AdvancementRef, criterion: &str) -> bool {
        let mut result = false;
        let progress = self.progress.get_mut_or_start_progress(advancement);
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
    fn mark_for_visibility_update(&mut self, advancement: AdvancementRef) {
        let node = REGISTRY.advancements.by_key(advancement.key());
        if let Some(node) = node {
            self.roots_to_update
                .insert(REGISTRY.advancements.root(node));
        }
    }

    /// modify the added and the removed from the available tree status
    fn update_tree_visibility(
        &mut self,
        root: AdvancementNodeRef,
        added: &mut Vec<AdvancementNodeRef>,
        removed: &mut Vec<Identifier>,
    ) {
        visibility_evaluator::evaluate_visibility(
            root,
            self,
            &mut |player_advancement, node| {
                player_advancement
                    .progress
                    .get_mut_or_start_progress(node.value())
                    .is_done()
            },
            &mut move |player_advancement, node, should_be_visible| {
                let advancement = node.value();
                if should_be_visible {
                    if player_advancement.visible.insert(advancement) {
                        added.push(node);
                        if player_advancement.progress.has_progress(advancement) {
                            player_advancement.progress_changed.insert(advancement);
                        }
                    }
                } else if player_advancement.visible.remove(advancement) {
                    removed.push(advancement.key().clone());
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
            let mut added: Vec<AdvancementNodeRef> = Vec::new();
            let mut removed: Vec<Identifier> = Vec::new();
            for root in self.roots_to_update.clone() {
                self.update_tree_visibility(root, &mut added, &mut removed);
            }
            self.roots_to_update.clear();
            for advancement in &self.progress_changed {
                if self.visible.contains(advancement) {
                    progress.insert(
                        advancement.key().clone(),
                        self.progress.get_progress(advancement).unwrap_or_else(|| {
                            unreachable!("progress changed should only contains valid advancement")
                        }),
                    );
                }
            }
            self.progress_changed.clear();
            if !progress.is_empty() || !added.is_empty() || !removed.is_empty() {
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

    /// change the selected tab and update the client with it
    pub fn set_selected_tab(&mut self, player: &Player, advancement: Option<AdvancementRef>) {
        let old = self.last_selected_tab;
        if let Some(value) = advancement
            && value.is_root()
            && value.display().is_some()
        {
            self.last_selected_tab = advancement;
        } else {
            self.last_selected_tab = None;
        }

        if old != self.last_selected_tab {
            player.send_packet(CSelectAdvancementsTab::new(
                self.last_selected_tab.map(|e| e.key().clone()),
            ));
        }
    }

    /// Snapshots the advancements that still have progress for persistence.
    pub(crate) fn persisted_progress(&self) -> Vec<(Identifier, AdvancementProgress)> {
        self.progress
            .iter()
            .filter(|(_, progress)| progress.has_progress())
            .map(|(advancement, progress)| (advancement.key().clone(), progress.clone()))
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
            let Some(advancement) = REGISTRY.advancements.value_by_key(&advancement_id) else {
                tracing::warn!(
                    "Ignored advancement '{}' in saved progress - it doesn't exist anymore?",
                    advancement_id
                );
                continue;
            };
            self.progress.start_progress(advancement, progress);
            self.progress_changed.insert(advancement);
            self.mark_for_visibility_update(advancement);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use steel_registry::init_vanilla_registry;

    fn advancement_with_criteria() -> AdvancementRef {
        REGISTRY
            .advancements
            .advancements()
            .iter()
            .copied()
            .find(|advancement| !advancement.requirements().names().is_empty())
            .expect("vanilla registry should contain an advancement with criteria")
    }

    #[test]
    fn persisted_progress_skips_advancements_without_progress() {
        init_vanilla_registry();

        let advancement = advancement_with_criteria();
        let untouched = REGISTRY
            .advancements
            .advancements()
            .iter()
            .copied()
            .find(|candidate| *candidate != advancement)
            .expect("vanilla registry should contain more than one advancement");

        let mut player_advancement = PlayerAdvancement::default();
        let criterion = advancement
            .requirements()
            .names()
            .into_iter()
            .next()
            .expect("advancement should have a criterion");
        player_advancement
            .progress
            .get_mut_or_start_progress(advancement)
            .grant_progress(&criterion);
        player_advancement
            .progress
            .get_mut_or_start_progress(untouched)
            .grant_progress("steel_never_completed");

        let persisted = player_advancement.persisted_progress();

        assert_eq!(persisted.len(), 1);
        assert_eq!(persisted[0].0, *advancement.key());
    }

    #[test]
    fn apply_persisted_progress_restores_progress_and_resets_client_state() {
        init_vanilla_registry();

        let advancement = advancement_with_criteria();
        let criterion = advancement
            .requirements()
            .names()
            .into_iter()
            .next()
            .expect("advancement should have a criterion");

        let mut source = PlayerAdvancement::default();
        source
            .progress
            .get_mut_or_start_progress(advancement)
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
            .get_progress(advancement)
            .expect("progress should be restored");
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
