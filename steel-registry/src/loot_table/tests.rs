use crate::blocks::{
    Block,
    properties::{BlockStateProperties, DoubleBlockHalf},
};
use crate::data_component_predicate::DataComponentMatchers;
use crate::data_components::vanilla_components::INSTRUMENT;
use crate::item_predicate::{
    BlockPredicate, StatePropertiesPredicate, StatePropertyMatcher, StatePropertyValueMatcher,
};
use crate::vanilla_instrument_tags::InstrumentTag;
use crate::vanilla_items;
use crate::{
    RegistryHolderSet, biome::BiomeRef, test_support::init_test_registry, vanilla_biomes,
    vanilla_blocks, vanilla_loot_tables,
};

use super::*;
use steel_utils::BlockPos;
use steel_utils::random::Random as _;
use steel_utils::random::legacy_random::LegacyRandom;

fn test_rng() -> LegacyRandom {
    LegacyRandom::from_seed(12_345)
}

static FILL_STONE_FUNCTIONS: [ConditionalLootFunction; 1] = [ConditionalLootFunction {
    function: LootFunction::SetCount {
        count: NumberProvider::Constant(70.0),
        add: false,
    },
    conditions: &[],
}];
static FILL_DIRT_FUNCTIONS: [ConditionalLootFunction; 1] = [ConditionalLootFunction {
    function: LootFunction::SetCount {
        count: NumberProvider::Constant(7.0),
        add: false,
    },
    conditions: &[],
}];
static FILL_STONE_ENTRIES: [LootEntry; 1] = [LootEntry::Item {
    name: Identifier::vanilla_static("stone"),
    weight: 1,
    quality: 0,
    conditions: &[],
    functions: &FILL_STONE_FUNCTIONS,
}];
static FILL_DIRT_ENTRIES: [LootEntry; 1] = [LootEntry::Item {
    name: Identifier::vanilla_static("dirt"),
    weight: 1,
    quality: 0,
    conditions: &[],
    functions: &FILL_DIRT_FUNCTIONS,
}];
static FILL_SWORD_ENTRIES: [LootEntry; 1] = [LootEntry::Item {
    name: Identifier::vanilla_static("iron_sword"),
    weight: 1,
    quality: 0,
    conditions: &[],
    functions: &[],
}];
static FILL_POOLS: [LootPool; 3] = [
    LootPool {
        rolls: NumberProvider::Constant(1.0),
        bonus_rolls: 0.0,
        entries: &FILL_STONE_ENTRIES,
        conditions: &[],
        functions: &[],
    },
    LootPool {
        rolls: NumberProvider::Constant(1.0),
        bonus_rolls: 0.0,
        entries: &FILL_DIRT_ENTRIES,
        conditions: &[],
        functions: &[],
    },
    LootPool {
        rolls: NumberProvider::Constant(1.0),
        bonus_rolls: 0.0,
        entries: &FILL_SWORD_ENTRIES,
        conditions: &[],
        functions: &[],
    },
];
static FILL_TABLE: LootTable = LootTable {
    key: Identifier::new_static("steel", "test/deterministic_fill"),
    loot_type: LootType::Chest,
    pools: &FILL_POOLS,
    functions: &[],
    random_sequence: None,
};

fn init_test_registries() {
    init_test_registry();
}

struct FixedBiomeLevel {
    biome: BiomeRef,
}

impl LootLevel for FixedBiomeLevel {
    fn biome_at(&self, _pos: BlockPos) -> Option<BiomeRef> {
        Some(self.biome)
    }

    fn block_state_at(&self, _pos: BlockPos) -> Option<BlockStateId> {
        None
    }
}

struct FixedBlockLevel {
    pos: BlockPos,
    state: Option<BlockStateId>,
}

impl LootLevel for FixedBlockLevel {
    fn biome_at(&self, _pos: BlockPos) -> Option<BiomeRef> {
        None
    }

    fn block_state_at(&self, pos: BlockPos) -> Option<BlockStateId> {
        (pos == self.pos).then_some(self.state).flatten()
    }
}

