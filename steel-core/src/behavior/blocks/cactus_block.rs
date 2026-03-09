//! Cactus block behavior.
//!
//! Cactus grows up to 3 blocks tall via random ticks. It requires sand below
//! and breaks if any solid block or lava is adjacent horizontally.

use std::sync::Arc;

use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, Direction};
use steel_registry::vanilla_fluid_tags;
use steel_registry::{REGISTRY, vanilla_damage_types};
use steel_registry::{vanilla_block_tags, vanilla_blocks};
use steel_utils::{BlockPos, BlockStateId, types::UpdateFlags};

use crate::behavior::BlockStateBehaviorExt;
use crate::behavior::block::BlockBehaviour;
use crate::behavior::context::BlockPlaceContext;
use crate::entity::Entity;
use crate::entity::damage::DamageSource;
use crate::world::World;

/// Maximum cactus stack height (vanilla: 3 blocks).
const MAX_CACTUS_HEIGHT: u32 = 3;

/// Age at which cactus can attempt to grow a flower (vanilla 1.21+).
const CACTUS_FLOWER_AGE: u8 = 8;

/// Chance for small cactus (< 3 blocks) to spawn flower.
const FLOWER_CHANCE_SMALL: f64 = 0.1;

/// Chance for tall cactus (>= 3 blocks) to spawn flower.
const FLOWER_CHANCE_TALL: f64 = 0.25;

/// Behavior for cactus blocks.
///
/// Cactus has special requirements:
/// - Must be placed on sand, red sand, or another cactus
/// - Cannot have solid blocks adjacent horizontally
/// - Grows up to 3 blocks tall via random ticks
/// - Damages entities that touch it (1 HP per tick)
pub struct CactusBlock {
    block: BlockRef,
}

impl CactusBlock {
    /// Creates a new cactus block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    /// Checks if cactus can survive at the given position.
    ///
    /// Survival requirements:
    /// 1. No solid blocks on horizontal neighbors
    /// 2. No lava on horizontal neighbors
    /// 3. Block below must be `CACTUS`, `SAND`, or `RED_SAND`
    /// 4. Block above must not be liquid
    fn can_survive(world: &World, pos: BlockPos) -> bool {
        // Check horizontal neighbors - no solid blocks or lava
        for dir in [
            Direction::North,
            Direction::South,
            Direction::East,
            Direction::West,
        ] {
            let neighbor_pos = dir.relative(&pos);
            let neighbor = world.get_block_state(&neighbor_pos);
            if neighbor.is_solid() {
                return false;
            }

            let fluid = neighbor.get_fluid_state();
            if REGISTRY
                .fluids
                .is_in_tag(fluid.fluid_id, &vanilla_fluid_tags::LAVA_TAG)
            {
                return false;
            }
        }

        // Block below must be CACTUS or SAND variant
        let below_pos = pos.below();
        let below = world.get_block_state(&below_pos);
        let below_block = below.get_block();

        let valid_below = below_block == vanilla_blocks::CACTUS
            || steel_registry::REGISTRY
                .blocks
                //TODO: In 26.1 this tag is changed
                .is_in_tag(below_block, &vanilla_block_tags::SAND_TAG);

        if !valid_below {
            return false;
        }

        // Block above must not be liquid
        let above = world.get_block_state(&pos.above());

        if !above.get_fluid_state().is_empty() {
            return false;
        }

        true
    }
}

impl BlockBehaviour for CactusBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let pos = context.relative_pos;
        if Self::can_survive(context.world, pos) {
            Some(self.block.default_state())
        } else {
            None
        }
    }

    fn tick(&self, _state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        if !Self::can_survive(world, pos) {
            world.destroy_block(pos, true);
        }
    }

    fn is_randomly_ticking(&self, _state: BlockStateId) -> bool {
        true
    }

    fn random_tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        let above_pos = pos.offset(0, 1, 0);

        if !world.get_block_state(&above_pos).is_air() {
            return;
        }

        // Count cactus blocks below
        let mut height = 1u32;
        let age = state.get_value(&BlockStateProperties::AGE_15);

        while world
            .get_block_state(&pos.offset(0, -(height as i32), 0))
            .get_block()
            == vanilla_blocks::CACTUS
        {
            height += 1;
            if height == MAX_CACTUS_HEIGHT && age == 15 {
                return;
            }
        }

        // At age 8, chance to grow a cactus flower above
        if age == CACTUS_FLOWER_AGE && Self::can_survive(world, above_pos) {
            let chance = if height >= MAX_CACTUS_HEIGHT {
                FLOWER_CHANCE_TALL
            } else {
                FLOWER_CHANCE_SMALL
            };
            if rand::random::<f64>() <= chance {
                world.set_block(
                    above_pos,
                    vanilla_blocks::CACTUS_FLOWER.default_state(),
                    UpdateFlags::UPDATE_ALL,
                );
            }
        } else if age == 15 && height < MAX_CACTUS_HEIGHT {
            world.set_block(
                above_pos,
                vanilla_blocks::CACTUS.default_state(),
                UpdateFlags::UPDATE_ALL,
            );
            let new_state = state.set_value(&BlockStateProperties::AGE_15, 0);
            world.set_block(pos, new_state, UpdateFlags::UPDATE_NONE);
            world.neighbor_changed(above_pos, vanilla_blocks::CACTUS, false);
        }

        if age < 15 {
            let new_state = state.set_value(&BlockStateProperties::AGE_15, age + 1);
            world.set_block(pos, new_state, UpdateFlags::UPDATE_NONE);
        }
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &World,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        // Vanilla: only schedule a tick if the cactus can't survive
        if !Self::can_survive(world, pos) {
            world.schedule_block_tick_default(pos, self.block, 1);
        }
        state
    }

    fn entity_inside(
        &self,
        _state: BlockStateId,
        _world: &World,
        _pos: BlockPos,
        entity: &dyn Entity,
    ) {
        entity.hurt(
            &DamageSource::environment(vanilla_damage_types::CACTUS),
            1.0,
        );
    }

    // TODO: Implement when pathfinding is supported
    // fn is_pathfindable(&self, _state: BlockStateId, _path_type: PathComputationType) -> bool {
    //     false // Mobs should avoid walking into cactus
    // }
}
