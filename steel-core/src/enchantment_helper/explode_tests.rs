//! Wind Burst (`minecraft:explode` post-attack effect) behavior.

use std::sync::Arc;

use glam::DVec3;
use steel_registry::{
    init_vanilla_registry, vanilla_damage_types, vanilla_entities, vanilla_items,
};
use steel_utils::{ChunkPos, Identifier};

use super::{EnchantmentPostAttackContext, do_post_attack_effects_from_item};
use crate::behavior::init_behaviors;
use crate::entity::damage::DamageSource;
use crate::entity::entities::PigEntity;
use crate::entity::{Entity as _, SharedEntity, next_entity_id};
use crate::test_support::{enchanted_item, fresh_test_world, insert_ready_full_chunk};

/// Vanilla Wind Burst only fires once the attacker has fallen more than this.
const WIND_BURST_MIN_FALL_DISTANCE: f64 = 1.5;

/// Hits a detached victim with a Wind Burst mace swung by a pig attacker that
/// has fallen `fall_distance`, returning the attacker's velocity afterwards.
fn attacker_velocity_after_wind_burst(
    world_key: &'static str,
    level: u32,
    fall_distance: f64,
) -> DVec3 {
    init_vanilla_registry();
    init_behaviors();

    let fixture = fresh_test_world(world_key);
    let world = &fixture.world;
    insert_ready_full_chunk(world, ChunkPos::new(0, 0));

    let attacker = Arc::new(PigEntity::new(
        &vanilla_entities::PIG,
        next_entity_id(),
        DVec3::new(8.5, 64.0, 8.5),
        Arc::downgrade(world),
    ));
    world
        .try_add_entity(Arc::clone(&attacker) as SharedEntity)
        .expect("attacker should attach to the loaded chunk");
    attacker.set_fall_distance(fall_distance);

    let victim: SharedEntity = Arc::new(PigEntity::new(
        &vanilla_entities::PIG,
        next_entity_id(),
        DVec3::new(9.5, 64.0, 8.5),
        Arc::downgrade(world),
    ));
    let attacker_entity: SharedEntity = Arc::<PigEntity>::clone(&attacker);
    let damage_source = DamageSource::direct(&vanilla_damage_types::MOB_ATTACK, attacker_entity);
    let context = EnchantmentPostAttackContext::new(&victim, &damage_source);
    let mace = enchanted_item(
        &vanilla_items::MACE,
        Identifier::vanilla_static("wind_burst"),
        level,
    );

    do_post_attack_effects_from_item(world, &mace, &context);

    attacker.velocity()
}

#[test]
fn wind_burst_requires_attacker_fall_distance() {
    let grounded = attacker_velocity_after_wind_burst(
        "wind_burst_short_fall",
        1,
        WIND_BURST_MIN_FALL_DISTANCE - 0.5,
    );
    assert_eq!(
        grounded,
        DVec3::ZERO,
        "a short fall must not trigger the burst"
    );

    let falling = attacker_velocity_after_wind_burst(
        "wind_burst_long_fall",
        1,
        WIND_BURST_MIN_FALL_DISTANCE + 0.5,
    );
    assert!(
        falling.y > 0.0,
        "the burst should launch the attacker upward, got {falling:?}"
    );
}

#[test]
fn wind_burst_knockback_scales_with_level_lookup() {
    // Vanilla's level lookup table: [1.2, 1.75, 2.2].
    const LEVEL_ONE_KNOCKBACK: f64 = 1.2;
    const LEVEL_THREE_KNOCKBACK: f64 = 2.2;

    let fall_distance = WIND_BURST_MIN_FALL_DISTANCE + 0.5;
    let level_one = attacker_velocity_after_wind_burst("wind_burst_level_one", 1, fall_distance);
    let level_three =
        attacker_velocity_after_wind_burst("wind_burst_level_three", 3, fall_distance);

    let expected_ratio = LEVEL_THREE_KNOCKBACK / LEVEL_ONE_KNOCKBACK;
    let ratio = level_three.y / level_one.y;
    assert!(
        (ratio - expected_ratio).abs() < 1.0e-6,
        "level 3 knockback should be {expected_ratio}x level 1, got {ratio}"
    );
}