#[test]
fn number_provider_integer_conversion_matches_java_round() {
    let mut random = test_rng();

    assert_eq!(
        NumberProvider::Constant(0.5)
            .get_int(&mut random)
            .expect("constant provider should evaluate"),
        1
    );
    assert_eq!(
        NumberProvider::Constant(-0.5)
            .get_int(&mut random)
            .expect("constant provider should evaluate"),
        0
    );
    assert_eq!(
        NumberProvider::Uniform { min: 1.5, max: 1.5 }
            .get_int(&mut random)
            .expect("uniform provider should evaluate"),
        2
    );
}

#[test]
fn fill_matches_vanilla_split_and_shuffle_order() {
    init_test_registries();
    let mut items = vec![ItemStack::empty(); 9];
    items[4] = ItemStack::new(&vanilla_items::BARRIER);
    let mut random = LegacyRandom::from_seed(42);
    let mut context = LootContext::new(&mut random);

    FILL_TABLE
        .fill(&mut items, &mut context)
        .expect("test table should fill");

    let actual = items
        .iter()
        .map(|stack| (stack.item().key.path.as_ref(), stack.count()))
        .collect::<Vec<_>>();
    assert_eq!(
        actual,
        [
            ("stone", 35),
            ("stone", 6),
            ("dirt", 5),
            ("dirt", 1),
            ("barrier", 1),
            ("stone", 9),
            ("iron_sword", 1),
            ("stone", 20),
            ("dirt", 1),
        ]
    );
}

#[test]
fn fill_discards_overflow_without_replacing_occupied_slots() {
    init_test_registries();
    let mut items = vec![ItemStack::new(&vanilla_items::BARRIER), ItemStack::empty()];
    let mut random = LegacyRandom::from_seed(42);
    let mut context = LootContext::new(&mut random);

    FILL_TABLE
        .fill(&mut items, &mut context)
        .expect("test table should fill");

    assert!(items[0].is(&vanilla_items::BARRIER));
    assert!(!items[1].is_empty());
}

#[test]
fn test_oak_log_loot() {
    init_test_registries();
    let mut rng = test_rng();

    let mut ctx = LootContext::new(&mut rng);
    let items = vanilla_loot_tables::BLOCKS_OAK_LOG
        .get_random_items(&mut ctx)
        .expect("oak log loot should evaluate");

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].count, 1);
    assert_eq!(items[0].item.key, Identifier::vanilla_static("oak_log"));
}

#[test]
fn set_instrument_selects_from_the_configured_holder_set() {
    init_test_registries();
    let mut rng = test_rng();
    let mut ctx = LootContext::new(&mut rng);
    let mut goat_horn = ItemStack::new(&vanilla_items::GOAT_HORN);
    let function = LootFunction::SetInstrument {
        options: InstrumentOptions::Tag(InstrumentTag::REGULAR_GOAT_HORNS),
    };

    function
        .apply(&mut goat_horn, &mut ctx)
        .expect("instrument function should evaluate");

    let selected = goat_horn
        .get(INSTRUMENT)
        .and_then(|component| component.instrument().as_reference())
        .expect("set_instrument should select a registered instrument");
    assert!(
        REGISTRY
            .instruments
            .is_in_tag(selected, &InstrumentTag::REGULAR_GOAT_HORNS)
    );
}

#[test]
fn test_diamond_ore_loot_no_silk_touch() {
    // Without silk touch, diamond ore should drop diamond (not the ore block)
    init_test_registries();
    let mut rng = test_rng();

    let mut ctx = LootContext::new(&mut rng);
    let items = vanilla_loot_tables::BLOCKS_DIAMOND_ORE
        .get_random_items(&mut ctx)
        .expect("diamond ore loot should evaluate");

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].count, 1);
    // Without silk touch enchantment, diamond ore drops diamond
    assert_eq!(items[0].item.key, Identifier::vanilla_static("diamond"));
}

#[test]
fn test_grass_block_loot_no_silk_touch() {
    // Without silk touch, grass block should drop dirt
    init_test_registries();
    let mut rng = test_rng();

    let mut ctx = LootContext::new(&mut rng);
    let items = vanilla_loot_tables::BLOCKS_GRASS_BLOCK
        .get_random_items(&mut ctx)
        .expect("grass block loot should evaluate");

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].count, 1);
    // Without silk touch, grass block drops dirt
    assert_eq!(items[0].item.key, Identifier::vanilla_static("dirt"));
}

