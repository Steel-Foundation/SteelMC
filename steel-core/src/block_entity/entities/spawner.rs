//! Mob spawner block entity implementation.

use std::sync::Weak;

use simdnbt::borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView};
use simdnbt::owned::NbtCompound;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::vanilla_block_entity_types;
use steel_utils::locks::SyncMutex;
use steel_utils::{BlockPos, BlockStateId, DowncastType, DowncastTypeKey};

use crate::block_entity::{BlockEntity, BlockEntityBase};
use crate::world::World;

const SPAWN_DATA_TAG: &str = "SpawnData";
/// Vanilla `SpawnData.ENTITY_TAG`.
const ENTITY_TAG: &str = "entity";
const SPAWN_POTENTIALS_TAG: &str = "SpawnPotentials";

struct SpawnerState {
    /// The spawned-entity payload of vanilla's next `SpawnData`
    /// (`SpawnData.entity`), holding at least the `id` string.
    ///
    /// Spawn delay/count/potentials fields stay unmodeled until spawner
    /// ticking exists; they follow vanilla's load defaults meanwhile.
    next_spawn_entity: Option<NbtCompound>,
}

/// Mob spawner block entity.
pub struct SpawnerBlockEntity {
    base: BlockEntityBase,
    state: SyncMutex<SpawnerState>,
}

// SAFETY: Steel owns this concrete block entity key.
unsafe impl DowncastType for SpawnerBlockEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:block_entity/spawner");
}

impl SpawnerBlockEntity {
    /// Creates a new mob spawner block entity.
    #[must_use]
    pub fn new(level: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
        Self {
            base: BlockEntityBase::new(&vanilla_block_entity_types::MOB_SPAWNER, level, pos, state),
            state: SyncMutex::new(SpawnerState {
                next_spawn_entity: None,
            }),
        }
    }

    /// Sets the entity type this spawner spawns, mirroring vanilla
    /// `BaseSpawner.setEntityId` by writing the type id into the next spawn
    /// data's `entity` payload. Callers broadcast [`BlockEntity::get_update_tag`]
    /// afterwards, mirroring vanilla's `sendBlockUpdated`.
    pub fn set_entity_id(&self, entity_type: EntityTypeRef) {
        let mut state = self.state.lock();
        let entity = state.next_spawn_entity.get_or_insert_with(NbtCompound::new);
        entity.remove("id");
        entity.insert("id", entity_type.key.to_string());
        drop(state);

        self.set_changed();
    }
}

impl BlockEntity for SpawnerBlockEntity {
    fn base(&self) -> &BlockEntityBase {
        &self.base
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        let state = self.state.lock();
        if let Some(entity) = &state.next_spawn_entity {
            // Vanilla serializes `nextSpawnData` through `SpawnData.CODEC`,
            // which wraps the entity payload in an `entity` compound.
            let mut spawn_data = NbtCompound::new();
            spawn_data.insert(ENTITY_TAG, entity.clone());
            nbt.insert(SPAWN_DATA_TAG, spawn_data);
        }
    }

    fn load_additional(&self, nbt: &BorrowedNbtCompound<'_>) {
        let view: NbtCompoundView<'_, '_> = nbt.into();
        let next_spawn_entity = view
            .compound(SPAWN_DATA_TAG)
            .and_then(|spawn_data| spawn_data.compound(ENTITY_TAG))
            .map(|entity| entity.to_owned());
        self.state.lock().next_spawn_entity = next_spawn_entity;
    }

    fn get_update_tag(&self) -> Option<NbtCompound> {
        // Vanilla `SpawnerBlockEntity.getUpdateTag`: custom save data without
        // `SpawnPotentials`.
        let mut tag = self.save_custom_only();
        tag.remove(SPAWN_POTENTIALS_TAG);
        Some(tag)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::borrow::read_compound;
    use simdnbt::owned::{NbtCompound, NbtTag};
    use steel_registry::vanilla_blocks;
    use steel_registry::{REGISTRY, RegistryExt, init_vanilla_registry};
    use steel_utils::Identifier;

    use super::*;

    fn spawner() -> SpawnerBlockEntity {
        SpawnerBlockEntity::new(
            Weak::new(),
            BlockPos::ZERO,
            vanilla_blocks::SPAWNER.default_state(),
        )
    }

    fn entity_ref(name: &'static str) -> EntityTypeRef {
        REGISTRY
            .entity_types
            .by_key(&Identifier::vanilla_static(name))
            .unwrap_or_else(|| panic!("{name} should be registered"))
    }

    fn spawn_entity_id(tag: &NbtCompound) -> Option<String> {
        Some(
            tag.compound(SPAWN_DATA_TAG)?
                .compound(ENTITY_TAG)?
                .string("id")?
                .to_string(),
        )
    }

    #[test]
    fn set_entity_id_writes_a_single_vanilla_id_into_spawn_data() {
        init_vanilla_registry();
        let spawner = spawner();

        spawner.set_entity_id(entity_ref("pig"));
        spawner.set_entity_id(entity_ref("cow"));

        // `NbtCompound::get` returns the first match, so this fails if setting
        // the id stacked entries instead of replacing them.
        let tag = spawner.get_update_tag().expect("spawners sync to clients");
        assert_eq!(spawn_entity_id(&tag), Some("minecraft:cow".to_owned()));
    }

    #[test]
    fn worldgen_spawn_data_survives_save_and_update_tag_drops_potentials() {
        init_vanilla_registry();

        let mut entity_nbt = NbtCompound::new();
        entity_nbt.insert("id", "minecraft:cave_spider");
        let mut spawn_data = NbtCompound::new();
        spawn_data.insert("entity", entity_nbt);
        let mut saved = NbtCompound::new();
        saved.insert("Delay", 20_i16);
        saved.insert("SpawnData", spawn_data);
        saved.insert(
            "SpawnPotentials",
            NbtTag::List(simdnbt::owned::NbtList::Empty),
        );

        let mut bytes = Vec::new();
        saved.write(&mut bytes);
        let borrowed = read_compound(&mut Cursor::new(bytes.as_slice()))
            .expect("test spawner NBT should reborrow");

        let spawner = spawner();
        spawner.load_additional(&borrowed);

        let round_tripped = spawner.save_custom_only();
        assert_eq!(
            spawn_entity_id(&round_tripped),
            Some("minecraft:cave_spider".to_owned())
        );

        let update_tag = spawner.get_update_tag().expect("spawners sync to clients");
        assert!(!update_tag.contains(SPAWN_POTENTIALS_TAG));
        assert_eq!(
            spawn_entity_id(&update_tag),
            Some("minecraft:cave_spider".to_owned())
        );
    }
}
