pub mod progress;
pub mod visibility_evaluator;

use std::sync::Arc;
use crate::player::Player;
use progress::{AdvancementProgress, AdvancementProgressMap};
use rustc_hash::{FxHashMap, FxHashSet};
use std::time::UNIX_EPOCH;
use steel_protocol::packets::game::c_update_advancement::CUpdateAdvancements;
use steel_registry::REGISTRY;
use steel_registry::advancement::registry::{AdvancementNode, AdvancementNodeRef, AdvancementRef};
use steel_registry::advancement::{Advancement, AdvancementProgressData, AdvancementRewards, Criteria};
use steel_registry::loot_table::LootContext;
use steel_utils::Identifier;
use crate::entity::{Entity, LivingEntity};
use crate::entity::living_entity::living_entity_loot_ref;

/// Manages a player's collection of advancements.
///
/// This handles saving, loading, and tracking the state of granted / revoked advancements.
#[derive(Debug, Default)]
pub struct PlayerAdvancement {
    pub progress: AdvancementProgressMap,
    pub is_first_packet: bool,
    pub roots_to_update: FxHashSet<AdvancementNodeRef>,
    pub visible: FxHashSet<AdvancementRef>,
    pub progress_changed: FxHashSet<AdvancementRef>,
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
    pub(crate) fn award(
        &mut self,
        advancement: AdvancementRef,
        criterion: &str,
    ) {
        let mut result = false;
        let progress = self.progress.get_mut_or_start_progress(advancement);
        let was_done = progress.is_done();
        if progress.grantProgress(criterion) {
            //self.unregisterListeners(advancement);
            self.progress_changed.add(advancement);
            result = true;
            if !was_done && progress.isDone() {
                advancement.rewards.grant(self.player);
                advancement.value().display().ifPresent(display -> {
                    if display.shouldAnnounceChat() && this.player.level().getGameRules().get(GameRules.SHOW_ADVANCEMENT_MESSAGES) {
                        this.playerList.broadcastSystemMessage(display.getType().createAnnouncement(holder, this.player), false);
                    }
                });
            }
        }

        if !was_done && progress.isDone() {
            this.markForVisibilityUpdate(holder);
        }

        result;
    }

    pub fn revoke(&mut self, advancement: AdvancementRef, criterion: &str) {
        let mut result = false;
        let progress = self.progress.get_mut_or_start_progress(advancement);
        let was_done = progress.is_done();
        if progress.revoke_progress(criterion) {
            //self.registerListeners(advancement);
            self.progress_changed.add(advancement);
            result = true;
        }

        if was_done && !progress.is_done() {
            self.mark_for_visibility_update(advancement);
        }

        result
    }

    fn mark_for_visibility_update(&mut self, advancement: AdvancementRef) {
        let node = REGISTRY.advancements.get_by_key(&advancement.key);
        if let Some(node) = node {
            self.roots_to_update.insert(node.root());
        }
    }

    fn update_tree_visibility(
        &mut self,
        root: &AdvancementNode,
        added: &mut Vec<AdvancementRef>,
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
                        added.push(advancement);
                        if player_advancement.progress.map.contains_key(advancement) {
                            player_advancement.progress_changed.insert(advancement);
                        }
                    }
                } else if player_advancement.visible.remove(advancement) {
                    removed.push(advancement.key.clone());
                }
            },
        );
    }

    pub fn flush_dirty(&mut self, player: &Player, show_advancement: bool) {
        if self.is_first_packet || !self.roots_to_update.is_empty() {
            let mut progress: FxHashMap<Identifier, &AdvancementProgress> = FxHashMap::default();
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