#[test]
fn test_stone_loot_no_silk_touch() {
    // Without silk touch, stone should drop cobblestone
    init_test_registries();
    let mut rng = test_rng();

    let mut ctx = LootContext::new(&mut rng);
    let items = vanilla_loot_tables::BLOCKS_STONE
        .get_random_items(&mut ctx)
        .expect("stone loot should evaluate");

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].count, 1);
    // Without silk touch, stone drops cobblestone
    assert_eq!(items[0].item.key, Identifier::vanilla_static("cobblestone"));
}

#[test]
fn test_pig_loot_drops_raw_porkchop_when_not_on_fire() {
    init_test_registries();
    let mut rng = test_rng();
    let pig_key = Identifier::vanilla_static("pig");
    let pig = EntityRef {
        entity_type: Some(&pig_key),
        flags: EntityRefFlags::default(),
        equipment: None,
        custom_name: None,
    };

    let mut ctx = LootContext::new(&mut rng).with_this_entity(pig);
    let items = vanilla_loot_tables::ENTITIES_PIG
        .get_random_items(&mut ctx)
        .expect("pig loot should evaluate");

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].item.key, Identifier::vanilla_static("porkchop"));
    assert!((1..=3).contains(&items[0].count));
}

#[test]
fn test_pig_loot_smelt_condition_uses_entity_fire_flag() {
    init_test_registries();
    let mut rng = test_rng();
    let pig_key = Identifier::vanilla_static("pig");
    let pig = EntityRef {
        entity_type: Some(&pig_key),
        flags: EntityRefFlags {
            is_on_fire: true,
            ..EntityRefFlags::default()
        },
        equipment: None,
        custom_name: None,
    };

    let mut ctx = LootContext::new(&mut rng).with_this_entity(pig);
    let items = vanilla_loot_tables::ENTITIES_PIG
        .get_random_items(&mut ctx)
        .expect("pig loot should evaluate");

    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0].item.key,
        Identifier::vanilla_static("cooked_porkchop")
    );
    assert!((1..=3).contains(&items[0].count));
}

#[test]
fn test_explosion_decay_function() {
    // Test the explosion_decay function directly
    init_test_registries();

    // explosion_decay reduces count based on 1/radius probability per item
    let cond_func = ConditionalLootFunction {
        function: LootFunction::ExplosionDecay,
        conditions: &[],
    };

    let mut total_survived = 0;
    let initial_count = 10;
    let mut rng = test_rng();

    for _ in 0..100 {
        let mut ctx = LootContext::new(&mut rng).with_explosion(4.0);
        let mut item = ItemStack::with_count(&crate::vanilla_items::STONE, initial_count);
        cond_func
            .function
            .apply(&mut item, &mut ctx)
            .expect("explosion decay should evaluate");
        total_survived += item.count;
    }

    // With 10 items each trial, 100 trials = 1000 items total
    // Each has 25% (1/4.0) chance to survive = ~250 expected
    // Allow for variance: 150-350 range
    assert!(
        total_survived > 150 && total_survived < 350,
        "Expected ~250 items with explosion decay (25% of 1000), got {total_survived}"
    );
}

#[test]
fn ominous_bottle_amplifier_function_clamps_to_persistent_range() {
    use crate::data_components::vanilla_components::OMINOUS_BOTTLE_AMPLIFIER;

    init_test_registries();
    for (provided, expected) in [(-3.0, 0), (2.0, 2), (9.0, 4)] {
        let mut rng = test_rng();
        let mut context = LootContext::new(&mut rng);
        let mut item = ItemStack::new(&crate::vanilla_items::OMINOUS_BOTTLE);
        LootFunction::SetOminousBottleAmplifier {
            amplifier: NumberProvider::Constant(provided),
        }
        .apply(&mut item, &mut context)
        .expect("ominous amplifier should evaluate");

        assert_eq!(
            item.get(OMINOUS_BOTTLE_AMPLIFIER)
                .map(|amplifier| amplifier.value()),
            Some(expected)
        );
    }
}

#[test]
fn test_survives_explosion_condition() {
    init_test_registries();

    // Test that survives_explosion condition works
    // Gravel has survives_explosion on its alternatives
    let mut survived = 0;
    let mut rng = test_rng();
    for _ in 0..100 {
        let mut ctx = LootContext::new(&mut rng).with_explosion(4.0);
        let items = vanilla_loot_tables::BLOCKS_GRAVEL
            .get_random_items(&mut ctx)
            .expect("gravel loot should evaluate");
        if !items.is_empty() {
            survived += 1;
        }
    }

    // With radius 4.0, ~25% should survive
    assert!(
        survived > 10 && survived < 50,
        "Expected ~25% survival rate, got {survived}%"
    );
}

