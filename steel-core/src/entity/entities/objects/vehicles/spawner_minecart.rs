//! Spawner minecart entity.

use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;

use steel_macros::entity_behavior;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::vanilla_entity_data::SpawnerMinecartEntityData;
use steel_registry::{items::ItemRef, vanilla_blocks, vanilla_items};
use steel_utils::axis::Axis;
use steel_utils::block_util::FoundRectangle;
use steel_utils::entity_events::EntityStatus;
use steel_utils::locks::SyncMutex;
use steel_utils::{BlockStateId, Direction, DowncastType, DowncastTypeKey};

use super::abstract_minecart::{
    AbstractMinecart, AbstractMinecartBase, minecart_relative_portal_position,
};
use super::minecart_behavior::MinecartBehavior;
use super::vehicle_entity::{
    VEHICLE_DIMENSION_CHANGING_DELAY, VehicleEntity, vehicle_ignores_explosion,
};
use crate::block_entity::entities::BaseSpawner;
use crate::entity::{
    DamageSource, Entity, EntityBase, EntityBaseLoad, EntityEventSource, EntityMovementEmission,
    EntitySyncedData,
};
use crate::physics::{MoveResult, MoverType};
use crate::world::{Explosion, World};

/// A minecart carrying a mob spawner.
#[entity_behavior(class = "MinecartSpawner")]
pub struct SpawnerMinecartEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    entity_data: SyncMutex<SpawnerMinecartEntityData>,
    minecart_base: AbstractMinecartBase,
    spawner: BaseSpawner,
}

// SAFETY: This key is owned by Steel and uniquely identifies `SpawnerMinecartEntity`.
unsafe impl DowncastType for SpawnerMinecartEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/spawner_minecart");
}

impl SpawnerMinecartEntity {
    /// Creates a spawner minecart.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self {
            base: EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
            entity_data: SyncMutex::new(SpawnerMinecartEntityData::new()),
            minecart_base: AbstractMinecartBase::new(),
            spawner: BaseSpawner::new(),
        }
    }

    /// Creates a spawner minecart from saved data.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self {
            base: EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
            entity_data: SyncMutex::new(SpawnerMinecartEntityData::new()),
            minecart_base: AbstractMinecartBase::new(),
            spawner: BaseSpawner::new(),
        }
    }

    /// Returns the minecart's spawner state.
    #[must_use]
    pub const fn spawner(&self) -> &BaseSpawner {
        &self.spawner
    }
}

impl VehicleEntity for SpawnerMinecartEntity {
    fn hurt_time(&self) -> i32 {
        *self.entity_data.lock().vehicle_entity().id_hurt.get()
    }

    fn set_hurt_time(&self, value: i32) {
        self.entity_data
            .lock()
            .vehicle_entity_mut()
            .id_hurt
            .set(value);
    }

    fn hurt_dir(&self) -> i32 {
        *self.entity_data.lock().vehicle_entity().id_hurtdir.get()
    }

    fn set_hurt_dir(&self, value: i32) {
        self.entity_data
            .lock()
            .vehicle_entity_mut()
            .id_hurtdir
            .set(value);
    }

    fn damage(&self) -> f32 {
        *self.entity_data.lock().vehicle_entity().id_damage.get()
    }

    fn set_damage(&self, value: f32) {
        self.entity_data
            .lock()
            .vehicle_entity_mut()
            .id_damage
            .set(value);
    }

    fn vehicle_drop_item(&self) -> ItemRef {
        &vanilla_items::MINECART
    }
}

impl AbstractMinecart for SpawnerMinecartEntity {
    fn minecart_base(&self) -> &AbstractMinecartBase {
        &self.minecart_base
    }

    fn custom_display_block(&self) -> Option<BlockStateId> {
        *self
            .entity_data
            .lock()
            .abstract_minecart()
            .id_custom_display_block
            .get()
    }

    fn set_custom_display_block(&self, value: Option<BlockStateId>) {
        self.entity_data
            .lock()
            .abstract_minecart_mut()
            .id_custom_display_block
            .set(value);
    }

    fn display_offset(&self) -> i32 {
        *self
            .entity_data
            .lock()
            .abstract_minecart()
            .id_display_offset
            .get()
    }

    fn set_display_offset(&self, value: i32) {
        self.entity_data
            .lock()
            .abstract_minecart_mut()
            .id_display_offset
            .set(value);
    }

    fn default_display_block_state(&self) -> BlockStateId {
        vanilla_blocks::SPAWNER.default_state()
    }
}

impl Entity for SpawnerMinecartEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn tick(self: Arc<Self>) {
        let entity = Arc::clone(&self).into_shared_entity();
        self.tick_minecart(&entity);

        let Some(world) = self.level() else {
            return;
        };
        self.spawner.server_tick(&world, self.block_position(), || {
            // Spawner minecarts use event 1 to reset the client spawn delay.
            self.broadcast_entity_event(EntityStatus::Jump);
        });
    }

    fn get_default_gravity(&self) -> f64 {
        self.minecart_default_gravity()
    }

    fn get_air_drag(&self) -> f64 {
        self.minecart_air_drag()
    }

    fn block_speed_factor(&self) -> f32 {
        self.minecart_block_speed_factor()
    }

    fn move_entity(self: Arc<Self>, mover_type: MoverType, delta: DVec3) -> Option<MoveResult> {
        self.minecart_move_entity(mover_type, delta)
    }

    fn apply_effects_from_blocks(&self) {
        self.minecart_apply_effects_from_blocks();
    }

    fn known_movement(&self) -> DVec3 {
        MinecartBehavior::known_movement(self.default_known_movement())
    }

    fn motion_direction(&self) -> Direction {
        MinecartBehavior::motion_direction(self)
    }

    fn is_on_rails(&self) -> bool {
        self.minecart_base().on_rails()
    }

    fn is_pickable(&self) -> bool {
        self.minecart_is_pickable()
    }

    fn is_pushable(&self) -> bool {
        self.minecart_is_pushable()
    }

    fn movement_emission(&self) -> EntityMovementEmission {
        self.minecart_movement_emission()
    }

    fn blocks_building(&self) -> bool {
        self.minecart_blocks_building()
    }

    fn dimension_changing_delay(&self) -> i32 {
        VEHICLE_DIMENSION_CHANGING_DELAY
    }

    fn get_relative_portal_position(&self, axis: Axis, area: FoundRectangle) -> DVec3 {
        minecart_relative_portal_position(self, axis, area)
    }

    fn passenger_attachment_point(&self, passenger: &dyn Entity) -> DVec3 {
        self.minecart_passenger_attachment_point(passenger)
    }

    fn can_collide_with(&self, other: &dyn Entity) -> bool {
        self.minecart_can_collide_with(other)
    }

    fn push_entity(&self, entity: &dyn Entity) {
        self.minecart_push_entity(entity);
    }

    fn hurt(&self, world: &World, source: &DamageSource, amount: f32) -> bool {
        self.vehicle_hurt(world, source, amount)
    }

    fn ignore_explosion(&self, explosion: &dyn Explosion) -> bool {
        vehicle_ignores_explosion(explosion)
    }

    fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
        Some(&self.entity_data)
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.save_abstract_minecart_data(nbt);
        self.spawner.save(nbt);
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.load_abstract_minecart_data(nbt);
        self.spawner.load(&nbt);
    }

    // TODO: Implement getPickResult.
}

#[cfg(test)]
#[path = "tests/spawner_minecart.rs"]
mod tests;
