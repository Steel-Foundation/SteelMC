//! Boat vehicle entity (`Boat` / `ChestBoat`).
//!
//! Steel currently implements boat placement, mounting, gravity/float physics,
//! and item drops. Paddle propulsion, lily-pad breaking, and chest container
//! access are not yet implemented.

use std::sync::Weak;

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::entity_behavior;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_items;
use steel_utils::axis::Axis;
use steel_utils::block_util::FoundRectangle;
use steel_utils::locks::SyncMutex;
use steel_utils::types::InteractionHand;
use steel_utils::{DowncastType, DowncastTypeKey};

use crate::behavior::InteractionResult;
use crate::entity::damage::DamageSource;
use crate::entity::{
    Entity, EntityBase, EntityBaseLoad, MoverType, RemovalReason, SharedEntity,
    reset_forward_direction_of_relative_portal_position,
};
use crate::portal::portal_shape::PortalShape;
use crate::world::World;

/// Runtime state for a boat entity.
#[derive(Debug)]
struct BoatState {
    out_of_control_ticks: i32,
    left_paddle: bool,
    right_paddle: bool,
}

impl BoatState {
    const fn new() -> Self {
        Self {
            out_of_control_ticks: 0,
            left_paddle: false,
            right_paddle: false,
        }
    }
}

/// Entity behavior for the vanilla `Boat` class.
#[entity_behavior(class = "Boat")]
pub struct BoatEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    state: SyncMutex<BoatState>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `BoatEntity`.
unsafe impl DowncastType for BoatEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/boat");
}

impl BoatEntity {
    /// Creates a new Boat entity.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self {
            base: EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
            state: SyncMutex::new(BoatState::new()),
        }
    }

    /// Creates a Boat entity from saved data.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self {
            base: EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
            state: SyncMutex::new(BoatState::new()),
        }
    }

    /// Drops the boat's item, mirroring vanilla `Boat.destroy`/`chestBoat.destroy`.
    fn drop_as_items(&self) {
        if self.level().is_none() {
            return;
        }
        let drop = if self.entity_type().is_abstract_boat {
            &vanilla_items::OAK_BOAT
        } else {
            &vanilla_items::OAK_CHEST_BOAT
        };
        let _ = self.spawn_at_location(ItemStack::new(drop), 0.0);
    }

    /// Stores the client's paddle state and clears the out-of-control timer.
    ///
    /// Mirrors vanilla `Boat.setPaddleState`.
    pub fn set_paddle_state(&self, left: bool, right: bool) {
        let mut state = self.state.lock();
        state.left_paddle = left;
        state.right_paddle = right;
        state.out_of_control_ticks = 0;
    }

    /// Returns whether the left paddle is turning.
    pub fn left_paddle_is_turning(&self) -> bool {
        self.state.lock().left_paddle
    }

    /// Returns whether the right paddle is turning.
    pub fn right_paddle_is_turning(&self) -> bool {
        self.state.lock().right_paddle
    }
}

impl Entity for BoatEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn is_pickable(&self) -> bool {
        !self.is_removed()
    }

    fn is_pushable(&self) -> bool {
        true
    }

    fn blocks_building(&self) -> bool {
        true
    }

    fn controlling_passenger(&self) -> Option<SharedEntity> {
        self.passengers().into_iter().next()
    }

    fn get_default_gravity(&self) -> f64 {
        0.04
    }

    fn dimension_changing_delay(&self) -> i32 {
        10
    }

    fn get_relative_portal_position(&self, axis: Axis, portal_area: FoundRectangle) -> DVec3 {
        reset_forward_direction_of_relative_portal_position(PortalShape::get_relative_position(
            portal_area,
            axis,
            self.position(),
            self.dimensions_for_pose(self.pose()),
        ))
    }

    fn interact_entity(
        &self,
        player: &crate::player::Player,
        _hand: InteractionHand,
        _location: DVec3,
    ) -> InteractionResult {
        if self.is_vehicle() || player.is_secondary_use_active() {
            return InteractionResult::Pass;
        }
        if let Some(world) = self.level()
            && let Some(vehicle) = world.get_entity_by_id(self.id())
        {
            player.start_riding(&vehicle);
        }
        InteractionResult::Success
    }

    fn hurt(&self, _world: &World, source: &DamageSource, _amount: f32) -> bool {
        if self.is_invulnerable_to_base(source) {
            return false;
        }
        self.drop_as_items();
        self.set_removed(RemovalReason::Killed);
        self.game_event(&steel_registry::vanilla_game_events::ENTITY_DIE);
        true
    }

    fn tick(&self) {
        self.base_tick();

        self.apply_gravity();

        let velocity = self.velocity();
        if self
            .move_entity(MoverType::SelfMovement, velocity)
            .is_some()
        {
            let drag = if self.is_in_water() { 0.5 } else { 0.9 };
            self.set_velocity(DVec3::new(
                velocity.x * drag,
                velocity.y * 0.95,
                velocity.z * drag,
            ));
        }
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        let state = self.state.lock();
        nbt.insert("OutOfControlTicks", state.out_of_control_ticks);
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        if let Some(ticks) = nbt.int("OutOfControlTicks") {
            self.state.lock().out_of_control_ticks = ticks;
        }
    }
}

