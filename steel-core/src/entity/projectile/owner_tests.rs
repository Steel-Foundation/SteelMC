use std::io::Cursor;
use std::sync::Arc;

use glam::DVec3;
use simdnbt::borrow::read_compound;
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_registry::vanilla_entities;
use steel_utils::{ChunkPos, UuidExt as _};

use crate::entity::{
    Entity as _, EntityReference, LivingEntity as _, Projectile, RemovalReason, SharedEntity,
    entities::{EnderPearlEntity, PigEntity},
};
use crate::server::worlds::WorldMap;
use crate::test_support::{TestPlayerBuilder, insert_ready_full_chunk, test_domain};
use crate::world::World;

fn pearl(world: &Arc<World>) -> EnderPearlEntity {
    EnderPearlEntity::new(
        &vanilla_entities::ENDER_PEARL,
        10,
        DVec3::ZERO,
        Arc::downgrade(world),
    )
}

#[test]
fn cached_dead_owner_takes_precedence_until_cache_expires() {
    let domain = test_domain("cached_owner", &["overworld", "the_nether"]);
    let worlds: Vec<_> = domain.values().collect();
    let owner: SharedEntity = TestPlayerBuilder::new(Arc::clone(worlds[0]), "Owner", 1).build();
    let pearl = pearl(worlds[1]);
    pearl.set_owner_entity(Some(&owner));
    insert_ready_full_chunk(worlds[1], ChunkPos::new(0, 0));
    let duplicate: SharedEntity = TestPlayerBuilder::new(Arc::clone(worlds[1]), "Duplicate", 2)
        .uuid(owner.uuid())
        .build();
    worlds[1]
        .try_add_entity(Arc::clone(&duplicate))
        .expect("register duplicate UUID");

    owner.as_player().expect("player").set_health(0.0);
    assert!(Arc::ptr_eq(
        &pearl.get_owner().expect("cached dead player"),
        &owner
    ));
    drop(owner);
    assert!(Arc::ptr_eq(
        &pearl.get_owner().expect("expired cache UUID fallback"),
        &duplicate
    ));
}

#[test]
fn owner_lookup_isolated_by_domain_for_uuid_and_cache() {
    let first = test_domain("owner_first", &["overworld", "the_nether"]);
    let second = test_domain("owner_second", &["overworld"]);
    let mut worlds = WorldMap::new("owner_first".to_owned(), &[], &[]);
    for world in first.values().chain(second.values()) {
        worlds.insert(world.key.clone(), Arc::clone(world));
    }
    let local_world = first.server_default_world().expect("local world");
    let foreign_world = second.server_default_world().expect("foreign world");
    insert_ready_full_chunk(foreign_world, ChunkPos::new(0, 0));
    let foreign: SharedEntity =
        TestPlayerBuilder::new(Arc::clone(foreign_world), "Foreign", 1).build();
    foreign_world
        .try_add_entity(Arc::clone(&foreign))
        .expect("foreign owner");
    let pearl = pearl(local_world);
    pearl.set_owner_uuid(Some(foreign.uuid()));
    assert!(pearl.get_owner().is_none());
    pearl.set_owner_entity(Some(&foreign));
    assert!(pearl.get_owner().is_none());

    insert_ready_full_chunk(local_world, ChunkPos::new(0, 0));
    let local: SharedEntity = TestPlayerBuilder::new(Arc::clone(local_world), "Local", 2)
        .uuid(foreign.uuid())
        .build();
    local_world
        .try_add_entity(Arc::clone(&local))
        .expect("local owner");
    assert!(Arc::ptr_eq(
        &pearl.get_owner().expect("same-domain fallback"),
        &local
    ));
}

#[test]
fn owner_cache_rejects_mismatched_uuid_and_deflection_updates_owner() {
    let domain = test_domain("cache_owner_uuid", &["overworld"]);
    let world = domain.server_default_world().expect("world");
    let owner: SharedEntity = TestPlayerBuilder::new(Arc::clone(world), "Owner", 1).build();
    let other: SharedEntity = TestPlayerBuilder::new(Arc::clone(world), "Other", 2).build();
    let pearl = pearl(world);
    pearl.set_owner_uuid(Some(owner.uuid()));
    pearl.cache_owner_entity(&other);
    assert!(pearl.get_owner().is_none());
    pearl.cache_owner_entity(&owner);
    pearl.deflect(
        super::ProjectileDeflection::None,
        None,
        Some(EntityReference::from_uuid(other.uuid())),
        false,
    );
    assert!(pearl.get_owner().is_none());
    pearl.cache_owner_entity(&other);
    assert!(Arc::ptr_eq(
        &pearl.get_owner().expect("deflected owner"),
        &other
    ));

    pearl.deflect(super::ProjectileDeflection::None, None, None, false);
    assert!(pearl.owner_uuid().is_none());
}

