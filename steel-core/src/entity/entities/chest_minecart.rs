//! Chest minecart state needed by structure generation and persistence.

use std::str::FromStr;
use std::sync::Weak;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};

use crossbeam::atomic::AtomicCell;
use glam::DVec3;
use simdnbt::borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView};
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_registry::blocks::shapes::AABBd;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::vanilla_entities;
use steel_utils::Identifier;
use steel_utils::locks::SyncMutex;
use uuid::Uuid;

use crate::entity::{Entity, EntityBase};
use crate::world::World;

/// Chest minecart entity state used by mineshaft generation.
///
/// Steel does not yet implement minecart movement or container interaction, so this
/// entity currently preserves the vanilla placement and loot-table state that
/// structure generation creates.
pub struct ChestMinecartEntity {
    base: EntityBase,
    velocity: SyncMutex<DVec3>,
    rotation: AtomicCell<(f32, f32)>,
    on_ground: AtomicBool,
    first_tick: AtomicBool,
    loot_table: SyncMutex<Option<Identifier>>,
    loot_table_seed: AtomicI64,
}

impl ChestMinecartEntity {
    /// Creates a new chest minecart entity.
    #[must_use]
    pub fn new(id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self {
            base: EntityBase::new(id, position, world),
            velocity: SyncMutex::new(DVec3::ZERO),
            rotation: AtomicCell::new((0.0, 0.0)),
            on_ground: AtomicBool::new(false),
            first_tick: AtomicBool::new(true),
            loot_table: SyncMutex::new(None),
            loot_table_seed: AtomicI64::new(0),
        }
    }

    /// Creates a chest minecart entity from saved data.
    #[must_use]
    pub fn from_saved(
        id: i32,
        position: DVec3,
        uuid: Uuid,
        velocity: DVec3,
        rotation: (f32, f32),
        on_ground: bool,
        world: Weak<World>,
    ) -> Self {
        Self {
            base: EntityBase::with_uuid(id, uuid, position, world),
            velocity: SyncMutex::new(velocity),
            rotation: AtomicCell::new(rotation),
            on_ground: AtomicBool::new(on_ground),
            first_tick: AtomicBool::new(false),
            loot_table: SyncMutex::new(None),
            loot_table_seed: AtomicI64::new(0),
        }
    }

    /// Sets the deferred loot table used when the container is first opened.
    pub fn set_loot_table(&self, loot_table: Identifier, seed: i64) {
        *self.loot_table.lock() = Some(loot_table);
        self.loot_table_seed.store(seed, Ordering::Relaxed);
    }

    const fn nbt_bool(value: bool) -> i8 {
        if value { 1 } else { 0 }
    }
}

impl Entity for ChestMinecartEntity {
    fn base(&self) -> Option<&EntityBase> {
        Some(&self.base)
    }

    fn entity_type(&self) -> EntityTypeRef {
        &vanilla_entities::CHEST_MINECART
    }

    fn bounding_box(&self) -> AABBd {
        let pos = self.position();
        let dims = self.entity_type().dimensions;
        let half_width = f64::from(dims.width) / 2.0;
        let height = f64::from(dims.height);
        AABBd {
            min_x: pos.x - half_width,
            min_y: pos.y,
            min_z: pos.z - half_width,
            max_x: pos.x + half_width,
            max_y: pos.y + height,
            max_z: pos.z + half_width,
        }
    }

    fn rotation(&self) -> (f32, f32) {
        self.rotation.load()
    }

    fn velocity(&self) -> DVec3 {
        *self.velocity.lock()
    }

    fn set_velocity(&self, velocity: DVec3) {
        *self.velocity.lock() = velocity;
    }

    fn on_ground(&self) -> bool {
        self.on_ground.load(Ordering::Relaxed)
    }

    fn set_on_ground(&self, on_ground: bool) {
        self.on_ground.store(on_ground, Ordering::Relaxed);
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        nbt.insert("FlippedRotation", Self::nbt_bool(false));
        nbt.insert(
            "HasTicked",
            Self::nbt_bool(self.first_tick.load(Ordering::Relaxed)),
        );

        if let Some(loot_table) = self.loot_table.lock().as_ref() {
            nbt.insert("LootTable", loot_table.to_string());
            let seed = self.loot_table_seed.load(Ordering::Relaxed);
            if seed != 0 {
                nbt.insert("LootTableSeed", NbtTag::Long(seed));
            }
        }
    }

    fn load_additional(&self, nbt: &BorrowedNbtCompound<'_>) {
        let nbt: NbtCompoundView<'_, '_> = nbt.into();

        if let Some(first_tick) = nbt.byte("HasTicked") {
            self.first_tick.store(first_tick != 0, Ordering::Relaxed);
        }

        let loot_table = nbt
            .string("LootTable")
            .and_then(|value| Identifier::from_str(&value.to_string()).ok());
        *self.loot_table.lock() = loot_table;
        self.loot_table_seed
            .store(nbt.long("LootTableSeed").unwrap_or(0), Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chest_minecart_saves_structure_loot_table_state() {
        let minecart = ChestMinecartEntity::new(1, DVec3::new(1.5, 2.5, 3.5), Weak::new());
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
        assert_eq!(nbt.byte("FlippedRotation"), Some(0));
    }
}
