//! Vanilla `SculkSensorBlock` — vibration-activated redstone source.

use std::sync::{Arc, Weak};

use steel_macros::block_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::block_entity_type::BlockEntityTypeRef;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{
    BlockStateProperties, BoolProperty, EnumProperty, IntProperty, SculkSensorPhase,
};
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_block_entity_types;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_entities;
use steel_registry::vanilla_game_events;
use steel_registry::{sound_events, vanilla_blocks};
use steel_utils::types::UpdateFlags;
use steel_utils::value_providers::IntProvider;
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::behavior::blocks::redstone::{MAX_REDSTONE_SIGNAL, MIN_REDSTONE_SIGNAL};
use crate::behavior::block::{schedule_water_tick_if_waterlogged, try_drop_experience};
use crate::behavior::{BlockBehavior, BlockEntityCreation, BlockPlaceContext};
use crate::block_entity::entities::{CalibratedSculkSensorBlockEntity, SculkSensorBlockEntity};
use crate::block_entity::BlockEntityTicker;
use crate::entity::Entity;
use steel_utils::Downcast;
use crate::entity::ai::path::PathComputationType;
use crate::world::game_event::GameEventContext;
use crate::world::{LevelReader, ScheduledTickAccess, SignalQueryContext, World};

const PHASE: &EnumProperty<SculkSensorPhase> = &BlockStateProperties::SCULK_SENSOR_PHASE;
const POWER: &IntProperty = &BlockStateProperties::POWER;
const WATERLOGGED: &BoolProperty = &BlockStateProperties::WATERLOGGED;

const ACTIVE_TICKS: i32 = 30;
const COOLDOWN_TICKS: i32 = 10;
const EXPERIENCE: IntProvider = IntProvider::Constant(5);
const RESET_ON_PLACE_FLAGS: UpdateFlags =
    UpdateFlags::UPDATE_CLIENTS.union(UpdateFlags::UPDATE_KNOWN_SHAPE);

/// Vanilla `SculkSensorBlock`.
#[block_behavior]
pub struct SculkSensorBlock {
    block: BlockRef,
}

impl SculkSensorBlock {
    /// Creates sculk-sensor behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    /// Vanilla `SculkSensorBlock.getActiveTicks`.
    #[must_use]
    pub const fn active_ticks() -> i32 {
        ACTIVE_TICKS
    }

    /// Vanilla `SculkSensorBlock.canActivate`.
    #[must_use]
    pub fn can_activate(state: BlockStateId) -> bool {
        state.get_value(PHASE) == SculkSensorPhase::Inactive
    }

    fn update_neighbours(world: &Arc<World>, pos: BlockPos, state: BlockStateId) {
        let block = state.get_block();
        world.update_neighbors_at(pos, block);
        world.update_neighbors_at(pos.below(), block);
    }

    /// Vanilla `SculkSensorBlock.deactivate`.
    pub fn deactivate(world: &Arc<World>, pos: BlockPos, state: BlockStateId) {
        world.set_block(
            pos,
            state
                .set_value(PHASE, SculkSensorPhase::Cooldown)
                .set_value(POWER, 0_u8),
            UpdateFlags::UPDATE_ALL,
        );
        world.schedule_block_tick_default(pos, state.get_block(), COOLDOWN_TICKS);
        Self::update_neighbours(world, pos, state);
    }

    /// Vanilla `SculkSensorBlock.activate`.
    pub fn activate(
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        source: Option<&dyn Entity>,
        power: i32,
        frequency: i32,
    ) {
        world.set_block(
            pos,
            state
                .set_value(PHASE, SculkSensorPhase::Active)
                .set_value(POWER, power.clamp(MIN_REDSTONE_SIGNAL, MAX_REDSTONE_SIGNAL) as u8),
            UpdateFlags::UPDATE_ALL,
        );
        world.schedule_block_tick_default(pos, state.get_block(), Self::active_ticks_for(state));
        Self::update_neighbours(world, pos, state);
        Self::try_resonate(world, pos, source, frequency);
        world.game_event(
            &vanilla_game_events::SCULK_SENSOR_TENDRILS_CLICKING,
            pos,
            &GameEventContext::new(source, None),
        );
        if !state.get_value(WATERLOGGED) {
            let pitch = rand::random::<f32>().mul_add(0.2, 0.8);
            world.play_sound(
                &sound_events::BLOCK_SCULK_SENSOR_CLICKING,
                SoundSource::Blocks,
                pos,
                1.0,
                pitch,
                None,
            );
        }
    }

    fn active_ticks_for(state: BlockStateId) -> i32 {
        if state.get_block() == &vanilla_blocks::CALIBRATED_SCULK_SENSOR {
            10
        } else {
            ACTIVE_TICKS
        }
    }

    fn try_resonate(
        world: &Arc<World>,
        pos: BlockPos,
        source: Option<&dyn Entity>,
        frequency: i32,
    ) {
        if !(1..=15).contains(&frequency) {
            return;
        }
        for direction in Direction::ALL {
            let neighbour = pos.relative(direction);
            let neighbour_state = world.get_block_state(neighbour);
            if !neighbour_state
                .get_block()
                .has_tag(&BlockTag::VIBRATION_RESONATORS)
            {
                continue;
            }
            let event = resonance_event(frequency);
            world.game_event(
                event,
                neighbour,
                &GameEventContext::new(source, Some(neighbour_state)),
            );
            world.play_sound(
                &sound_events::BLOCK_AMETHYST_BLOCK_RESONATE,
                SoundSource::Blocks,
                neighbour,
                1.0,
                resonance_pitch(frequency),
                None,
            );
        }
    }
}

