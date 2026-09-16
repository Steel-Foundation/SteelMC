//! `FrostedIceBlock` behavior (`net.minecraft.world.level.block.FrostedIceBlock`).

use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, IntProperty};
use steel_registry::item_stack::ItemStack;
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId, Direction};

use super::ice_block::{BASE_MELT_LIGHT_LEVEL, IceBlock};
use crate::behavior::{BlockBehavior, BlockPlaceContext};
use crate::block_entity::SharedBlockEntity;
use crate::chunk::light::LightLayer;
use crate::player::Player;
use crate::world::{LevelReader, World};

const AGE: &IntProperty = &BlockStateProperties::AGE_3;
const MAX_AGE: u8 = 3;
const NEIGHBORS_TO_AGE: u8 = 4;
const NEIGHBORS_TO_MELT: u8 = 2;
const PLACE_TICK_MIN: i32 = 60;
const PLACE_TICK_MAX: i32 = 120;
const MELT_TICK_MIN: i32 = 20;
const MELT_TICK_MAX: i32 = 40;

/// Vanilla `FrostedIceBlock`.
#[block_behavior]
pub struct FrostedIceBlock {
    block: BlockRef,
}

impl FrostedIceBlock {
    /// Creates a frosted ice block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    /// Vanilla `FrostedIceBlock` neighbor-count check.
    fn fewer_neighbors_than(&self, world: &dyn LevelReader, pos: BlockPos, limit: u8) -> bool {
        let mut count = 0;
        for direction in Direction::ALL {
            if world.get_block_state(pos.relative(direction)).get_block() == self.block {
                count += 1;
                if count >= limit {
                    return false;
                }
            }
        }
        true
    }

    /// Vanilla `FrostedIceBlock.slightlyMelt`.
    ///
    /// Returns `true` when the block fully melted into water or air.
    fn slightly_melt(state: BlockStateId, world: &Arc<World>, pos: BlockPos) -> bool {
        let age = state.get_value(AGE);
        if age < MAX_AGE {
            world.set_block(
                pos,
                state.set_value(AGE, age + 1),
                UpdateFlags::UPDATE_CLIENTS,
            );
            false
        } else {
            IceBlock::melt(state, world, pos);
            true
        }
    }

    fn melt_tick_delay() -> i32 {
        rand::random_range(MELT_TICK_MIN..=MELT_TICK_MAX)
    }

    fn melt_brightness(world: &Arc<World>, pos: BlockPos, state: BlockStateId) -> bool {
        let brightness = if world.is_end_dimension_type() {
            world.light_value_at(LightLayer::Block, pos)
        } else {
            world.max_local_raw_brightness(pos, world.sky_darkening())
        };
        i32::from(brightness)
            > i32::from(BASE_MELT_LIGHT_LEVEL)
                - i32::from(state.get_value(AGE))
                - i32::from(state.get_light_dampening())
    }
}

