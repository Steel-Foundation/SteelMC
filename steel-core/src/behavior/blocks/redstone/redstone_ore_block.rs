//! Vanilla redstone and deepslate redstone ore behavior.

use std::sync::Arc;

use rand::RngExt as _;
use steel_macros::block_behavior;
use steel_registry::REGISTRY;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::enchantment_effect::EnchantmentEffectComponent;
use steel_registry::item_stack::ItemStack;
use steel_utils::types::{InteractionHand, UpdateFlags};
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::{
    BlockBehavior, BlockHitResult, BlockPlaceContext, InteractionResult, InventoryAccess,
    PlacementSource,
};
use crate::entity::Entity;
use crate::player::Player;
use crate::world::World;

/// Vanilla `RedStoneOreBlock` behavior shared by both ore variants.
#[block_behavior]
pub struct RedStoneOreBlock {
    block: BlockRef,
}

impl RedStoneOreBlock {
    /// Creates a redstone ore behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    fn interact(state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        if state.get_value(&BlockStateProperties::LIT) {
            return;
        }

        world.set_block(
            pos,
            state.set_value(&BlockStateProperties::LIT, true),
            UpdateFlags::UPDATE_ALL,
        );
    }

    fn process_block_experience(tool: &ItemStack, base_experience: i32) -> i32 {
        tool.apply_unconditional_enchantment_value_effects(
            EnchantmentEffectComponent::BlockExperience,
            base_experience as f32,
        ) as i32
    }
}

impl BlockBehavior for RedStoneOreBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn attack(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos, _player: &Player) {
        Self::interact(state, world, pos);
    }

    fn step_on(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos, entity: &dyn Entity) {
        if !entity.is_stepping_carefully() {
            Self::interact(state, world, pos);
        }
        self.default_step_on(state, world, pos, entity);
    }

    fn use_item_on(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
        _hand: InteractionHand,
        hit_result: &BlockHitResult,
        inv: &mut InventoryAccess,
    ) -> InteractionResult {
        Self::interact(state, world, pos);

        let held_item = inv.with_item(|stack| stack.item());
        if REGISTRY.items.is_block_item(held_item)
            && BlockPlaceContext::new(world, PlacementSource::player_hand(player, inv), hit_result)
                .can_place()
        {
            InteractionResult::Pass
        } else {
            InteractionResult::Success
        }
    }

    fn random_tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        if state.get_value(&BlockStateProperties::LIT) {
            world.set_block(
                pos,
                state.set_value(&BlockStateProperties::LIT, false),
                UpdateFlags::UPDATE_ALL,
            );
        }
    }

    fn spawn_after_break(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        tool: &ItemStack,
        drop_experience: bool,
    ) {
        if !drop_experience {
            return;
        }

        let base_experience = rand::rng().random_range(1..=5);
        let experience = Self::process_block_experience(tool, base_experience);
        world.pop_experience(pos, experience);
    }

    // `animateTick` and interaction particles use client-local `Level.addParticle`.
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use glam::DVec3;
    use steel_registry::entity_type::EntityTypeRef;
    use steel_registry::test_support::init_test_registry;
    use steel_registry::{vanilla_blocks, vanilla_entities, vanilla_items};
    use steel_utils::{ChunkPos, Identifier, WorldAabb};

    use super::*;
    use crate::behavior::{BLOCK_BEHAVIORS, init_behaviors};
    use crate::entity::EntityBase;
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};

    struct StepEntity {
        base: EntityBase,
        careful: bool,
    }

    crate::entity::impl_test_downcast_type!(StepEntity);

    impl StepEntity {
        fn new(id: i32, world: &Arc<World>, careful: bool) -> Self {
            Self {
                base: EntityBase::new(
                    id,
                    DVec3::new(8.5, 65.0, 8.5),
                    vanilla_entities::PIG.dimensions,
                    Arc::downgrade(world),
                ),
                careful,
            }
        }
    }

    impl Entity for StepEntity {
        fn base(&self) -> &EntityBase {
            &self.base
        }

        fn entity_type(&self) -> EntityTypeRef {
            &vanilla_entities::PIG
        }

        fn is_stepping_carefully(&self) -> bool {
            self.careful
        }
    }

    #[test]
    fn both_ore_variants_light_from_steps_and_extinguish_on_random_ticks() {
        init_test_registry();
        init_behaviors();
        let world = fresh_test_world("redstone_ore_steps");
        let first_pos = BlockPos::new(8, 64, 8);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(first_pos));

        let careful_entity = StepEntity::new(7_003, &world, true);
        let ordinary_entity = StepEntity::new(7_004, &world, false);

        for (offset, block) in [
            &vanilla_blocks::REDSTONE_ORE,
            &vanilla_blocks::DEEPSLATE_REDSTONE_ORE,
        ]
        .into_iter()
        .enumerate()
        {
            let pos = first_pos.offset(offset as i32, 0, 0);
            let unlit = block.default_state();
            assert!(!unlit.is_randomly_ticking());
            assert!(world.set_block(pos, unlit, UpdateFlags::UPDATE_NONE));

            let behavior = BLOCK_BEHAVIORS.get_behavior(block);
            behavior.step_on(unlit, &world, pos, &careful_entity);
            assert!(
                !world
                    .get_block_state(pos)
                    .get_value(&BlockStateProperties::LIT)
            );

            behavior.step_on(unlit, &world, pos, &ordinary_entity);
            let lit = world.get_block_state(pos);
            assert!(lit.get_value(&BlockStateProperties::LIT));
            assert!(lit.is_randomly_ticking());

            behavior.random_tick(lit, &world, pos);
            assert!(
                !world
                    .get_block_state(pos)
                    .get_value(&BlockStateProperties::LIT)
            );
        }
    }

    #[test]
    fn world_drop_resources_dispatches_redstone_ore_experience() {
        init_test_registry();
        init_behaviors();
        let world = fresh_test_world("redstone_ore_post_break");
        let pos = BlockPos::new(8, 64, 8);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));

        world.drop_resources(vanilla_blocks::REDSTONE_ORE.default_state(), pos);

        let query = WorldAabb::new(7.0, 63.0, 7.0, 10.0, 67.0, 10.0);
        assert!(
            world
                .get_entities_in_aabb(&query)
                .iter()
                .any(|entity| entity.entity_type() == &vanilla_entities::EXPERIENCE_ORB)
        );
    }

    #[test]
    fn silk_touch_suppresses_redstone_ore_experience() {
        init_test_registry();
        let plain_tool = ItemStack::new(&vanilla_items::DIAMOND_PICKAXE);
        assert_eq!(
            RedStoneOreBlock::process_block_experience(&plain_tool, 5),
            5
        );

        let mut silk_touch_tool = ItemStack::new(&vanilla_items::DIAMOND_PICKAXE);
        silk_touch_tool.set_enchantments(&[(Identifier::vanilla_static("silk_touch"), 1)], false);
        assert_eq!(
            RedStoneOreBlock::process_block_experience(&silk_touch_tool, 5),
            0
        );
    }
}
