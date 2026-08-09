//! Vanilla blocks whose only class-specific behavior is dropping experience.

use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::item_stack::ItemStack;
use steel_utils::value_providers::IntProvider;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::{BlockBehavior, BlockPlaceContext};
use crate::world::World;

/// Vanilla `DropExperienceBlock` behavior.
#[block_behavior]
pub struct DropExperienceBlock {
    block: BlockRef,
    #[json_arg(int_provider, json = "xp_range")]
    experience: IntProvider,
}

impl DropExperienceBlock {
    /// Creates a block behavior with its extracted experience provider.
    #[must_use]
    pub const fn new(block: BlockRef, experience: IntProvider) -> Self {
        Self { block, experience }
    }
}

impl BlockBehavior for DropExperienceBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn spawn_after_break(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        tool: &ItemStack,
        drop_experience: bool,
    ) {
        if drop_experience {
            self.try_drop_experience(world, pos, tool, &self.experience);
        }
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::{init_vanilla_registry, vanilla_blocks, vanilla_entities, vanilla_items};
    use steel_utils::{ChunkPos, Downcast as _, Identifier, WorldAabb};

    use super::*;
    use crate::behavior::{BLOCK_BEHAVIORS, init_behaviors};
    use crate::entity::entities::ExperienceOrbEntity;
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};

    fn experience_in(world: &Arc<World>, bounds: &WorldAabb) -> i32 {
        world
            .get_entities_in_aabb(bounds)
            .iter()
            .filter(|entity| entity.entity_type() == &vanilla_entities::EXPERIENCE_ORB)
            .filter_map(|entity| entity.as_ref().downcast_ref::<ExperienceOrbEntity>())
            .map(|orb| orb.value() * orb.count())
            .sum()
    }

    #[test]
    fn generated_constant_and_uniform_providers_control_ore_experience() {
        init_vanilla_registry();
        init_behaviors();
        let world = fresh_test_world("drop_experience_provider_shapes");
        let zero_pos = BlockPos::new(8, 64, 8);
        let ranged_pos = BlockPos::new(12, 64, 8);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(zero_pos));

        world.drop_resources(vanilla_blocks::IRON_ORE.default_state(), zero_pos);
        let zero_bounds = WorldAabb::new(7.0, 63.0, 7.0, 10.0, 67.0, 10.0);
        assert_eq!(experience_in(&world, &zero_bounds), 0);

        world.drop_resources(vanilla_blocks::DIAMOND_ORE.default_state(), ranged_pos);
        let ranged_bounds = WorldAabb::new(11.0, 63.0, 7.0, 14.0, 67.0, 10.0);
        assert!((3..=7).contains(&experience_in(&world, &ranged_bounds)));
    }

    #[test]
    fn silk_touch_and_disabled_experience_suppress_ore_experience() {
        init_vanilla_registry();
        init_behaviors();
        let world = fresh_test_world("drop_experience_suppression");
        let pos = BlockPos::new(8, 64, 8);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
        let state = vanilla_blocks::DIAMOND_ORE.default_state();
        let behavior = BLOCK_BEHAVIORS.get_behavior(&vanilla_blocks::DIAMOND_ORE);

        let mut silk_touch_tool = ItemStack::new(&vanilla_items::DIAMOND_PICKAXE);
        silk_touch_tool.set_enchantments(&[(Identifier::vanilla_static("silk_touch"), 1)], false);
        behavior.spawn_after_break(state, &world, pos, &silk_touch_tool, true);

        let plain_tool = ItemStack::new(&vanilla_items::DIAMOND_PICKAXE);
        assert!(plain_tool.is_correct_tool_for_drops(state));
        behavior.spawn_after_break(state, &world, pos.offset(1, 0, 0), &plain_tool, false);

        let bounds = WorldAabb::new(7.0, 63.0, 7.0, 11.0, 67.0, 10.0);
        assert_eq!(experience_in(&world, &bounds), 0);
    }
}
