//! Cactus block behavior.
//!
//! Cactus grows up to 3 blocks tall via random ticks. It requires sand below
//! and breaks if any solid block or lava is adjacent horizontally.

use std::ptr;

use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, Direction};
use steel_registry::vanilla_blocks;
use steel_utils::{BlockPos, BlockStateId, types::UpdateFlags};
use steel_registry::blocks::BlockRef;

use crate::behavior::block::BlockBehaviour;
use crate::behavior::context::BlockPlaceContext;
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
/// - Damages entities that touch it (TODO)
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
    /// 2. No lava on horizontal neighbors (TODO: fluid check)
    /// 3. Block below must be CACTUS, SAND, or RED_SAND
    /// 4. Block above must not be liquid (TODO: fluid check)
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
            // Solid check using block config (blocks with collision are solid)
            if neighbor.get_block().config.has_collision {
                return false;
            }
            // TODO: Check for lava fluid when fluid system is implemented
            // if world.get_fluid_state(&neighbor_pos).is(FluidTags::LAVA) { return false; }
        }

        // Block below must be CACTUS or SAND variant
        let below_pos = pos.offset(0, -1, 0);
        let below = world.get_block_state(&below_pos);
        let below_block = below.get_block();

        let valid_below = ptr::eq(below_block, vanilla_blocks::CACTUS)
            || steel_registry::REGISTRY
                .blocks
                .is_in_tag(below_block, &steel_utils::Identifier::vanilla_static("sand"));

        if !valid_below {
            return false;
        }

        // TODO: Block above must not be liquid
        // let above = world.get_block_state(&pos.offset(0, 1, 0));
        // if above.get_fluid_state().is_liquid() { return false; }

        true
    }
}

impl BlockBehaviour for CactusBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let pos = context.relative_pos;
        if Self::can_survive(context.world, pos) {
            Some(self.block.default_state())
        } else {
            None // Cannot place here
        }
    }

    /// Called when this cactus block is placed.
    ///
    /// HACK: Vanilla uses `scheduleTick` in `updateShape` to schedule destruction,
    /// then `tick()` performs the actual destruction on the next tick.
    /// Since SteelMC doesn't have scheduled block ticks yet, we check survival
    /// immediately in `on_place` instead. This produces the same visible result
    /// but without the 1-tick delay.
    ///
    /// TODO: Replace with proper `scheduleTick` + `tick()` pattern when available.
    fn on_place(
        &self,
        state: BlockStateId,
        world: &World,
        pos: BlockPos,
        old_state: BlockStateId,
        _moved_by_piston: bool,
    ) {
        // Don't check if replacing the same block type (e.g., state change)
        if ptr::eq(state.get_block(), old_state.get_block()) {
            return;
        }

        // HACK: Immediate destruction instead of vanilla's scheduled tick
        if !Self::can_survive(world, pos) {
            // Play break particles and sound
            world.destroy_block_effect(pos, u32::from(state.0), None);
            // Remove the block
            world.set_block(pos, vanilla_blocks::AIR.default_state(), UpdateFlags::UPDATE_ALL);
            // TODO: Drop cactus item via pop_resource
        }
    }

    fn is_randomly_ticking(&self, _state: BlockStateId) -> bool {
        true
    }

    fn random_tick(&self, state: BlockStateId, world: &World, pos: BlockPos) {
        let above_pos = pos.offset(0, 1, 0);
        
        // Vanilla line 56: if (serverLevel.isEmptyBlock(blockPos2))
        if !world.get_block_state(&above_pos).is_air() {
            return;
        }
        
        // Vanilla lines 57-64: Count cactus blocks below and check max height
        let mut i = 1u32;
        let age = state.get_value(&BlockStateProperties::AGE_15);
        
        // Vanilla: while (serverLevel.getBlockState(blockPos.below(i)).is(this))
        while ptr::eq(
            world.get_block_state(&pos.offset(0, -(i as i32), 0)).get_block(),
            vanilla_blocks::CACTUS,
        ) {
            // Vanilla: if (++i == 3 && j == 15) return;
            i += 1;
            if i == MAX_CACTUS_HEIGHT && age == 15 {
                return;
            }
        }
        
        // At this point, `i` is the cactus stack height (1 = just this block, 2 = one below, 3 = two below)
        
        // Vanilla lines 66-70: Cactus Flower logic (1.21+)
        // At age 8, there's a chance to spawn a cactus flower above
        if age == CACTUS_FLOWER_AGE && Self::can_survive(world, above_pos) {
            // Probability: 25% if height >= MAX_CACTUS_HEIGHT blocks, 10% otherwise
            let chance = if i >= MAX_CACTUS_HEIGHT {
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
        }
        
        // Vanilla lines 71-76: Age 15 and height < MAX_CACTUS_HEIGHT → grow new cactus block
        // The new block's on_place method will check can_survive and destroy if needed
        else if age == 15 && i < MAX_CACTUS_HEIGHT {
            world.set_block(
                above_pos,
                vanilla_blocks::CACTUS.default_state(),
                UpdateFlags::UPDATE_ALL,
            );
            // Reset age of current block to 0
            let new_state = state.set_value(&BlockStateProperties::AGE_15, 0);
            world.set_block(pos, new_state, UpdateFlags::UPDATE_CLIENTS);
        }
        
        // Vanilla lines 78-80: Increment age if < 15
        if age < 15 {
            let new_state = state.set_value(&BlockStateProperties::AGE_15, age + 1);
            world.set_block(pos, new_state, UpdateFlags::UPDATE_CLIENTS);
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
        // If we can't survive, destroy ourselves with animation
        if !Self::can_survive(world, pos) {
            // Play break particles and sound
            world.destroy_block_effect(pos, u32::from(state.0), None);

            // TODO: Drop cactus item using pop_resource when Arc<World> is available
            // For now, the block just disappears (loot table handles drops on player break)
            return vanilla_blocks::AIR.default_state();
        }
        state
    }

    // TODO: Implement when BlockBehaviour trait supports collision shapes
    // fn get_collision_shape(&self, _state: BlockStateId) -> VoxelShape {
    //     // Vanilla: SHAPE_COLLISION = Block.column(14.0, 0.0, 15.0)
    //     // Slightly smaller than full block (14/16 width, 15/16 height)
    //     // This makes entities touch the cactus and take damage
    // }

    // TODO: Implement when BlockBehaviour trait supports outline shapes
    // fn get_shape(&self, _state: BlockStateId) -> VoxelShape {
    //     // Vanilla: SHAPE = Block.column(14.0, 0.0, 16.0)
    //     // 14/16 width, full height for visual outline
    // }

    // TODO: Implement when entity-block collision is supported
    // fn entity_inside(&self, _state: BlockStateId, world: &World, pos: BlockPos, entity: &Entity) {
    //     // Vanilla: entity.hurt(level.damageSources().cactus(), 1.0F);
    //     // Deal 1 damage (half heart) to entities inside the cactus
    // }

    // TODO: Implement when pathfinding is supported
    // fn is_pathfindable(&self, _state: BlockStateId, _path_type: PathComputationType) -> bool {
    //     false // Mobs should avoid walking into cactus
    // }
}
