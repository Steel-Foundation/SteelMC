//! Vanilla `MinecartChest`.

use std::str::FromStr;
use std::sync::Weak;

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_macros::entity_behavior;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::vanilla_game_rules::MOB_GRIEFING;
use steel_utils::Identifier;
use steel_utils::locks::CheckedMutex;
use steel_utils::{DowncastType, DowncastTypeKey};

use crate::entity::prelude::*;
use crate::entity::{
    EntityBase, EntityBaseLoad, EntityMovementEmission, EntityProps,
    reset_forward_direction_of_relative_portal_position,
};
use crate::portal::portal_shape::PortalShape;
use crate::world::World;

/// A chest minecart with its placement and loot table state.
///
/// TODO: add the `AbstractMinecart` movement and the chest container; this
/// keeps what structure generation places and persists.
#[entity_behavior(class = "MinecartChest")]
#[derive(Entity)]
#[entity(hooks = Self::ENTITY, save = Self::save_chest, load = Self::load_chest)]
pub struct ChestMinecartEntity {
    #[entity(base)]
    base: EntityBase,
    #[entity(entity_type)]
    entity_type: EntityTypeRef,
    state: CheckedMutex<ChestMinecartState>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `ChestMinecartEntity`.
unsafe impl DowncastType for ChestMinecartEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/chest_minecart");
}

struct ChestMinecartState {
    first_tick: bool,
    loot_table: Option<Identifier>,
    loot_table_seed: i64,
}

impl ChestMinecartEntity {
    const ENTITY: EntityProps<Self> = EntityProps {
        movement_emission: |_| EntityMovementEmission::Events,
        is_pickable: |minecart| !minecart.is_removed(),
        is_pushable: |_| true,
        blocks_building: |_| true,
        ignore_explosion: |_, explosion| {
            !explosion.world().get_game_rule(&MOB_GRIEFING)
                && explosion
                    .indirect_source_entity()
                    .is_some_and(|entity| entity.as_mob().is_some())
        },
        dimension_changing_delay: |_| 10,
        relative_portal_position: |minecart, axis, portal_area| {
            reset_forward_direction_of_relative_portal_position(PortalShape::get_relative_position(
                portal_area,
                axis,
                minecart.position(),
                minecart.dimensions_for_pose(minecart.pose()),
            ))
        },
        ..EntityProps::new()
    };

    /// Creates a chest minecart.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::from_base(
            EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
            true,
        )
    }

    /// Reconstructs a chest minecart from saved base data.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self::from_base(
            EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
            false,
        )
    }

    const fn from_base(base: EntityBase, entity_type: EntityTypeRef, first_tick: bool) -> Self {
        Self {
            base,
            entity_type,
            state: CheckedMutex::new(ChestMinecartState {
                first_tick,
                loot_table: None,
                loot_table_seed: 0,
            }),
        }
    }

    /// Sets the loot table that fills the chest when it is first opened.
    pub fn set_loot_table(&self, loot_table: Identifier, seed: i64) {
        let mut state = self.state.lock();
        state.loot_table = Some(loot_table);
        state.loot_table_seed = seed;
    }

    fn save_chest(&self, nbt: &mut NbtCompound) {
        nbt.insert("FlippedRotation", 0_i8);
        let state = self.state.lock();
        nbt.insert("HasTicked", i8::from(state.first_tick));
        if let Some(loot_table) = &state.loot_table {
            nbt.insert("LootTable", loot_table.to_string());
            if state.loot_table_seed != 0 {
                nbt.insert("LootTableSeed", NbtTag::Long(state.loot_table_seed));
            }
        }
    }

    fn load_chest(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        let loot_table = nbt
            .string("LootTable")
            .and_then(|value| Identifier::from_str(&value.to_string()).ok());
        let mut state = self.state.lock();
        if let Some(first_tick) = nbt.byte("HasTicked") {
            state.first_tick = first_tick != 0;
        }
        state.loot_table = loot_table;
        state.loot_table_seed = nbt.long("LootTableSeed").unwrap_or(0);
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::vanilla_entities;
    use steel_utils::axis::Axis;
    use steel_utils::block_util::FoundRectangle;

    use super::*;

    fn minecart(position: DVec3) -> ChestMinecartEntity {
        ChestMinecartEntity::new(&vanilla_entities::CHEST_MINECART, 1, position, Weak::new())
    }

    #[test]
    fn chest_minecart_saves_structure_loot_table_state() {
        let minecart = minecart(DVec3::new(1.5, 2.5, 3.5));
        minecart.set_loot_table(
            Identifier::new_static("minecraft", "chests/abandoned_mineshaft"),
            42,
        );

        let mut nbt = NbtCompound::new();
        minecart.save_additional(&mut nbt);

        assert_eq!(
            nbt.string("LootTable").map(ToString::to_string),
            Some("minecraft:chests/abandoned_mineshaft".to_owned())
        );
        assert_eq!(nbt.long("LootTableSeed"), Some(42));
        assert_eq!(nbt.byte("HasTicked"), Some(1));
    }

    #[test]
    fn relative_portal_position_resets_the_forward_offset() {
        let minecart = minecart(DVec3::new(12.0, 66.0, 20.75));
        let portal_area = FoundRectangle {
            min_corner: steel_utils::BlockPos::new(10, 64, 20),
            axis1_size: 4,
            axis2_size: 5,
        };

        assert!(
            minecart
                .relative_portal_position(Axis::X, portal_area)
                .z
                .abs()
                < f64::EPSILON
        );
    }
}
