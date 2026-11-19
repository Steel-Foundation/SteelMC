use futures::executor::block_on;
use glam::DVec3;
use steel_registry::{
    REGISTRY, RegistryEntry, RegistryExt, biome::BiomeRef, item_stack::ItemStack, vanilla_biomes,
};
use steel_utils::{
    BlockPos, ChunkPos,
    random::{Random, legacy_random::LegacyRandom},
};

use super::{vanilla::fixtures, *};
use crate::{
    bootstrap::init_globals,
    chunk::{chunk_holder::ChunkHolder, paletted_container::BiomePalette, status::ChunkStatus},
    test_support::{fresh_test_world, insert_ready_full_chunk},
    world::World,
};

fn fill_biome(holder: &ChunkHolder, biome: BiomeRef) {
    let id = u16::try_from(biome.try_id().expect("registered biome")).expect("biome id");
    for section in &holder
        .try_chunk(ChunkStatus::Biomes)
        .expect("biomes")
        .sections()
        .sections
    {
        section.write().biomes = BiomePalette::Homogeneous(id);
    }
}

fn evaluate(
    table: &LootTable,
    world: Option<&World>,
    origin: Option<DVec3>,
    resumable: bool,
) -> (OperationResult<Vec<ItemStack>>, i64) {
    let mut tables = LootTableBehaviorRegistry::new();
    fixtures::register(&mut tables);
    let mut random = LegacyRandom::from_seed(17);
    let mut context = LootContext::new(&mut random);
    context.tables = Some(&tables);
    context.world = world;
    context.origin = origin;
    let result = if resumable {
        block_on(table.get_random_items_async(&mut context))
    } else {
        table.get_random_items(&mut context)
    };
    (result, random.next_i64())
}

#[test]
fn camp_map_location_checks_exclude_only_the_current_biome_in_both_codegen_paths() {
    init_globals();
    let fixture = fresh_test_world("camp_map_location");
    let world = &fixture.world;
    let holder = insert_ready_full_chunk(world, ChunkPos::new(0, 0));
    let source: serde_json::Value = serde_json::from_str(include_str!(
        "../../../steel-utils/build_assets/builtin_datapacks/minecraft/loot_table/chests/abandoned_camp_common_chest.json"
    )).expect("extracted camp table");
    let biomes = source["pools"]
        .as_array()
        .expect("pools")
        .iter()
        .flat_map(|pool| pool["entries"].as_array().expect("entries"))
        .filter(|entry| entry["name"] == "minecraft:abandoned_camp_map")
        .map(|entry| {
            let key = entry["condition"]["term"]["predicate"]["biomes"]
                .as_str()
                .expect("camp biome")
                .parse()
                .expect("biome key");
            REGISTRY.biomes.by_key(&key).expect("registered camp biome")
        })
        .collect::<Vec<_>>();
    assert_ne!(biomes, [] as [BiomeRef; 0]);
    for (index, biome) in biomes.iter().enumerate() {
        fill_biome(&holder, biome);
        let mut previous_random = None;
        for resumable in [false, true] {
            let (items, random) = evaluate(
                &fixtures::LOCATION_CAMP_BIOMES,
                Some(world),
                Some(DVec3::new(8.5, 70.5, 8.5)),
                resumable,
            );
            let counts = items
                .expect("camp biome conditions")
                .iter()
                .map(ItemStack::count)
                .collect::<Vec<_>>();
            let expected = (1..=biomes.len())
                .filter(|count| *count != index + 1)
                .map(|count| count as i32)
                .collect::<Vec<_>>();
            assert_eq!(counts, expected, "biome={}", biome.key);
            if let Some(previous) = previous_random {
                assert_eq!(random, previous);
            }
            previous_random = Some(random);
        }
    }
}

#[test]
fn location_checks_require_origin_but_empty_predicates_do_not_require_loaded_chunks() {
    init_globals();
    let fixture = fresh_test_world("location_unloaded");
    let world = &fixture.world;
    let origin = Some(DVec3::new(8.5, 70.5, 8.5));
    for resumable in [false, true] {
        for table in [
            &fixtures::LOCATION_BIOME,
            &fixtures::LOCATION_NO_PREDICATE,
            &fixtures::LOCATION_EMPTY_PREDICATE,
            &fixtures::LOCATION_UNSUPPORTED,
        ] {
            assert_eq!(
                evaluate(table, None, None, resumable)
                    .0
                    .expect("absent origin"),
                []
            );
        }
        for table in [
            &fixtures::LOCATION_NO_PREDICATE,
            &fixtures::LOCATION_EMPTY_PREDICATE,
        ] {
            assert_eq!(
                evaluate(table, None, origin, resumable)
                    .0
                    .expect("empty predicate")
                    .len(),
                1
            );
        }
        assert_eq!(
            evaluate(&fixtures::LOCATION_BIOME, Some(world), origin, resumable)
                .0
                .expect("unloaded biome"),
            []
        );
        assert!(matches!(
            evaluate(&fixtures::LOCATION_BIOME, None, origin, resumable).0,
            Err(OperationError::Unsupported { .. })
        ));
        assert!(matches!(
            evaluate(
                &fixtures::LOCATION_UNSUPPORTED,
                Some(world),
                origin,
                resumable
            )
            .0,
            Err(OperationError::Unsupported { .. })
        ));
    }
    assert!(!world.chunk_map.has_chunk(ChunkPos::new(0, 0)));
}

