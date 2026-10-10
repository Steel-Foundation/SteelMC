//! Vanilla rideable minecart.

use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;

use steel_macros::entity_behavior;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::vanilla_entity_data::MinecartEntityData;
use steel_registry::{items::ItemRef, vanilla_items};
use steel_utils::axis::Axis;
use steel_utils::block_util::FoundRectangle;
use steel_utils::locks::SyncMutex;
use steel_utils::types::InteractionHand;
use steel_utils::{BlockPos, BlockStateId, Direction, DowncastType, DowncastTypeKey};

use super::abstract_minecart::{
    AbstractMinecart, AbstractMinecartBase, AbstractMinecartEventSource,
};
use super::minecart_behavior::MinecartBehavior;
use super::old_minecart_behavior::OldMinecartBehavior;
use super::vehicle_entity::VehicleEntity;
use crate::behavior::InteractionResult;
use crate::entity::{
    DamageSource, Entity, EntityBase, EntityBaseLoad, EntityEventSource, EntityMovementEmission,
    EntitySyncedData,
};
use crate::physics::{MoveResult, MoverType};
use crate::player::Player;
use crate::world::World;

/// A minecart entity.
#[entity_behavior(class = "Minecart")]
pub struct MinecartEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    entity_data: SyncMutex<MinecartEntityData>,
    minecart_base: AbstractMinecartBase,
    behavior: OldMinecartBehavior, // TODO(minecart-improvements): choose New/Old per world flag
}

// SAFETY: This key is owned by Steel and uniquely identifies `MinecartEntity`.
unsafe impl DowncastType for MinecartEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/minecart");
}

impl MinecartEntity {
    /// Creates a new minecart entity.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self {
            base: EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
            entity_data: SyncMutex::new(MinecartEntityData::new()),
            minecart_base: AbstractMinecartBase::new(),
            behavior: OldMinecartBehavior,
        }
    }

    /// Creates a minecart entity from saved data.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self {
            base: EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
            entity_data: SyncMutex::new(MinecartEntityData::new()),
            minecart_base: AbstractMinecartBase::new(),
            behavior: OldMinecartBehavior,
        }
    }
}

impl VehicleEntity for MinecartEntity {
    fn hurt_time(&self) -> i32 {
        *self
            .entity_data
            .lock()
            .abstract_minecart()
            .vehicle_entity()
            .id_hurt
            .get()
    }

    fn set_hurt_time(&self, value: i32) {
        self.entity_data
            .lock()
            .abstract_minecart_mut()
            .vehicle_entity_mut()
            .id_hurt
            .set(value);
    }

    fn hurt_dir(&self) -> i32 {
        *self
            .entity_data
            .lock()
            .abstract_minecart()
            .vehicle_entity()
            .id_hurtdir
            .get()
    }

    fn set_hurt_dir(&self, value: i32) {
        self.entity_data
            .lock()
            .abstract_minecart_mut()
            .vehicle_entity_mut()
            .id_hurtdir
            .set(value);
    }

    fn damage(&self) -> f32 {
        *self
            .entity_data
            .lock()
            .abstract_minecart()
            .vehicle_entity()
            .id_damage
            .get()
    }

    fn set_damage(&self, value: f32) {
        self.entity_data
            .lock()
            .abstract_minecart_mut()
            .vehicle_entity_mut()
            .id_damage
            .set(value);
    }

    fn vehicle_drop_item(&self) -> ItemRef {
        &vanilla_items::MINECART
    }
}

impl AbstractMinecart for MinecartEntity {
    fn minecart_base(&self) -> &AbstractMinecartBase {
        &self.minecart_base
    }

    fn minecart_behavior(&self) -> &dyn MinecartBehavior {
        &self.behavior
    }

    fn is_rideable(&self) -> bool {
        true
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

    fn activate_minecart(&self, _world: &World, _pos: BlockPos, powered: bool) {
        if !powered {
            return;
        }
        if self.is_vehicle() {
            self.eject_passengers();
        }
        if self.hurt_time() == 0 {
            self.set_hurt_dir(-self.hurt_dir());
            self.set_hurt_time(10);
            self.set_damage(50.0);
            self.mark_hurt();
        }
    }
}

impl Entity for MinecartEntity {
    fn interact(
        &self,
        player: &Player,
        _hand: InteractionHand,
        _location: DVec3,
    ) -> InteractionResult {
        if player.is_secondary_use_active() || self.is_vehicle() {
            return InteractionResult::Pass;
        }

        let Some(world) = self.level() else {
            return InteractionResult::Pass;
        };
        let Some(minecart) = world.get_entity_by_id(self.id()) else {
            return InteractionResult::Pass;
        };

        let _ = player.start_riding(&minecart);
        InteractionResult::Pass
    }

    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn tick(self: Arc<Self>) {
        let entity = self.into_shared_entity();
        if let Some(minecart) = entity.as_abstract_minecart() {
            minecart.tick_minecart(&entity);
        }
    }

    fn get_default_gravity(&self) -> f64 {
        self.minecart_default_gravity()
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
        self.minecart_behavior()
            .known_movement(self.default_known_movement())
    }

    fn motion_direction(&self) -> Direction {
        self.minecart_behavior()
            .motion_direction(self.as_abstract_minecart_event_source())
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
        self.vehicle_dimension_changing_delay()
    }

    fn get_relative_portal_position(&self, axis: Axis, area: FoundRectangle) -> DVec3 {
        self.minecart_relative_portal_position(axis, area)
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

    fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
        Some(&self.entity_data)
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.save_abstract_minecart_data(nbt);
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.load_abstract_minecart_data(nbt);
    }

    // TODO: Implement getPickResult.
}
