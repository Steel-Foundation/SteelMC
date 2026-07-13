//! Chest minecart state needed by structure generation and persistence.

use std::str::FromStr;
use std::sync::Weak;

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_macros::entity_behavior;
use steel_registry::entity_type::EntityTypeRef;
use steel_utils::Identifier;
use steel_utils::axis::Axis;
use steel_utils::block_util::FoundRectangle;
use steel_utils::{DowncastType, DowncastTypeKey};

use crate::entity::{
    Entity, EntityBase, EntityBaseLoad, SharedEntity,
    reset_forward_direction_of_relative_portal_position,
};
use crate::portal::portal_shape::PortalShape;
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

// SAFETY: This key is owned by Steel and uniquely identifies `ChestMinecartEntity`.
unsafe impl DowncastType for ChestMinecartEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/chest_minecart");
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

    fn get_relative_portal_position(&self, axis: Axis, portal_area: FoundRectangle) -> DVec3 {
        reset_forward_direction_of_relative_portal_position(PortalShape::get_relative_position(
            portal_area,
            axis,
            self.position(),
            self.dimensions_for_pose(self.pose()),
        ))
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

    #[test]
    fn chest_minecart_relative_portal_position_resets_forward_offset() {
        let minecart = ChestMinecartEntity::new(
            &vanilla_entities::CHEST_MINECART,
            1,
            DVec3::new(12.0, 66.0, 20.75),
            Weak::new(),
        );
        let portal_area = FoundRectangle {
            min_corner: steel_utils::BlockPos::new(10, 64, 20),
            axis1_size: 4,
            axis2_size: 5,
        };

        let mut guard = minecart.lock_entity();
        let minecart: &mut ChestMinecartEntity = guard.downcast().unwrap();

        assert!(
            minecart
                .get_relative_portal_position(Axis::X, portal_area)
                .z
                .abs()
                < f64::EPSILON
        );
    }
}
