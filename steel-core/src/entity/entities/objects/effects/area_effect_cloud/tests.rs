use std::io::Cursor;
use std::sync::Arc;

use glam::DVec3;
use simdnbt::borrow::read_compound;
use simdnbt::owned::NbtCompound;
use steel_registry::data_components::PotionContents;
use steel_registry::{
    init_vanilla_registry, vanilla_entities, vanilla_mob_effects, vanilla_potions,
};
use steel_utils::ChunkPos;

use super::AreaEffectCloudEntity;
use crate::behavior::init_behaviors;
use crate::entity::entities::PigEntity;
use crate::entity::{Entity, LivingEntity, SharedEntity};
use crate::test_support::{fresh_test_world, insert_ready_full_chunk};
use crate::world::World;

const POSITION: DVec3 = DVec3::new(8.0, 64.0, 8.0);

/// A cloud configured like the one a lingering potion leaves behind.
fn lingering_cloud(world: &Arc<World>) -> Arc<AreaEffectCloudEntity> {
    let cloud = Arc::new(AreaEffectCloudEntity::new(
        &vanilla_entities::AREA_EFFECT_CLOUD,
        1,
        POSITION,
        Arc::downgrade(world),
    ));
    cloud.set_radius(3.0);
    cloud.set_radius_on_use(-0.5);
    cloud.set_duration(600);
    cloud.set_wait_time(10);
    cloud.set_radius_per_tick(-cloud.radius() / cloud.duration() as f32);
    cloud.set_potion_contents(PotionContents::of(&vanilla_potions::POISON));
    cloud
}

fn tick(cloud: &Arc<AreaEffectCloudEntity>) {
    cloud.advance_tick_count();
    Arc::clone(cloud).tick();
}

#[test]
fn lingering_cloud_shrinks_away_before_its_duration_ends() {
    init_vanilla_registry();
    init_behaviors();
    let fixture = fresh_test_world("area_effect_cloud_shrinks");
    let cloud = lingering_cloud(&fixture.world);

    let mut ticks = 0;
    while !cloud.is_removed() && ticks < 1000 {
        tick(&cloud);
        ticks += 1;
        if ticks < 10 {
            assert!(cloud.is_waiting(), "waits for its wait time");
        }
    }

    // The per-tick shrink removes it ~500 ticks after the wait, well before
    // `wait_time + duration` (610).
    assert!((500..610).contains(&ticks), "removed after {ticks} ticks");
}

#[test]
fn affects_living_entities_once_per_reapplication_delay() {
    init_vanilla_registry();
    init_behaviors();
    let fixture = fresh_test_world("area_effect_cloud_affects_pig");
    let world = &fixture.world;
    insert_ready_full_chunk(world, ChunkPos::new(0, 0));
    let pig = Arc::new(PigEntity::new(
        &vanilla_entities::PIG,
        2,
        POSITION,
        Arc::downgrade(world),
    ));
    let shared_pig: SharedEntity = Arc::<PigEntity>::clone(&pig);
    world.try_add_entity(shared_pig).expect("register pig");
    let cloud = lingering_cloud(world);

    for _ in 0..9 {
        tick(&cloud);
    }
    assert!(!pig.has_mob_effect(vanilla_mob_effects::POISON));

    tick(&cloud);
    assert!(pig.has_mob_effect(vanilla_mob_effects::POISON));
    assert!(
        cloud.radius() < 2.5,
        "each affected entity shrinks the cloud by 0.5"
    );

    pig.remove_mob_effect(vanilla_mob_effects::POISON);
    for _ in 0..19 {
        tick(&cloud);
    }
    assert!(
        !pig.has_mob_effect(vanilla_mob_effects::POISON),
        "victims wait for the reapplication delay"
    );
    tick(&cloud);
    assert!(pig.has_mob_effect(vanilla_mob_effects::POISON));
}

#[test]
fn extreme_saved_timings_wrap_like_vanilla_int_arithmetic() {
    init_vanilla_registry();
    init_behaviors();
    let fixture = fresh_test_world("area_effect_cloud_wrapping");
    let world = &fixture.world;
    insert_ready_full_chunk(world, ChunkPos::new(0, 0));
    let pig = Arc::new(PigEntity::new(
        &vanilla_entities::PIG,
        2,
        POSITION,
        Arc::downgrade(world),
    ));
    let shared_pig: SharedEntity = Arc::<PigEntity>::clone(&pig);
    world.try_add_entity(shared_pig).expect("register pig");
    let cloud = lingering_cloud(world);

    let mut saved = NbtCompound::new();
    saved.insert("Age", 9);
    saved.insert("Duration", i32::MAX);
    saved.insert("DurationOnUse", i32::MAX);
    saved.insert("WaitTime", i32::MIN);
    saved.insert("ReapplicationDelay", i32::MAX);
    let mut bytes = Vec::new();
    saved.write(&mut bytes);
    let borrowed = read_compound(&mut Cursor::new(&bytes))
        .unwrap_or_else(|error| panic!("test nbt should reborrow: {error}"));
    cloud.load_additional((&borrowed).into());
    cloud.set_potion_contents(PotionContents::of(&vanilla_potions::POISON));

    // At age 10 the effect pass runs and every timing sum overflows
    // (`10 - MIN`, `10 + MAX`, `MAX + MAX`); Java wraps these silently.
    tick(&cloud);
    assert!(pig.has_mob_effect(vanilla_mob_effects::POISON));
    assert!(cloud.is_removed(), "the wrapped duration drops to zero or below");
}

#[test]
fn load_restores_age_and_resizes_to_saved_radius() {
    init_vanilla_registry();
    let fixture = fresh_test_world("area_effect_cloud_load");
    let cloud = lingering_cloud(&fixture.world);

    let mut saved = NbtCompound::new();
    saved.insert("Age", 42);
    saved.insert("Radius", 1.0f32);
    let mut bytes = Vec::new();
    saved.write(&mut bytes);
    let borrowed = read_compound(&mut Cursor::new(&bytes))
        .unwrap_or_else(|error| panic!("test nbt should reborrow: {error}"));
    cloud.load_additional((&borrowed).into());

    assert_eq!(cloud.tick_count(), 42);
    assert_eq!(cloud.bounding_box().width(), 2.0);
    assert_eq!(cloud.duration(), -1, "a missing duration never expires");
}
