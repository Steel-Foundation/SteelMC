//! Vanilla `SculkSensorBlockEntity`.

use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::BaseNbtCompound as BorrowedNbtCompound;
use simdnbt::borrow::NbtCompound as NbtCompoundView;
use simdnbt::owned::NbtCompound;
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
use crate::world::World;

const LISTENER_RADIUS: i32 = 8;

/// Vanilla `SculkSensorBlockEntity`.
pub struct SculkSensorBlockEntity {
    base: BlockEntityBase,
    world: Weak<World>,
    last_vibration_frequency: SyncMutex<i32>,
    vibration_data: SharedVibrationData,
    listener: Arc<VibrationListener>,
}

struct SculkSensorUser {
    pos: BlockPos,
    world: Weak<World>,
}

unsafe impl DowncastType for SculkSensorBlockEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:block_entity/sculk_sensor");
}

impl SculkSensorBlockEntity {
    /// Creates a sculk sensor block entity.
    #[must_use]
    pub fn new(level: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
        let data = Arc::new(SyncMutex::new(VibrationData::new()));
        let user = Arc::new(SculkSensorUser {
            pos,
            world: Weak::clone(&level),
        });
        let listener = Arc::new(VibrationListener::new(user, Arc::clone(&data)));
        Self {
            base: BlockEntityBase::new(
                &vanilla_block_entity_types::SCULK_SENSOR,
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

    /// Vanilla `SculkSensorBlock.stepOn` forced vibration.
    pub fn try_step_on(&self, world: &Arc<World>, entity: &dyn Entity, state: BlockStateId) {
        let context = GameEventContext::new(Some(entity), Some(state));
        if !self.listener_user_can_receive(world, entity.block_position(), &vanilla_game_events::STEP, &context)
        {
            return;
        }
        self.listener.force_schedule_vibration(
            world,
            &vanilla_game_events::STEP,
            &context,
            entity.position(),
        );
    }

    fn listener_user_can_receive(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        event: GameEventRef,
        context: &GameEventContext<'_>,
    ) -> bool {
        SculkSensorUser {
            pos: self.base.pos(),
            world: Weak::clone(&self.world),
        }
        .can_receive_vibration(world, pos, event, context)
    }
}

impl BlockEntity for SculkSensorBlockEntity {
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
        let user = SculkSensorUser {
            pos: self.base.pos(),
            world: Weak::clone(&self.world),
        };
        tick_vibration(world, &self.vibration_data, &user);
    }

    fn game_event_listener(&self) -> Option<SharedGameEventListener> {
        Some(Arc::clone(&self.listener) as _)
    }
}

impl VibrationUser for SculkSensorUser {
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
        if pos == self.pos
            && (std::ptr::eq(event, &vanilla_game_events::BLOCK_DESTROY)
                || std::ptr::eq(event, &vanilla_game_events::BLOCK_PLACE))
        {
            return false;
        }
        if game_event_frequency(event) == 0 {
            return false;
        }
        SculkSensorBlock::can_activate(world.get_block_state(self.pos))
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
            && let Some(sensor) = block_entity.downcast_ref::<SculkSensorBlockEntity>()
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
