use steel_registry::data_components::components::{FireworkExplosion, Fireworks};
use steel_registry::{init_vanilla_registry, vanilla_entities};
use steel_utils::ChunkPos;

use super::*;

use crate::entity::entities::PigEntity;
use crate::test_support::load_additional;
use crate::test_support::{fresh_loaded_test_world, fresh_test_world, insert_ready_full_chunk};

fn rocket_item(flight_duration: i32, explosions: usize) -> ItemStack {
    let mut item = ItemStack::new(&vanilla_items::FIREWORK_ROCKET);
    let fireworks = Fireworks::new(
        flight_duration,
        vec![FireworkExplosion::default(); explosions],
    )
    .unwrap_or_else(|error| panic!("valid firework component should construct: {error}"));
    item.set(FIREWORKS, fireworks);
    item
}

#[test]
fn launched_rocket_flies_for_its_flight_duration() {
    init_vanilla_registry();
    let rocket = FireworkRocketEntity::launched(
        &vanilla_entities::FIREWORK_ROCKET,
        1,
        DVec3::ZERO,
        Weak::new(),
        rocket_item(3, 0),
    );

    assert!((40..=51).contains(&rocket.flight.lock().lifetime));
    assert_eq!(rocket.velocity().y.to_bits(), 0.05_f64.to_bits());
}

#[test]
fn attached_rocket_keeps_its_target_when_world_ids_are_reused() {
    init_vanilla_registry();
    let world_fixture = fresh_test_world("rocket_attachment_identity");
    let world = &world_fixture.world;
    let owner_world_fixture = fresh_test_world("rocket_owner_world");
    let owner_world = &owner_world_fixture.world;
    insert_ready_full_chunk(world, ChunkPos::new(0, 0));
    let target: SharedEntity = PigEntity::new(
        &vanilla_entities::PIG,
        900_019,
        DVec3::ZERO,
        Arc::downgrade(owner_world),
    );
    let rocket = FireworkRocketEntity::attached_to_living(
        &vanilla_entities::FIREWORK_ROCKET,
        900_020,
        Arc::downgrade(world),
        ItemStack::new(&vanilla_items::FIREWORK_ROCKET),
        &target,
    );
    assert_eq!(
        *rocket.entity_data.lock().attached_to_target.get(),
        Some(900_019)
    );
    assert!(Arc::ptr_eq(
        &rocket.projectile().owner().expect("owner in other world"),
        &target
    ));

    target.set_removed(RemovalReason::Discarded);
    let original_generation = target.generation();
    let replacement: SharedEntity = PigEntity::new(
        &vanilla_entities::PIG,
        target.id(),
        DVec3::ZERO,
        Arc::downgrade(world),
    );
    world
        .try_add_entity(replacement)
        .expect("replacement registration");
    assert!(Arc::ptr_eq(
        &rocket.attached_entity(world).expect("original attachment"),
        &target
    ));
    drop(target);
    assert!(
        rocket
            .attached_entity(world)
            .is_none_or(|entity| entity.generation() == original_generation)
    );
}

#[test]
fn explosion_damages_an_entity_reusing_the_attached_id() {
    init_vanilla_registry();
    let world_fixture = fresh_loaded_test_world("firework_explosion_identity");
    let world = &world_fixture.world;
    let position = DVec3::new(8.0, 64.0, 8.0);
    let original: SharedEntity = PigEntity::new(
        &vanilla_entities::PIG,
        900_001,
        position,
        Arc::downgrade(world),
    );
    world
        .try_add_entity(Arc::clone(&original))
        .expect("register original attachment");
    let rocket = FireworkRocketEntity::attached_to_living(
        &vanilla_entities::FIREWORK_ROCKET,
        900_002,
        Arc::downgrade(world),
        rocket_item(1, 1),
        &original,
    );

    original.set_removed(RemovalReason::Discarded);
    let replacement = PigEntity::new(
        &vanilla_entities::PIG,
        original.id(),
        position + DVec3::X,
        Arc::downgrade(world),
    );
    world
        .try_add_entity(Arc::<PigEntity>::clone(&replacement))
        .expect("register replacement");

    let health = replacement.living().health();
    rocket.deal_explosion_damage(world);
    assert!(
        replacement.living().health() < health,
        "a different entity with the attachment id must take explosion damage"
    );
}

#[test]
fn rocket_saves_and_loads_its_flight() {
    init_vanilla_registry();
    let rocket = FireworkRocketEntity::launched(
        &vanilla_entities::FIREWORK_ROCKET,
        1,
        DVec3::ZERO,
        Weak::new(),
        ItemStack::new(&vanilla_items::FIREWORK_ROCKET),
    );
    {
        let mut flight = rocket.flight.lock();
        flight.life = 7;
        flight.lifetime = 29;
    }
    rocket.set_shot_at_angle(true);
    rocket
        .projectile()
        .set_owner_uuid(Some(uuid::Uuid::from_u128(42)));

    let mut nbt = NbtCompound::new();
    rocket.save_additional(&mut nbt);
    assert_eq!(nbt.int("Life"), Some(7));
    assert_eq!(nbt.int("LifeTime"), Some(29));
    assert_eq!(nbt.byte("ShotAtAngle"), Some(1));

    let loaded = FireworkRocketEntity::new(
        &vanilla_entities::FIREWORK_ROCKET,
        2,
        DVec3::ZERO,
        Weak::new(),
    );
    load_additional(&*loaded, &nbt);

    {
        let flight = loaded.flight.lock();
        assert_eq!((flight.life, flight.lifetime), (7, 29));
    }
    assert!(loaded.is_shot_at_angle());
    assert_eq!(
        loaded.projectile().owner_uuid(),
        Some(uuid::Uuid::from_u128(42))
    );
    assert!(loaded.item().is(&vanilla_items::FIREWORK_ROCKET));
}

#[test]
fn elytra_boost_matches_vanilla_vector_formula() {
    let movement = DVec3::new(0.2, -0.1, 0.4);
    let look_angle = DVec3::new(0.0, 0.0, 1.0);

    assert_eq!(
        elytra_boosted_velocity(movement, look_angle),
        DVec3::new(0.1, -0.05, 1.05)
    );
}