#[test]
fn all_vanilla_chest_tables_have_a_supported_preflight() {
    init_test_registries();
    let level = FixedBiomeLevel {
        biome: &vanilla_biomes::PLAINS,
    };
    let mut chest_tables = 0;
    let mut map_tables = 0;

    for (id, table) in REGISTRY.loot_tables.iter() {
        if table.loot_type != LootType::Chest {
            continue;
        }
        chest_tables += 1;
        let requirements = table
            .requirements()
            .unwrap_or_else(|error| panic!("{} failed preflight: {error}", table.key));
        if !requirements.exploration_maps().is_empty() {
            map_tables += 1;
            continue;
        }
        for sample in 0..4_u64 {
            let mut rng = LegacyRandom::from_seed((id as u64).wrapping_mul(31) + sample);
            let mut context = LootContext::new(&mut rng)
                .with_origin(0.5, 64.5, 0.5)
                .with_level(&level);
            table
                .get_random_items(&mut context)
                .unwrap_or_else(|error| {
                    panic!("{} failed synchronous evaluation: {error}", table.key)
                });
        }
    }

    assert!(chest_tables > 0);
    assert!(map_tables > 0);
}

fn validate_generated_function_conditions(functions: &[ConditionalLootFunction]) -> LootResult<()> {
    for function in functions {
        super::requirements::validate_conditions(function.conditions)?;
        match &function.function {
            LootFunction::SetContents { entries, .. } => {
                for entry in *entries {
                    validate_generated_entry_conditions(entry)?;
                }
            }
            LootFunction::ModifyContents { modifier, .. }
            | LootFunction::Sequence {
                functions: modifier,
            } => validate_generated_function_conditions(modifier)?,
            LootFunction::Filtered { modifier, .. } => {
                validate_generated_function_conditions(std::slice::from_ref(*modifier))?;
            }
            LootFunction::SetCount { .. }
            | LootFunction::ExplosionDecay
            | LootFunction::ApplyBonus { .. }
            | LootFunction::EnchantedCountIncrease { .. }
            | LootFunction::LimitCount { .. }
            | LootFunction::SetDamage { .. }
            | LootFunction::EnchantRandomly { .. }
            | LootFunction::EnchantWithLevels { .. }
            | LootFunction::CopyComponents { .. }
            | LootFunction::CopyState { .. }
            | LootFunction::SetComponents { .. }
            | LootFunction::SetCustomData { .. }
            | LootFunction::FurnaceSmelt { .. }
            | LootFunction::ExplorationMap { .. }
            | LootFunction::SetName { .. }
            | LootFunction::SetOminousBottleAmplifier { .. }
            | LootFunction::SetPotion { .. }
            | LootFunction::SetStewEffect { .. }
            | LootFunction::SetInstrument { .. }
            | LootFunction::SetEnchantments { .. }
            | LootFunction::SetItem { .. }
            | LootFunction::CopyName { .. }
            | LootFunction::SetLore { .. }
            | LootFunction::SetLootTable { .. }
            | LootFunction::SetAttributes { .. }
            | LootFunction::FillPlayerHead { .. }
            | LootFunction::CopyCustomData { .. }
            | LootFunction::SetBannerPattern { .. }
            | LootFunction::SetFireworks { .. }
            | LootFunction::SetFireworkExplosion { .. }
            | LootFunction::SetBookCover { .. }
            | LootFunction::SetWrittenBookPages { .. }
            | LootFunction::SetWritableBookPages { .. }
            | LootFunction::ToggleTooltips { .. }
            | LootFunction::Discard
            | LootFunction::Reference(_) => {}
        }
    }
    Ok(())
}

