//! Normal mob-spawner block entity and its persistent spawn configuration.

mod base_spawner;
mod spawn_data;
#[cfg(test)]
mod tests;

use std::sync::{Arc, Weak};

use simdnbt::borrow::BaseNbtCompound as BorrowedNbtCompound;
use simdnbt::owned::NbtCompound;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::vanilla_block_entity_types;
use steel_utils::{BlockPos, BlockStateId, DowncastType, DowncastTypeKey};

use crate::block_entity::{BlockEntity, BlockEntityBase};
use crate::world::World;

pub use base_spawner::BaseSpawner;
pub use spawn_data::{CustomSpawnRules, EquipmentTable, LightRange, SpawnData, WeightedSpawnData};

/// Capability implemented by normal spawner block entities.
pub trait Spawner {
    /// Sets the next spawn entity type.
    fn set_entity_id(&self, entity_type: EntityTypeRef);
}

/// Concrete normal mob-spawner block entity.
pub struct SpawnerBlockEntity {
    base: BlockEntityBase,
    spawner: BaseSpawner,
}

// SAFETY: This key is owned by Steel and uniquely identifies `SpawnerBlockEntity`.
unsafe impl DowncastType for SpawnerBlockEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:block_entity/spawner");
}

impl SpawnerBlockEntity {
    /// Creates a normal mob-spawner block entity.
    #[must_use]
    pub fn new(level: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
        Self {
            base: BlockEntityBase::new(&vanilla_block_entity_types::MOB_SPAWNER, level, pos, state),
            spawner: BaseSpawner::new(),
        }
    }
}

impl Spawner for SpawnerBlockEntity {
    fn set_entity_id(&self, entity_type: EntityTypeRef) {
        self.spawner.set_entity_id(entity_type);
        self.set_changed();
        if let Some(world) = self.get_level() {
            world.send_block_updated(self.get_block_pos());
        }
    }
}

impl BlockEntity for SpawnerBlockEntity {
    fn base(&self) -> &BlockEntityBase {
        &self.base
    }

    fn load_additional(&self, nbt: &BorrowedNbtCompound<'_>) {
        self.spawner.load(nbt);
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.spawner.save(nbt);
    }

    fn get_update_tag(&self) -> Option<NbtCompound> {
        let mut nbt = self.save_custom_only();
        nbt.remove("SpawnPotentials");
        Some(nbt)
    }

    fn trigger_event(&self, event: i32, _data: i32) -> bool {
        self.spawner.on_event_triggered(event)
    }

    fn as_spawner(&self) -> Option<&dyn Spawner> {
        Some(self)
    }

    fn tick(&self, world: &Arc<World>) {
        let result = self.spawner.server_tick(world, self.get_block_pos());
        if result.state_changed {
            self.set_changed();
        }
        if result.next_spawn_data_changed {
            world.send_block_updated(self.get_block_pos());
        }
    }
}
