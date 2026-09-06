use super::*;

/// Round-trips an owned NBT compound back into a borrowed view for loading.
fn reborrow(nbt: &NbtCompound) -> Vec<u8> {
    let mut bytes = Vec::new();
    nbt.write(&mut bytes);
    bytes
}

#[test]
fn turtle_saves_home_pos_and_has_egg() {
    let turtle = detached_turtle();
    turtle.set_home_pos(BlockPos::new(12, 64, -8));
    turtle.set_has_egg(true);

    let mut nbt = NbtCompound::new();
    turtle.save_additional(&mut nbt);

    assert_eq!(
        nbt.int_array("home_pos").map(<[i32]>::to_vec),
        Some(vec![12, 64, -8])
    );
    assert_eq!(nbt.byte("has_egg"), Some(1));
}

#[test]
fn turtle_loads_home_pos_and_has_egg() {
    let mut nbt = NbtCompound::new();
    nbt.insert("home_pos", NbtTag::IntArray(vec![12, 64, -8]));
    nbt.insert("has_egg", true);
    let bytes = reborrow(&nbt);
    let borrowed = read_borrowed_compound(&mut Cursor::new(&bytes))
        .unwrap_or_else(|error| panic!("test nbt should reborrow: {error}"));

    let turtle = detached_turtle();
    turtle.load_additional((&borrowed).into());

    assert_eq!(turtle.home_pos(), BlockPos::new(12, 64, -8));
    assert!(turtle.has_egg());
}

#[test]
fn turtle_without_saved_home_defaults_to_its_block_position() {
    let nbt = NbtCompound::new();
    let bytes = reborrow(&nbt);
    let borrowed = read_borrowed_compound(&mut Cursor::new(&bytes))
        .unwrap_or_else(|error| panic!("test nbt should reborrow: {error}"));

    init_vanilla_registry();
    let turtle = TurtleEntity::new(
        &vanilla_entities::TURTLE,
        1,
        DVec3::new(5.0, 63.0, 9.0),
        Weak::new(),
    );
    turtle.load_additional((&borrowed).into());

    assert_eq!(turtle.home_pos(), turtle.block_position());
    assert!(!turtle.has_egg());
}
