use super::*;

#[test]
fn pig_saves_vanilla_mob_age_and_variant_data() {
    init_test_registry();

    let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
    pig.set_can_pick_up_loot(true);
    pig.set_persistence_required();
    pig.set_guaranteed_drop(EquipmentSlot::Saddle);
    pig.set_home_to(BlockPos::new(11, 64, -3), 7);
    pig.set_death_loot_table(Some(Identifier::vanilla_static("entities/pig")));
    pig.set_death_loot_table_seed(1234);
    let leash_holder: SharedEntity = Arc::new(PigEntity::new(
        &vanilla_entities::PIG,
        2,
        DVec3::ZERO,
        Weak::new(),
    ));
    assert!(pig.set_leashed_to(&leash_holder));
    pig.set_no_ai(true);
    pig.set_left_handed(true);
    pig.set_age(-24_000);
    pig.set_forced_age(12);
    pig.set_age_locked(true);
    pig.set_variant(&vanilla_pig_variants::WARM);
    pig.set_sound_variant(&vanilla_pig_sound_variants::BIG);

    let mut nbt = NbtCompound::new();
    pig.save_additional(&mut nbt);

    assert_eq!(nbt.byte("CanPickUpLoot"), Some(1));
    assert_eq!(nbt.byte("PersistenceRequired"), Some(1));
    let Some(drop_chances) = nbt.compound("drop_chances") else {
        panic!("non-default mob drop chances should be saved");
    };
    assert_eq!(drop_chances.float("saddle"), Some(2.0));
    assert_eq!(drop_chances.float("head"), None);
    assert_eq!(nbt.int("home_radius"), Some(7));
    assert_eq!(
        nbt.int_array("home_pos").map(<[i32]>::to_vec),
        Some(vec![11, 64, -3])
    );
    assert_eq!(
        nbt.string("DeathLootTable").map(ToString::to_string),
        Some("minecraft:entities/pig".to_owned())
    );
    assert_eq!(nbt.long("DeathLootTableSeed"), Some(1234));
    let Some(leash) = nbt.compound("leash") else {
        panic!("live leash holder should save as a UUID compound");
    };
    assert_eq!(
        leash.int_array("UUID").map(<[i32]>::to_vec),
        Some(leash_holder.uuid().to_int_array().to_vec())
    );
    assert_eq!(nbt.byte("NoAI"), Some(1));
    assert_eq!(nbt.byte("LeftHanded"), Some(1));
    assert_eq!(nbt.int("Age"), Some(-24_000));
    assert_eq!(nbt.int("ForcedAge"), Some(12));
    assert_eq!(nbt.byte("AgeLocked"), Some(1));
    assert_eq!(
        nbt.string("variant").map(ToString::to_string),
        Some("minecraft:warm".to_owned())
    );
    assert_eq!(
        nbt.string("sound_variant").map(ToString::to_string),
        Some("minecraft:big".to_owned())
    );
}

#[test]
fn pig_loads_vanilla_mob_age_and_variant_data() {
    init_test_registry();

    let mut nbt = NbtCompound::new();
    nbt.insert("CanPickUpLoot", 1_i8);
    nbt.insert("PersistenceRequired", 1_i8);
    let mut drop_chances = NbtCompound::new();
    drop_chances.insert("saddle", 2.0_f32);
    nbt.insert("drop_chances", NbtTag::Compound(drop_chances));
    nbt.insert("home_radius", 7_i32);
    nbt.insert("home_pos", NbtTag::IntArray(vec![11, 64, -3]));
    nbt.insert("DeathLootTable", "minecraft:entities/pig");
    nbt.insert("DeathLootTableSeed", 1234_i64);
    let leash_uuid = Uuid::from_u128(43);
    let mut leash = NbtCompound::new();
    leash.insert("UUID", NbtTag::IntArray(leash_uuid.to_int_array().to_vec()));
    nbt.insert("leash", NbtTag::Compound(leash));
    nbt.insert("NoAI", 1_i8);
    nbt.insert("LeftHanded", 1_i8);
    nbt.insert("Age", -24_000_i32);
    nbt.insert("ForcedAge", 12_i32);
    nbt.insert("AgeLocked", 1_i8);
    nbt.insert("variant", "minecraft:cold");
    nbt.insert("sound_variant", "minecraft:mini");

    let mut bytes = Vec::new();
    nbt.write(&mut bytes);
    let borrowed = read_borrowed_compound(&mut Cursor::new(&bytes))
        .unwrap_or_else(|error| panic!("test nbt should reborrow: {error}"));

    let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
    pig.load_additional((&borrowed).into());

    assert!(pig.can_pick_up_loot());
    assert!(pig.is_persistence_required());
    assert_eq!(
        pig.equipment_drop_chance(EquipmentSlot::Saddle).to_bits(),
        2.0_f32.to_bits()
    );
    assert_eq!(
        pig.equipment_drop_chance(EquipmentSlot::Head).to_bits(),
        0.085_f32.to_bits()
    );
    assert!(pig.has_home());
    assert_eq!(pig.home_radius(), 7);
    assert_eq!(pig.home_position(), BlockPos::new(11, 64, -3));
    let mut saved = NbtCompound::new();
    pig.save_additional(&mut saved);
    assert_eq!(
        saved.string("DeathLootTable").map(ToString::to_string),
        Some("minecraft:entities/pig".to_owned())
    );
    assert_eq!(saved.long("DeathLootTableSeed"), Some(1234));
    assert!(pig.may_be_leashed());
    assert!(!pig.is_leashed());
    assert_eq!(
        pig.leash_attachment(),
        Some(LeashAttachment::Entity(leash_uuid))
    );
    assert!(pig.is_no_ai());
    assert!(pig.is_left_handed());
    assert_eq!(pig.get_age(), -24_000);
    assert_eq!(pig.forced_age(), 12);
    assert!(pig.is_age_locked());
    assert_eq!(pig.variant().key, vanilla_pig_variants::COLD.key);
    assert_eq!(
        pig.sound_variant().key,
        vanilla_pig_sound_variants::MINI.key
    );
}