impl BlockBehavior for FrostedIceBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn on_place(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _old_state: BlockStateId,
        _moved_by_piston: bool,
    ) {
        world.schedule_block_tick_default(
            pos,
            self.block,
            rand::random_range(PLACE_TICK_MIN..=PLACE_TICK_MAX),
        );
    }

    #[expect(
        clippy::collapsible_if,
        reason = "matches vanilla FrostedIceBlock.tick control flow"
    )]
    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        if rand::random_range(0..3) == 0
            || self.fewer_neighbors_than(world.as_ref(), pos, NEIGHBORS_TO_AGE)
        {
            if Self::melt_brightness(world, pos, state) && Self::slightly_melt(state, world, pos) {
                for direction in Direction::ALL {
                    let neighbor_pos = pos.relative(direction);
                    let neighbor = world.get_block_state(neighbor_pos);
                    if neighbor.get_block() == self.block
                        && !Self::slightly_melt(neighbor, world, neighbor_pos)
                    {
                        world.schedule_block_tick_default(
                            neighbor_pos,
                            self.block,
                            Self::melt_tick_delay(),
                        );
                    }
                }
                return;
            }
        }

        world.schedule_block_tick_default(pos, self.block, Self::melt_tick_delay());
    }

    fn handle_neighbor_changed(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        source_block: BlockRef,
        _moved_by_piston: bool,
    ) {
        if source_block == self.block
            && self.fewer_neighbors_than(world.as_ref(), pos, NEIGHBORS_TO_MELT)
        {
            IceBlock::melt(state, world, pos);
        }
    }

    fn get_clone_item_stack(
        &self,
        _block: BlockRef,
        _state: BlockStateId,
        _include_data: bool,
    ) -> Option<ItemStack> {
        None
    }

    fn player_destroy(
        &self,
        world: &Arc<World>,
        player: &Player,
        pos: BlockPos,
        state: BlockStateId,
        block_entity: Option<&SharedBlockEntity>,
        tool: &ItemStack,
    ) {
        IceBlock::new(self.block).player_destroy(world, player, pos, state, block_entity, tool);
    }

    fn random_tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        IceBlock::new(self.block).random_tick(state, world, pos);
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::blocks::block_state_ext::BlockStateExt;
    use steel_registry::blocks::properties::BlockStateProperties;
    use steel_registry::{init_vanilla_registry, vanilla_blocks, vanilla_dimension_types};
    use steel_utils::{BlockPos, ChunkPos, Direction, Identifier, types::UpdateFlags};

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::test_support::{
        TestLevel, fresh_test_world, fresh_test_world_with_dimension_type, insert_ready_full_chunk,
    };

    fn behavior() -> FrostedIceBlock {
        FrostedIceBlock::new(&vanilla_blocks::FROSTED_ICE)
    }

    fn aged(age: u8) -> BlockStateId {
        vanilla_blocks::FROSTED_ICE
            .default_state()
            .set_value(&BlockStateProperties::AGE_3, age)
    }

    fn world_with_block(key: &'static str, pos: BlockPos, state: BlockStateId) -> Arc<World> {
        init_vanilla_registry();
        init_behaviors();
        let world = fresh_test_world(key);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
        assert!(world.set_block(pos, state, UpdateFlags::UPDATE_NONE));
        world
    }

    #[test]
    fn pick_block_returns_empty() {
        init_vanilla_registry();
        let state = vanilla_blocks::FROSTED_ICE.default_state();
        assert!(
            behavior()
                .get_clone_item_stack(&vanilla_blocks::FROSTED_ICE, state, false)
                .is_none()
        );
    }

    #[test]
    fn custom_world_key_retains_end_dimension_semantics() {
        let world = fresh_test_world_with_dimension_type(
            "other",
            "the_end",
            &vanilla_dimension_types::THE_END,
        );

        assert_ne!(world.key, Identifier::vanilla_static("the_end"));
        assert!(world.is_end_dimension_type());
    }

    #[test]
    fn fewer_neighbors_than_counts_adjacent_frosted_ice() {
        init_vanilla_registry();
        let pos = BlockPos::ZERO;
        let frosted = vanilla_blocks::FROSTED_ICE.default_state();
        let isolated = TestLevel::default().with_block(pos, frosted);
        assert!(behavior().fewer_neighbors_than(&isolated, pos, NEIGHBORS_TO_AGE));
        assert!(behavior().fewer_neighbors_than(&isolated, pos, NEIGHBORS_TO_MELT));

        let mut packed = TestLevel::default().with_block(pos, frosted);
        for direction in Direction::ALL {
            packed = packed.with_block(pos.relative(direction), frosted);
        }
        assert!(!behavior().fewer_neighbors_than(&packed, pos, NEIGHBORS_TO_AGE));
        assert!(!behavior().fewer_neighbors_than(&packed, pos, NEIGHBORS_TO_MELT));

        let one_neighbor = TestLevel::default()
            .with_block(pos, frosted)
            .with_block(pos.above(), frosted);
        assert!(behavior().fewer_neighbors_than(&one_neighbor, pos, NEIGHBORS_TO_AGE));
        assert!(behavior().fewer_neighbors_than(&one_neighbor, pos, NEIGHBORS_TO_MELT));
    }

    #[test]
    fn slightly_melt_increments_age_before_melting() {
        init_vanilla_registry();
        let pos = BlockPos::new(8, 64, 8);
        let world = world_with_block("frosted_ice_age", pos, aged(0));

        assert!(!FrostedIceBlock::slightly_melt(
            world.get_block_state(pos),
            &world,
            pos
        ));
        assert_eq!(world.get_block_state(pos).get_value(AGE), 1);

        assert!(!FrostedIceBlock::slightly_melt(
            world.get_block_state(pos),
            &world,
            pos
        ));
        assert_eq!(world.get_block_state(pos).get_value(AGE), 2);

        assert!(!FrostedIceBlock::slightly_melt(
            world.get_block_state(pos),
            &world,
            pos
        ));
        assert_eq!(world.get_block_state(pos).get_value(AGE), 3);

        assert!(FrostedIceBlock::slightly_melt(
            world.get_block_state(pos),
            &world,
            pos
        ));
        assert_eq!(
            world.get_block_state(pos),
            vanilla_blocks::WATER.default_state()
        );
    }

    #[test]
    fn on_place_schedules_a_tick() {
        init_vanilla_registry();
        let pos = BlockPos::new(8, 64, 8);
        let world = world_with_block(
            "frosted_ice_place",
            pos,
            vanilla_blocks::STONE.default_state(),
        );
        let state = vanilla_blocks::FROSTED_ICE.default_state();
        assert!(world.set_block(
            pos,
            vanilla_blocks::AIR.default_state(),
            UpdateFlags::UPDATE_NONE
        ));
        behavior().on_place(
            state,
            &world,
            pos,
            vanilla_blocks::AIR.default_state(),
            false,
        );
        assert!(world.has_scheduled_block_tick(pos, &vanilla_blocks::FROSTED_ICE));
    }

    #[test]
    fn neighbor_change_melts_isolated_frosted_ice() {
        init_vanilla_registry();
        let pos = BlockPos::new(8, 64, 8);
        let world = world_with_block("frosted_ice_neighbor", pos, aged(0));

        behavior().handle_neighbor_changed(
            world.get_block_state(pos),
            &world,
            pos,
            &vanilla_blocks::FROSTED_ICE,
            false,
        );
        assert_eq!(
            world.get_block_state(pos),
            vanilla_blocks::WATER.default_state()
        );
    }

    #[test]
    fn neighbor_change_keeps_well_supported_frosted_ice() {
        init_vanilla_registry();
        let pos = BlockPos::new(8, 64, 8);
        let world = world_with_block("frosted_ice_supported", pos, aged(0));
        assert!(world.set_block(pos.above(), aged(0), UpdateFlags::UPDATE_NONE,));
        assert!(world.set_block(pos.below(), aged(0), UpdateFlags::UPDATE_NONE,));

        behavior().handle_neighbor_changed(
            world.get_block_state(pos),
            &world,
            pos,
            &vanilla_blocks::FROSTED_ICE,
            false,
        );
        assert_eq!(
            world.get_block_state(pos).get_block(),
            &vanilla_blocks::FROSTED_ICE
        );
    }
}
