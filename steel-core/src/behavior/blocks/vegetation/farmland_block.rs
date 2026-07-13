//! Farmland block implementation.

use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::{vanilla_blocks, vanilla_game_events, vanilla_game_rules};
use steel_utils::{BlockPos, BlockStateId, types::UpdateFlags};

use crate::behavior::block::{
    BlockBehavior, EntityFallDamage, EntityFallOnContext, push_entities_up,
};
use crate::behavior::context::BlockPlaceContext;
use crate::entity::Entity;
use crate::world::World;
use crate::world::game_event_context::GameEventContext;

/// Maximum moisture level for farmland.
const MAX_MOISTURE: u8 = 7;
const TRAMPLE_VOLUME_THRESHOLD: f64 = 0.512;

/// Behavior for farmland blocks.
///
/// Farmland has a moisture level (0-7) that affects crop growth speed.
/// - Moisture increases to max (7) when near water
/// - Moisture decreases by 1 each random tick when not near water
/// - Farmland turns back to dirt when moisture reaches 0 and no crop is planted
#[block_behavior]
pub struct FarmlandBlock {
    block: BlockRef,
}

impl FarmlandBlock {
    /// Creates a new farmland block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    /// Checks if there is water within a 9x9x2 area centered on the farmland.
    /// Vanilla checks from (-4, 0, -4) to (4, 1, 4) relative to the farmland.
    ///
    /// This checks both water blocks and waterlogged blocks (matching vanilla's
    /// FluidTags.WATER check on fluid state).
    fn is_near_water(world: &Arc<World>, pos: BlockPos) -> bool {
        for dy in 0..=1 {
            for dx in -4..=4 {
                for dz in -4..=4 {
                    let check_pos = pos.offset(dx, dy, dz);
                    let state = world.get_block_state(check_pos);

                    // Check if block is water
                    if state.get_block() == &vanilla_blocks::WATER {
                        return true;
                    }

                    // Check if block is waterlogged
                    if state
                        .try_get_value(&BlockStateProperties::WATERLOGGED)
                        .unwrap_or(false)
                    {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Checks if the block above is a crop that should maintain the farmland.
    /// This prevents farmland from turning to dirt when crops are planted.
    fn should_maintain_farmland(world: &Arc<World>, pos: BlockPos) -> bool {
        let above = world.get_block_state(pos.above());
        let block = above.get_block();

        // Check for crops that maintain farmland
        // In vanilla this uses the MAINTAINS_FARMLAND tag
        block == &vanilla_blocks::WHEAT
            || block == &vanilla_blocks::CARROTS
            || block == &vanilla_blocks::POTATOES
            || block == &vanilla_blocks::BEETROOTS
            || block == &vanilla_blocks::MELON_STEM
            || block == &vanilla_blocks::PUMPKIN_STEM
            || block == &vanilla_blocks::ATTACHED_MELON_STEM
            || block == &vanilla_blocks::ATTACHED_PUMPKIN_STEM
            || block == &vanilla_blocks::TORCHFLOWER_CROP
            || block == &vanilla_blocks::PITCHER_CROP
    }

    #[must_use]
    fn should_turn_to_dirt_on_fall(
        context: EntityFallOnContext<'_>,
        mob_griefing: bool,
        random_float: f32,
    ) -> bool {
        f64::from(random_float) < context.fall_distance - 0.5
            && context.entity.is_living_entity
            && (context.entity.is_player() || mob_griefing)
            && context.entity.bounding_box_width_squared_height() > TRAMPLE_VOLUME_THRESHOLD
    }

    /// Turns the farmland into dirt.
    fn turn_to_dirt(
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        source_entity: Option<&dyn Entity>,
    ) {
        let dirt_state = push_entities_up(state, vanilla_blocks::DIRT.default_state(), world, pos);
        if world.set_block(pos, dirt_state, UpdateFlags::UPDATE_ALL) {
            world.game_event(
                &vanilla_game_events::BLOCK_CHANGE,
                pos,
                &GameEventContext::new(source_entity, Some(dirt_state)),
            );
        }
    }
}

impl BlockBehavior for FarmlandBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        // Farmland is placed with moisture 0
        Some(
            self.block
                .default_state()
                .set_value(&BlockStateProperties::MOISTURE, 0u8),
        )
    }

    fn is_randomly_ticking(&self, _state: BlockStateId) -> bool {
        // Farmland always needs random ticks to manage moisture
        true
    }

    fn random_tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        let moisture: u8 = state.get_value(&BlockStateProperties::MOISTURE);

        // TODO: Check for rain when weather is implemented
        let is_near_water = Self::is_near_water(world, pos);

        if !is_near_water {
            // Not near water - decrease moisture or turn to dirt
            if moisture > 0 {
                // Decrease moisture by 1
                let new_state = state.set_value(&BlockStateProperties::MOISTURE, moisture - 1);
                world.set_block(pos, new_state, UpdateFlags::UPDATE_CLIENTS);
            } else if !Self::should_maintain_farmland(world, pos) {
                // No moisture and no crop - turn to dirt
                Self::turn_to_dirt(state, world, pos, None);
            }
        } else if moisture < MAX_MOISTURE {
            // Near water - hydrate to max
            let new_state = state.set_value(&BlockStateProperties::MOISTURE, MAX_MOISTURE);
            world.set_block(pos, new_state, UpdateFlags::UPDATE_CLIENTS);
        }
    }

    fn fall_on(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        context: EntityFallOnContext<'_>,
    ) -> Option<EntityFallDamage> {
        let mob_griefing = world
            .get_game_rule(&vanilla_game_rules::MOB_GRIEFING)
            .as_bool()
            == Some(true);
        let random_float = rand::random::<f32>();
        if Self::should_turn_to_dirt_on_fall(context, mob_griefing, random_float) {
            Self::turn_to_dirt(state, world, pos, context.source_entity());
        }

        self.default_fall_on(state, world, pos, context)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use steel_registry::{sound_events, vanilla_entities};

    use crate::behavior::EntityFallOnFacts;

    fn fall_context(
        fall_distance: f64,
        entity_type_is_player: bool,
        is_living_entity: bool,
        bounding_box_width: f64,
        bounding_box_height: f64,
    ) -> EntityFallOnContext<'static> {
        EntityFallOnContext::new(
            fall_distance,
            false,
            EntityFallOnFacts::new(
                if entity_type_is_player {
                    &vanilla_entities::PLAYER
                } else {
                    &vanilla_entities::ZOMBIE
                },
                is_living_entity,
                bounding_box_width,
                bounding_box_height,
                (
                    &sound_events::ENTITY_GENERIC_SMALL_FALL,
                    &sound_events::ENTITY_GENERIC_BIG_FALL,
                ),
            ),
            None,
        )
    }

    #[test]
    fn fall_trampling_requires_random_below_fall_distance_minus_half() {
        assert!(FarmlandBlock::should_turn_to_dirt_on_fall(
            fall_context(1.0, true, true, 0.6, 1.8),
            false,
            0.49,
        ));
        assert!(!FarmlandBlock::should_turn_to_dirt_on_fall(
            fall_context(1.0, true, true, 0.6, 1.8),
            false,
            0.5,
        ));
    }

    #[test]
    fn non_player_living_entities_need_mob_griefing_to_trample() {
        let context = fall_context(1.0, false, true, 0.6, 1.8);

        assert!(!FarmlandBlock::should_turn_to_dirt_on_fall(
            context, false, 0.0,
        ));
        assert!(FarmlandBlock::should_turn_to_dirt_on_fall(
            context, true, 0.0,
        ));
    }

    #[test]
    fn small_or_non_living_entities_do_not_trample() {
        assert!(!FarmlandBlock::should_turn_to_dirt_on_fall(
            fall_context(1.0, true, false, 0.6, 1.8),
            false,
            0.0,
        ));
        assert!(!FarmlandBlock::should_turn_to_dirt_on_fall(
            fall_context(1.0, true, true, 0.25, 0.25),
            false,
            0.0,
        ));
    }
}
