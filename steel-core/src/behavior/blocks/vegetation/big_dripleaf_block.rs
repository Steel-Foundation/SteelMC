use rand::{Rng, RngExt};
use std::sync::Arc;
use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, BoolProperty, EnumProperty, Tilt};
use steel_registry::fluid::{FluidState, FluidStateExt};
use steel_registry::sound_event::SoundEventRef;
use steel_registry::sound_events::{BLOCK_BIG_DRIPLEAF_TILT_DOWN, BLOCK_BIG_DRIPLEAF_TILT_UP};
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_blocks;
use steel_registry::vanilla_fluids::{self};
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId, Direction};

use super::BlockRef;
use crate::behavior::BlockStateBehaviorExt;
use crate::behavior::block::BlockBehavior;
use crate::behavior::blocks::BigDripleafStemBlock;
use crate::behavior::blocks::vegetation::bonemealable::{BonemealAction, Bonemealable};
use crate::behavior::context::BlockPlaceContext;
use crate::entity::{Entity, InsideBlockEffectCollector};
use crate::world::tick_scheduler::TickPriority;
use crate::world::{LevelReader, ScheduledTickAccess, World};

const TILT: EnumProperty<Tilt> = BlockStateProperties::TILT;
const WATERLOGGED: BoolProperty = BlockStateProperties::WATERLOGGED;
const FACING: EnumProperty<Direction> = BlockStateProperties::FACING;

/// Vanilla `BigDripleafBlock` survival.
///
/// Survives if the block below is big dripleaf (self), big dripleaf stem, or
/// in the `SUPPORTS_BIG_DRIPLEAF` tag.
// TODO: Implement projectile tilt.
#[block_behavior]
pub struct BigDripleafBlock {
    block: BlockRef,
}

impl BigDripleafBlock {
    /// Creates a new big dripleaf block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
    fn can_entity_tilt(pos: &BlockPos, entity: &dyn Entity) -> bool {
        entity.on_ground() && entity.position().y > f64::from(pos.y()) + 0.6875_f64
    }
    fn set_tilt_and_schedule_tick(
        &self,
        state_id: BlockStateId,
        world: &Arc<World>,
        pos: &BlockPos,
        tilt: Tilt,
        sound_wrapper: Option<SoundEventRef>,
    ) {
        Self::set_tilt(state_id, world, pos, tilt.clone());
        if let Some(tilt_sound) = sound_wrapper {
            Self::play_tilt_sound(world, pos, tilt_sound);
        }
        let tick_delay = match tilt {
            Tilt::None => None,
            Tilt::Unstable | Tilt::Partial => Some(10),
            Tilt::Full => Some(100),
        };
        if let Some(tick_delay) = tick_delay {
            world.schedule_block_tick(*pos, self.block, tick_delay, TickPriority::Normal);
        }
    }
    fn set_tilt(state_id: BlockStateId, world: &Arc<World>, pos: &BlockPos, new_tilt: Tilt) {
        world.set_block(
            *pos,
            state_id.set_value(&TILT, new_tilt),
            UpdateFlags::UPDATE_ALL,
        );
    }
    fn play_tilt_sound(world: &Arc<World>, pos: &BlockPos, tilt_sound: SoundEventRef) {
        let pitch = rand::rng().random_range(0.8f32..1.2f32);
        world.play_block_sound(tilt_sound, *pos, 1f32, pitch, None);
    }
    fn reset_tilt(state_id: BlockStateId, world: &Arc<World>, pos: &BlockPos) {
        Self::set_tilt(state_id, world, pos, Tilt::None);
        let tilt = state_id.get_value(&TILT);

        if tilt != Tilt::None {
            Self::play_tilt_sound(world, pos, &BLOCK_BIG_DRIPLEAF_TILT_UP);
        }
    }
    fn can_replace(old_state: BlockStateId) -> bool {
        old_state.is_air()
            || old_state.get_block() == &vanilla_blocks::WATER
            || old_state.get_block() == &vanilla_blocks::SMALL_DRIPLEAF
    }
    /// Determines whether big dripleaf can grow into target position
    pub fn can_grow_into(world: &dyn LevelReader, pos: BlockPos) -> bool {
        let state = world.get_block_state(pos);
        !world.is_outside_build_height(pos.y()) && Self::can_replace(state)
    }
    /// Places big dripleaf block on target position with properties
    pub fn place(
        world: &Arc<World>,
        pos: BlockPos,
        fluid_state: FluidState,
        facing: Direction,
    ) -> bool {
        let new_state = vanilla_blocks::BIG_DRIPLEAF
            .default_state()
            .set_value(&WATERLOGGED, fluid_state.is_water())
            .set_value(&FACING, facing);
        world.set_block(pos, new_state, UpdateFlags::UPDATE_ALL)
    }
}