fn validate_generated_entry_conditions(entry: &LootEntry) -> LootResult<()> {
    super::requirements::validate_conditions(entry.conditions())?;
    validate_generated_function_conditions(entry.functions())?;
    match entry {
        LootEntry::InlineLootTable { pools, .. } => {
            for pool in *pools {
                validate_generated_pool_conditions(pool)?;
            }
        }
        LootEntry::Alternatives { children, .. }
        | LootEntry::Group { children, .. }
        | LootEntry::Sequence { children, .. } => {
            for child in *children {
                validate_generated_entry_conditions(child)?;
            }
        }
        LootEntry::Item { .. }
        | LootEntry::LootTableRef { .. }
        | LootEntry::Tag { .. }
        | LootEntry::Empty { .. }
        | LootEntry::Dynamic { .. }
        | LootEntry::Slots { .. } => {}
    }
    Ok(())
}

fn validate_generated_pool_conditions(pool: &LootPool) -> LootResult<()> {
    super::requirements::validate_conditions(pool.conditions)?;
    validate_generated_function_conditions(pool.functions)?;
    for entry in pool.entries {
        validate_generated_entry_conditions(entry)?;
    }
    Ok(())
}

#[test]
fn all_generated_loot_conditions_have_supported_preflight() {
    init_test_registries();

    for (_, table) in REGISTRY.loot_tables.iter() {
        validate_generated_function_conditions(table.functions)
            .unwrap_or_else(|error| panic!("{} has unsupported conditions: {error}", table.key));
        for pool in table.pools {
            validate_generated_pool_conditions(pool).unwrap_or_else(|error| {
                panic!("{} has unsupported conditions: {error}", table.key)
            });
        }
    }
}

#[test]
fn exploration_map_defaults_match_vanilla_structure_tag_and_radius() {
    init_test_registries();

    let requirements = vanilla_loot_tables::CHESTS_SHIPWRECK_MAP
        .requirements()
        .expect("shipwreck map requirements should validate");
    let [request] = requirements.exploration_maps() else {
        panic!("shipwreck map should require exactly one exploration lookup");
    };

    assert_eq!(
        request.destination,
        Identifier::vanilla_static("on_treasure_maps")
    );
    assert_eq!(request.decoration, Identifier::vanilla_static("red_x"));
    assert_eq!(request.zoom, 1);
    assert_eq!(request.search_radius, 50);
    assert!(!request.skip_existing_chunks);
}

#[test]
fn abandoned_mineshaft_bounce_disc_requires_sulfur_caves() {
    init_test_registries();
    let condition = vanilla_loot_tables::CHESTS_ABANDONED_MINESHAFT
        .pools
        .iter()
        .flat_map(|pool| pool.entries)
        .find_map(|entry| match entry {
            LootEntry::Item {
                name, conditions, ..
            } if *name == Identifier::vanilla_static("music_disc_bounce") => conditions.first(),
            _ => None,
        })
        .expect("bounce disc entry should retain its location condition");

    for (biome, expected) in [
        (&*vanilla_biomes::PLAINS, false),
        (&*vanilla_biomes::SULFUR_CAVES, true),
    ] {
        let level = FixedBiomeLevel { biome };
        let mut random = test_rng();
        let mut context = LootContext::new(&mut random)
            .with_origin(4.5, 20.0, 8.5)
            .with_level(&level);
        assert_eq!(
            condition
                .test(&mut context)
                .expect("biome location condition should evaluate"),
            expected
        );
    }
}

#[test]
fn location_check_block_predicate_matches_loaded_offset_state() {
    static BLOCKS: [&Block; 1] = [&vanilla_blocks::TALL_GRASS];
    static UPPER_STATE: [StatePropertyMatcher; 1] = [StatePropertyMatcher::borrowed(
        "half",
        StatePropertyValueMatcher::borrowed_exact("upper"),
    )];

    init_test_registries();
    let target_pos = BlockPos::new(4, 21, 8);
    let upper = vanilla_blocks::TALL_GRASS.default_state().set_value(
        &BlockStateProperties::DOUBLE_BLOCK_HALF,
        DoubleBlockHalf::Upper,
    );
    let lower = vanilla_blocks::TALL_GRASS.default_state().set_value(
        &BlockStateProperties::DOUBLE_BLOCK_HALF,
        DoubleBlockHalf::Lower,
    );

    for blocks in [
        RegistryHolderSet::borrowed_direct(&BLOCKS),
        RegistryHolderSet::Tag(Identifier::vanilla_static("replaceable")),
    ] {
        let condition = LootCondition::LocationCheck {
            offset_x: 0,
            offset_y: 1,
            offset_z: 0,
            predicate: LocationPredicate {
                biomes: None,
                block: Some(BlockPredicate::new(
                    Some(blocks),
                    Some(StatePropertiesPredicate::borrowed(&UPPER_STATE)),
                    None,
                    DataComponentMatchers::ANY,
                )),
            },
        };
        for (state, expected) in [(Some(upper), true), (Some(lower), false), (None, false)] {
            let level = FixedBlockLevel {
                pos: target_pos,
                state,
            };
            let mut random = test_rng();
            let mut context = LootContext::new(&mut random)
                .with_origin(4.5, 20.5, 8.5)
                .with_level(&level);
            assert_eq!(
                condition
                    .test(&mut context)
                    .expect("block location condition should evaluate"),
                expected
            );
        }
    }
}

