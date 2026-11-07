use super::*;

#[test]
fn pig_initializes_vanilla_living_attributes_and_health() {
    init_vanilla_registry();
    let pig = pig(1);

    assert_eq!(pig.living().health().to_bits(), 10.0_f32.to_bits());
    assert_eq!(
        pig.living()
            .attributes()
            .lock()
            .required_value(vanilla_attributes::MOVEMENT_SPEED)
            .to_bits(),
        0.25_f64.to_bits()
    );
    assert!(PigEntity::is_food(&ItemStack::new(&vanilla_items::CARROT)));
    assert!(!PigEntity::is_food(&ItemStack::new(&vanilla_items::WHEAT)));
}

#[test]
fn pig_sounds_switch_to_the_baby_set() {
    init_vanilla_registry();
    let pig = pig(1);
    pig.set_sound_variant(&vanilla_pig_sound_variants::MINI);
    let variant = pig.sound_variant();

    assert_eq!(
        pig.mob().ambient_sound().map(|sound| &sound.key),
        Some(&variant.adult_sounds.ambient_sound.key)
    );
    pig.animal().ageable().set_baby(true);
    assert_eq!(
        pig.mob().ambient_sound().map(|sound| &sound.key),
        Some(&variant.baby_sounds.ambient_sound.key)
    );
}

#[test]
fn saddle_slot_requires_an_alive_adult() {
    init_vanilla_registry();
    let pig = pig(1);
    let saddle = ItemStack::new(&vanilla_items::SADDLE);

    assert!(
        pig.living()
            .is_equippable_in_slot(&saddle, EquipmentSlot::Saddle)
    );
    assert!(pig.living().can_equip_with_dispenser(&saddle));

    pig.animal().ageable().set_baby(true);
    assert!(
        !pig.living()
            .is_equippable_in_slot(&saddle, EquipmentSlot::Saddle)
    );
}

#[test]
fn saddle_equips_with_the_pig_saddle_sound() {
    init_vanilla_registry();
    let pig = pig(1);
    let saddle = ItemStack::new(&vanilla_items::SADDLE);

    assert_eq!(
        pig.living()
            .equip_sound(EquipmentSlot::Saddle, &saddle)
            .map(|sound| &sound.key),
        Some(&sound_events::ENTITY_PIG_SADDLE.key)
    );
}

#[test]
fn pig_saves_and_loads_its_saddle_and_variants() {
    init_vanilla_registry();
    let pig = pig(1);
    saddle(&pig);
    pig.set_variant(&vanilla_pig_variants::COLD);
    pig.set_sound_variant(&vanilla_pig_sound_variants::BIG);

    let mut nbt = NbtCompound::new();
    pig.save_additional(&mut nbt);
    let mut bytes = Vec::new();
    nbt.write(&mut bytes);
    let borrowed = read_borrowed_compound(&mut Cursor::new(&bytes))
        .unwrap_or_else(|error| panic!("test nbt should reborrow: {error}"));

    let loaded = self::pig(2);
    loaded.load_additional((&borrowed).into());
    assert!(loaded.mob().is_saddled());
    assert_eq!(loaded.variant().key, vanilla_pig_variants::COLD.key);
    assert_eq!(
        loaded.sound_variant().key,
        vanilla_pig_sound_variants::BIG.key
    );
}

#[test]
fn pig_breeding_offspring_inherits_parent_variant() {
    init_globals();
    let world_fixture = fresh_test_world("pig_breed_offspring");
    let pig = pig(1);
    let partner = self::pig(2);
    pig.set_variant(&vanilla_pig_variants::WARM);
    partner.set_variant(&vanilla_pig_variants::COLD);

    let offspring = pig
        .animal()
        .ageable()
        .breed_offspring(&world_fixture.world, &partner.animal().ageable())
        .expect("pigs breed pigs");
    let offspring = offspring
        .downcast_ref::<PigEntity>()
        .expect("pig offspring is a pig");

    let key = &offspring.variant().key;
    assert!(key == &vanilla_pig_variants::WARM.key || key == &vanilla_pig_variants::COLD.key);
}
