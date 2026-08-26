//! Bell Block entity behavior.

use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::BaseNbtCompound as BorrowedNbtCompound;
use simdnbt::owned::NbtCompound;
use steel_protocol::packets::game::SoundSource;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::Direction;
use steel_registry::{
    REGISTRY, TaggedRegistryExt as _, sound_events, vanilla_block_entity_types,
    vanilla_entity_type_tags::EntityTypeTag, vanilla_mob_effects,
};
use steel_utils::{
    BlockPos, BlockStateId, DowncastType, DowncastTypeKey, WorldAabb, locks::SyncMutex,
};

use crate::block_entity::{BlockEntity, BlockEntityBase};
use crate::entity::{MobEffectInstance, SharedEntity};
use crate::world::World;

const RING_EVENT_ID: i32 = 1;
const RING_DURATION: i32 = 50;
const GLOW_DURATION: i32 = 60;
const ENTITY_SEARCH_INTERVAL: i64 = 60;
const RESONATION_DURATION: i32 = 40;
const RESONATION_DELAY: i32 = 5;
const SEARCH_RADIUS: f64 = 48.0;
const HEAR_BELL_RADIUS: f64 = 32.0;
const GLOW_RADIUS: f64 = 48.0;

/// Stores the transient state and entity reactions for a ringing bell.
pub struct BellBlockEntity {
    base: BlockEntityBase,
    state: SyncMutex<BellState>,
}

struct BellState {
    last_ring_timestamp: i64,
    ticks: i32,
    shaking: bool,
    click_direction: Option<Direction>,
    nearby_entities: Option<Vec<SharedEntity>>,
    resonating: bool,
    resonation_ticks: i32,
}
// SAFETY: This key uniquely identifies `BellBlockEntity`.
unsafe impl DowncastType for BellBlockEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:block_entity/bell");
}

impl BellBlockEntity {
    /// Creates a bell block entity at `pos` with the supplied block state.
    #[must_use]
    pub fn new(world: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
        Self {
            base: BlockEntityBase::new(&vanilla_block_entity_types::BELL, world, pos, state),
            state: SyncMutex::new(BellState {
                last_ring_timestamp: 0,
                ticks: 0,
                shaking: false,
                click_direction: None,
                nearby_entities: None,
                resonating: false,
                resonation_ticks: 0,
            }),
        }
    }

    /// Starts the bell animation and broadcasts its ring event.
    pub fn on_hit(&self, direction: Direction) {
        {
            let mut state = self.state.lock();
            state.click_direction = Some(direction);
            if state.shaking {
                state.ticks = 0;
            } else {
                state.shaking = true;
            }
        }

        let Some(world) = self.get_level() else {
            return;
        };
        world.block_event(
            self.get_block_pos(),
            self.get_block_state().get_block(),
            RING_EVENT_ID,
            direction.get_3d_data_value(),
        );
    }

    fn refresh_nearby_entities(&self, world: &World) {
        let game_time = world.game_time();
        let should_search = {
            let state = self.state.lock();
            state.nearby_entities.is_none()
                || game_time > state.last_ring_timestamp + ENTITY_SEARCH_INTERVAL
        };

        if should_search {
            let pos = self.get_block_pos();
            let bounds = WorldAabb::new(
                f64::from(pos.x()),
                f64::from(pos.y()),
                f64::from(pos.z()),
                f64::from(pos.x() + 1),
                f64::from(pos.y() + 1),
                f64::from(pos.z() + 1),
            )
            .inflate(SEARCH_RADIUS);
            let entities = world
                .get_entities_in_aabb(&bounds)
                .into_iter()
                .filter(|entity| entity.as_living_entity().is_some())
                .collect();

            let mut state = self.state.lock();
            state.nearby_entities = Some(entities);
            state.last_ring_timestamp = game_time;
        }
    }