#[test]
fn pig_saves_delayed_fence_knot_leash_as_vanilla_block_pos_int_array() {
    init_test_registry();

    let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
    pig.set_delayed_leash_attachment(LeashAttachment::FenceKnot(BlockPos::new(4, 65, -9)));

    let mut nbt = NbtCompound::new();
    pig.save_additional(&mut nbt);

    assert_eq!(
        nbt.int_array("leash").map(<[i32]>::to_vec),
        Some(vec![4, 65, -9])
    );
}

#[test]
fn pig_saves_live_fence_knot_leash_as_vanilla_block_pos_int_array() {
    init_test_registry();

    let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
    let knot: SharedEntity = Arc::new(LeashFenceKnotEntity::new_attached(
        &vanilla_entities::LEASH_KNOT,
        2,
        BlockPos::new(4, 65, -9),
        Weak::new(),
    ));
    assert!(pig.set_leashed_to(&knot));

    let mut nbt = NbtCompound::new();
    pig.save_additional(&mut nbt);

    assert_eq!(
        nbt.int_array("leash").map(<[i32]>::to_vec),
        Some(vec![4, 65, -9])
    );
}

#[test]
fn pig_loads_delayed_fence_knot_leash_from_vanilla_block_pos_int_array() {
    init_test_registry();

    let mut nbt = NbtCompound::new();
    nbt.insert("leash", NbtTag::IntArray(vec![4, 65, -9]));

    let mut bytes = Vec::new();
    nbt.write(&mut bytes);
    let borrowed = read_borrowed_compound(&mut Cursor::new(&bytes))
        .unwrap_or_else(|error| panic!("test nbt should reborrow: {error}"));

    let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
    pig.load_additional((&borrowed).into());

    assert!(pig.may_be_leashed());
    assert!(!pig.is_leashed());
    assert_eq!(
        pig.leash_attachment(),
        Some(LeashAttachment::FenceKnot(BlockPos::new(4, 65, -9)))
    );
}

#[test]
fn pig_drop_leash_clears_live_leash_state() {
    init_test_registry();

    let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
    let holder: SharedEntity = Arc::new(PigEntity::new(
        &vanilla_entities::PIG,
        2,
        DVec3::ZERO,
        Weak::new(),
    ));
    assert!(pig.set_leashed_to(&holder));

    pig.drop_leash();

    assert!(!pig.is_leashed());
    assert!(!pig.may_be_leashed());
}

#[test]
fn pig_remove_leash_clears_live_leash_state() {
    init_test_registry();

    let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
    let holder: SharedEntity = Arc::new(PigEntity::new(
        &vanilla_entities::PIG,
        2,
        DVec3::ZERO,
        Weak::new(),
    ));
    assert!(pig.set_leashed_to(&holder));

    pig.remove_leash();

    assert!(!pig.is_leashed());
    assert!(!pig.may_be_leashed());
}

#[test]
fn pig_drop_all_leash_connections_clears_own_live_leash() {
    init_test_registry();

    let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
    let holder: SharedEntity = Arc::new(PigEntity::new(
        &vanilla_entities::PIG,
        2,
        DVec3::ZERO,
        Weak::new(),
    ));
    assert!(pig.set_leashed_to(&holder));

    assert!(pig.drop_all_leash_connections(None));

    assert!(!pig.is_leashed());
    assert!(!pig.may_be_leashed());
}
