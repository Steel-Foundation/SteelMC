//! Sculk Catalyst block entity - spreads sculk when mobs die nearby.

use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::BaseNbtCompound as BorrowedNbtCompound;
use simdnbt::borrow::NbtCompound as NbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_protocol::packets::game::SoundSource;
use steel_registry::game_events::GameEventRef;
use steel_registry::vanilla_block_entity_types;
use steel_registry::vanilla_game_events;
use steel_utils::locks::SyncMutex;
use steel_utils::{BlockPos, BlockStateId, Downcast, DowncastType, DowncastTypeKey};

use crate::block_entity::{BlockEntity, BlockEntityBase};
use crate::world::game_event::{GameEventContext, GameEventDeliveryMode, GameEventListener};
use crate::world::{SculkSpreader, World};

const SPREAD_RADIUS: i32 = 8;

struct CatalystState {
    spreader: SculkSpreader,
}

impl Default for CatalystState {
    fn default() -> Self {
        Self {
            spreader: SculkSpreader::new(),
        }
    }
}

/// Sculk Catalyst block entity - spreads sculk when mobs die nearby.
pub struct SculkCatalystBlockEntity {
    base: BlockEntityBase,
    state: SyncMutex<CatalystState>,
    listener: Arc<SculkCatalystListener>,
}

unsafe impl DowncastType for SculkCatalystBlockEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:block_entity/sculk_catalyst");
}

impl SculkCatalystBlockEntity {
    /// Creates a new sculk catalyst block entity.
    #[must_use]
    pub fn new(pos: BlockPos, state: BlockStateId, level: Weak<World>) -> Self {
        let listener = Arc::new(SculkCatalystListener { pos });
        Self {
            base: BlockEntityBase::new(
                &vanilla_block_entity_types::SCULK_CATALYST,
                level,
                pos,
                state,
            ),
            state: SyncMutex::new(CatalystState::default()),
            listener,
        }
    }

    fn handle_death(&self, _world: &Arc<World>, death_pos: DVec3, experience: i32) {
        if experience <= 0 {
            return;
        }

        let mut state = self.state.lock();
        state.spreader.add_cursors_from_death(BlockPos::from(death_pos), experience);
        drop(state);

        log::debug!(
            "Sculk Catalyst at {:?} received {} XP from death at {:?}",
            self.base.pos(),
            experience,
            death_pos
        );

        self.set_changed();
    }

    /// Ticks the sculk spreader
    pub fn tick(&self, world: &Arc<World>) {
        let mut state = self.state.lock();
        let changed = state.spreader.tick(world, self.base.pos());
        drop(state);

        if changed {
            // Play bloom sound occasionally
            if (world.game_time() % 10) == 0 {
                world.play_sound(
                    &steel_registry::sound_events::BLOCK_SCULK_CATALYST_BLOOM,
                    SoundSource::Blocks,
                    self.base.pos(),
                    1.0,
                    1.0,
                    None,
                );
            }
            self.set_changed();
        }
    }
}

impl BlockEntity for SculkCatalystBlockEntity {
    fn base(&self) -> &BlockEntityBase {
        &self.base
    }

    fn load_additional(&self, nbt: &BorrowedNbtCompound<'_>) {
        let nbt_view: NbtCompoundView<'_, '_> = nbt.into();

        if let Some(spreader_nbt) = nbt_view.compound("sculk_spreader") {
            let mut state = self.state.lock();
            state.spreader = SculkSpreader::load(&spreader_nbt);
        }
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        let state = self.state.lock();
        if state.spreader.has_active_cursors() {
            nbt.insert("sculk_spreader", state.spreader.save());
        }
    }

    fn tick(&self, world: &Arc<World>) {
        self.tick(world);
    }

    fn game_event_listener(&self) -> Option<crate::world::game_event::SharedGameEventListener> {
        Some(Arc::clone(&self.listener) as _)
    }
}

struct SculkCatalystListener {
    pos: BlockPos,
}

impl GameEventListener for SculkCatalystListener {
    fn listener_pos(&self) -> Option<DVec3> {
        let (x, y, z) = self.pos.get_center();
        Some(DVec3::new(x, y, z))
    }

    fn listener_radius(&self) -> i32 {
        SPREAD_RADIUS
    }

    fn delivery_mode(&self) -> GameEventDeliveryMode {
        GameEventDeliveryMode::ByDistance
    }

    fn handle_game_event(
        &self,
        world: &Arc<World>,
        event: GameEventRef,
        context: &GameEventContext<'_>,
        source_pos: DVec3,
    ) -> bool {
        // Only react to entity death events
        if !std::ptr::eq(event, &vanilla_game_events::ENTITY_DIE) {
            return false;
        }

        // Get dying entity
        let Some(_entity) = context.source_entity() else {
            return false;
        };

        // Calculate experience from entity
        // TODO: Get actual XP from entity type
        let experience = 5; // Default XP

        // Trigger sculk spread
        if let Some(block_entity) = world.get_block_entity(self.pos) {
            if let Some(catalyst) = block_entity.downcast_ref::<SculkCatalystBlockEntity>() {
                catalyst.handle_death(world, source_pos, experience);
                return true;
            }
        }
        false
    }
}