#[test]
fn generated_block_location_tables_have_supported_preflight() {
    init_test_registries();

    for table in [
        &vanilla_loot_tables::BLOCKS_TALL_GRASS,
        &vanilla_loot_tables::BLOCKS_LARGE_FERN,
    ] {
        table
            .requirements()
            .unwrap_or_else(|error| panic!("{} failed preflight: {error}", table.key));
    }
}

#[test]
fn failed_fill_does_not_publish_partial_slots() {
    static FUNCTIONS: [ConditionalLootFunction; 2] = [
        ConditionalLootFunction {
            function: LootFunction::SetCount {
                count: NumberProvider::Constant(12.0),
                add: false,
            },
            conditions: &[],
        },
        ConditionalLootFunction {
            function: LootFunction::CopyState {
                block: Identifier::vanilla_static("stone"),
                properties: &[],
            },
            conditions: &[],
        },
    ];
    static ENTRIES: [LootEntry; 1] = [LootEntry::Item {
        name: Identifier::vanilla_static("stone"),
        weight: 1,
        quality: 0,
        conditions: &[],
        functions: &FUNCTIONS,
    }];
    static POOLS: [LootPool; 1] = [LootPool {
        rolls: NumberProvider::Constant(1.0),
        bonus_rolls: 0.0,
        entries: &ENTRIES,
        conditions: &[],
        functions: &[],
    }];
    static TABLE: LootTable = LootTable {
        key: Identifier::new_static("steel", "test/failing_fill"),
        loot_type: LootType::Chest,
        pools: &POOLS,
        functions: &[],
        random_sequence: None,
    };

    init_test_registries();
    let original = vec![ItemStack::new(&vanilla_items::BARRIER), ItemStack::empty()];
    let mut slots = original.clone();
    let mut rng = test_rng();
    let mut context = LootContext::new(&mut rng);

    assert!(matches!(
        TABLE.fill(&mut slots, &mut context),
        Err(LootError::UnsupportedFunction("copy_state"))
    ));
    assert_eq!(slots, original);
}

#[test]
fn set_damage_uses_vanilla_remaining_durability_fraction() {
    init_test_registries();
    let mut sword = ItemStack::new(&vanilla_items::IRON_SWORD);
    let max_damage = sword.get_max_damage();
    let mut rng = test_rng();
    let mut context = LootContext::new(&mut rng);

    LootFunction::SetDamage {
        damage: NumberProvider::Constant(0.25),
        add: false,
    }
    .apply(&mut sword, &mut context)
    .expect("set_damage should evaluate");

    assert_eq!(
        sword.get_damage_value(),
        (max_damage as f32 * 0.75).floor() as i32
    );
}

#[test]
fn set_potion_preserves_existing_custom_contents() {
    use crate::RegistryReference;
    use crate::data_components::vanilla_components::{POTION_CONTENTS, PotionContents};

    init_test_registries();
    let mut potion = ItemStack::new(&vanilla_items::POTION);
    potion.set(
        POTION_CONTENTS,
        PotionContents::new(None, Some(0x123456), Vec::new(), Some("steel".to_owned())),
    );
    let mut rng = test_rng();
    let mut context = LootContext::new(&mut rng);
    LootFunction::SetPotion {
        id: Identifier::vanilla_static("healing"),
    }
    .apply(&mut potion, &mut context)
    .expect("set_potion should evaluate");

    let contents = potion
        .get(POTION_CONTENTS)
        .expect("potion contents should remain present");
    assert_eq!(
        contents.potion(),
        Some(RegistryReference::new(&crate::vanilla_potions::HEALING))
    );
    assert_eq!(contents.custom_color(), Some(0x123456));
    assert_eq!(contents.custom_name(), Some("steel"));
}

