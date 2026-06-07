//! Minimal End Crystal entity implementation for End spike worldgen.

use std::sync::Weak;

use glam::DVec3;
use simdnbt::borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView};
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::vanilla_entities;
use steel_registry::vanilla_entity_data::EndCrystalEntityData;
use steel_utils::{BlockPos, locks::SyncMutex};

use crate::entity::{Entity, EntityBase, EntityBaseLoad, EntitySyncedData};
use crate::world::World;

/// End Crystal entity state needed by worldgen and persistence.
///
/// Steel currently implements the synchronized data and saved fields used by generated
/// End spikes. Portal handling, dragon fight callbacks, and explosion behavior are still
/// intentionally left to the broader entity/combat foundations.
pub struct EndCrystalEntity {
    base: EntityBase,
    entity_data: SyncMutex<EndCrystalEntityData>,
    state: SyncMutex<EndCrystalState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EndCrystalState {
    invulnerable: bool,
}

impl EndCrystalState {
    const fn new() -> Self {
        Self {
            invulnerable: false,
        }
    }
}

impl EndCrystalEntity {
    /// Creates a new End Crystal entity.
    #[must_use]
    pub fn new(id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self {
            base: EntityBase::new(
                id,
                position,
                vanilla_entities::END_CRYSTAL.dimensions,
                world,
            ),
            entity_data: SyncMutex::new(EndCrystalEntityData::new()),
            state: SyncMutex::new(EndCrystalState::new()),
        }
    }

    /// Creates an End Crystal entity from saved data.
    #[must_use]
    pub fn from_saved(load: EntityBaseLoad) -> Self {
        Self {
            base: EntityBase::from_load(load, vanilla_entities::END_CRYSTAL.dimensions),
            entity_data: SyncMutex::new(EndCrystalEntityData::new()),
            state: SyncMutex::new(EndCrystalState::new()),
        }
    }

    /// Sets the optional beam target.
    pub fn set_beam_target(&self, target: Option<BlockPos>) {
        self.entity_data.lock().beam_target.set(target);
    }

    /// Returns the optional beam target.
    #[must_use]
    pub fn beam_target(&self) -> Option<BlockPos> {
        *self.entity_data.lock().beam_target.get()
    }

    /// Sets whether the crystal renders its bedrock base.
    pub fn set_show_bottom(&self, show_bottom: bool) {
        self.entity_data.lock().show_bottom.set(show_bottom);
    }

    /// Returns whether the crystal renders its bedrock base.
    #[must_use]
    pub fn shows_bottom(&self) -> bool {
        *self.entity_data.lock().show_bottom.get()
    }

    /// Sets the vanilla invulnerable flag.
    pub fn set_invulnerable(&self, invulnerable: bool) {
        self.state.lock().invulnerable = invulnerable;
    }

    /// Returns the vanilla invulnerable flag.
    #[must_use]
    pub fn is_invulnerable(&self) -> bool {
        self.state.lock().invulnerable
    }

    /// Sets position and rotation, matching vanilla `Entity.snapTo`.
    ///
    /// # Panics
    ///
    /// Panics if the active world entity manager rejects the snap position. This is an invariant
    /// failure for loaded end crystals.
    pub fn snap_to(&self, position: DVec3, yaw: f32, pitch: f32) {
        if let Err(error) = self.base.try_set_position(position) {
            panic!(
                "failed to commit end crystal {} snap position: {error}",
                self.base.id()
            );
        }
        self.base.set_rotation((yaw, pitch));
        self.set_old_position_to_current();
    }

    const fn nbt_bool(value: bool) -> i8 {
        if value { 1 } else { 0 }
    }
}

impl Entity for EndCrystalEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        &vanilla_entities::END_CRYSTAL
    }

    fn tick(&self) {
        // TODO: Implement portal handling, fire refresh, dragon fight callbacks, and explosion behavior.
    }

    fn is_pickable(&self) -> bool {
        true
    }

    fn blocks_building(&self) -> bool {
        true
    }

    fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
        Some(&self.entity_data)
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        if let Some(target) = self.beam_target() {
            nbt.insert(
                "beam_target",
                NbtTag::IntArray(vec![target.x(), target.y(), target.z()]),
            );
        }

        nbt.insert("ShowBottom", Self::nbt_bool(self.shows_bottom()));
        // TODO: Move `Invulnerable` into shared entity save data once `EntityBase` owns it.
        nbt.insert("Invulnerable", Self::nbt_bool(self.is_invulnerable()));
    }

    fn load_additional(&self, nbt: &BorrowedNbtCompound<'_>) {
        let nbt: NbtCompoundView<'_, '_> = nbt.into();

        if let Some(target) = nbt.int_array("beam_target")
            && target.len() == 3
        {
            self.set_beam_target(Some(BlockPos::new(target[0], target[1], target[2])));
        }

        if let Some(show_bottom) = nbt.byte("ShowBottom") {
            self.set_show_bottom(show_bottom != 0);
        }

        if let Some(invulnerable) = nbt.byte("Invulnerable") {
            self.set_invulnerable(invulnerable != 0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    use simdnbt::borrow::read_compound as read_borrowed_compound;

    #[test]
    fn end_crystal_saves_invulnerable_state() {
        let crystal = EndCrystalEntity::new(1, DVec3::new(1.5, 2.5, 3.5), Weak::new());
        crystal.set_invulnerable(true);

        let mut nbt = NbtCompound::new();
        crystal.save_additional(&mut nbt);

        assert_eq!(nbt.byte("Invulnerable"), Some(1));

        let loaded = EndCrystalEntity::new(2, DVec3::new(4.5, 5.5, 6.5), Weak::new());
        let mut bytes = Vec::new();
        nbt.write(&mut bytes);
        let borrowed =
            read_borrowed_compound(&mut Cursor::new(&bytes)).expect("test nbt should reborrow");
        loaded.load_additional(&borrowed);
        assert!(loaded.is_invulnerable());
    }

    #[test]
    fn end_crystal_is_pickable_like_vanilla() {
        let crystal = EndCrystalEntity::new(1, DVec3::new(1.5, 2.5, 3.5), Weak::new());

        assert!(crystal.is_pickable());
    }

    #[test]
    fn end_crystal_blocks_building_like_vanilla() {
        let crystal = EndCrystalEntity::new(1, DVec3::ZERO, Weak::new());

        assert!(crystal.blocks_building());
    }
}
