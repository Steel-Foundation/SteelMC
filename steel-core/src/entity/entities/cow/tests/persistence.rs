use steel_registry::init_vanilla_registry;
use super::*;

#[test]
fn cow_saves_vanilla_age_and_variant_data() {
    init_vanilla_registry();

    let cow = CowEntity::new(&vanilla_entities::COW, 1, DVec3::ZERO, Weak::new());
    cow.set_age(-24_000);
    cow.set_forced_age(12);
    cow.set_age_locked(true);
    cow.set_variant(&vanilla_cow_variants::WARM);
    cow.set_sound_variant(&vanilla_cow_sound_variants::MOODY);

    let mut nbt = NbtCompound::new();
    cow.save_additional(&mut nbt);

    assert_eq!(nbt.int("Age"), Some(-24_000));
    assert_eq!(nbt.int("ForcedAge"), Some(12));
    assert_eq!(nbt.byte("AgeLocked"), Some(1));
    assert_eq!(
        nbt.string("variant").map(ToString::to_string),
        Some("minecraft:warm".to_owned())
    );
    assert_eq!(
        nbt.string("sound_variant").map(ToString::to_string),
        Some("minecraft:moody".to_owned())
    );
}

#[test]
fn cow_loads_vanilla_age_and_variant_data() {
    init_vanilla_registry();

    let mut nbt = NbtCompound::new();
    nbt.insert("Age", -24_000_i32);
    nbt.insert("ForcedAge", 12_i32);
    nbt.insert("AgeLocked", 1_i8);
    nbt.insert("variant", "minecraft:cold");
    nbt.insert("sound_variant", "minecraft:moody");

    let mut bytes = Vec::new();
    nbt.write(&mut bytes);
    let borrowed = read_borrowed_compound(&mut Cursor::new(&bytes))
        .unwrap_or_else(|error| panic!("test nbt should reborrow: {error}"));

    let cow = CowEntity::new(&vanilla_entities::COW, 1, DVec3::ZERO, Weak::new());
    cow.load_additional((&borrowed).into());

    assert_eq!(cow.get_age(), -24_000);
    assert_eq!(cow.forced_age(), 12);
    assert!(cow.is_age_locked());
    assert_eq!(cow.variant().key, vanilla_cow_variants::COLD.key);
    assert_eq!(
        cow.sound_variant().key,
        vanilla_cow_sound_variants::MOODY.key
    );
}