#[test]
fn set_stew_effect_scales_only_timed_effects() {
    use crate::data_components::vanilla_components::SUSPICIOUS_STEW_EFFECTS;

    static SATURATION: [StewEffect; 1] = [StewEffect {
        effect_type: Identifier::vanilla_static("saturation"),
        duration: NumberProvider::Constant(7.0),
    }];
    static NIGHT_VISION: [StewEffect; 1] = [StewEffect {
        effect_type: Identifier::vanilla_static("night_vision"),
        duration: NumberProvider::Constant(7.0),
    }];

    init_test_registries();
    for (effects, expected_duration) in [(&SATURATION[..], 7), (&NIGHT_VISION[..], 140)] {
        let mut stew = ItemStack::new(&vanilla_items::SUSPICIOUS_STEW);
        let mut rng = test_rng();
        let mut context = LootContext::new(&mut rng);
        LootFunction::SetStewEffect { effects }
            .apply(&mut stew, &mut context)
            .expect("set_stew_effect should evaluate");

        let applied = stew
            .get(SUSPICIOUS_STEW_EFFECTS)
            .expect("stew effect component should be present");
        assert_eq!(applied.effects().len(), 1);
        assert_eq!(applied.effects()[0].duration(), expected_duration);
    }
}

#[test]
fn set_name_uses_compiled_translation_component() {
    use crate::data_components::vanilla_components::CUSTOM_NAME;
    use text_components::content::Content;

    init_test_registries();
    let mut map = ItemStack::new(&vanilla_items::FILLED_MAP);
    let mut rng = test_rng();
    let mut context = LootContext::new(&mut rng);
    LootFunction::SetName {
        name: LootText::Translation(|| {
            steel_utils::translations::FILLED_MAP_BURIED_TREASURE
                .msg()
                .into()
        }),
        target: NameTarget::CustomName,
    }
    .apply(&mut map, &mut context)
    .expect("set_name should evaluate");

    let Some(Content::Translate(message)) = map.get(CUSTOM_NAME).map(|name| &name.content) else {
        panic!("set_name should produce a translated custom name");
    };
    assert_eq!(message.key.as_ref(), "filled_map.buried_treasure");
}

#[test]
fn enchant_randomly_consumes_vanilla_selection_and_level_draws() {
    static OPTIONS: [Identifier; 1] = [Identifier::vanilla_static("mending")];

    init_test_registries();
    let mut book = ItemStack::new(&vanilla_items::BOOK);
    let mut rng = LegacyRandom::from_seed(91);
    let mut expected_rng = LegacyRandom::from_seed(91);
    expected_rng.next_i32_bounded(1);
    expected_rng.next_i32_between(1, 1);
    let mut context = LootContext::new(&mut rng);

    LootFunction::EnchantRandomly {
        options: EnchantmentOptions::List(&OPTIONS),
    }
    .apply(&mut book, &mut context)
    .expect("enchant_randomly should evaluate");

    assert!(book.is(&vanilla_items::ENCHANTED_BOOK));
    assert_eq!(
        book.get_enchantments_for_crafting()
            .map(|enchantments| { enchantments.get_level(&Identifier::vanilla_static("mending")) }),
        Some(1)
    );
    assert_eq!(rng.next_i32(), expected_rng.next_i32());
}

#[test]
fn set_enchantments_transmutes_books_and_preserves_components() {
    use crate::data_components::vanilla_components::CUSTOM_NAME;
    use text_components::TextComponent;

    static ENCHANTMENTS: [(Identifier, NumberProvider); 1] = [(
        Identifier::vanilla_static("wind_burst"),
        NumberProvider::Constant(1.0),
    )];

    init_test_registries();
    let custom_name = TextComponent::from("Preserved book name");
    let mut book = ItemStack::new(&vanilla_items::BOOK);
    book.set(CUSTOM_NAME, custom_name.clone());
    let mut random = test_rng();
    let mut context = LootContext::new(&mut random);

    LootFunction::SetEnchantments {
        enchantments: &ENCHANTMENTS,
        add: false,
    }
    .apply(&mut book, &mut context)
    .expect("set_enchantments should evaluate");

    assert!(book.is(&vanilla_items::ENCHANTED_BOOK));
    assert_eq!(book.get(CUSTOM_NAME), Some(&custom_name));
    assert_eq!(
        book.get_enchantments_for_crafting().map(|enchantments| {
            enchantments.get_level(&Identifier::vanilla_static("wind_burst"))
        }),
        Some(1)
    );
}

