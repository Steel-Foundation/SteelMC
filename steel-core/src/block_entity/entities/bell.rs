use std::{
    any::Any,
    sync::{Arc, Weak},
};

use glam::DVec3;
use steel_protocol::packets::game::SoundSource;
use steel_registry::TaggedRegistryExt;
use steel_utils::{BlockPos, BlockStateId, Direction, WorldAabb};

use crate::{block_entity::BlockEntity, entity::Entity, world::World};

pub struct BellBlockEntity {
    level: Weak<World>,
    position: BlockPos,
    state: BlockStateId,
    removed: bool,

    shaking: bool,
    ticks: i32,
    click_direction: Option<Direction>,
    last_ring_timestamp: i64,
    resonating: bool,
    resonation_ticks: i32,
}

impl BellBlockEntity {
    #[must_use]
    pub const fn new(level: Weak<World>, position: BlockPos, state: BlockStateId) -> Self {
        Self {
            level,
            position,
            state,
            removed: false,
            shaking: false,
            ticks: 0,
            click_direction: None,
            last_ring_timestamp: 0,
            resonating: false,
            resonation_ticks: 0,
        }
    }

    /// Called when the bell is struck (player, projectile, redstone).
    pub fn on_hit(&mut self, direction: Direction) {
        self.click_direction = Some(direction);
        if self.shaking {
            self.ticks = 0;
        } else {
            self.shaking = true;
        }
        self.resonating = false;
        self.resonation_ticks = 0;
        self.update_nearby_entities();
        self.set_changed();
    }

    /// Refreshes the cached list of nearby entities and sets their `HEARD_BELL_TIME` memory.
    fn update_nearby_entities(&mut self) {
        let Some(world) = self.level.upgrade() else {
            return;
        };
        let now = world.level_data.read().game_time();
        if now <= self.last_ring_timestamp + 60 {
            return; // already up‑to‑date
        }
        self.last_ring_timestamp = now;

        // Query living entities in a 48‑block cube.
        let aabb = WorldAabb::new(
            f64::from(self.position.x()) - 48.0,
            f64::from(self.position.y()) - 48.0,
            f64::from(self.position.z()) - 48.0,
            f64::from(self.position.x()) + 48.0,
            f64::from(self.position.y()) + 48.0,
            f64::from(self.position.z()) + 48.0,
        );
        let entities: Vec<Arc<dyn Entity>> = world
            .get_entities_in_aabb(&aabb)
            .into_iter()
            .filter(|e| e.is_living_entity())
            .collect();

        // Set memory for entities within 32 blocks (server side only).
        for entity in &entities {
            if entity.is_alive() && !entity.is_removed() {
                let dist_sq = entity.position().distance_squared(DVec3::new(
                    f64::from(self.position.x()) + 0.5,
                    f64::from(self.position.y()) + 0.5,
                    f64::from(self.position.z()) + 0.5,
                ));
                if dist_sq <= 32.0 * 32.0 {
                    // TODO: set MemoryModuleType::HEARD_BELL_TIME when AI system is available.
                    // entity.brain().set_memory(MemoryModuleType::HEARD_BELL_TIME, now);
                }
            }
        }
    }

    /// Returns true if any raider is within 32 blocks.
    fn are_raiders_nearby(world: &World, pos: BlockPos) -> bool {
        let aabb = WorldAabb::new(
            f64::from(pos.x()) - 32.0,
            f64::from(pos.y()) - 32.0,
            f64::from(pos.z()) - 32.0,
            f64::from(pos.x()) + 32.0,
            f64::from(pos.y()) + 32.0,
            f64::from(pos.z()) + 32.0,
        );
        for entity in world.get_entities_in_aabb(&aabb) {
            if entity.is_alive()
                && !entity.is_removed()
                && steel_registry::REGISTRY.entity_types.is_in_tag(
                    entity.entity_type(),
                    &steel_registry::vanilla_entity_type_tags::EntityTypeTag::RAIDERS,
                )
            {
                return true;
            }
        }
        false
    }

    /// Applies the Glowing effect to all raiders within 48 blocks.
    fn make_raiders_glow(world: &World, pos: BlockPos) {
        let aabb = WorldAabb::new(
            f64::from(pos.x()) - 48.0,
            f64::from(pos.y()) - 48.0,
            f64::from(pos.z()) - 48.0,
            f64::from(pos.x()) + 48.0,
            f64::from(pos.y()) + 48.0,
            f64::from(pos.z()) + 48.0,
        );
        for entity in world.get_entities_in_aabb(&aabb) {
            if entity.is_alive()
                && !entity.is_removed()
                && steel_registry::REGISTRY.entity_types.is_in_tag(
                    entity.entity_type(),
                    &steel_registry::vanilla_entity_type_tags::EntityTypeTag::RAIDERS,
                )
            {
                // TODO: add effect via entity.add_effect(MobEffects::GLOWING, 60) when available.
            }
        }
    }
}

impl BlockEntity for BellBlockEntity {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn get_type(&self) -> steel_registry::block_entity_type::BlockEntityTypeRef {
        &steel_registry::vanilla_block_entity_types::BELL
    }

    fn get_block_pos(&self) -> BlockPos {
        self.position
    }

    fn get_block_state(&self) -> BlockStateId {
        self.state
    }

    fn set_block_state(&mut self, state: BlockStateId) {
        self.state = state;
    }

    fn is_removed(&self) -> bool {
        self.removed
    }

    fn set_removed(&mut self) {
        self.removed = true;
    }

    fn clear_removed(&mut self) {
        self.removed = false;
    }

    fn get_level(&self) -> Option<Arc<World>> {
        self.level.upgrade()
    }

    fn load_additional(&mut self, _nbt: &simdnbt::borrow::BaseNbtCompound<'_>) {
        // Nothing to load beyond position/state.
    }

    fn save_additional(&self, _nbt: &mut simdnbt::owned::NbtCompound) {
        // Nothing to save beyond position/state.
    }

    fn is_ticking(&self) -> bool {
        true
    }

    fn tick(&mut self, world: &Arc<World>) {
        if self.shaking {
            self.ticks += 1;
        }

        if self.ticks >= 50 {
            self.shaking = false;
            self.ticks = 0;
            self.set_changed();
        }

        if self.ticks >= 5
            && self.resonation_ticks == 0
            && Self::are_raiders_nearby(world, self.position)
        {
            self.resonating = true;
            world.play_sound(
                &steel_registry::sound_events::BLOCK_BELL_RESONATE,
                SoundSource::Blocks,
                self.position,
                2.0_f32,
                1.0_f32,
                None,
            );
        }

        if self.resonating {
            self.resonation_ticks += 1;
            if self.resonation_ticks >= 40 {
                Self::make_raiders_glow(world, self.position);
                self.resonating = false;
                self.resonation_ticks = 0;
            }
        }
    }
}