    fn is_raider_near(entity: &SharedEntity, pos: BlockPos, radius: f64) -> bool {
        if !entity.is_alive() || entity.is_removed() {
            return false;
        }
        if !REGISTRY
            .entity_types
            .is_in_tag(entity.entity_type(), &EntityTypeTag::RAIDERS)
        {
            return false;
        }

        let center = DVec3::new(
            f64::from(pos.x()) + 0.5,
            f64::from(pos.y()) + 0.5,
            f64::from(pos.z()) + 0.5,
        );
        entity.position().distance_squared(center) < radius * radius
    }

    fn has_nearby_raider(entities: Option<&[SharedEntity]>, pos: BlockPos) -> bool {
        entities.is_some_and(|entities| {
            entities
                .iter()
                .any(|entity| Self::is_raider_near(entity, pos, HEAR_BELL_RADIUS))
        })
    }

    fn glow_nearby_raiders(entities: Option<&[SharedEntity]>, pos: BlockPos) {
        let Some(entities) = entities else {
            return;
        };

        for entity in entities {
            if !Self::is_raider_near(entity, pos, GLOW_RADIUS) {
                continue;
            }
            let Some(living) = entity.as_living_entity() else {
                continue;
            };
            living.add_mob_effect(MobEffectInstance::with_duration(
                vanilla_mob_effects::GLOWING,
                GLOW_DURATION,
                0,
            ));
        }
    }
}

impl BlockEntity for BellBlockEntity {
    fn base(&self) -> &BlockEntityBase {
        &self.base
    }

    fn load_additional(&self, _nbt: &BorrowedNbtCompound<'_>) {}

    fn save_additional(&self, _nbt: &mut NbtCompound) {}

    fn trigger_event(&self, event: i32, data: i32) -> bool {
        if event != RING_EVENT_ID {
            return false;
        }

        let Some(world) = self.get_level() else {
            return false;
        };
        self.refresh_nearby_entities(&world);

        let mut state = self.state.lock();
        state.resonation_ticks = 0;
        state.click_direction = Some(Direction::from_3d_data_value(data));
        state.ticks = 0;
        state.shaking = true;
        true
    }
    // TODO: make bell sleep when not used
    fn tick(&self, world: &Arc<World>) {
        let pos = self.get_block_pos();
        let mut state = self.state.lock();

        if state.shaking {
            state.ticks += 1;
        }
        if state.ticks >= RING_DURATION {
            state.shaking = false;
            state.ticks = 0;
        }

        if state.ticks >= RESONATION_DELAY
            && state.resonation_ticks == 0
            && Self::has_nearby_raider(state.nearby_entities.as_deref(), pos)
        {
            state.resonating = true;
            world.play_sound(
                &sound_events::BLOCK_BELL_RESONATE,
                SoundSource::Blocks,
                pos,
                1.0,
                1.0,
                None,
            );
        }

        if !state.resonating {
            return;
        }
        if state.resonation_ticks < RESONATION_DURATION {
            state.resonation_ticks += 1;
            return;
        }

        Self::glow_nearby_raiders(state.nearby_entities.as_deref(), pos);
        state.resonating = false;
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::blocks::properties::Direction;
    use steel_registry::{init_vanilla_registry, vanilla_blocks};

    use super::*;
    use crate::test_support::fresh_test_world;

    #[test]
    fn ring_event_starts_shaking_in_the_supplied_direction() {
        init_vanilla_registry();
        let world = fresh_test_world("bell_ring_event");
        let bell = BellBlockEntity::new(
            Arc::downgrade(&world),
            BlockPos::new(4, 64, 4),
            vanilla_blocks::BELL.default_state(),
        );

        assert!(bell.trigger_event(RING_EVENT_ID, Direction::West.get_3d_data_value()));

        let state = bell.state.lock();
        assert!(state.shaking);
        assert_eq!(state.ticks, 0);
        assert_eq!(state.click_direction, Some(Direction::West));
    }

    #[test]
    fn unrelated_block_event_is_rejected() {
        init_vanilla_registry();
        let world = fresh_test_world("bell_unrelated_event");
        let bell = BellBlockEntity::new(
            Arc::downgrade(&world),
            BlockPos::new(4, 64, 4),
            vanilla_blocks::BELL.default_state(),
        );

        assert!(!bell.trigger_event(2, 0));
        assert!(!bell.state.lock().shaking);
    }
}
