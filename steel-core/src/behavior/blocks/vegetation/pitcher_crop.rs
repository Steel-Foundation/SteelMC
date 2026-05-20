use std::sync::Arc;

use rand::RngExt;
use steel_macros::block_behavior;
use steel_registry::{
    REGISTRY, TaggedRegistryExt,
    blocks::{
        BlockRef,
        block_state_ext::BlockStateExt,
        properties::{BlockStateProperties, DoubleBlockHalf, EnumProperty, IntProperty},
    },
    vanilla_block_tags, vanilla_blocks, vanilla_entities, vanilla_game_rules,
};
use steel_utils::{BlockPos, BlockStateId, types::UpdateFlags};

use crate::{
    behavior::{
        BlockBehavior, BlockPlaceContext,
        blocks::vegetation::{
            Vegetation,
            bonemealable::Bonemealable,
            vegetation_block::{double_plant_can_survive, double_plant_update_shape},
        },
    },
    entity::Entity,
    world::{LevelReader, ScheduledTickAccess, World},
};

const HALF_PROPERTY: EnumProperty<DoubleBlockHalf> = BlockStateProperties::DOUBLE_BLOCK_HALF;
const AGE_PROPERTY: IntProperty = BlockStateProperties::AGE_4;

/// Behavior for Pitcher Crops
#[block_behavior]
pub struct PitcherCropBlock {
    block: BlockRef,
}

impl PitcherCropBlock {
    /// Creates a new Pitcher Crop Block Behavior
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    fn is_lower(state: BlockStateId) -> bool {
        state.get_block() == &vanilla_blocks::PITCHER_CROP
            && state.get_value(&BlockStateProperties::DOUBLE_BLOCK_HALF) == DoubleBlockHalf::Lower
    }

    fn get_growth_speed(&self, world: &Arc<World>, pos: BlockPos) -> f32 {
        let mut speed = 1.0f32;
        let below = pos.below();

        // Check 3x3 area of farmland below
        for dx in -1..=1 {
            for dz in -1..=1 {
                let check_pos = below.offset(dx, 0, dz);
                let block_state = world.get_block_state(check_pos);
                let mut block_speed = 0.0f32;

                if steel_registry::REGISTRY.blocks.is_in_tag(
                    block_state.get_block(),
                    &vanilla_block_tags::GROWS_CROPS_TAG,
                ) {
                    block_speed = 1.0;
                    // Check moisture level (defaults to 0 for non-farmland blocks)
                    let moisture = block_state
                        .try_get_value(&BlockStateProperties::MOISTURE)
                        .unwrap_or(0);
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
        let north = world.get_block_state(pos.north());
        let south = world.get_block_state(pos.south());
        let west = world.get_block_state(pos.west());
        let east = world.get_block_state(pos.east());

        let horizontal_row = self.block == west.get_block() || self.block == east.get_block();
        let vertical_row = self.block == north.get_block() || self.block == south.get_block();

        if horizontal_row && vertical_row {
            // Crops in both directions - penalty
            speed /= 2.0;
        } else {
            // Check diagonals
            let nw = world.get_block_state(pos.north().west());
            let ne = world.get_block_state(pos.north().east());
            let sw = world.get_block_state(pos.south().west());
            let se = world.get_block_state(pos.south().east());

            let has_diagonal = self.block == nw.get_block()
                || self.block == ne.get_block()
                || self.block == sw.get_block()
                || self.block == se.get_block();

            if has_diagonal {
                speed /= 2.0;
            }
        }

        speed
    }

    fn get_lower_half(
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
    ) -> Option<(BlockStateId, BlockPos)> {
        if Self::is_lower(state) {
            Some((state, pos))
        } else {
            let (state_below, pos_below) = (world.get_block_state(pos.below()), pos.below());
            if Self::is_lower(state_below) {
                Some((state_below, pos_below))
            } else {
                None
            }
        }
    }

    fn grow(world: &Arc<World>, lower_state: BlockStateId, lower_pos: BlockPos, increase: u8) {
        let new_age = (lower_state.get_value(&AGE_PROPERTY) + increase).min(4);
        if !Self::can_grow(world, lower_state, lower_pos, new_age) {
            return;
        }

        let new_state = lower_state.set_value(&AGE_PROPERTY, new_age);
        world.set_block(lower_pos, new_state, UpdateFlags::UPDATE_CLIENTS);

        if new_age >= 3 {
            world.set_block(
                lower_pos.above(),
                new_state.set_value(&HALF_PROPERTY, DoubleBlockHalf::Upper),
                UpdateFlags::UPDATE_ALL,
            );
        }
    }

    fn can_grow(world: &dyn LevelReader, state: BlockStateId, pos: BlockPos, new_age: u8) -> bool {
        let state_above = world.get_block_state(pos.above());
        state.get_value(&AGE_PROPERTY) < 4
            && world.raw_brightness(pos, 0) >= 8
            && !world.is_outside_build_height(pos.above().y())
            && (new_age < 3
                || state_above.is_air()
                || state_above.get_block() == &vanilla_blocks::PITCHER_CROP)
    }
}

impl BlockBehavior for PitcherCropBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        if self.may_place_on(
            context.world.get_block_state(context.relative_pos.below()),
            context.world,
            context.relative_pos.below(),
        ) {
            Some(self.block.default_state())
        } else {
            None
        }
    }

    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        if Self::is_lower(state) && world.raw_brightness(pos, 0) < 8 {
            return false;
        }

        double_plant_can_survive(self, state, world, pos)
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: steel_utils::Direction,
        _neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        if state.get_value(&AGE_PROPERTY) >= 3 {
            double_plant_update_shape(self, state, world, pos, direction, neighbor_state)
        } else if self.can_survive(state, world, pos) {
            state
        } else {
            vanilla_blocks::AIR.default_state()
        }
    }