/// Entity behavior for the vanilla `ChestBoat` class.
#[entity_behavior(class = "ChestBoat")]
pub struct ChestBoatEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    state: SyncMutex<BoatState>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `ChestBoatEntity`.
unsafe impl DowncastType for ChestBoatEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/chest_boat");
}

impl ChestBoatEntity {
    /// Creates a new ChestBoat entity.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self {
            base: EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
            state: SyncMutex::new(BoatState::new()),
        }
    }

    /// Creates a ChestBoat entity from saved data.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self {
            base: EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
            state: SyncMutex::new(BoatState::new()),
        }
    }

    /// Drops the boat's item, mirroring vanilla `Boat.destroy`/`chestBoat.destroy`.
    fn drop_as_items(&self) {
        if self.level().is_none() {
            return;
        }
        let drop = if self.entity_type().is_abstract_boat {
            &vanilla_items::OAK_BOAT
        } else {
            &vanilla_items::OAK_CHEST_BOAT
        };
        let _ = self.spawn_at_location(ItemStack::new(drop), 0.0);
    }

    /// Stores the client's paddle state and clears the out-of-control timer.
    ///
    /// Mirrors vanilla `Boat.setPaddleState`.
    pub fn set_paddle_state(&self, left: bool, right: bool) {
        let mut state = self.state.lock();
        state.left_paddle = left;
        state.right_paddle = right;
        state.out_of_control_ticks = 0;
    }

    /// Returns whether the left paddle is turning.
    pub fn left_paddle_is_turning(&self) -> bool {
        self.state.lock().left_paddle
    }

    /// Returns whether the right paddle is turning.
    pub fn right_paddle_is_turning(&self) -> bool {
        self.state.lock().right_paddle
    }
}

impl Entity for ChestBoatEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn is_pickable(&self) -> bool {
        !self.is_removed()
    }

    fn is_pushable(&self) -> bool {
        true
    }

    fn blocks_building(&self) -> bool {
        true
    }

    fn controlling_passenger(&self) -> Option<SharedEntity> {
        self.passengers().into_iter().next()
    }

    fn get_default_gravity(&self) -> f64 {
        0.04
    }

    fn dimension_changing_delay(&self) -> i32 {
        10
    }

    fn get_relative_portal_position(&self, axis: Axis, portal_area: FoundRectangle) -> DVec3 {
        reset_forward_direction_of_relative_portal_position(PortalShape::get_relative_position(
            portal_area,
            axis,
            self.position(),
            self.dimensions_for_pose(self.pose()),
        ))
    }

    fn interact_entity(
        &self,
        player: &crate::player::Player,
        _hand: InteractionHand,
        _location: DVec3,
    ) -> InteractionResult {
        if self.is_vehicle() || player.is_secondary_use_active() {
            return InteractionResult::Pass;
        }
        if let Some(world) = self.level()
            && let Some(vehicle) = world.get_entity_by_id(self.id())
        {
            player.start_riding(&vehicle);
        }
        InteractionResult::Success
    }

    fn hurt(&self, _world: &World, source: &DamageSource, _amount: f32) -> bool {
        if self.is_invulnerable_to_base(source) {
            return false;
        }
        self.drop_as_items();
        self.set_removed(RemovalReason::Killed);
        self.game_event(&steel_registry::vanilla_game_events::ENTITY_DIE);
        true
    }

    fn tick(&self) {
        self.base_tick();

        self.apply_gravity();

        let velocity = self.velocity();
        if self
            .move_entity(MoverType::SelfMovement, velocity)
            .is_some()
        {
            let drag = if self.is_in_water() { 0.5 } else { 0.9 };
            self.set_velocity(DVec3::new(
                velocity.x * drag,
                velocity.y * 0.95,
                velocity.z * drag,
            ));
        }
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        let state = self.state.lock();
        nbt.insert("OutOfControlTicks", state.out_of_control_ticks);
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        if let Some(ticks) = nbt.int("OutOfControlTicks") {
            self.state.lock().out_of_control_ticks = ticks;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use steel_registry::vanilla_entities;

    #[test]
    fn boat_saves_and_loads_out_of_control_ticks() {
        use std::io::Cursor;

        use simdnbt::borrow::read_compound as read_borrowed_compound;

        let boat = BoatEntity::new(
            &vanilla_entities::OAK_BOAT,
            1,
            DVec3::new(1.0, 2.0, 3.0),
            Weak::new(),
        );
        boat.state.lock().out_of_control_ticks = 7;

        let mut nbt = NbtCompound::new();
        boat.save_additional(&mut nbt);
        assert_eq!(nbt.int("OutOfControlTicks"), Some(7));

        let mut bytes = Vec::new();
        nbt.write(&mut bytes);
        let borrowed = read_borrowed_compound(&mut Cursor::new(&bytes))
            .unwrap_or_else(|error| panic!("test NBT should reborrow: {error}"));
        let loaded = BoatEntity::new(&vanilla_entities::OAK_BOAT, 2, DVec3::ZERO, Weak::new());
        loaded.load_additional((&borrowed).into());
        assert_eq!(loaded.state.lock().out_of_control_ticks, 7);
    }
}
