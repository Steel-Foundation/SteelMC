use super::*;
use crate::loot::vanilla_loot_tables;
use std::env::temp_dir;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::{random::Random as _, saved_data::SavedDataManager};
use tokio::runtime::Builder;
use uuid::Uuid;

fn draw(sequences: &RandomSequences, key: &Identifier) -> i64 {
    sequences
        .with_random(key, |random| Ok(random.next_i64()))
        .expect("sequence draw")
}

#[test]
fn named_stream_matches_vanilla_seed_mixing_including_signed_salt_and_long_ids() {
    // Vectors from the local 26.3 RandomSequence/RandomSupport/Xoroshiro algorithms.
    let cases = [
        (
            12_345,
            0,
            true,
            true,
            "minecraft:chests/spawn_bonus_chest",
            [
                6_700_914_765_511_778_066,
                6_121_494_372_251_030_582,
                967_742_638_914_460_039,
                -1_480_460_906_689_149_994,
                1_191_408_393_472_339_827,
            ],
        ),
        (
            -12_345,
            -7,
            true,
            true,
            "example:a/sequence/name/longer/than/the/const/hash/single/block/limit",
            [
                -4_383_890_445_237_850_921,
                5_531_724_626_660_852_481,
                1_587_013_998_812_793_459,
                8_910_661_085_973_150_940,
                3_502_389_519_962_455_039,
            ],
        ),
        (
            12_345,
            -7,
            false,
            true,
            "minecraft:chests/spawn_bonus_chest",
            [
                2_809_111_903_727_117_879,
                -3_953_918_996_012_185_374,
                7_381_898_726_096_833_145,
                762_107_990_019_836_066,
                3_433_414_185_605_817_688,
            ],
        ),
        (
            12_345,
            -7,
            true,
            false,
            "minecraft:chests/spawn_bonus_chest",
            [
                4_976_822_084_399_020_243,
                8_336_963_870_357_203_337,
                2_279_520_072_693_895_720,
                942_728_389_794_975_228,
                -6_204_121_049_765_451_670,
            ],
        ),
    ];
    for (seed, salt, include_world_seed, include_sequence_id, key, expected) in cases {
        let sequences = RandomSequences::from_persistent(
            seed,
            PersistentRandomSequences {
                salt,
                include_world_seed,
                include_sequence_id,
                ..PersistentRandomSequences::default()
            },
        )
        .expect("valid settings");
        let key = key.parse().expect("valid key");
        assert_eq!(expected.map(|_| draw(&sequences, &key)), expected);
    }
}

#[tokio::test]
async fn saved_stream_resumes_and_later_draws_remain_dirty() {
    let dir = temp_dir().join(format!("steel-loot-sequences-{}", Uuid::new_v4()));
    let storage = SavedDataManager::new(Some(&dir));
    let sequences = RandomSequences::from_persistent(12_345, PersistentRandomSequences::default())
        .expect("settings");
    let key = Identifier::vanilla_static("chests/spawn_bonus_chest");
    for _ in 0..7 {
        draw(&sequences, &key);
    }
    let (revision, persistent) = sequences.pending_save().expect("snapshot").expect("dirty");
    storage
        .save(saved_data_names::RANDOM_SEQUENCES, &persistent)
        .await
        .expect("save");
    let expected = draw(&sequences, &key);
    sequences.saved_revision.store(revision, Ordering::Release);
    assert!(sequences.pending_save().expect("snapshot").is_some());
    let loaded = storage
        .load_or_default(saved_data_names::RANDOM_SEQUENCES)
        .await
        .expect("load");
    let restored = RandomSequences::from_persistent(12_345, loaded).expect("restore");
    assert_eq!(draw(&restored, &key), expected);
    // Drawing another named stream does not advance this one.
    draw(&restored, &Identifier::vanilla_static("another_sequence"));
    assert_eq!(draw(&restored, &key), draw(&sequences, &key));
}

#[test]
fn recursive_access_reports_an_error_without_waiting() {
    let sequences = RandomSequences::from_persistent(0, PersistentRandomSequences::default())
        .expect("settings");
    let key = Identifier::vanilla_static("test");
    sequences
        .with_random(&key, |_| {
            assert!(matches!(
                sequences.with_random(&key, |_| Ok(())),
                Err(LootError::RandomUnavailable(_))
            ));
            Ok(())
        })
        .expect("outer access");
    draw(&sequences, &key);
}