#[test]
fn removed_owner_is_not_resolved_from_world_index() {
    let domain = test_domain("replaced_owner", &["overworld", "the_nether"]);
    let worlds: Vec<_> = domain.values().collect();
    insert_ready_full_chunk(worlds[0], ChunkPos::new(0, 0));
    let original: SharedEntity =
        TestPlayerBuilder::new(Arc::clone(worlds[0]), "Original", 1).build();
    worlds[0]
        .try_add_entity(Arc::clone(&original))
        .expect("original owner");
    let pearl = pearl(worlds[1]);
    pearl.set_owner_entity(Some(&original));
    original.set_removed(RemovalReason::Killed);
    assert!(pearl.get_owner().is_none());
}

#[test]
fn uuid_owner_is_attributed_on_projectile_entity_hit() {
    let domain = test_domain("owner_attribution", &["overworld", "the_nether"]);
    let worlds: Vec<_> = domain.values().collect();
    for world in &worlds {
        insert_ready_full_chunk(world, ChunkPos::new(0, 0));
    }
    let owner: SharedEntity = TestPlayerBuilder::new(Arc::clone(worlds[0]), "Owner", 1).build();
    worlds[0]
        .try_add_entity(Arc::clone(&owner))
        .expect("register owner");
    let projectile = Arc::new(pearl(worlds[1]));
    projectile.set_owner_uuid(Some(owner.uuid()));
    let victim = Arc::new(PigEntity::new(
        &vanilla_entities::PIG,
        2,
        DVec3::ZERO,
        Arc::downgrade(worlds[1]),
    ));
    let target: SharedEntity = Arc::<PigEntity>::clone(&victim);
    worlds[1]
        .try_add_entity(Arc::clone(&target))
        .expect("register victim");
    Arc::clone(&projectile).on_hit_entity(&target, DVec3::ZERO);
    let source = victim.last_damage_source().expect("projectile hit source");
    assert!(Arc::ptr_eq(
        source.causing_entity().expect("cross-dimension shooter"),
        &owner
    ));
    assert_eq!(
        source
            .direct_entity()
            .expect("direct projectile")
            .generation(),
        projectile.generation()
    );
}

fn load_projectile(pearl: &EnderPearlEntity, nbt: &NbtCompound) {
    let mut bytes = Vec::new();
    nbt.write(&mut bytes);
    let borrowed = read_compound(&mut Cursor::new(&bytes)).expect("borrow test NBT");
    pearl.load_projectile((&borrowed).into());
}

#[test]
fn serialized_owner_resolves_by_uuid_and_loading_clears_old_cache() {
    let domain = test_domain("serialized_owner", &["overworld", "the_nether"]);
    let worlds: Vec<_> = domain.values().collect();
    insert_ready_full_chunk(worlds[0], ChunkPos::new(0, 0));
    let owner: SharedEntity = TestPlayerBuilder::new(Arc::clone(worlds[0]), "Owner", 1).build();
    worlds[0]
        .try_add_entity(Arc::clone(&owner))
        .expect("register owner");
    let original = pearl(worlds[1]);
    original.set_owner_entity(Some(&owner));
    let mut nbt = NbtCompound::new();
    original.save_projectile(&mut nbt);
    assert_eq!(
        nbt.int_array("Owner"),
        Some(owner.uuid().to_int_array().as_slice())
    );
    let loaded = pearl(worlds[1]);
    load_projectile(&loaded, &nbt);
    assert!(Arc::ptr_eq(
        &loaded.get_owner().expect("loaded UUID owner"),
        &owner
    ));

    for owner_tag in [
        None,
        Some(NbtTag::IntArray(vec![1, 2, 3])),
        Some(NbtTag::String("invalid".into())),
    ] {
        loaded.set_owner_entity(Some(&owner));
        let mut nbt = NbtCompound::new();
        if let Some(tag) = owner_tag {
            nbt.insert("Owner", tag);
        }
        load_projectile(&loaded, &nbt);
        assert!(loaded.get_owner().is_none());
    }
}
