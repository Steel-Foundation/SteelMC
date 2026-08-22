use super::*;
use steel_registry::init_vanilla_registry;

#[test]
fn chicken_initializes_vanilla_living_attributes_and_health() {
    init_vanilla_registry();

    let chicken = ChickenEntity::new(&vanilla_entities::CHICKEN, 1, DVec3::ZERO, Weak::new());

    assert_eq!(chicken.get_health().to_bits(), 4.0_f32.to_bits());
    let attributes = chicken.attributes().lock();
    assert_eq!(
        attributes
            .required_value(vanilla_attributes::MAX_HEALTH)
            .to_bits(),
        4.0_f64.to_bits()
    );
    assert!(
        (attributes.required_value(vanilla_attributes::MOVEMENT_SPEED) - f64::from(0.25_f32)).abs()
            < 1e-12
    );
}

#[test]
fn chicken_uses_vanilla_chicken_food_tag() {
    init_vanilla_registry();

    assert!(ChickenEntity::is_food(&ItemStack::new(
        &vanilla_items::WHEAT_SEEDS
    )));
    assert!(!ChickenEntity::is_food(&ItemStack::new(
        &vanilla_items::STONE
    )));
}

#[test]
fn chicken_sound_methods_follow_selected_sound_variant() {
    init_vanilla_registry();

    let chicken = ChickenEntity::new(&vanilla_entities::CHICKEN, 1, DVec3::ZERO, Weak::new());
    let source = DamageSource::environment(&vanilla_damage_types::GENERIC);

    assert_eq!(
        Mob::ambient_sound(&chicken).map(|sound| &sound.key),
        Some(&sound_events::ENTITY_CHICKEN_AMBIENT.key)
    );

    chicken.set_sound_variant(&vanilla_chicken_sound_variants::PICKY);

    assert_eq!(
        Mob::ambient_sound(&chicken).map(|sound| &sound.key),
        Some(&chicken.sound_variant().adult_sounds.ambient_sound.key)
    );
    assert_eq!(
        LivingEntity::hurt_sound(&chicken, &source).map(|sound| &sound.key),
        Some(&chicken.sound_variant().adult_sounds.hurt_sound.key)
    );
    assert_eq!(
        LivingEntity::death_sound(&chicken).map(|sound| &sound.key),
        Some(&chicken.sound_variant().adult_sounds.death_sound.key)
    );
}

#[test]
fn chicken_sounds_switch_to_baby_set_for_babies() {
    init_vanilla_registry();

    let chicken = ChickenEntity::new(&vanilla_entities::CHICKEN, 1, DVec3::ZERO, Weak::new());
    chicken.set_baby(true);

    assert_eq!(
        Mob::ambient_sound(&chicken).map(|sound| &sound.key),
        Some(&sound_events::ENTITY_BABY_CHICKEN_AMBIENT.key)
    );
}

#[test]
fn chicken_jockey_rewards_ten_experience() {
    init_vanilla_registry();

    let chicken = ChickenEntity::new(&vanilla_entities::CHICKEN, 1, DVec3::ZERO, Weak::new());

    let normal_reward = LivingEntity::base_experience_reward(&chicken);
    assert!(
        (1..=3).contains(&normal_reward),
        "adult chicken reward should be 1..=3, got {normal_reward}"
    );

    chicken.set_chicken_jockey(true);
    assert_eq!(
        LivingEntity::base_experience_reward(&chicken),
        CHICKEN_JOCKEY_EXPERIENCE_REWARD
    );
}

#[test]
fn chicken_jockey_does_not_despawn_far_away() {
    init_vanilla_registry();

    let chicken = ChickenEntity::new(&vanilla_entities::CHICKEN, 1, DVec3::ZERO, Weak::new());

    assert!(!Mob::remove_when_far_away(&chicken, f64::MAX));

    chicken.set_chicken_jockey(true);
    assert!(Mob::remove_when_far_away(&chicken, f64::MAX));
}

