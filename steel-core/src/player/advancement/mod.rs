//! advancement linked to the player.
//!
//! Groups the advancementPlayer, advancement progress and visibility evaluator

pub mod progress;
pub mod rewards;
mod visibility_evaluator;

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
    /// This is useful when the advancement registry has been updated, and the player's data needs to be aligned with the new state.
    pub fn sync(&mut self, current: Arc<FrozenAdvancementRegistry>) {
        if Arc::ptr_eq(&self.registry, &current) {
            return;
        }
        // TODO: clear advancement listeners once listener registration is implemented.
        self.progress.clear();
        self.visible.clear();
        self.roots_to_update.clear();
        self.progress_changed.clear();
        self.is_first_packet = true;
        self.last_selected_tab = None;
        self.registry = current;
        // TODO: reload advancement progress from storage once advancement saving/loading exists.
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
}
