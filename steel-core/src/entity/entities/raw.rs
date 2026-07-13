//! NBT-preserving fallback entity.

use std::sync::Weak;

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_registry::entity_type::EntityTypeRef;
use steel_utils::{DowncastType, DowncastTypeKey, Identifier, UuidExt};
use uuid::Uuid;

use crate::entity::{
    Entity, EntityBase, EntityBaseLoad, EntityIdentifier, SharedEntity, next_entity_id,
};

/// Steel-specific fallback for entity types whose runtime behavior is not implemented yet.
///
/// Vanilla has concrete classes for every entity type. Steel uses this only to preserve
/// worldgen and disk NBT until the corresponding typed implementation is added.
pub struct RawEntity {
    base: Weak<EntityBase>,
    entity_type: EntityTypeRef,
    data: NbtCompound,
}

// SAFETY: This key identifies the Steel fallback implementation, independently
// of the Minecraft entity registry entry stored inside it.
unsafe impl DowncastType for RawEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/raw");
}

impl RawEntity {
    /// Creates a fresh raw entity for an entity type Steel cannot behaviorally model yet.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef) -> SharedEntity {
        Self::new_raw(next_entity_id(), entity_type)
    }

    /// Todo
    #[must_use]
    pub fn new_raw(id: i32, entity_type: EntityTypeRef) -> SharedEntity {
        EntityBase::pack_with(
            id,
            DVec3::ZERO,
            entity_type.dimensions,
            Weak::new(),
            |base| Self {
                base,
                entity_type,
                data: NbtCompound::new(),
            },
        )
    }

    /// Creates a raw entity from base entity data.
    #[must_use]
    pub fn from_saved(load: EntityBaseLoad, entity_type: EntityTypeRef) -> SharedEntity {
        EntityBase::pack_loaded_with(load, entity_type.dimensions, |base| Self {
            base,
            entity_type,
            data: NbtCompound::new(),
        })
    }

    /// Creates a fresh raw entity placed and oriented for worldgen structure spawning.
    ///
    /// Configures position, rotation, and vanilla `PersistenceRequired` at construction so
    /// callers never have to lock and downcast back to `RawEntity` — the key-based
    /// [`LockedEntity::downcast`](crate::entity::LockedEntity::downcast) cannot match a raw
    /// entity because it carries the real entity type key, not the `RawEntity::KEY` placeholder.
    /// Position is set directly (the construction path), matching vanilla `Entity.snapTo` for a
    /// not-yet-spawned entity.
    #[must_use]
    pub fn new_for_worldgen(
        entity_type: EntityTypeRef,
        position: DVec3,
        yaw: f32,
        pitch: f32,
        persistence_required: bool,
    ) -> SharedEntity {
        let mut data = NbtCompound::new();
        if persistence_required {
            data.insert("PersistenceRequired", 1_i8);
        }
        let base = EntityBase::pack_with(
            next_entity_id(),
            position,
            entity_type.dimensions,
            Weak::new(),
            |base| Self {
                base,
                entity_type,
                data,
            },
        );
        base.set_rotation((yaw, pitch));
        base
    }
}

impl Entity for RawEntity {
    fn base_weak(&self) -> &Weak<EntityBase> {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn tick(&mut self) {
        // TODO: Replace raw entity ticking with full vanilla behavior for this entity type.
    }

    fn attackable(&self) -> bool {
        false
    }

    fn projectile_owner_uuid(&self) -> Option<Uuid> {
        if !self.entity_type.is_projectile {
            return None;
        }

        self.projectile_owner_uuid_from_nbt()
    }

    fn load_additional(&mut self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.data = nbt.to_owned();
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        *nbt = self.data.clone();
    }
}

impl EntityIdentifier for RawEntity {
    const KEY: Identifier = Identifier::new_static("steel", "unimplemented");
}

impl RawEntity {
    fn projectile_owner_uuid_from_nbt(&self) -> Option<Uuid> {
        let owner = self.data.int_array("Owner")?;
        Uuid::from_int_array(owner)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Weak;

    use simdnbt::owned::{NbtCompound, NbtTag};
    use steel_registry::vanilla_entities;
    use steel_utils::UuidExt;
    use uuid::Uuid;

    use crate::entity::Entity as _;

    use super::RawEntity;

    #[test]
    fn raw_projectile_reads_vanilla_owner_uuid() {
        let owner = Uuid::from_u128(42);
        let mut data = NbtCompound::new();
        data.insert("Owner", NbtTag::IntArray(owner.to_int_array().to_vec()));
        let entity = RawEntity {
            base: Weak::new(),
            entity_type: &vanilla_entities::ENDER_PEARL,
            data,
        };

        assert_eq!(entity.projectile_owner_uuid(), Some(owner));
    }
}
