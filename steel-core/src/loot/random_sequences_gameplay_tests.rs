use super::*;

#[test]
fn mob_death_loot_continues_across_worlds_and_explicit_seeds_bypass_the_sequence() {
    use crate::{
        behavior::init_behaviors,
        entity::{
            LivingEntity as _, Mob as _, damage::DamageSource, entities::PigEntity,
            entity_loot_ref, living_loot_equipment, next_entity_id,
        },
        loot::{DamageSourceInfo, LootContext},
        test_support::{dropped_items, insert_ready_full_chunk, test_domain},
    };
    use glam::DVec3;
    use steel_registry::{vanilla_damage_types, vanilla_entities};
    use steel_utils::ChunkPos;

    steel_registry::init_vanilla_registry();
    init_behaviors();
    let worlds = test_domain("death_sequences", &["overworld", "nether"]);
    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let _domains = runtime
        .block_on(DomainRandomSequences::load(&worlds))
        .expect("streams");
    let primary = worlds.default_world("death_sequences").expect("primary");
    let nether = worlds
        .get(&Identifier::new_static("death_sequences", "nether"))
        .expect("nether");
    let expected = RandomSequences::new_for_test(primary.level_data.read().seed());
    let table = &vanilla_loot_tables::ENTITIES_PIG;
    let sequence = table.random_sequence.as_ref().expect("named sequence");
    let source = DamageSource::environment(&vanilla_damage_types::GENERIC);
    let position = DVec3::new(8.5, 64.0, 8.5);
    for world in [primary, nether] {
        insert_ready_full_chunk(world, ChunkPos::from_entity_pos(position));
        for seed in [42, 0, -17, 0, 0] {
            let pig = PigEntity::new(
                &vanilla_entities::PIG,
                next_entity_id(),
                position,
                Arc::downgrade(world),
            );
            pig.set_death_loot_table_seed(seed);
            let equipment = living_loot_equipment(&pig);
            let generate = |rng: &mut RandomSource| {
                let mut context = LootContext::new(rng)
                    .with_world(world)
                    .with_origin(position)
                    .with_this_entity(entity_loot_ref(&pig, Some(&equipment)))
                    .with_damage_source(DamageSourceInfo {
                        damage_type: Some(&source.damage_type.key),
                        tags: &[],
                        is_direct: source.is_direct(),
                    });
                table.get_random_items(&mut context)
            };
            let expected_items = if seed == 0 {
                expected.with_random(sequence, generate)
            } else {
                generate(&mut RandomSource::Legacy(LegacyRandom::from_seed(
                    seed as u64,
                )))
            }
            .expect("expected death loot");
            let before = dropped_items(world, position).len();
            pig.drop_from_loot_table(&source, false);
            assert_eq!(
                dropped_items(world, position).len() - before,
                expected_items.len()
            );
            let actual = bound(world);
            assert_eq!(
                draw(&actual, sequence),
                draw(&expected, sequence),
                "seed {seed}"
            );
        }
    }
}

#[test]
fn repeated_shearing_uses_the_outer_table_sequence_for_nested_color_tables() {
    use crate::{
        behavior::init_behaviors,
        entity::{entities::SheepEntity, entity_loot_ref, living_loot_equipment, next_entity_id},
        loot::LootContext,
        test_support::{dropped_items, fresh_test_world, insert_ready_full_chunk},
    };
    use glam::DVec3;
    use steel_registry::{DyeColor, item_stack::ItemStack, vanilla_entities, vanilla_items};
    use steel_utils::ChunkPos;

    steel_registry::init_vanilla_registry();
    init_behaviors();
    let fixture = fresh_test_world("shearing_sequences");
    let world = &fixture.world;
    let expected = RandomSequences::new_for_test(world.level_data.read().seed());
    let table = &vanilla_loot_tables::SHEARING_SHEEP;
    let sequence = table.random_sequence.as_ref().expect("named sequence");
    let position = DVec3::new(8.5, 64.0, 8.5);
    insert_ready_full_chunk(world, ChunkPos::from_entity_pos(position));
    let sheep = SheepEntity::new(
        &vanilla_entities::SHEEP,
        next_entity_id(),
        position,
        Arc::downgrade(world),
    );
    let tool = ItemStack::new(&vanilla_items::SHEARS);
    for color in [DyeColor::Red, DyeColor::Blue, DyeColor::White] {
        sheep.set_sheared(false);
        sheep.set_color(color);
        let equipment = living_loot_equipment(&sheep);
        let items = expected
            .with_random(sequence, |random| {
                let mut context = LootContext::new(random)
                    .with_world(world)
                    .with_origin(position)
                    .with_this_entity(entity_loot_ref(&sheep, Some(&equipment)))
                    .with_tool(&tool);
                table.get_random_items(&mut context)
            })
            .expect("expected shearing loot");
        let expected_count: i32 = items.iter().map(ItemStack::count).sum();
        let before = dropped_items(world, position).len();
        sheep.shear(world, &tool);
        assert!(sheep.is_sheared());
        assert_eq!(
            dropped_items(world, position).len() - before,
            expected_count as usize
        );
        let actual = bound(world);
        assert_eq!(draw(&actual, sequence), draw(&expected, sequence));
    }
}

#[test]
fn constant_mooshroom_and_egg_loot_still_register_their_named_streams() {
    use std::io::Cursor;

    use simdnbt::{borrow::read_compound, owned::NbtCompound};

    use crate::{
        behavior::init_behaviors,
        entity::{
            Entity as _, LivingEntity as _, SharedEntity,
            entities::{ChickenEntity, MushroomCowEntity},
            next_entity_id,
        },
        test_support::{dropped_items, fresh_test_world, insert_ready_full_chunk},
    };
    use glam::DVec3;
    use steel_registry::{item_stack::ItemStack, vanilla_entities, vanilla_items};
    use steel_utils::ChunkPos;

    steel_registry::init_vanilla_registry();
    init_behaviors();
    let fixture = fresh_test_world("constant_loot_sequences");
    let world = &fixture.world;
    let position = DVec3::new(8.5, 64.0, 8.5);
    insert_ready_full_chunk(world, ChunkPos::from_entity_pos(position));
    let mooshroom = MushroomCowEntity::new(
        &vanilla_entities::MOOSHROOM,
        next_entity_id(),
        position,
        Arc::downgrade(world),
    );
    mooshroom.shear(world, &ItemStack::new(&vanilla_items::SHEARS));
    assert_eq!(dropped_items(world, position).len(), 5);

    let chicken = Arc::new(ChickenEntity::new(
        &vanilla_entities::CHICKEN,
        next_entity_id(),
        position,
        Arc::downgrade(world),
    ));
    let mut nbt = NbtCompound::new();
    nbt.insert("EggLayTime", 1_i32);
    let mut bytes = Vec::new();
    nbt.write(&mut bytes);
    let borrowed = read_compound(&mut Cursor::new(&bytes)).expect("chicken data");
    chicken.load_additional((&borrowed).into());
    let entity: SharedEntity = Arc::clone(&chicken) as SharedEntity;
    chicken.ai_step(&entity);
    assert_eq!(dropped_items(world, position).len(), 6);
    let actual = bound(world);
    let (_, saved) = actual
        .pending_save()
        .expect("snapshot")
        .expect("loot used streams");
    let keys: Vec<_> = saved.sequences.keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        [
            "minecraft:gameplay/chicken_lay",
            "minecraft:shearing/mooshroom"
        ]
    );
}
