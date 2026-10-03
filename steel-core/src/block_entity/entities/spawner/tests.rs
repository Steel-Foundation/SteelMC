use std::{
    convert::Infallible,
    io::Cursor,
    sync::{Arc, Weak},
};

use rand::TryRng;
use simdnbt::{
    borrow::read_compound,
    owned::{NbtList, NbtTag},
};
use steel_registry::{init_vanilla_registry, vanilla_blocks, vanilla_entities};
use steel_utils::WorldAabb;
use steel_utils::nbt::parse_snbt_compound;

use crate::entity::init_entities;
use crate::player::ResetReason;
use crate::test_support::{TestPlayerBuilder, fresh_test_world};

use super::base_spawner::choose_weighted;
use super::*;

#[derive(Debug)]
struct FixedRng(u64);

impl TryRng for FixedRng {
    type Error = Infallible;

    fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
        Ok(self.0 as u32)
    }

    fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
        Ok(self.0)
    }

    fn try_fill_bytes(&mut self, bytes: &mut [u8]) -> Result<(), Self::Error> {
        bytes.fill(self.0 as u8);
        Ok(())
    }
}

fn entity(id: &str) -> NbtCompound {
    let mut entity = NbtCompound::new();
    entity.insert("id", id);
    entity
}

fn spawn_data(id: &str) -> SpawnData {
    SpawnData {
        entity_to_spawn: entity(id),
        custom_spawn_rules: None,
        equipment: None,
    }
}

fn load_spawner(nbt: &NbtCompound) -> BaseSpawner {
    let mut bytes = Vec::new();
    nbt.write(&mut bytes);
    let borrowed =
        read_compound(&mut Cursor::new(bytes.as_slice())).expect("spawner NBT should be readable");
    let spawner = BaseSpawner::new();
    spawner.load(&borrowed);
    spawner
}

#[test]
fn custom_spawner_light_ranges_decode_vanilla_interval_forms() {
    let mut compound = NbtCompound::new();
    compound.insert("min_inclusive", 2_i8);
    compound.insert("max_inclusive", 7_i16);
    for (tag, expected) in [
        (NbtTag::Int(0), LightRange { min: 0, max: 0 }),
        (NbtTag::Byte(5), LightRange { min: 5, max: 5 }),
        (
            NbtTag::List(NbtList::Int(vec![2, 7])),
            LightRange { min: 2, max: 7 },
        ),
        (NbtTag::IntArray(vec![2, 7]), LightRange { min: 2, max: 7 }),
        (NbtTag::Compound(compound), LightRange { min: 2, max: 7 }),
    ] {
        for key in ["block_light_limit", "sky_light_limit"] {
            let mut root = NbtCompound::new();
            root.insert(key, tag.clone());
            let mut bytes = Vec::new();
            root.write(&mut bytes);
            let borrowed = read_compound(&mut Cursor::new(bytes.as_slice()))
                .expect("light range NBT should be readable");
            let rules = CustomSpawnRules::from_nbt(&(&borrowed).into())
                .expect("vanilla interval form should decode");
            let range = if key == "block_light_limit" {
                rules.block_light_limit
            } else {
                rules.sky_light_limit
            };
            assert_eq!(range, expected);
        }
    }
}

#[test]
fn custom_spawner_light_ranges_default_malformed_but_reject_out_of_bounds() {
    for (tag, expected) in [
        (
            NbtTag::List(NbtList::Int(vec![7, 2])),
            Some(LightRange::DEFAULT),
        ),
        (
            NbtTag::List(NbtList::Int(vec![2])),
            Some(LightRange::DEFAULT),
        ),
        (
            NbtTag::Compound(NbtCompound::new()),
            Some(LightRange::DEFAULT),
        ),
        (NbtTag::Int(16), None),
        (NbtTag::List(NbtList::Int(vec![-1, 7])), None),
    ] {
        let mut root = NbtCompound::new();
        root.insert("block_light_limit", tag);
        let mut bytes = Vec::new();
        root.write(&mut bytes);
        let borrowed = read_compound(&mut Cursor::new(bytes.as_slice()))
            .expect("light range NBT should be readable");
        let rules = CustomSpawnRules::from_nbt(&(&borrowed).into());
        assert_eq!(rules.map(|rules| rules.block_light_limit), expected);
    }
}

