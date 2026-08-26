use std::io::Cursor;

use simdnbt::borrow::read_compound as read_borrowed_compound;
use steel_registry::{init_vanilla_registry, vanilla_attributes, vanilla_entities, vanilla_items};

use super::*;

fn new_fox() -> FoxEntity {
    FoxEntity::new(&vanilla_entities::FOX, 1, DVec3::ZERO, Weak::new())
}

#[test]
fn fox_starts_red_and_picks_up_loot() {
    init_vanilla_registry();

    let fox = new_fox();

    assert_eq!(fox.variant(), FoxVariant::Red);
    assert!(
        Mob::can_pick_up_loot(&fox),
        "vanilla foxes have canPickUpLoot enabled"
    );
    assert_eq!(fox.get_health().to_bits(), fox.get_max_health().to_bits());
    let attributes = fox.attributes().lock();
    assert_eq!(
        attributes
            .required_value(vanilla_attributes::MAX_HEALTH)
            .to_bits(),
        10.0_f64.to_bits()
    );
}

#[test]
fn fox_variant_round_trips() {
    init_vanilla_registry();

    let fox = new_fox();

    fox.set_variant(FoxVariant::Snow);
    assert_eq!(fox.variant(), FoxVariant::Snow);

    fox.set_variant(FoxVariant::Red);
    assert_eq!(fox.variant(), FoxVariant::Red);
}

#[test]
fn fox_flags_are_independent_bits() {
    init_vanilla_registry();

    let fox = new_fox();

    fox.set_sitting(true);
    fox.set_crouching(true);
    assert!(fox.is_sitting());
    assert!(fox.is_crouching());
    assert!(!fox.is_sleeping());

    // Clearing one flag must not disturb the others sharing the byte.
    fox.set_sitting(false);
    assert!(!fox.is_sitting());
    assert!(fox.is_crouching());
}

#[test]
fn fox_uses_vanilla_fox_food_tag() {
    init_vanilla_registry();

    assert!(FoxEntity::is_food(&ItemStack::new(
        &vanilla_items::SWEET_BERRIES
    )));
    assert!(!FoxEntity::is_food(&ItemStack::new(&vanilla_items::STONE)));
}

#[test]
fn fox_saves_and_loads_variant_and_state_flags() {
    init_vanilla_registry();

    let fox = new_fox();
    fox.set_variant(FoxVariant::Snow);
    fox.set_sleeping(true);
    fox.set_sitting(true);
    fox.set_crouching(true);

    let mut nbt = NbtCompound::new();
    fox.save_additional(&mut nbt);
    assert_eq!(nbt.byte("Sleeping"), Some(1));
    assert_eq!(nbt.byte("Sitting"), Some(1));
    assert_eq!(nbt.byte("Crouching"), Some(1));

    let mut bytes = Vec::new();
    nbt.write(&mut bytes);
    let borrowed = read_borrowed_compound(&mut Cursor::new(&bytes))
        .unwrap_or_else(|error| panic!("test nbt should reborrow: {error}"));

    let loaded = new_fox();
    loaded.load_additional((&borrowed).into());

    assert_eq!(loaded.variant(), FoxVariant::Snow);
    assert!(loaded.is_sleeping());
    assert!(loaded.is_sitting());
    assert!(loaded.is_crouching());
}

#[test]
fn fox_kit_inherits_a_parent_variant() {
    init_vanilla_registry();

    let parent = new_fox();
    let partner = new_fox();
    parent.set_variant(FoxVariant::Snow);
    partner.set_variant(FoxVariant::Snow);

    let offspring = new_fox();
    parent.initialize_breed_offspring(&partner, &offspring);

    // Both parents are snow foxes, so the random pick is snow either way.
    assert_eq!(offspring.variant(), FoxVariant::Snow);
}
