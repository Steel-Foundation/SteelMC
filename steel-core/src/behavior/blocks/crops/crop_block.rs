//! Crop block implementation (wheat, carrots, potatoes, beetroot).

use std::sync::Arc;

use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, IntProperty};
use steel_registry::item_stack::ItemStack;
use steel_registry::items::item::BlockHitResult;
use steel_registry::vanilla_blocks;
use steel_utils::types::InteractionHand;
use steel_utils::{BlockPos, BlockStateId, types::UpdateFlags};

use crate::behavior::InteractionResult;
use crate::behavior::block::BlockBehaviour;
use crate::behavior::blocks::crops::bonemealable::{Bonemealable, CropBonemealExt};
use crate::behavior::context::BlockPlaceContext;
use crate::player::Player;
use crate::world::World;

/// Behavior for crop blocks (wheat, carrots, potatoes).
///
/// Crops grow through random ticks when placed on farmland with sufficient light.
/// Growth speed is affected by nearby farmland moisture and crop arrangement.
pub struct CropBlock {
    block: BlockRef,
    age_property: IntProperty,
    max_age: u8,
}

pub trait CropLike {
    fn block(&self) -> BlockRef;
    fn age_property(&self) -> &IntProperty;
    fn max_age(&self) -> u8;

    fn get_age(&self, state: BlockStateId) -> u8 {
        state.get_value(self.age_property())
    }

    fn get_state_for_age(&self, age: u8) -> BlockStateId {
        self.block()
            .default_state()
            .set_value(self.age_property(), age)
    }

    fn is_max_age(&self, state: BlockStateId) -> bool {
        state.get_value(self.age_property()) >= self.max_age()
    }

    /// Calculates the growth speed based on surrounding farmland.
    ///
    /// Factors affecting growth speed:
    /// - Farmland below: +1.0 (dry) or +3.0 (hydrated)
    /// - Adjacent farmland: +0.25 (dry) or +0.75 (hydrated)
    /// - Same crop in row: /2.0 speed penalty
    fn get_growth_speed(&self, world: &World, pos: BlockPos) -> f32 {
        let mut speed = 1.0f32;
        let below = pos.below();

        // Check 3x3 area of farmland below
        for dx in -1..=1 {
            for dz in -1..=1 {
                let check_pos = below.offset(dx, 0, dz);
                let block_state = world.get_block_state(&check_pos);
                let mut block_speed = 0.0f32;

                if block_state.get_block() == vanilla_blocks::FARMLAND {
                    block_speed = 1.0;
                    // Check moisture level
                    let moisture = block_state.get_value(&BlockStateProperties::MOISTURE);
                    if moisture > 0 {
                        block_speed = 3.0;
                    }
                }

                // Diagonal/adjacent farmland contributes less
                if dx != 0 || dz != 0 {
                    block_speed /= 4.0;
                }

                speed += block_speed;
            }
        }

        // Check for same crop in adjacent positions (reduces growth speed)
        let north = world.get_block_state(&pos.north());
        let south = world.get_block_state(&pos.south());
        let west = world.get_block_state(&pos.west());
        let east = world.get_block_state(&pos.east());

        let block = self.block();

        let horizontal_row = block == west.get_block() || block == east.get_block();
        let vertical_row = block == north.get_block() || block == south.get_block();

        if horizontal_row && vertical_row {
            // Crops in both directions - penalty
            speed /= 2.0;
        } else {
            // Check diagonals
            let nw = world.get_block_state(&pos.north().west());
            let ne = world.get_block_state(&pos.north().east());
            let sw = world.get_block_state(&pos.south().west());
            let se = world.get_block_state(&pos.south().east());

            let has_diagonal = block == nw.get_block()
                || block == ne.get_block()
                || block == sw.get_block()
                || block == se.get_block();

            if has_diagonal {
                speed /= 2.0;
            }
        }

        speed
    }

    fn on_random_tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        // TODO: Check light level >= 9 when light engine is implemented
        // For now, assume sufficient light

        let age = self.get_age(state);
        if age < self.max_age() {
            let growth_speed = self.get_growth_speed(world, pos);

            // Random chance to grow based on growth speed
            // Vanilla formula: random.nextInt((int)(25.0F / growthSpeed) + 1) == 0
            let growth_chance = (25.0 / growth_speed) as u32 + 1;

            if rand::random::<u32>().is_multiple_of(growth_chance) {
                let new_state = self.get_state_for_age(age + 1);
                world.set_block(pos, new_state, UpdateFlags::UPDATE_CLIENTS);
            }
        }
    }
}

impl CropBlock {
    /// Creates a new crop block behavior with the default age property (0-7).
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self {
            block,
            age_property: BlockStateProperties::AGE_7,
            max_age: 7,
        }
    }

    /// Creates a new crop block behavior with a custom age property.
    #[must_use]
    pub const fn with_age(block: BlockRef, age_property: IntProperty, max_age: u8) -> Self {
        Self {
            block,
            age_property,
            max_age,
        }
    }
}

impl CropLike for CropBlock {
    fn block(&self) -> BlockRef {
        self.block
    }

    fn age_property(&self) -> &IntProperty {
        &self.age_property
    }

    fn max_age(&self) -> u8 {
        self.max_age
    }
}

impl Bonemealable for CropBlock {
    fn get_age_increase(&self, _world: &World) -> u8 {
        rand::random_range(2..=5)
    }

    fn apply_bonemeal(&self, state: BlockStateId, world: &World, pos: BlockPos) {
        self.default_apply_bonemeal(state, world, pos);
    }

    fn is_bonemealable(&self, state: BlockStateId, _world: &World, _pos: BlockPos) -> bool {
        !self.is_max_age(state)
    }
}

impl BlockBehaviour for CropBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        // Crops are placed at age 0
        Some(self.get_state_for_age(0))
    }

    fn is_randomly_ticking(&self, state: BlockStateId) -> bool {
        // Only tick if not fully grown
        !self.is_max_age(state)
    }

    fn random_tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        self.on_random_tick(state, world, pos);
    }

    fn use_item_on(
        &self,
        item_stack: &ItemStack,
        state: BlockStateId,
        world: &World,
        pos: BlockPos,
        _player: &Player,
        _hand: InteractionHand,
        _hit_result: &BlockHitResult,
    ) -> InteractionResult {
        self.default_use_item_on(item_stack, state, world, pos)
    }
}