#[test]
fn unsupported_spawn_data_delays_and_reselects_without_inserting() {
    init_vanilla_registry();
    init_entities();

    let mut root = NbtCompound::new();
    root.insert("Delay", 0_i16);
    root.insert("MinSpawnDelay", 10_i32);
    root.insert("MaxSpawnDelay", 10_i32);
    root.insert("SpawnCount", 1_i32);

    root.insert("SpawnData", spawn_data("minecraft:blaze").to_nbt());

    let mut potential = NbtCompound::new();
    potential.insert("data", spawn_data("minecraft:pig").to_nbt());
    potential.insert("weight", 1_i32);
    root.insert("SpawnPotentials", NbtList::Compound(vec![potential]));

    let spawner = load_spawner(&root);

    let world = fresh_test_world("spawner_skips_unsupported_entity");
    let player = TestPlayerBuilder::new(Arc::clone(&world), "SpawnerPlayer", 1).build();
    assert!(world.add_player(player, ResetReason::InitialJoin));

    let result = spawner.server_tick(&world, BlockPos::ZERO);

    assert!(result.state_changed);
    assert!(result.next_spawn_data_changed);
    assert_eq!(spawner.spawn_delay(), 10);
    assert_eq!(
        spawner.get_or_create_next_spawn_data(),
        spawn_data("minecraft:pig")
    );
    assert!(
        !world.has_entity_in_aabb_matching(
            &WorldAabb::new(-4.0, -4.0, -4.0, 5.0, 5.0, 5.0),
            |entity| entity.as_player().is_none(),
        )
    );
}

#[test]
fn weighted_selection_uses_only_positive_weights_and_preserves_order() {
    let entries = [
        WeightedSpawnData {
            weight: -10,
            data: spawn_data("minecraft:pig"),
        },
        WeightedSpawnData {
            weight: 1,
            data: spawn_data("minecraft:zombie"),
        },
        WeightedSpawnData {
            weight: 3,
            data: spawn_data("minecraft:skeleton"),
        },
    ];

    for (random, expected) in [(0, "minecraft:zombie"), (1_u64 << 62, "minecraft:skeleton")] {
        assert_eq!(
            choose_weighted(&entries, &mut FixedRng(random)),
            Some(spawn_data(expected))
        );
    }
}

#[test]
fn nbt_round_trip_keeps_short_spawner_fields_and_spawn_data() {
    let root = parse_snbt_compound(
        r#"{
        Delay:37s, MinSpawnDelay:40, MaxSpawnDelay:120, SpawnCount:5s,
        MaxNearbyEntities:9, RequiredPlayerRange:20s, SpawnRange:6,
        SpawnData:{
            entity:{id:"minecraft:zombie"},
            custom_spawn_rules:{
                block_light_limit:{min_inclusive:2,max_inclusive:7},
                sky_light_limit:{min_inclusive:1,max_inclusive:5}
            },
            equipment:{loot_table:"minecraft:chests/simple_dungeon",slot_drop_chances:0.25f}
        },
        SpawnPotentials:[{data:{entity:{id:"minecraft:skeleton"}},weight:2}]
    }"#,
    )
    .expect("valid spawner fixture");
    let spawner = load_spawner(&root);
    let mut saved = NbtCompound::new();
    spawner.save(&mut saved);
    for (key, expected) in [
        ("Delay", 37),
        ("MinSpawnDelay", 40),
        ("MaxSpawnDelay", 120),
        ("SpawnCount", 5),
        ("MaxNearbyEntities", 9),
        ("RequiredPlayerRange", 20),
        ("SpawnRange", 6),
    ] {
        assert_eq!(saved.short(key), Some(expected), "{key}");
    }
    for key in ["SpawnData", "SpawnPotentials"] {
        assert_eq!(saved.get(key), root.get(key), "{key}");
    }
}

#[test]
fn update_tag_omits_spawn_potentials_for_client_sync() {
    init_vanilla_registry();
    let mut root = NbtCompound::new();
    root.insert("SpawnData", spawn_data("minecraft:zombie").to_nbt());
    root.insert(
        "SpawnPotentials",
        NbtList::Compound(vec![NbtCompound::new()]),
    );
    let mut bytes = Vec::new();
    root.write(&mut bytes);
    let borrowed =
        read_compound(&mut Cursor::new(bytes.as_slice())).expect("spawner NBT should be readable");

    let spawner = SpawnerBlockEntity::new(
        Weak::new(),
        BlockPos::ZERO,
        vanilla_blocks::SPAWNER.default_state(),
    );
    spawner.load_additional(&borrowed);
    let update_tag = spawner
        .get_update_tag()
        .expect("spawner should provide an update tag");

    assert!(update_tag.get("SpawnPotentials").is_none());
    assert!(update_tag.get("SpawnData").is_some());
}

#[test]
fn set_entity_id_normalizes_the_active_spawn_data() {
    init_vanilla_registry();
    let spawner = BaseSpawner::new();
    spawner.set_entity_id(&vanilla_entities::BLAZE);

    let mut saved = NbtCompound::new();
    spawner.save(&mut saved);
    assert_eq!(
        saved.compound("SpawnData"),
        Some(&spawn_data("minecraft:blaze").to_nbt())
    );
}

#[test]
fn client_spawn_event_resets_to_minimum_delay_only_for_event_one() {
    let mut root = NbtCompound::new();
    root.insert("Delay", 100_i16);
    root.insert("MinSpawnDelay", 33_i32);
    let spawner = load_spawner(&root);

    assert!(!spawner.on_client_event_triggered(2));
    assert_eq!(spawner.spawn_delay(), 100);
    assert!(spawner.on_client_event_triggered(1));
    assert_eq!(spawner.spawn_delay(), 33);
}
