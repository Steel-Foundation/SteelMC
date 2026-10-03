use super::*;
use steel_registry::init_vanilla_registry;

#[test]
fn chicken_saves_vanilla_state_and_variant_data() {
    init_vanilla_registry();

    let chicken = ChickenEntity::new(&vanilla_entities::CHICKEN, 1, DVec3::ZERO, Weak::new());
    chicken.set_age(-24_000);
    chicken.set_variant(&vanilla_chicken_variants::WARM);
    chicken.set_sound_variant(&vanilla_chicken_sound_variants::PICKY);
    chicken.set_chicken_jockey(true);
    chicken.set_egg_time(1234);

    let mut nbt = NbtCompound::new();
    chicken.save_additional(&mut nbt);

    assert_eq!(nbt.int("Age"), Some(-24_000));
    assert_eq!(nbt.byte("IsChickenJockey"), Some(1));
    assert_eq!(nbt.int("EggLayTime"), Some(1234));
    assert_eq!(
        nbt.string("variant").map(ToString::to_string),
        Some("minecraft:warm".to_owned())
    );
    assert_eq!(
        nbt.string("sound_variant").map(ToString::to_string),
        Some("minecraft:picky".to_owned())
    );
}

#[test]
fn chicken_loads_vanilla_state_and_variant_data() {
    init_vanilla_registry();

    let mut nbt = NbtCompound::new();
    nbt.insert("Age", -24_000_i32);
    nbt.insert("IsChickenJockey", 1_i8);
    nbt.insert("EggLayTime", 5678_i32);
    nbt.insert("variant", "minecraft:cold");
    nbt.insert("sound_variant", "minecraft:picky");

    let mut bytes = Vec::new();
    nbt.write(&mut bytes);
    let borrowed = read_borrowed_compound(&mut Cursor::new(&bytes))
        .unwrap_or_else(|error| panic!("test nbt should reborrow: {error}"));

    let chicken = ChickenEntity::new(&vanilla_entities::CHICKEN, 1, DVec3::ZERO, Weak::new());
    chicken.load_additional((&borrowed).into());

    assert_eq!(chicken.get_age(), -24_000);
    assert!(chicken.is_chicken_jockey());
    assert_eq!(chicken.egg_time(), 5678);
    assert_eq!(chicken.variant().key, vanilla_chicken_variants::COLD.key);
    assert_eq!(
        chicken.sound_variant().key,
        vanilla_chicken_sound_variants::PICKY.key
    );
}

#[test]
fn chicken_load_defaults_jockey_and_keeps_random_egg_time_when_absent() {
    init_vanilla_registry();

    let mut nbt = NbtCompound::new();
    nbt.insert("variant", "minecraft:temperate");
    nbt.insert("sound_variant", "minecraft:classic");

    let mut bytes = Vec::new();
    nbt.write(&mut bytes);
    let borrowed = read_borrowed_compound(&mut Cursor::new(&bytes))
        .unwrap_or_else(|error| panic!("test nbt should reborrow: {error}"));

    let chicken = ChickenEntity::new(&vanilla_entities::CHICKEN, 1, DVec3::ZERO, Weak::new());
    chicken.load_additional((&borrowed).into());

    assert!(!chicken.is_chicken_jockey());
    assert!(
        (EGG_LAY_MIN_DELAY_TICKS..EGG_LAY_MIN_DELAY_TICKS + EGG_LAY_RANDOM_RANGE_TICKS)
            .contains(&chicken.egg_time()),
        "constructor-initialized egg time should stay in the vanilla range"
    );
}
