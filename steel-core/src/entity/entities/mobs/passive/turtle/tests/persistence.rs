use super::*;

fn reborrow(nbt: &NbtCompound) -> Vec<u8> {
    let mut bytes = Vec::new();
    nbt.write(&mut bytes);
    bytes
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
