//! Mob spawner block entity implementation.

use std::sync::Weak;

use glam::DVec3;

use simdnbt::borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView};
use simdnbt::owned::NbtCompound;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::{REGISTRY, RegistryExt, vanilla_block_entity_types};
use steel_utils::locks::SyncMutex;
use steel_utils::{BlockPos, BlockStateId, DowncastType, DowncastTypeKey, Identifier, WorldAabb};

use crate::block_entity::{BlockEntity, BlockEntityBase};
use crate::entity::{ENTITIES, Entity as _, EntitySpawnReason, next_entity_id};
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
    delay: i32,
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
                delay: 20,
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

    /// Runs the vanilla mob-spawner tick for the currently configured entity.
    pub fn tick_spawner(&self, world: &std::sync::Arc<World>) {
        let pos = self.get_block_pos();
        let Some(entity_id) = self
            .state
            .lock()
            .next_spawn_entity
            .as_ref()
            .and_then(|entity| entity.string("id"))
            .map(ToOwned::to_owned)
        else {
            return;
        };
        if world
            .nearest_player(
                DVec3::new(
                    f64::from(pos.x()) + 0.5,
                    f64::from(pos.y()) + 0.5,
                    f64::from(pos.z()) + 0.5,
                ),
                16.0,
                |p| !p.is_spectator(),
            )
            .is_none()
        {
            return;
        }
        let mut state = self.state.lock();
        if state.delay > 0 {
            state.delay -= 1;
            return;
        }
        let Ok(key) = entity_id.to_string().parse::<Identifier>() else {
            return;
        };
        let Some(entity_type) = REGISTRY.entity_types.by_key(&key) else {
            return;
        };
        let center = DVec3::new(
            f64::from(pos.x()) + 0.5,
            f64::from(pos.y()),
            f64::from(pos.z()) + 0.5,
        );
        let nearby = WorldAabb::new(
            center.x - 4.0,
            center.y - 2.0,
            center.z - 4.0,
            center.x + 4.0,
            center.y + 2.0,
            center.z + 4.0,
        );
        if world
            .get_entities_in_aabb_matching(&nearby, |entity| {
                entity.entity_type().key == entity_type.key
            })
            .len()
            >= 6
        {
            state.delay = 20;
            return;
        }
        let spawn = DVec3::new(
            center.x + rand::random_range(-3.5..3.5),
            center.y,
            center.z + rand::random_range(-3.5..3.5),
        );
        let Some(entity) = ENTITIES.create(
            entity_type,
            next_entity_id(),
            spawn,
            std::sync::Arc::downgrade(world),
        ) else {
            state.delay = 20;
            return;
        };
        if entity.try_set_position(spawn).is_ok() {
            if let Some(mob) = entity.as_mob() {
                mob.finalize_spawn(world, EntitySpawnReason::Spawner, None);
            }
            let _ = world.try_add_entity(entity);
        }
        state.delay = rand::random_range(200..=800);
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
        nbt.insert("Delay", state.delay);
    }

    fn load_additional(&self, nbt: &BorrowedNbtCompound<'_>) {
        let view: NbtCompoundView<'_, '_> = nbt.into();
        let next_spawn_entity = view
            .compound(SPAWN_DATA_TAG)
            .and_then(|spawn_data| spawn_data.compound(ENTITY_TAG))
            .map(|entity| entity.to_owned());
        let delay = view.int("Delay").unwrap_or(20);
        let mut state = self.state.lock();
        state.next_spawn_entity = next_spawn_entity;
        state.delay = delay;
    }

    fn get_update_tag(&self) -> Option<NbtCompound> {
        // Vanilla `SpawnerBlockEntity.getUpdateTag`: custom save data without
        // `SpawnPotentials`.
        let mut tag = self.save_custom_only();
        tag.remove(SPAWN_POTENTIALS_TAG);
        Some(tag)
    }

    fn tick(&self, world: &std::sync::Arc<World>) {
        self.tick_spawner(world);
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
