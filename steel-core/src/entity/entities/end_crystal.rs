//! Minimal End Crystal entity implementation for End spike worldgen.

use std::sync::Weak;

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_macros::entity_behavior;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::vanilla_entities;
use steel_registry::vanilla_entity_data::EndCrystalEntityData;
use steel_utils::{BlockPos, DowncastType, DowncastTypeKey};

use crate::entity::{Entity, EntityBase, EntityBaseLoad, EntitySyncedData, SharedEntity};
use crate::world::World;

/// End Crystal entity state needed by worldgen and persistence.
///
/// Steel currently implements the synchronized data and saved fields used by generated
/// End spikes. Portal handling, dragon fight callbacks, and explosion behavior are still
/// intentionally left to the broader entity/combat foundations.
#[entity_behavior(class = "end_crystal")]
pub struct EndCrystal {
    base: Weak<EntityBase>,
    entity_data: EndCrystalEntityData,
}

// SAFETY: This key is owned by Steel and uniquely identifies `EndCrystal`.
unsafe impl DowncastType for EndCrystal {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/end_crystal");
}

impl EndCrystal {
    /// Creates a new End Crystal entity.
    #[must_use]
    pub fn new(id: i32, position: DVec3, world: Weak<World>) -> SharedEntity {
        EntityBase::pack_with(
            id,
            position,
            vanilla_entities::END_CRYSTAL.dimensions,
            world,
            |base| Self {
                base,
                entity_data: EndCrystalEntityData::new(),
            },
        )
    }

    /// Restores an End Crystal `SharedEntity` from persistent data.
    #[must_use]
    pub fn from_saved(load: EntityBaseLoad) -> SharedEntity {
        EntityBase::pack_loaded_with(load, vanilla_entities::END_CRYSTAL.dimensions, |base| {
            Self {
                base,
                entity_data: EndCrystalEntityData::new(),
            }
        })
    }

    /// Sets the optional beam target.
    pub fn set_beam_target(&mut self, target: Option<BlockPos>) {
        self.entity_data.set_beam_target(target);
    }

    /// Returns the optional beam target.
    #[must_use]
    pub fn beam_target(&self) -> Option<BlockPos> {
        *self.entity_data.beam_target.get()
    }

    /// Sets whether the crystal renders its bedrock base.
    pub fn set_show_bottom(&mut self, show_bottom: bool) {
        self.entity_data.set_show_bottom(show_bottom);
    }

    /// Returns whether the crystal renders its bedrock base.
    #[must_use]
    pub fn shows_bottom(&self) -> bool {
        *self.entity_data.show_bottom.get()
    }

    /// Sets position and rotation, matching vanilla `Entity.snapTo`.
    ///
    /// # Panics
    ///
    /// Panics if the active world entity manager rejects the snap position. This is an invariant
    /// failure for loaded end crystals.
    pub fn snap_to(&mut self, position: DVec3, yaw: f32, pitch: f32) {
        if let Err(error) = self.try_set_position(position) {
            panic!(
                "failed to commit end crystal {} snap position: {error}",
                self.id()
            );
        }
        self.set_rotation((yaw, pitch));
        self.set_old_position_to_current();
    }

    const fn nbt_bool(value: bool) -> i8 {
        if value { 1 } else { 0 }
    }
}

impl Entity for EndCrystal {
    fn base_weak(&self) -> &Weak<EntityBase> {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        &vanilla_entities::END_CRYSTAL
    }

    fn tick(&mut self) {
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

    fn synced_data_mut(&mut self) -> Option<&mut dyn EntitySyncedData> {
        Some(&mut self.entity_data)
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        if let Some(target) = self.beam_target() {
            nbt.insert(
                "beam_target",
                NbtTag::IntArray(vec![target.x(), target.y(), target.z()]),
            );
        }

        nbt.insert("ShowBottom", Self::nbt_bool(self.shows_bottom()));
    }

    fn load_additional(&mut self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        if let Some(target) = nbt.int_array("beam_target")
            && target.len() == 3
        {
            self.set_beam_target(Some(BlockPos::new(target[0], target[1], target[2])));
        }

        if let Some(show_bottom) = nbt.byte("ShowBottom") {
            self.set_show_bottom(show_bottom != 0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn end_crystal_does_not_duplicate_shared_invulnerable_state() {
        let crystal = EndCrystal::new(crate::entity::next_entity_id(), DVec3::ZERO, Weak::new());

        let mut nbt = NbtCompound::new();
        crystal.with_entity(|e| e.save_additional(&mut nbt));

        assert_eq!(nbt.byte("Invulnerable"), None);
    }

    #[test]
    fn end_crystal_is_pickable_like_vanilla() {
        let crystal = EndCrystal::new(crate::entity::next_entity_id(), DVec3::ZERO, Weak::new());

        assert!(crystal.with_entity(|e| e.is_pickable()));
    }

    #[test]
    fn end_crystal_blocks_building_like_vanilla() {
        let crystal = EndCrystal::new(crate::entity::next_entity_id(), DVec3::ZERO, Weak::new());

        assert!(crystal.with_entity(|e| e.blocks_building()));
    }
}