    fn entity_inside(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        entity: &dyn Entity,
    ) {
        if entity.entity_type() == &vanilla_entities::RAVAGER
            && world
                .get_game_rule(&vanilla_game_rules::MOB_GRIEFING)
                .as_bool()
                == Some(true)
        {
            world.destroy_block(pos, true);
        }
    }

    fn is_randomly_ticking(&self, state: BlockStateId) -> bool {
        state.get_value(&HALF_PROPERTY) == DoubleBlockHalf::Lower
            && state.get_value(&AGE_PROPERTY) < 4
    }

    fn random_tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        let Some((lower_state, lower_pos)) = Self::get_lower_half(state, world, pos) else {
            return;
        };
        let growth_speed = self.get_growth_speed(world, lower_pos);
        let should_progress_growth =
            rand::rng().random_range(0_i32..((25.0 / growth_speed) as i32 + 1)) == 0;
        if should_progress_growth {
            Self::grow(world, lower_state, lower_pos, 1);
        }
    }

    fn as_bonemealable(&self) -> Option<&dyn Bonemealable> {
        Some(self)
    }
}

impl Vegetation for PitcherCropBlock {
    fn may_place_on(&self, state: BlockStateId, _world: &dyn LevelReader, _pos: BlockPos) -> bool {
        REGISTRY
            .blocks
            .is_in_tag(state.get_block(), &vanilla_block_tags::SUPPORTS_CROPS_TAG)
    }
}

impl Bonemealable for PitcherCropBlock {
    fn is_valid_bonemeal_target(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
    ) -> bool {
        let Some((lower_state, lower_pos)) = Self::get_lower_half(state, world, pos) else {
            return false;
        };
        if lower_state.get_block() != &vanilla_blocks::PITCHER_CROP
            || lower_state.get_value(&HALF_PROPERTY) != DoubleBlockHalf::Lower
        {
            return false;
        }

        let new_age = lower_state.get_value(&AGE_PROPERTY) + 1;

        Self::can_grow(world, lower_state, lower_pos, new_age)
    }

    fn perform_bonemeal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        _rng: &mut dyn rand::Rng,
        pos: BlockPos,
    ) {
        if let Some((lower_state, lower_pos)) = Self::get_lower_half(state, world, pos) {
            Self::grow(world, lower_state, lower_pos, 1);
        }
    }
}