#[test]
fn chicken_breeding_offspring_inherits_parent_variant() {
    init_vanilla_registry();

    let chicken = ChickenEntity::new(&vanilla_entities::CHICKEN, 1, DVec3::ZERO, Weak::new());
    let partner = ChickenEntity::new(&vanilla_entities::CHICKEN, 2, DVec3::ZERO, Weak::new());
    let offspring = ChickenEntity::new(&vanilla_entities::CHICKEN, 3, DVec3::ZERO, Weak::new());

    chicken.set_variant(&vanilla_chicken_variants::WARM);
    partner.set_variant(&vanilla_chicken_variants::COLD);
    offspring.set_variant(&vanilla_chicken_variants::TEMPERATE);

    chicken.initialize_breed_offspring(&partner, &offspring);

    let variant_key = &offspring.variant().key;
    assert!(
        variant_key == &vanilla_chicken_variants::WARM.key
            || variant_key == &vanilla_chicken_variants::COLD.key
    );
}

#[test]
fn chicken_finalize_spawn_assigns_registered_variant_and_sound_variant() {
    init_vanilla_registry();

    let world = fresh_test_world("chicken_finalize_spawn");
    let chicken = ChickenEntity::new(
        &vanilla_entities::CHICKEN,
        1,
        DVec3::new(0.0, 80.0, 0.0),
        Arc::downgrade(&world),
    );

    let _ = Mob::finalize_spawn(&chicken, &world, EntitySpawnReason::Natural, None);

    assert!(
        REGISTRY
            .chicken_variants
            .id_from_key(&chicken.variant().key)
            .is_some()
    );
    assert!(
        REGISTRY
            .chicken_sound_variants
            .id_from_key(&chicken.sound_variant().key)
            .is_some()
    );
}

#[test]
fn chicken_egg_timer_decrements_only_for_adult_non_jockeys() {
    init_vanilla_registry();

    let world = fresh_test_world("chicken_egg_timer");
    let chicken = ChickenEntity::new(
        &vanilla_entities::CHICKEN,
        1,
        DVec3::new(0.0, 80.0, 0.0),
        Arc::downgrade(&world),
    );

    chicken.set_egg_time(2);
    chicken.tick_egg_laying();
    assert_eq!(chicken.egg_time(), 1);

    chicken.set_baby(true);
    chicken.set_egg_time(2);
    chicken.tick_egg_laying();
    assert_eq!(chicken.egg_time(), 2);

    chicken.set_baby(false);
    chicken.set_chicken_jockey(true);
    chicken.set_egg_time(2);
    chicken.tick_egg_laying();
    assert_eq!(chicken.egg_time(), 2);
}

#[test]
fn chicken_resets_egg_timer_within_vanilla_range_after_laying() {
    init_vanilla_registry();

    let world = fresh_test_world("chicken_egg_lay");
    let chicken = ChickenEntity::new(
        &vanilla_entities::CHICKEN,
        1,
        DVec3::new(0.0, 80.0, 0.0),
        Arc::downgrade(&world),
    );

    chicken.set_egg_time(1);
    chicken.tick_egg_laying();

    let egg_time = chicken.egg_time();
    assert!(
        (EGG_LAY_MIN_DELAY_TICKS..EGG_LAY_MIN_DELAY_TICKS + EGG_LAY_RANDOM_RANGE_TICKS)
            .contains(&egg_time),
        "egg timer should reset to the vanilla delay range, got {egg_time}"
    );
}

#[test]
fn chicken_slow_fall_reduces_downward_velocity() {
    init_vanilla_registry();

    let chicken = ChickenEntity::new(&vanilla_entities::CHICKEN, 1, DVec3::ZERO, Weak::new());

    chicken.set_on_ground(false);
    let velocity_input = DVec3::new(1.0, -2.0, 3.0);
    chicken.set_velocity(velocity_input);
    chicken.tick_flapping();

    let velocity = chicken.velocity();
    assert_eq!(velocity.x.to_bits(), 1.0_f64.to_bits());
    assert!(
        (velocity.y - velocity_input.y * f64::from(FALL_DRAG_Y)).abs() < 1e-6,
        "fall drag should scale the downward velocity by the vanilla constant"
    );
    assert_eq!(velocity.z.to_bits(), 3.0_f64.to_bits());
}
