//! Vanilla `CalibratedSculkSensorBlockEntity`.

use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::BaseNbtCompound as BorrowedNbtCompound;
use simdnbt::borrow::NbtCompound as NbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::game_events::GameEventRef;
use steel_registry::vanilla_block_entity_types;
use steel_registry::vanilla_game_events;
use steel_utils::locks::SyncMutex;
use steel_utils::{BlockPos, BlockStateId, Downcast, DowncastType, DowncastTypeKey};

use crate::behavior::blocks::SculkSensorBlock;
use crate::block_entity::{BlockEntity, BlockEntityBase};
use crate::entity::Entity;
use crate::world::game_event::vibration::{
    SharedVibrationData, VibrationData, VibrationListener, VibrationUser, game_event_frequency,
    load_vibration_data, redstone_strength_for_distance, save_vibration_data, tick_vibration,
};
use crate::world::game_event::{GameEventContext, SharedGameEventListener};
use crate::world::{SignalGetter as _, World};

const LISTENER_RADIUS: i32 = 16;

/// Vanilla `CalibratedSculkSensorBlockEntity`.
pub struct CalibratedSculkSensorBlockEntity {
    base: BlockEntityBase,
    world: Weak<World>,
    last_vibration_frequency: SyncMutex<i32>,
    vibration_data: SharedVibrationData,
    listener: Arc<VibrationListener>,
}

struct CalibratedSensorUser {
    pos: BlockPos,
    world: Weak<World>,
}

unsafe impl DowncastType for CalibratedSculkSensorBlockEntity {
    const TYPE_KEY: DowncastTypeKey =
        DowncastTypeKey::new("steel:block_entity/calibrated_sculk_sensor");
}

impl CalibratedSculkSensorBlockEntity {
    /// Creates a calibrated sculk sensor block entity.
    #[must_use]
    pub fn new(level: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
        let data = Arc::new(SyncMutex::new(VibrationData::new()));
        let user = Arc::new(CalibratedSensorUser {
            pos,
            world: Weak::clone(&level),
        });
        let listener = Arc::new(VibrationListener::new(user, Arc::clone(&data)));
        Self {
            base: BlockEntityBase::new(
                &vanilla_block_entity_types::CALIBRATED_SCULK_SENSOR,
                Weak::clone(&level),
                pos,
                state,
            ),
            world: level,
            last_vibration_frequency: SyncMutex::new(0),
            vibration_data: data,
            listener,
        }
    }

    /// Vanilla `getLastVibrationFrequency`.
    #[must_use]
    pub fn last_vibration_frequency(&self) -> i32 {
        *self.last_vibration_frequency.lock()
    }

    fn set_last_vibration_frequency(&self, frequency: i32) {
        *self.last_vibration_frequency.lock() = frequency;
        self.set_changed();
    }
}

impl BlockEntity for CalibratedSculkSensorBlockEntity {
    fn base(&self) -> &BlockEntityBase {
        &self.base
    }

    fn load_additional(&self, nbt: &BorrowedNbtCompound<'_>) {
        let nbt_view: NbtCompoundView<'_, '_> = nbt.into();
        if let Some(freq) = nbt_view.int("last_vibration_frequency") {
            *self.last_vibration_frequency.lock() = freq;
        }
        if let Some(listener) = nbt_view.compound("listener") {
            *self.vibration_data.lock() = load_vibration_data(&listener);
        }
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        nbt.insert(
            "last_vibration_frequency",
            self.last_vibration_frequency(),
        );
        nbt.insert("listener", save_vibration_data(&self.vibration_data.lock()));
    }

    fn tick(&self, world: &Arc<World>) {
        let user = CalibratedSensorUser {
            pos: self.base.pos(),
            world: Weak::clone(&self.world),
        };
        tick_vibration(world, &self.vibration_data, &user);
    }

    fn game_event_listener(&self) -> Option<SharedGameEventListener> {
        Some(Arc::clone(&self.listener) as _)
    }
}

impl VibrationUser for CalibratedSensorUser {
    fn listener_radius(&self) -> i32 {
        LISTENER_RADIUS
    }

    fn listener_pos(&self) -> Option<DVec3> {
        let (x, y, z) = self.pos.get_center();
        Some(DVec3::new(x, y, z))
    }

    fn can_trigger_avoid_vibration(&self) -> bool {
        true
    }

    fn requires_adjacent_chunks_to_be_ticking(&self) -> bool {
        true
    }

    fn can_receive_vibration(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        event: GameEventRef,
        _context: &GameEventContext<'_>,
    ) -> bool {
        let state = world.get_block_state(self.pos);
        let facing = state.get_value(&BlockStateProperties::HORIZONTAL_FACING);
        let back = facing.opposite();
        let filter = world.get_signal(self.pos.relative(back), back);
        let frequency = game_event_frequency(event);
        if filter != 0 && frequency != filter {
            return false;
        }
        if pos == self.pos
            && (std::ptr::eq(event, &vanilla_game_events::BLOCK_DESTROY)
                || std::ptr::eq(event, &vanilla_game_events::BLOCK_PLACE))
        {
            return false;
        }
        if frequency == 0 {
            return false;
        }
        SculkSensorBlock::can_activate(state)
    }

    fn on_receive_vibration(
        &self,
        world: &Arc<World>,
        _pos: BlockPos,
        event: GameEventRef,
        source: Option<&dyn Entity>,
        _projectile_owner: Option<&dyn Entity>,
        receiving_distance: f32,
    ) {
        let state = world.get_block_state(self.pos);
        if !SculkSensorBlock::can_activate(state) {
            return;
        }
        let frequency = game_event_frequency(event);
        if let Some(block_entity) = world.get_block_entity(self.pos)
            && let Some(sensor) = block_entity.downcast_ref::<CalibratedSculkSensorBlockEntity>()
        {
            sensor.set_last_vibration_frequency(frequency);
        }
        let power = redstone_strength_for_distance(receiving_distance, LISTENER_RADIUS);
        SculkSensorBlock::activate(world, self.pos, state, source, power, frequency);
    }

    fn on_data_changed(&self) {
        let Some(world) = self.world.upgrade() else {
            return;
        };
        if let Some(block_entity) = world.get_block_entity(self.pos) {
            block_entity.set_changed();
        }
    }
}
