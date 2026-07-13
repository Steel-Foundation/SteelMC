//! Chest minecart state needed by structure generation and persistence.

use std::str::FromStr;
use std::sync::Weak;

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_macros::entity_behavior;
use steel_registry::entity_type::EntityTypeRef;
use steel_utils::Identifier;

use crate::entity::{Entity, EntityBase, EntityBaseLoad, SharedEntity};
use crate::world::World;

/// Chest minecart entity state used by mineshaft generation.
///
/// Steel does not yet implement minecart movement or container interaction, so this
/// entity currently preserves the vanilla placement and loot-table state that
/// structure generation creates.
#[entity_behavior(class = "minecart_chest", identifier = "chest_minecart")]
pub struct ChestMinecartEntity {
    base: Weak<EntityBase>,
    entity_type: EntityTypeRef,
    first_tick: bool,
    loot_table: Option<Identifier>,
    loot_table_seed: i64,
}

impl ChestMinecartEntity {
    /// Creates a new chest minecart entity.
    #[must_use]
    pub fn new(
        entity_type: EntityTypeRef,
        id: i32,
        position: DVec3,
        world: Weak<World>,
    ) -> SharedEntity {
        EntityBase::pack_with(id, position, entity_type.dimensions, world, |base| Self {
            base,
            entity_type,
            first_tick: true,
            loot_table: None,
            loot_table_seed: 0,
        })
    }

    /// Restores a chest minecart `SharedEntity` from persistent data.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> SharedEntity {
        EntityBase::pack_loaded_with(load, entity_type.dimensions, |base| Self {
            base,
            entity_type,
            first_tick: true,
            loot_table: None,
            loot_table_seed: 0,
        })
    }

    /// Sets the deferred loot table used when the container is first opened.
    pub fn set_loot_table(&mut self, loot_table: Identifier, seed: i64) {
        self.loot_table = Some(loot_table);
        self.loot_table_seed = seed;
    }

    const fn nbt_bool(value: bool) -> i8 {
        if value { 1 } else { 0 }
    }
}

impl Entity for ChestMinecartEntity {
    fn base_weak(&self) -> &Weak<EntityBase> {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn is_pickable(&self) -> bool {
        !self.is_removed()
    }

    fn is_pushable(&mut self) -> bool {
        true
    }

    fn blocks_building(&self) -> bool {
        true
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        nbt.insert("FlippedRotation", Self::nbt_bool(false));
        nbt.insert("HasTicked", Self::nbt_bool(self.first_tick));

        if let Some(loot_table) = self.loot_table.as_ref() {
            nbt.insert("LootTable", loot_table.to_string());
            if self.loot_table_seed != 0 {
                nbt.insert("LootTableSeed", NbtTag::Long(self.loot_table_seed));
            }
        }
    }

    fn load_additional(&mut self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        let loot_table = nbt
            .string("LootTable")
            .and_then(|value| Identifier::from_str(&value.to_string()).ok());
        if let Some(first_tick) = nbt.byte("HasTicked") {
            self.first_tick = first_tick != 0;
        }
        self.loot_table = loot_table;
        self.loot_table_seed = nbt.long("LootTableSeed").unwrap_or(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use steel_registry::vanilla_entities;

    #[test]
    fn chest_minecart_saves_structure_loot_table_state() {
        let minecart = ChestMinecartEntity::new(
            &vanilla_entities::CHEST_MINECART,
            crate::entity::next_entity_id(),
            DVec3::ZERO,
            Weak::new(),
        );
        let mut nbt = NbtCompound::new();

        {
            let mut minecart = minecart.lock_entity();
            let minecart: &mut ChestMinecartEntity = minecart.downcast().unwrap();

            minecart.set_loot_table(
                Identifier::new_static("minecraft", "chests/abandoned_mineshaft"),
                42,
            );
            minecart.save_additional(&mut nbt);
        }

        assert_eq!(
            nbt.string("LootTable").map(ToString::to_string),
            Some("minecraft:chests/abandoned_mineshaft".to_owned())
        );
        assert_eq!(nbt.long("LootTableSeed"), Some(42));
        assert_eq!(nbt.byte("HasTicked"), Some(1));
        assert_eq!(nbt.byte("FlippedRotation"), Some(0));
    }

    #[test]
    fn chest_minecart_is_pickable_and_pushable_like_vanilla() {
        let minecart = ChestMinecartEntity::new(
            &vanilla_entities::CHEST_MINECART,
            crate::entity::next_entity_id(),
            DVec3::ZERO,
            Weak::new(),
        );

        minecart.with_entity(|e| {
            assert!(e.is_pickable());
            assert!(e.is_pushable());
            assert!(e.blocks_building());
        });
    }
}
