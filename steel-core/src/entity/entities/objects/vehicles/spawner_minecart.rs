//! Vanilla `MinecartSpawner`: a minecart carrying a mob spawner.

use crate::entity::entities::objects::vehicles::abstract_minecart::AbstractMinecartTable;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::entity_behavior;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::items::ItemRef;
use steel_registry::vanilla_entity_data::SpawnerMinecartEntityData;
use steel_registry::{vanilla_blocks, vanilla_items};
use steel_utils::entity_events::EntityStatus;
use steel_utils::locks::CheckedMutex;

use super::abstract_minecart::{self, AbstractMinecartBase, AbstractMinecartProps};
use super::vehicle_entity;
use crate::block_entity::entities::BaseSpawner;
use crate::entity::EntityTable;
use crate::entity::prelude::*;
use crate::entity::{EntityBase, EntityProps};

/// A minecart carrying a mob spawner.
// TODO: implement getPickResult.
#[entity_behavior(class = "MinecartSpawner")]
#[derive(Entity)]
#[entity(save = Self::save_spawner, load = Self::load_spawner)]
#[entity(key = "steel:entity/spawner_minecart", constructors)]
pub struct SpawnerMinecartEntity {
    #[entity(base)]
    base: EntityBase,
    #[entity(entity_type)]
    entity_type: EntityTypeRef,
    #[entity(synced)]
    entity_data: CheckedMutex<SpawnerMinecartEntityData>,
    #[entity(component)]
    minecart: AbstractMinecartBase,
    spawner: BaseSpawner,
}

impl vehicle_entity::Parts for SpawnerMinecartEntity {
    fn drop_item(&self) -> ItemRef {
        &vanilla_items::MINECART
    }
}

impl EntityTable for SpawnerMinecartEntity {
    const ENTITY: EntityProps<Self> = EntityProps {
        tick: |minecart| {
            abstract_minecart::tick(minecart);
            minecart.tick_spawner();
        },
        ..abstract_minecart::entity_props()
    };
}

impl SpawnerMinecartEntity {
    fn from_base(base: EntityBase, entity_type: EntityTypeRef) -> Self {
        Self {
            base,
            entity_type,
            entity_data: CheckedMutex::new(SpawnerMinecartEntityData::new()),
            minecart: AbstractMinecartBase::new(),
            spawner: BaseSpawner::new(),
        }
    }

    /// Returns the minecart's spawner state.
    #[must_use]
    pub const fn spawner(&self) -> &BaseSpawner {
        &self.spawner
    }

    fn tick_spawner(&self) {
        let Some(world) = self.level() else {
            return;
        };
        self.spawner.server_tick(&world, self.block_position(), || {
            // Spawner minecarts use event 1 to reset the client spawn delay.
            self.broadcast_entity_event(EntityStatus::Jump);
        });
    }

    fn save_spawner(&self, nbt: &mut NbtCompound) {
        self.spawner.save(nbt);
    }

    fn load_spawner(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.spawner.load(&nbt);
    }
}

impl AbstractMinecartTable for SpawnerMinecartEntity {
    const MINECART: AbstractMinecartProps<Self> = AbstractMinecartProps {
        default_display_block_state: |_| vanilla_blocks::SPAWNER.default_state(),
        ..AbstractMinecartProps::new()
    };
}

#[cfg(test)]
#[path = "tests/spawner_minecart.rs"]
mod tests;
