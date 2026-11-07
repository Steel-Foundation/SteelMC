use super::*;
use crate::bootstrap::init_globals;
use steel_registry::init_vanilla_registry;

fn cow(id: i32) -> CowEntity {
    CowEntity::new(&vanilla_entities::COW, id, DVec3::ZERO, Weak::new())
}

#[test]
fn cow_initializes_vanilla_living_attributes_and_health() {
    init_vanilla_registry();
    let cow = cow(1);

    assert_eq!(cow.living().health().to_bits(), 10.0_f32.to_bits());
    let attributes = cow.living().attributes().lock();
    assert_eq!(
        attributes
            .required_value(vanilla_attributes::MAX_HEALTH)
            .to_bits(),
        10.0_f64.to_bits()
    );
    assert!(
        (attributes.required_value(vanilla_attributes::MOVEMENT_SPEED) - f64::from(0.2_f32)).abs()
            < 1e-12
    );
}

#[test]
fn cow_uses_vanilla_cow_food_tag() {
    init_vanilla_registry();

    assert!(abstract_cow::is_food(&ItemStack::new(
        &vanilla_items::WHEAT
    )));
    assert!(!abstract_cow::is_food(&ItemStack::new(
        &vanilla_items::STONE
    )));
}

#[test]
fn cow_sounds_follow_the_sound_variant_at_cow_volume() {
    init_vanilla_registry();
    let cow = cow(1);
    let source = DamageSource::environment(&vanilla_damage_types::GENERIC);

    assert_eq!(cow.living().sound_volume().to_bits(), 0.4_f32.to_bits());
    assert_eq!(
        cow.mob().ambient_sound().map(|sound| &sound.key),
        Some(&sound_events::ENTITY_COW_AMBIENT.key)
    );

    cow.set_sound_variant(&vanilla_cow_sound_variants::MOODY);
    assert_eq!(
        cow.mob().ambient_sound().map(|sound| &sound.key),
        Some(&cow.sound_variant().ambient_sound.key)
    );
    assert_eq!(
        cow.living().hurt_sound(&source).map(|sound| &sound.key),
        Some(&cow.sound_variant().hurt_sound.key)
    );
}

#[test]
fn adult_cow_fills_a_bucket_with_milk() {
    init_globals();
    let world_fixture = fresh_test_world("cow_milking");
    let player = TestPlayerBuilder::new(Arc::clone(&world_fixture.world), "Milker", 10).build();
    player
        .inventory
        .lock()
        .set_selected_item(ItemStack::new(&vanilla_items::BUCKET));
    let cow = cow(1);

    assert_eq!(
        cow.mob().mob_interact(&player, InteractionHand::MainHand),
        InteractionResult::Success
    );
    assert!(
        player
            .inventory
            .lock()
            .get_selected_item()
            .is(&vanilla_items::MILK_BUCKET)
    );
}

#[test]
fn baby_cow_cannot_be_milked() {
    init_globals();
    let world_fixture = fresh_test_world("baby_cow_milking");
    let player = TestPlayerBuilder::new(Arc::clone(&world_fixture.world), "Milker", 11).build();
    player
        .inventory
        .lock()
        .set_selected_item(ItemStack::new(&vanilla_items::BUCKET));
    let cow = cow(1);
    cow.animal().ageable().set_baby(true);

    assert_eq!(
        cow.mob().mob_interact(&player, InteractionHand::MainHand),
        InteractionResult::Pass
    );
    assert!(
        player
            .inventory
            .lock()
            .get_selected_item()
            .is(&vanilla_items::BUCKET)
    );
}

#[test]
fn cow_breeding_offspring_inherits_parent_variant() {
    init_globals();
    let world_fixture = fresh_test_world("cow_breed_offspring");
    let world = &world_fixture.world;
    let cow = cow(1);
    let partner = self::cow(2);
    cow.set_variant(&vanilla_cow_variants::WARM);
    partner.set_variant(&vanilla_cow_variants::COLD);

    let offspring = cow
        .animal()
        .ageable()
        .breed_offspring(world, &partner.animal().ageable())
        .expect("cows breed cows");
    let offspring = offspring
        .downcast_ref::<CowEntity>()
        .expect("cow offspring is a cow");

    let variant_key = &offspring.variant().key;
    assert!(
        variant_key == &vanilla_cow_variants::WARM.key
            || variant_key == &vanilla_cow_variants::COLD.key
    );
}

#[test]
fn cow_finalize_spawn_assigns_registered_variant_and_sound_variant() {
    init_vanilla_registry();
    let world_fixture = fresh_test_world("cow_finalize_spawn");
    let world = &world_fixture.world;
    let cow = CowEntity::new(
        &vanilla_entities::COW,
        1,
        DVec3::new(0.0, 80.0, 0.0),
        Arc::downgrade(world),
    );

    let _ = cow
        .mob()
        .finalize_spawn(world, EntitySpawnReason::Natural, None);

    assert!(
        REGISTRY
            .cow_variants
            .id_from_key(&cow.variant().key)
            .is_some()
    );
    assert!(
        REGISTRY
            .cow_sound_variants
            .id_from_key(&cow.sound_variant().key)
            .is_some()
    );
}
