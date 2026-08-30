use super::*;

#[test]
fn sheep_saves_vanilla_wool_color_and_sheared_state() {
    init_vanilla_registry();

    let sheep = SheepEntity::new(&vanilla_entities::SHEEP, 1, DVec3::ZERO, Weak::new());
    sheep.set_color(DyeColor::Pink);
    sheep.set_sheared(true);

    let mut nbt = NbtCompound::new();
    sheep.save_additional(&mut nbt);

    assert_eq!(nbt.byte("Color"), Some(DyeColor::Pink.id() as i8));
    assert_eq!(nbt.byte("Sheared"), Some(1));
}

#[test]
fn sheep_loads_vanilla_wool_color_and_sheared_state() {
    init_vanilla_registry();

    let mut nbt = NbtCompound::new();
    nbt.insert("Color", DyeColor::Pink.id() as i8);
    nbt.insert("Sheared", 1_i8);

    let mut bytes = Vec::new();
    nbt.write(&mut bytes);
    let borrowed = read_borrowed_compound(&mut Cursor::new(&bytes))
        .unwrap_or_else(|error| panic!("test nbt should reborrow: {error}"));

    let sheep = SheepEntity::new(&vanilla_entities::SHEEP, 1, DVec3::ZERO, Weak::new());
    sheep.load_additional((&borrowed).into());

    assert_eq!(sheep.color(), DyeColor::Pink);
    assert!(sheep.is_sheared());
}

/// Vanilla reads these fields through `ValueInput.getNumericTag`, which accepts
/// any numeric tag type and converts it with `NumericTag.byteValue()`.
#[test]
fn sheep_loads_wool_color_and_sheared_state_from_any_numeric_tag() {
    init_vanilla_registry();

    let mut nbt = NbtCompound::new();
    nbt.insert("Color", DyeColor::Pink.id() as i16);
    // 0.7 floors to byte 0, so this must be read as "not sheared".
    nbt.insert("Sheared", 0.7_f64);

    let mut bytes = Vec::new();
    nbt.write(&mut bytes);
    let borrowed = read_borrowed_compound(&mut Cursor::new(&bytes))
        .unwrap_or_else(|error| panic!("test nbt should reborrow: {error}"));

    let sheep = SheepEntity::new(&vanilla_entities::SHEEP, 1, DVec3::ZERO, Weak::new());
    sheep.set_sheared(true);
    sheep.load_additional((&borrowed).into());

    assert_eq!(sheep.color(), DyeColor::Pink);
    assert!(!sheep.is_sheared());
}
