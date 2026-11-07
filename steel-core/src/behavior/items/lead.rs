use crate::behavior::{InteractionResult, ItemBehavior, UseOnContext};
use crate::entity::entities::LeashFenceKnotEntity;
use crate::entity::{RemovalReason, leashables_leashed_to_holder_in_area_near_position};
use crate::player::Player;
use crate::world::World;
use crate::world::game_event::GameEventContext;
use std::sync::Arc;
use steel_macros::item_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::{sound_events, vanilla_game_events};
use steel_utils::BlockPos;

/// Vanilla lead item behavior.
///
/// The behavior does not handle the main functionality of a lead, but rather its block use functionality:
/// right-clicking a fence removes the knot from the user and places a knot at the right-clicked fence for
/// all entities (`Leashable`s) leashed by the user.
#[item_behavior]
pub struct LeadItem;

impl LeadItem {
    /// Binds all leashable mobs attached to a player to the block with the provided position.
    pub fn bind_player_mobs(
        player: &Player,
        world: &Arc<World>,
        pos: BlockPos,
    ) -> InteractionResult {
        let entities_to_leash = leashables_leashed_to_holder_in_area_near_position(
            world,
            pos.get_center().into(),
            player,
        );
        if entities_to_leash.is_empty() {
            return InteractionResult::Pass;
        }

        let existing_knot_exists = LeashFenceKnotEntity::get_knot(world, pos).is_some();
        let Some(knot) = LeashFenceKnotEntity::get_or_create_knot(world, pos) else {
            return InteractionResult::Pass;
        };
        let mut any_leashed = false;

        for entity in entities_to_leash {
            if let Some(leashable) = entity.as_leashable()
                && leashable.can_have_a_leash_attached_to(knot.as_ref())
            {
                leashable.set_leashed_to(&knot);
                any_leashed = true;
            }
        }

        if any_leashed {
            knot.play_sound(&sound_events::ITEM_LEAD_TIED, 1.0, 1.0);
            world.game_event(
                &vanilla_game_events::BLOCK_ATTACH,
                pos,
                &GameEventContext::new(Some(player), None),
            );
            InteractionResult::SuccessServer
        } else {
            if !existing_knot_exists {
                knot.set_removed(RemovalReason::Discarded);
            }
            InteractionResult::Pass
        }
    }
}

impl ItemBehavior for LeadItem {
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        let pos = context.hit_result.block_pos;
        let state = context.world.get_block_state(pos);
        if state.get_block().has_tag(&BlockTag::FENCES) {
            Self::bind_player_mobs(context.player, context.world, pos)
        } else {
            InteractionResult::Pass
        }
    }
}