impl BlockBehavior for BigDripleafBlock {
    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        let below = world.get_block_state(pos.below());
        let below_block = below.get_block();
        below_block == self.block
            || below_block == &vanilla_blocks::BIG_DRIPLEAF_STEM
            || below_block.has_tag(&BlockTag::SUPPORTS_BIG_DRIPLEAF)
    }
    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        if direction == Direction::Down && !self.can_survive(state, world, pos) {
            return vanilla_blocks::AIR.default_state();
        }
        if state.get_value(&WATERLOGGED) {
            world.schedule_fluid_tick_default(
                pos,
                &vanilla_fluids::WATER,
                vanilla_fluids::WATER.tick_delay as i32,
            );
        }

        if direction == Direction::Up && neighbor_state.get_block() == self.block {
            vanilla_blocks::BIG_DRIPLEAF_STEM
                .default_state()
                .with_properties_of(state)
        } else {
            state
        }
    }
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let below_state = context.world.get_block_state(context.relative_pos.below());
        let below_is_dripleaf_part = below_state.get_block() == &vanilla_blocks::BIG_DRIPLEAF
            || below_state.get_block() == &vanilla_blocks::BIG_DRIPLEAF_STEM;
        let facing = {
            if below_is_dripleaf_part {
                below_state.get_value(&FACING)
            } else {
                context.horizontal_direction.opposite()
            }
        };
        Some(
            self.block
                .default_state()
                .set_value(&WATERLOGGED, context.is_water_source())
                .set_value(&FACING, facing),
        )
    }
    fn entity_inside(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        entity: &dyn Entity,
        _effect_collector: &mut InsideBlockEffectCollector,
        _is_precise: bool,
    ) {
        let tilt = state.get_value(&TILT);
        //TODO: also check !level.hasNeighborSignal(pos)) once steel implements redstone
        if tilt == Tilt::None && BigDripleafBlock::can_entity_tilt(&pos, entity) {
            Self::set_tilt_and_schedule_tick(self, state, world, &pos, Tilt::Unstable, None);
        }
    }
    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        //if block_receives_redstone_power(world, pos) {
        //    reset_tilt(state.id, world, pos);
        //} else {
        let tilt = state.get_value(&TILT);

        if tilt == Tilt::Unstable {
            Self::set_tilt_and_schedule_tick(
                self,
                state,
                world,
                &pos,
                Tilt::Partial,
                Some(&BLOCK_BIG_DRIPLEAF_TILT_DOWN),
            );
        } else if tilt == Tilt::Partial {
            Self::set_tilt_and_schedule_tick(
                self,
                state,
                world,
                &pos,
                Tilt::Full,
                Some(&BLOCK_BIG_DRIPLEAF_TILT_DOWN),
            );
        } else if tilt == Tilt::Full {
            Self::reset_tilt(state, world, &pos);
        }
        //}
    }
    fn as_bonemealable(&self) -> Option<&dyn Bonemealable> {
        Some(self)
    }
}
impl Bonemealable for BigDripleafBlock {
    fn is_valid_bonemeal_target(
        &self,
        _state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
    ) -> bool {
        let grow_pos = pos.above();
        Self::can_grow_into(world, grow_pos)
    }

    fn is_bonemeal_success(
        &self,
        _state: BlockStateId,
        _world: &Arc<World>,
        _rng: &mut dyn Rng,
        _pos: BlockPos,
    ) -> bool {
        true
    }

    fn perform_bonemeal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        _rng: &mut dyn Rng,
        pos: BlockPos,
    ) {
        let above_pos = pos.above();
        if Self::can_grow_into(world, above_pos) {
            let facing = state.get_value(&FACING);
            BigDripleafStemBlock::place(
                world,
                pos,
                world.get_block_state(pos).get_fluid_state(),
                facing,
            );
            Self::place(
                world,
                above_pos,
                world.get_block_state(above_pos).get_fluid_state(),
                facing,
            );
        }
    }

    fn bonemeal_action_type(&self) -> BonemealAction {
        BonemealAction::Grower
    }
}
