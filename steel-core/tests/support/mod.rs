use std::cell::{Cell, RefCell};

use steel_registry::blocks::BlockRef;
use steel_registry::fluid::FluidRef;
use steel_registry::game_events::GameEventRef;
use steel_registry::sound_event::SoundEventRef;
use steel_registry::{vanilla_blocks, vanilla_fluids};
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId};

use crate::world::game_event_context::GameEventContext;
use crate::world::{LevelAccessor, LevelReader, ScheduledTickAccess};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PlacedBlockState {
    pub(crate) pos: BlockPos,
    pub(crate) state: BlockStateId,
    pub(crate) flags: UpdateFlags,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ScheduledBlockTick {
    pub(crate) pos: BlockPos,
    pub(crate) block: BlockRef,
    pub(crate) delay: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ScheduledFluidTick {
    pub(crate) pos: BlockPos,
    pub(crate) fluid: FluidRef,
    pub(crate) delay: i32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PlayedBlockSound {
    pub(crate) sound: SoundEventRef,
    pub(crate) pos: BlockPos,
    pub(crate) volume: f32,
    pub(crate) pitch: f32,
    pub(crate) exclude: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RecordedGameEvent {
    pub(crate) event: GameEventRef,
    pub(crate) pos: BlockPos,
    pub(crate) affected_state: Option<BlockStateId>,
}

pub(crate) struct TestLevel {
    blocks: RefCell<Vec<(BlockPos, BlockStateId)>>,
    default_block_state: RefCell<Option<BlockStateId>>,
    raw_brightness: Cell<u8>,
    min_y: Cell<i32>,
    height: Cell<i32>,
    fluid_tick_delay: Cell<i32>,
    pub(crate) placed_blocks: RefCell<Vec<PlacedBlockState>>,
    pub(crate) scheduled_block_ticks: RefCell<Vec<ScheduledBlockTick>>,
    pub(crate) scheduled_fluid_ticks: RefCell<Vec<ScheduledFluidTick>>,
    pub(crate) block_sounds: RefCell<Vec<PlayedBlockSound>>,
    pub(crate) game_events: RefCell<Vec<RecordedGameEvent>>,
}

impl Default for TestLevel {
    fn default() -> Self {
        Self {
            blocks: RefCell::new(Vec::new()),
            default_block_state: RefCell::new(None),
            raw_brightness: Cell::new(0),
            min_y: Cell::new(-64),
            height: Cell::new(384),
            fluid_tick_delay: Cell::new(5),
            placed_blocks: RefCell::new(Vec::new()),
            scheduled_block_ticks: RefCell::new(Vec::new()),
            scheduled_fluid_ticks: RefCell::new(Vec::new()),
            block_sounds: RefCell::new(Vec::new()),
            game_events: RefCell::new(Vec::new()),
        }
    }
}

impl TestLevel {
    pub(crate) fn with_default_block_state(self, state: BlockStateId) -> Self {
        *self.default_block_state.borrow_mut() = Some(state);
        self
    }

    pub(crate) fn with_block(self, pos: BlockPos, state: BlockStateId) -> Self {
        self.set_test_block(pos, state);
        self
    }

    pub(crate) fn with_raw_brightness(self, raw_brightness: u8) -> Self {
        self.raw_brightness.set(raw_brightness);
        self
    }

    pub(crate) fn with_min_y(self, min_y: i32) -> Self {
        self.min_y.set(min_y);
        self
    }

    pub(crate) fn with_height(self, height: i32) -> Self {
        self.height.set(height);
        self
    }

    pub(crate) fn set_test_block(&self, pos: BlockPos, state: BlockStateId) {
        let mut blocks = self.blocks.borrow_mut();
        if let Some((_, existing)) = blocks.iter_mut().find(|(block_pos, _)| *block_pos == pos) {
            *existing = state;
        } else {
            blocks.push((pos, state));
        }
    }

    pub(crate) fn last_placed_state(&self) -> Option<BlockStateId> {
        self.placed_blocks
            .borrow()
            .last()
            .map(|placed| placed.state)
    }

    pub(crate) fn scheduled_water_tick(&self) -> bool {
        self.scheduled_fluid_ticks
            .borrow()
            .iter()
            .any(|tick| tick.fluid == &vanilla_fluids::WATER)
    }
}

impl LevelReader for TestLevel {
    fn get_block_state(&self, pos: BlockPos) -> BlockStateId {
        self.blocks
            .borrow()
            .iter()
            .rev()
            .find(|(block_pos, _)| *block_pos == pos)
            .map_or_else(
                || {
                    self.default_block_state
                        .borrow()
                        .unwrap_or_else(|| vanilla_blocks::AIR.default_state())
                },
                |(_, state)| *state,
            )
    }

    fn raw_brightness(&self, _pos: BlockPos, _sky_darkening: u8) -> u8 {
        self.raw_brightness.get()
    }

    fn min_y(&self) -> i32 {
        self.min_y.get()
    }

    fn height(&self) -> i32 {
        self.height.get()
    }
}

impl ScheduledTickAccess for TestLevel {
    fn fluid_tick_delay(&self, _fluid: FluidRef) -> i32 {
        self.fluid_tick_delay.get()
    }

    fn schedule_block_tick_default(&self, pos: BlockPos, block: BlockRef, delay: i32) -> bool {
        self.scheduled_block_ticks
            .borrow_mut()
            .push(ScheduledBlockTick { pos, block, delay });
        true
    }

    fn schedule_fluid_tick_default(&self, pos: BlockPos, fluid: FluidRef, delay: i32) -> bool {
        self.scheduled_fluid_ticks
            .borrow_mut()
            .push(ScheduledFluidTick { pos, fluid, delay });
        true
    }
}

impl LevelAccessor for TestLevel {
    fn set_block_state(&self, pos: BlockPos, state: BlockStateId, flags: UpdateFlags) -> bool {
        self.set_test_block(pos, state);
        self.placed_blocks
            .borrow_mut()
            .push(PlacedBlockState { pos, state, flags });
        true
    }

    fn play_block_sound(
        &self,
        sound: SoundEventRef,
        pos: BlockPos,
        volume: f32,
        pitch: f32,
        exclude: Option<i32>,
    ) {
        self.block_sounds.borrow_mut().push(PlayedBlockSound {
            sound,
            pos,
            volume,
            pitch,
            exclude,
        });
    }

    fn game_event(&self, event: GameEventRef, pos: BlockPos, context: &GameEventContext<'_>) {
        self.game_events.borrow_mut().push(RecordedGameEvent {
            event,
            pos,
            affected_state: context.affected_state(),
        });
    }
}