#[test]
fn location_biome_holders_match_direct_references_tags_and_lists() {
    init_globals();
    let fixture = fresh_test_world("location_holders");
    let world = &fixture.world;
    let holder = insert_ready_full_chunk(world, ChunkPos::new(0, 0));
    for (biome, expected) in [
        (&*vanilla_biomes::BAMBOO_JUNGLE, [true, true, true, false]),
        (&*vanilla_biomes::CHERRY_GROVE, [false, false, true, false]),
        (&*vanilla_biomes::PLAINS, [false, false, false, false]),
    ] {
        fill_biome(&holder, biome);
        for (table, matches) in [
            &fixtures::LOCATION_BIOME,
            &fixtures::LOCATION_BIOME_TAG,
            &fixtures::LOCATION_BIOME_LIST,
            &fixtures::LOCATION_BIOME_EMPTY_LIST,
        ]
        .into_iter()
        .zip(expected)
        {
            for resumable in [false, true] {
                let items = evaluate(
                    table,
                    Some(world),
                    Some(DVec3::new(8.5, 70.5, 8.5)),
                    resumable,
                )
                .0
                .expect("biome holder match");
                assert_eq!(
                    !items.is_empty(),
                    matches,
                    "table={}, biome={}",
                    table.key,
                    biome.key
                );
            }
        }
    }
}

#[test]
fn location_offsets_floor_negative_coordinates_and_check_the_target_chunk_and_height() {
    init_globals();
    let fixture = fresh_test_world("location_offset");
    let world = &fixture.world;
    let origin = DVec3::new(-0.25, 69.5, -0.25);
    let holder = insert_ready_full_chunk(world, ChunkPos::new(-2, 0));
    fill_biome(&holder, &vanilla_biomes::BAMBOO_JUNGLE);
    for resumable in [false, true] {
        let items = evaluate(
            &fixtures::LOCATION_OFFSET,
            Some(world),
            Some(origin),
            resumable,
        )
        .0
        .expect("offset location");
        assert_eq!(items.len(), 1);
        assert_eq!(
            evaluate(
                &fixtures::LOCATION_BIOME,
                Some(world),
                Some(origin),
                resumable
            )
            .0
            .expect("unloaded unshifted origin"),
            []
        );
        let above = DVec3::new(origin.x, f64::from(world.get_max_y()), origin.z);
        assert_eq!(
            evaluate(
                &fixtures::LOCATION_OFFSET,
                Some(world),
                Some(above),
                resumable
            )
            .0
            .expect("outside build height"),
            []
        );
    }
    assert!(
        !world
            .chunk_map
            .has_chunk(ChunkPos::from_block_pos(BlockPos::containing(
                origin.x, origin.y, origin.z
            )))
    );
}

#[test]
fn location_biome_fuzzing_reads_neighbor_biome_chunks_without_loading_them_to_full() {
    use crate::{
        chunk::chunk_ticket_manager::ticket_level_for_status,
        worldgen::generator::vanilla::fuzzed_biome_quart_position,
    };
    use steel_worldgen::biomes::obfuscate_biome_seed;

    init_globals();
    let fixture = fresh_test_world("location_fuzzed_neighbor");
    let world = &fixture.world;
    let origin_chunk = ChunkPos::new(0, 0);
    let holder = insert_ready_full_chunk(world, origin_chunk);
    fill_biome(&holder, &vanilla_biomes::PLAINS);
    let seed = obfuscate_biome_seed(world.seed());
    let pos = (0..16)
        .flat_map(|x| (0..16).map(move |z| BlockPos::new(x, 70, z)))
        .find(|pos| {
            let quart = fuzzed_biome_quart_position(seed, *pos);
            ChunkPos::new(quart.x >> 2, quart.z >> 2) != origin_chunk
        })
        .expect("fuzzed sample crosses chunk boundary");
    let quart = fuzzed_biome_quart_position(seed, pos);
    let neighbor_pos = ChunkPos::new(quart.x >> 2, quart.z >> 2);
    let neighbor = insert_ready_full_chunk(world, neighbor_pos);
    fill_biome(&neighbor, &vanilla_biomes::BAMBOO_JUNGLE);
    let level = ticket_level_for_status(ChunkStatus::Biomes);
    neighbor.swap_load_level(level);
    neighbor.update_highest_allowed_status(Some(level));
    assert!(!world.chunk_map.has_chunk(neighbor_pos));
    for resumable in [false, true] {
        let origin = DVec3::new(f64::from(pos.x()), f64::from(pos.y()), f64::from(pos.z()));
        assert_eq!(
            evaluate(
                &fixtures::LOCATION_BIOME,
                Some(world),
                Some(origin),
                resumable
            )
            .0
            .expect("stored neighboring biome")
            .len(),
            1
        );
        assert!(!world.chunk_map.has_chunk(neighbor_pos));
    }
}