fn resonance_event(frequency: i32) -> steel_registry::game_events::GameEventRef {
    use steel_registry::vanilla_game_events;
    match frequency {
        1 => &vanilla_game_events::RESONATE_1,
        2 => &vanilla_game_events::RESONATE_2,
        3 => &vanilla_game_events::RESONATE_3,
        4 => &vanilla_game_events::RESONATE_4,
        5 => &vanilla_game_events::RESONATE_5,
        6 => &vanilla_game_events::RESONATE_6,
        7 => &vanilla_game_events::RESONATE_7,
        8 => &vanilla_game_events::RESONATE_8,
        9 => &vanilla_game_events::RESONATE_9,
        10 => &vanilla_game_events::RESONATE_10,
        11 => &vanilla_game_events::RESONATE_11,
        12 => &vanilla_game_events::RESONATE_12,
        13 => &vanilla_game_events::RESONATE_13,
        14 => &vanilla_game_events::RESONATE_14,
        _ => &vanilla_game_events::RESONATE_15,
    }
}

fn resonance_pitch(frequency: i32) -> f32 {
    // Vanilla `NoteBlock.getPitchFromNote` on the sensor tone map.
    const TONES: [i32; 16] = [0, 0, 2, 4, 6, 7, 9, 10, 12, 14, 15, 18, 19, 21, 22, 24];
    let note = TONES[frequency.clamp(0, 15) as usize];
    2.0_f32.powf((f32::from(note as i16) - 12.0) / 12.0)
}

impl BlockBehavior for SculkSensorBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(
            self.block
                .default_state()
                .set_value(WATERLOGGED, context.is_water_source()),
        )
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        schedule_water_tick_if_waterlogged(state, world, pos);
        state
    }

    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        match state.get_value(PHASE) {
            SculkSensorPhase::Active => Self::deactivate(world, pos, state),
            SculkSensorPhase::Cooldown => {
                world.set_block(
                    pos,
                    state.set_value(PHASE, SculkSensorPhase::Inactive),
                    UpdateFlags::UPDATE_ALL,
                );
                if !state.get_value(WATERLOGGED) {
                    let pitch = rand::random::<f32>().mul_add(0.2, 0.8);
                    world.play_sound(
                        &sound_events::BLOCK_SCULK_SENSOR_CLICKING_STOP,
                        SoundSource::Blocks,
                        pos,
                        1.0,
                        pitch,
                        None,
                    );
                }
            }
            SculkSensorPhase::Inactive => {}
        }
    }

    fn step_on(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos, entity: &dyn Entity) {
        if Self::can_activate(state)
            && entity.entity_type() != &vanilla_entities::WARDEN
            && let Some(block_entity) = world.get_block_entity(pos)
            && let Some(sensor) = block_entity.downcast_ref::<SculkSensorBlockEntity>()
        {
            sensor.try_step_on(world, entity, state);
        }
        self.default_step_on(state, world, pos, entity);
    }

    fn on_place(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        old_state: BlockStateId,
        _moved_by_piston: bool,
    ) {
        if old_state.get_block() != state.get_block()
            && i32::from(state.get_value(POWER)) > MIN_REDSTONE_SIGNAL
            && !world.has_scheduled_block_tick(pos, self.block)
        {
            world.set_block(pos, state.set_value(POWER, 0_u8), RESET_ON_PLACE_FLAGS);
        }
    }

    fn affect_neighbors_after_removal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _moved_by_piston: bool,
    ) {
        if state.get_value(PHASE) == SculkSensorPhase::Active {
            Self::update_neighbours(world, pos, state);
        }
    }

    fn is_signal_source(&self, _state: BlockStateId, _context: SignalQueryContext) -> bool {
        true
    }

    fn get_own_signal(
        &self,
        state: BlockStateId,
        _world: &dyn LevelReader,
        _pos: BlockPos,
        _context: SignalQueryContext,
    ) -> i32 {
        i32::from(state.get_value(POWER))
    }

    fn get_direct_signal(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
        direction: Direction,
        context: SignalQueryContext,
    ) -> i32 {
        if direction == Direction::Up {
            self.get_signal(state, world, pos, direction, context)
        } else {
            MIN_REDSTONE_SIGNAL
        }
    }

    fn has_analog_output_signal(&self, _state: BlockStateId) -> bool {
        true
    }

    fn get_analog_output_signal(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        if state.get_value(PHASE) != SculkSensorPhase::Active {
            return MIN_REDSTONE_SIGNAL;
        }
        world
            .get_block_entity(pos)
            .and_then(|entity| {
                entity
                    .downcast_ref::<SculkSensorBlockEntity>()
                    .map(SculkSensorBlockEntity::last_vibration_frequency)
                    .or_else(|| {
                        entity
                            .downcast_ref::<CalibratedSculkSensorBlockEntity>()
                            .map(CalibratedSculkSensorBlockEntity::last_vibration_frequency)
                    })
            })
            .unwrap_or(MIN_REDSTONE_SIGNAL)
    }

    fn is_pathfindable(&self, _state: BlockStateId, _computation_type: PathComputationType) -> bool {
        false
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
            try_drop_experience(world, pos, tool, &EXPERIENCE);
        }
    }

    fn new_block_entity(
        &self,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> BlockEntityCreation {
        BlockEntityCreation::Created(Arc::new(SculkSensorBlockEntity::new(level, pos, state)))
    }

    fn get_block_entity_ticker(
        &self,
        _world: &Arc<World>,
        _state: BlockStateId,
        block_entity_type: BlockEntityTypeRef,
    ) -> Option<BlockEntityTicker> {
        BlockEntityTicker::for_matching_entity_tick(
            block_entity_type,
            &vanilla_block_entity_types::SCULK_SENSOR,
        )
    }
}