#[test]
fn zero_seed_barrels_share_the_named_stream_for_generation_and_placement() {
    use crate::{
        block_entity::{BlockEntity as _, RandomizableContainer as _, entities::BarrelBlockEntity},
        inventory::{
            container::{Container as _, SimpleContainer},
            lock::ContainerLockGuard,
        },
        loot::LootContext,
        test_support::test_domain,
    };
    use steel_registry::{
        data_components::vanilla_components::SeededContainerLoot, vanilla_blocks,
    };
    use steel_utils::BlockPos;

    let worlds = test_domain("barrel_sequences", &["overworld", "nether"]);
    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let _domains = DomainRandomSequences::load(&worlds)
            .await
            .expect("bind streams");
        let primary = worlds.default_world("barrel_sequences").expect("primary");
        let expected = RandomSequences::from_persistent(
            primary.level_data.read().seed(),
            PersistentRandomSequences::default(),
        )
        .expect("settings");
        let table = &vanilla_loot_tables::CHESTS_SPAWN_BONUS_CHEST;
        let sequence = table.random_sequence.as_ref().expect("named sequence");
        for world in [
            primary,
            worlds
                .get(&Identifier::new_static("barrel_sequences", "nether"))
                .expect("nether"),
        ] {
            let barrel = BarrelBlockEntity::new(
                Arc::downgrade(world),
                BlockPos::new(1, 64, 1),
                vanilla_blocks::BARREL.default_state(),
            );
            barrel.set_loot_table(Some(SeededContainerLoot::new(table.key.clone(), 0)));
            let mut expected_items = SimpleContainer::new(27);
            expected
                .with_random(sequence, |random| {
                    let mut context = LootContext::new(random);
                    table
                        .generate_for_container(&mut context)?
                        .fill(&mut expected_items);
                    Ok(())
                })
                .expect("expected fill");
            let storage = barrel.container_ref().expect("container");
            let guard = ContainerLockGuard::lock_all(&[&storage]).expect("lazy zero-seed fill");
            assert_eq!(
                guard
                    .get(storage.container_id())
                    .expect("locked storage")
                    .items(),
                expected_items.items()
            );
            drop(guard);
        }
        let actual = primary
            .random_sequences
            .read()
            .as_ref()
            .cloned()
            .expect("bound");
        assert_eq!(draw(&actual, sequence), draw(&expected, sequence));
    });
}

#[test]
fn domain_worlds_share_sequences_and_domains_stay_isolated() {
    use crate::server::worlds::WorldMap;
    use crate::{config::ResolvedDomainConfig, test_support::test_domain};

    let domain_a = test_domain("loot_a", &["overworld", "nether"]);
    let domain_b = test_domain("loot_b", &["overworld"]);
    let configs = [
        ResolvedDomainConfig {
            name: "loot_a".to_owned(),
            default_world: Identifier::new_static("loot_a", "overworld"),
            worlds: domain_a.keys().cloned().collect(),
        },
        ResolvedDomainConfig {
            name: "loot_b".to_owned(),
            default_world: Identifier::new_static("loot_b", "overworld"),
            worlds: domain_b.keys().cloned().collect(),
        },
    ];
    let mut worlds = WorldMap::new("loot_a".to_owned(), &configs, &[]);
    for world in domain_a.values().chain(domain_b.values()) {
        worlds.insert(world.key.clone(), Arc::clone(world));
    }
    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let domains = DomainRandomSequences::load(&worlds)
            .await
            .expect("load domains");
        let a = worlds.default_world("loot_a").expect("default a");
        let table = &vanilla_loot_tables::CHESTS_SPAWN_BONUS_CHEST;
        let mut expected_seeded = LegacyRandom::from_seed(42);
        assert_eq!(
            a.with_loot_random(table, 42, |random| Ok(random.next_i64()))
                .expect("seeded draw"),
            expected_seeded.next_i64()
        );
        assert_eq!(
            domains
                .save(&worlds)
                .await
                .expect("seeded draws do not dirty named streams"),
            0
        );
        let nether = worlds
            .get(&Identifier::new_static("loot_a", "nether"))
            .expect("nether");
        let a_sequences = a.random_sequences.read().as_ref().cloned().expect("bound");
        let n_sequences = nether
            .random_sequences
            .read()
            .as_ref()
            .cloned()
            .expect("bound");
        let b_sequences = worlds
            .default_world("loot_b")
            .expect("default b")
            .random_sequences
            .read()
            .as_ref()
            .cloned()
            .expect("bound");
        assert!(Arc::ptr_eq(&a_sequences, &n_sequences));
        assert!(!Arc::ptr_eq(&a_sequences, &b_sequences));
        let key = Identifier::vanilla_static("test");
        let expected = RandomSequences::from_persistent(
            a.level_data.read().seed(),
            PersistentRandomSequences::default(),
        )
        .expect("settings");
        assert_eq!(draw(&a_sequences, &key), draw(&expected, &key));
        assert_eq!(draw(&n_sequences, &key), draw(&expected, &key));
        assert_eq!(domains.save(&worlds).await.expect("save"), 1);
        assert_eq!(domains.save(&worlds).await.expect("clean save"), 0);
    });
}
