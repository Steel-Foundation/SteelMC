// Moved from entity/tests/portals.rs with the entities it depends on.

#[test]
fn dimension_changing_delay_uses_vanilla_class_overrides() {
    let base = TypedTestEntity::new(1, &vanilla_entities::ITEM);
    assert_eq!(base.dimension_changing_delay(), 300);

    let unimplemented_minecart = TypedTestEntity::new(2, &vanilla_entities::MINECART);
    assert_eq!(unimplemented_minecart.dimension_changing_delay(), 300);

    let minecart = ChestMinecartEntity::new(
        &vanilla_entities::CHEST_MINECART,
        3,
        DVec3::ZERO,
        Weak::new(),
    );
    assert_eq!(minecart.dimension_changing_delay(), 10);

    let arrow = TypedTestEntity::new(4, &vanilla_entities::ARROW);
    assert_eq!(arrow.dimension_changing_delay(), 2);
}