#[test]
fn copy_components_without_its_optional_source_keeps_the_item() {
    static INCLUDE: [Identifier; 1] = [Identifier::vanilla_static("custom_name")];

    init_test_registries();
    let mut chest = ItemStack::new(&vanilla_items::CHEST);
    let mut random = test_rng();
    let mut context = LootContext::new(&mut random);

    LootFunction::CopyComponents {
        source: CopySource::BlockEntity,
        include: Some(&INCLUDE),
    }
    .apply(&mut chest, &mut context)
    .expect("an absent optional block-entity source should be a Vanilla no-op");

    assert!(chest.is(&vanilla_items::CHEST));
    assert_eq!(chest.count(), 1);
}

#[test]
fn copy_components_copies_only_included_block_entity_components() {
    use crate::data_components::DataComponentMap;
    use crate::data_components::vanilla_components::{CUSTOM_NAME, MAX_STACK_SIZE};
    use text_components::TextComponent;

    static INCLUDE: [Identifier; 1] = [Identifier::vanilla_static("custom_name")];

    init_test_registries();
    let custom_name = TextComponent::plain("Loot Chest");
    let mut components = DataComponentMap::new();
    components.set(CUSTOM_NAME, Some(custom_name.clone()));
    components.set(MAX_STACK_SIZE, Some(3));
    let block_entity = BlockEntityRef {
        block_entity_type: None,
        custom_name: None,
        inventory: None,
        components: Some(&components),
    };
    let mut random = test_rng();

    let mut chest = ItemStack::new(&vanilla_items::CHEST);
    let mut context = LootContext::new(&mut random).with_block_entity(block_entity);
    LootFunction::CopyComponents {
        source: CopySource::BlockEntity,
        include: Some(&INCLUDE),
    }
    .apply(&mut chest, &mut context)
    .expect("copy_components should evaluate");
    assert_eq!(chest.get(CUSTOM_NAME), Some(&custom_name));
    assert_eq!(chest.max_stack_size(), 64);

    let mut chest = ItemStack::new(&vanilla_items::CHEST);
    let mut context = LootContext::new(&mut random).with_block_entity(block_entity);
    LootFunction::CopyComponents {
        source: CopySource::BlockEntity,
        include: None,
    }
    .apply(&mut chest, &mut context)
    .expect("copy_components should evaluate");
    assert_eq!(chest.get(CUSTOM_NAME), Some(&custom_name));
    assert_eq!(chest.max_stack_size(), 3);
}

#[test]
fn enchant_with_levels_matches_vanilla_selection_order() {
    static OPTIONS: [Identifier; 2] = [
        Identifier::vanilla_static("sharpness"),
        Identifier::vanilla_static("smite"),
    ];

    init_test_registries();
    let mut sword = ItemStack::new(&vanilla_items::DIAMOND_SWORD);
    let mut rng = LegacyRandom::from_seed(42);
    let mut expected_rng = LegacyRandom::from_seed(42);
    expected_rng.next_i32_bounded(3);
    expected_rng.next_i32_bounded(3);
    expected_rng.next_f32();
    expected_rng.next_f32();
    expected_rng.next_i32_bounded(15);
    expected_rng.next_i32_bounded(50);
    let mut context = LootContext::new(&mut rng);

    LootFunction::EnchantWithLevels {
        levels: NumberProvider::Constant(30.0),
        options: EnchantmentOptions::List(&OPTIONS),
    }
    .apply(&mut sword, &mut context)
    .expect("enchant_with_levels should evaluate");

    assert_eq!(
        sword.get_enchantment_level(&Identifier::vanilla_static("sharpness")),
        3
    );
    assert_eq!(
        sword.get_enchantment_level(&Identifier::vanilla_static("smite")),
        0
    );
    assert_eq!(rng.next_i32(), expected_rng.next_i32());
}
