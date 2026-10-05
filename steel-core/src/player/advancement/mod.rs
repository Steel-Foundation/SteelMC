//! advancement linked to the player.
//!
//! Groups the advancementPlayer, advancement progress and visibility evaluator

pub mod progress;
mod visibility_evaluator;

use crate::entity::Entity;
use crate::entity::living_entity_loot_ref;
use crate::player::Player;
use progress::{AdvancementProgress, AdvancementProgressMap};
use rustc_hash::{FxHashMap, FxHashSet};
use std::time::UNIX_EPOCH;
use steel_protocol::packets::game::c_update_advancement::CUpdateAdvancements;
use steel_registry::REGISTRY;
use steel_registry::advancement::registry::{AdvancementNodeRef, AdvancementRef};
use steel_registry::advancement::{
    AdvancementProgressData, AdvancementRewards, Criteria,
};
use steel_registry::loot_table::LootContext;
use steel_registry::vanilla_game_rules::SHOW_ADVANCEMENT_MESSAGES;
use steel_utils::Identifier;

/// Manages a player's collection of advancements.
///
/// This handles saving, loading, and tracking the state of granted / revoked advancements.
#[derive(Debug, Default)]
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

/// Grants an advancement's rewards to a player.
///
/// Mirrors vanilla `AdvancementRewards.grant`: loot is rolled through the
/// `ADVANCEMENT_REWARD` loot params (this entity + origin), stacks that fully fit in
/// the inventory trigger the pickup sound, and leftovers are dropped as items with no
/// pickup delay that only the beneficiary can collect. Recipes and functions are not
/// handled here.
pub fn grant_reward(player: &Player, reward: &AdvancementRewards) {
    player.give_experience_points(reward.experience);

    let position = player.position();
    let mut rng = rand::rng();
    let mut ctx = LootContext::new(&mut rng)
        .with_this_entity(living_entity_loot_ref(player))
        .with_origin(position.x, position.y, position.z);

    let mut changes = false;
    for loot_table in &reward.loots {
        for mut item in loot_table.get_random_items(&mut ctx) {
            if player.add_item_with_sound(&mut item) {
                changes = true;
                continue;
            }
            if let Some(drop) = player.drop_item(item, false, false) {
                drop.set_no_pickup_delay();
                drop.set_owner(Some(player.gameprofile.id));
            }
        }
    }

    if changes {
        player.broadcast_inventory_changes();
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
                grant_reward(player, &advancement.rewards);
                if let Some(display) = &advancement.display
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
        let node = REGISTRY.advancements.by_key(&advancement.key);
        if let Some(node) = node {
            self.roots_to_update.insert(node.root());
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
                    .get_mut_or_start_progress(node.value)
                    .is_done()
            },
            &mut move |player_advancement, node, should_be_visible| {
                let advancement = node.value;
                if should_be_visible {
                    if player_advancement.visible.insert(advancement) {
                        added.push(node);
                        if player_advancement.progress.has_progress(advancement) {
                            player_advancement.progress_changed.insert(advancement);
                        }
                    }
                } else if player_advancement.visible.remove(advancement) {
                    removed.push(advancement.key.clone());
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
                        advancement.key.clone(),
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
