use std::io::Cursor;

use simdnbt::borrow::read_compound as read_borrowed_compound;
use steel_protocol::packets::game::RelativeMovement;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::init_vanilla_registry;
use steel_registry::{vanilla_damage_types, vanilla_entities};
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::types::UpdateFlags;
use steel_utils::{ChunkPos, WorldAabb};
use uuid::Uuid;

use super::*;
use crate::behavior::init_behaviors;
use crate::bootstrap::init_globals_once;
use crate::config::ResolvedDomainConfig;
use crate::entity::entities::PigEntity;
use crate::entity::{EntityFluidContact, LivingEntity, change_entity_world, next_entity_id};
use crate::physics::{CollisionWorld, WorldCollisionProvider};
use crate::player::ResetReason;
use crate::portal::{TeleportPostTransition, TeleportTransition};
use crate::server::worlds::WorldMap;
use crate::test_support::{
    TestPlayerBuilder, fresh_test_world, fresh_test_world_in_domain, insert_ready_full_chunk,
};

const TEST_TNT_POS: BlockPos = BlockPos::new(8, 64, 8);
const VANILLA_DEFAULT_FUSE_TIME: i32 = 80;
const VANILLA_INITIAL_HORIZONTAL_SPEED: f64 = 0.02;
const VANILLA_INITIAL_VERTICAL_SPEED: f32 = 0.2;
const VANILLA_MINIMUM_SHORT_FUSE: i32 = 10;
const VANILLA_SHORT_FUSE_RANDOM_RANGE: i32 = 20;
const VANILLA_MAX_EXPLOSION_POWER: f32 = 128.0;
const SERIALIZED_FUSE: i32 = 37;
const SERIALIZED_EXPLOSION_POWER: f32 = 6.5;
const FLOWING_WATER_LEVEL: u8 = 4;
const PLAYER_HURT_EXPERIENCE_TICKS: i32 = 100;
const EXPERIENCE_ORB_SEARCH_MARGIN: f64 = 2.0;
const TEST_EXPLOSION_POWER: f32 = 2.0;
const OVERLAPPING_TNT_CENTER_OFFSET: f64 = 0.25;
const COLLISION_QUERY_MARGIN: f64 = 1.0;
const FINAL_FUSE_TICK: i32 = 1;

fn test_tnt_position() -> DVec3 {
    let (x, y, z) = TEST_TNT_POS.get_bottom_center();
    DVec3::new(x, y, z)
}

fn ready_tnt_world(key: &'static str) -> Arc<World> {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world(key);
    insert_ready_full_chunk(&world, ChunkPos::from_block_pos(TEST_TNT_POS));
    world
}

#[test]
fn priming_applies_vanilla_motion_and_entity_properties() {
    const SEED: i64 = 0x7A71;

    init_vanilla_registry();
    let world = fresh_test_world("primed_tnt_initial_motion");
    world.set_random_seed_for_test(SEED);
    let mut expected = LegacyRandom::from_seed(SEED as u64);
    let angle = expected.next_f64() * TAU;
    let position = test_tnt_position();
    let entity = PrimedTntEntity::primed(
        &vanilla_entities::TNT,
        next_entity_id(),
        position,
        &world,
        None,
    );

    let velocity = entity.velocity();
    assert_eq!(
        velocity.x.to_bits(),
        (-angle.sin() * VANILLA_INITIAL_HORIZONTAL_SPEED).to_bits()
    );
    assert_eq!(
        velocity.y.to_bits(),
        f64::from(VANILLA_INITIAL_VERTICAL_SPEED).to_bits()
    );
    assert_eq!(
        velocity.z.to_bits(),
        (-angle.cos() * VANILLA_INITIAL_HORIZONTAL_SPEED).to_bits()
    );
    assert_eq!(world.with_random(Random::next_i64), expected.next_i64());
    assert_eq!(entity.fuse(), VANILLA_DEFAULT_FUSE_TIME);
    assert_eq!(entity.block_state(), vanilla_blocks::TNT.default_state());
    assert!(entity.blocks_building());
    assert!(entity.is_pickable());
    assert_eq!(entity.movement_emission(), EntityMovementEmission::None);
}

#[test]
fn primed_tnt_does_not_create_hard_movement_collision_shapes() {
    init_vanilla_registry();
    let world = fresh_test_world("primed_tnt_hard_collision_broad_phase");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let center = test_tnt_position();
    let first = Arc::new(PrimedTntEntity::new(
        &vanilla_entities::TNT,
        next_entity_id(),
        center - DVec3::X * OVERLAPPING_TNT_CENTER_OFFSET,
        Arc::downgrade(&world),
    ));
    let second = Arc::new(PrimedTntEntity::new(
        &vanilla_entities::TNT,
        next_entity_id(),
        center + DVec3::X * OVERLAPPING_TNT_CENTER_OFFSET,
        Arc::downgrade(&world),
    ));
    for entity in [Arc::clone(&first), Arc::clone(&second)] {
        let entity: SharedEntity = entity;
        world
            .try_add_entity(entity)
            .expect("primed TNT should enter the loaded test chunk");
    }

    let query = WorldAabb::encapsulating(&first.bounding_box(), &second.bounding_box())
        .inflate(COLLISION_QUERY_MARGIN);
    let collision_world = WorldCollisionProvider::for_entity(&world, first.as_ref());

    assert!(collision_world.get_entity_collisions(&query).is_empty());
    assert!(!collision_world.has_entity_collision(&query));
}

#[test]
fn shortened_fuse_uses_one_bounded_level_random_draw() {
    const SEED: i64 = 0xF05E;

    init_vanilla_registry();
    let world = fresh_test_world("primed_tnt_short_fuse_random");
    world.set_random_seed_for_test(SEED);
    let mut expected = LegacyRandom::from_seed(SEED as u64);
    let expected_fuse =
        expected.next_i32_bounded(VANILLA_SHORT_FUSE_RANDOM_RANGE) + VANILLA_MINIMUM_SHORT_FUSE;

    let actual_fuse = PrimedTntEntity::get_random_short_fuse(&world, VANILLA_DEFAULT_FUSE_TIME);

    assert_eq!(actual_fuse, expected_fuse);
    assert_eq!(world.with_random(Random::next_i64), expected.next_i64());
}

#[test]
fn type_specific_nbt_round_trip_preserves_fuse_block_power_and_owner() {
    init_vanilla_registry();
    let owner = Uuid::nil();
    let entity = PrimedTntEntity::new(
        &vanilla_entities::TNT,
        next_entity_id(),
        DVec3::ZERO,
        Weak::new(),
    );
    entity.set_fuse(SERIALIZED_FUSE);
    entity.set_block_state(
        vanilla_blocks::TNT
            .default_state()
            .set_value(&BlockStateProperties::UNSTABLE, true),
    );
    {
        let mut state = entity.state.lock();
        state.explosion_power = SERIALIZED_EXPLOSION_POWER;
        state.owner = Some(EntityReference::from_uuid(owner));
    }

    let mut encoded = NbtCompound::new();
    entity.save_additional(&mut encoded);
    let mut bytes = Vec::new();
    encoded.write(&mut bytes);
    let borrowed = read_borrowed_compound(&mut Cursor::new(bytes.as_slice()))
        .expect("test TNT NBT should reborrow");

    let loaded = PrimedTntEntity::new(
        &vanilla_entities::TNT,
        next_entity_id(),
        DVec3::ZERO,
        Weak::new(),
    );
    loaded.load_additional(BorrowedNbtCompoundView::from(&borrowed));

    assert_eq!(loaded.fuse(), SERIALIZED_FUSE);
    assert_eq!(loaded.block_state(), entity.block_state());
    let state = loaded.state.lock();
    assert!((state.explosion_power - SERIALIZED_EXPLOSION_POWER).abs() <= f32::EPSILON);
    assert_eq!(state.owner.as_ref().map(EntityReference::uuid), Some(owner));
}

#[test]
fn loaded_explosion_power_is_clamped_to_vanilla_bounds() {
    init_vanilla_registry();
    let entity = PrimedTntEntity::new(
        &vanilla_entities::TNT,
        next_entity_id(),
        DVec3::ZERO,
        Weak::new(),
    );
    let mut encoded = NbtCompound::new();
    encoded.insert("explosion_power", VANILLA_MAX_EXPLOSION_POWER + 1.0);
    let mut bytes = Vec::new();
    encoded.write(&mut bytes);
    let borrowed = read_borrowed_compound(&mut Cursor::new(bytes.as_slice()))
        .expect("test TNT NBT should reborrow");

    entity.load_additional(BorrowedNbtCompoundView::from(&borrowed));

    assert!(
        (entity.state.lock().explosion_power - VANILLA_MAX_EXPLOSION_POWER).abs() <= f32::EPSILON
    );
}

#[test]
fn tick_applies_physics_before_decrementing_the_fuse() {
    let world = ready_tnt_world("primed_tnt_tick_order");
    let entity = PrimedTntEntity::new(
        &vanilla_entities::TNT,
        next_entity_id(),
        test_tnt_position(),
        Arc::downgrade(&world),
    );

    entity.tick();

    assert_eq!(
        entity.position().y.to_bits(),
        (test_tnt_position().y - GRAVITY).to_bits()
    );
    assert_eq!(
        entity.velocity().y.to_bits(),
        (-GRAVITY * f64::from(AIR_DRAG)).to_bits()
    );
    assert_eq!(entity.fuse(), VANILLA_DEFAULT_FUSE_TIME - 1);
    assert!(!entity.is_removed());
}

#[test]
fn surviving_tick_updates_fluid_without_advancing_base_tick_eye_history() {
    let world = ready_tnt_world("primed_tnt_direct_fluid_update");
    let entity = PrimedTntEntity::new(
        &vanilla_entities::TNT,
        next_entity_id(),
        test_tnt_position(),
        Arc::downgrade(&world),
    );
    entity
        .base
        .set_fluid_contact(EntityFluidContact::from_parts(1.0, 0.0, true, false));

    entity.tick();

    assert_eq!(entity.fluid_contact(), EntityFluidContact::default());
    assert!(!entity.base.was_eye_in_water());
}

#[test]
fn fuse_zero_discards_tnt_and_disabled_rule_suppresses_the_explosion() {
    let world = ready_tnt_world("primed_tnt_disabled_explosion");
    assert!(world.set_game_rule(&TNT_EXPLODES, false));
    assert!(world.set_block(
        TEST_TNT_POS,
        vanilla_blocks::GLASS.default_state(),
        UpdateFlags::UPDATE_NONE,
    ));
    let entity = PrimedTntEntity::new(
        &vanilla_entities::TNT,
        next_entity_id(),
        test_tnt_position(),
        Arc::downgrade(&world),
    );
    entity.set_fuse(FINAL_FUSE_TICK);
    entity.tick();

    assert!(entity.is_removed());
    assert_eq!(entity.fuse(), 0);
    assert_eq!(
        world.get_block_state(TEST_TNT_POS).get_block(),
        &vanilla_blocks::GLASS
    );
}

#[test]
fn primed_tnt_is_immune_to_damage() {
    init_vanilla_registry();
    let entity = PrimedTntEntity::new(
        &vanilla_entities::TNT,
        next_entity_id(),
        DVec3::ZERO,
        Weak::new(),
    );
    let source = DamageSource::environment(&vanilla_damage_types::GENERIC);
    let arbitrary_damage = 100.0;

    assert!(!entity.hurt(
        &fresh_test_world("primed_tnt_damage_immunity"),
        &source,
        arbitrary_damage
    ));
    assert_eq!(entity.fuse(), VANILLA_DEFAULT_FUSE_TIME);
    assert!(!entity.is_removed());
}

#[test]
fn persisted_owner_restores_to_the_live_living_entity() {
    init_vanilla_registry();
    let world = fresh_test_world("primed_tnt_owner_restore");
    let player = TestPlayerBuilder::new(Arc::clone(&world), "Owner", next_entity_id()).build();
    assert!(world.add_player(Arc::clone(&player), ResetReason::InitialJoin));
    let entity = PrimedTntEntity::new(
        &vanilla_entities::TNT,
        next_entity_id(),
        DVec3::ZERO,
        Arc::downgrade(&world),
    );
    entity.state.lock().owner = Some(EntityReference::from_uuid(player.uuid()));

    assert_eq!(entity.owner().map(|owner| owner.id()), Some(player.id()));
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "the cross-world transition, explosion, attribution, and XP assertions form one regression"
)]
fn cross_world_recreation_preserves_owner_and_explosion_attribution() {
    init_globals_once();
    let source = fresh_test_world_in_domain("primed_tnt_recreation", "source");
    let target = fresh_test_world_in_domain("primed_tnt_recreation", "target");
    let domain = ResolvedDomainConfig {
        name: "primed_tnt_recreation".to_owned(),
        default_world: source.key.clone(),
        worlds: vec![source.key.clone(), target.key.clone()],
    };
    let mut worlds = WorldMap::new(domain.name.clone(), &[domain], &[]);
    worlds.insert(source.key.clone(), Arc::clone(&source));
    worlds.insert(target.key.clone(), Arc::clone(&target));

    let position = test_tnt_position();
    let chunk = ChunkPos::from_entity_pos(position);
    insert_ready_full_chunk(&source, chunk);
    insert_ready_full_chunk(&target, chunk);
    let player = TestPlayerBuilder::new(Arc::clone(&source), "Owner", next_entity_id()).build();
    assert!(source.add_player(Arc::clone(&player), ResetReason::InitialJoin));
    let owner: SharedEntity = player.clone();
    let previous = Arc::new(PrimedTntEntity::new(
        &vanilla_entities::TNT,
        next_entity_id(),
        position,
        Arc::downgrade(&source),
    ));
    previous.state.lock().owner = Some(EntityReference::from_entity(&owner));
    let previous_entity: SharedEntity = previous.clone();
    source
        .try_add_entity(Arc::clone(&previous_entity))
        .expect("source TNT should enter the loaded test chunk");
    let transition = TeleportTransition {
        target_world: Arc::clone(&target),
        position,
        rotation: previous.rotation(),
        velocity: previous.velocity(),
        relatives: RelativeMovement::NONE,
        portal_cooldown: 0,
        as_passenger: false,
        post_transition: TeleportPostTransition::do_nothing(),
    };

    let Some(recreated) = change_entity_world(previous_entity, &transition) else {
        panic!("primed TNT should recreate in the loaded target world");
    };
    let Some(recreated_tnt) =
        steel_utils::Downcast::downcast_ref::<PrimedTntEntity>(recreated.as_ref())
    else {
        panic!("recreated entity should retain the primed TNT implementation");
    };
    let Some(indirect_source) = recreated_tnt.explosion_indirect_source() else {
        panic!("recreated primed TNT should preserve explosion attribution");
    };

    assert!(Arc::ptr_eq(&indirect_source, &owner));
    assert!(recreated_tnt.state.lock().used_portal);
    assert_eq!(previous.removal_reason(), Some(RemovalReason::ChangedWorld));

    let victim = Arc::new(PigEntity::new(
        &vanilla_entities::PIG,
        next_entity_id(),
        position + DVec3::X,
        Arc::downgrade(&target),
    ));
    let victim_entity: SharedEntity = victim.clone();
    target
        .try_add_entity(victim_entity)
        .expect("victim should enter the loaded target chunk");

    recreated_tnt.set_fuse(FINAL_FUSE_TICK);
    recreated_tnt.tick();
    assert_eq!(
        recreated_tnt.removal_reason(),
        Some(RemovalReason::Discarded)
    );

    let Some(source) = victim.last_damage_source() else {
        panic!("victim should retain the TNT explosion damage source");
    };
    assert_eq!(source.damage_type, &vanilla_damage_types::PLAYER_EXPLOSION);
    let Some(direct) = source.direct_entity(&target) else {
        panic!("damage source should retain the discarded primed TNT");
    };
    let Some(responsible) = source.causing_entity(&target) else {
        panic!("damage source should resolve the cross-world owner");
    };
    let Some(last_hurt_by_mob) = victim.last_hurt_by_mob() else {
        panic!("victim should retain the cross-world responsible living entity");
    };
    assert!(Arc::ptr_eq(&direct, &recreated));
    assert!(direct.is_removed());
    assert_eq!(source.source_position_raw(), None);
    assert_eq!(
        source.effective_source_position(&target),
        Some(direct.position())
    );
    assert!(Arc::ptr_eq(&responsible, &owner));
    assert!(Arc::ptr_eq(&last_hurt_by_mob, &owner));
    assert_eq!(victim.last_hurt_by_player_uuid(), Some(owner.uuid()));
    assert_eq!(
        victim.last_hurt_by_player_memory_time(),
        PLAYER_HURT_EXPERIENCE_TICKS
    );
    let experience_orb_search = victim.bounding_box().inflate(EXPERIENCE_ORB_SEARCH_MARGIN);
    assert!(
        target
            .get_entities_in_aabb(&experience_orb_search)
            .iter()
            .any(|entity| entity.entity_type() == &vanilla_entities::EXPERIENCE_ORB),
        "player-attributed TNT kill should drop experience"
    );
}

#[test]
fn flowing_water_pushes_primed_tnt_trajectory() {
    let world = ready_tnt_world("primed_tnt_fluid_current");
    let flags = UpdateFlags::UPDATE_NONE
        | UpdateFlags::UPDATE_KNOWN_SHAPE
        | UpdateFlags::UPDATE_SKIP_ON_PLACE;
    assert!(world.set_block(
        TEST_TNT_POS.below(),
        vanilla_blocks::STONE.default_state(),
        flags,
    ));
    assert!(world.set_block(TEST_TNT_POS, vanilla_blocks::WATER.default_state(), flags,));
    assert!(
        world.set_block(
            TEST_TNT_POS.east(),
            vanilla_blocks::WATER
                .default_state()
                .set_value(&BlockStateProperties::LEVEL, FLOWING_WATER_LEVEL),
            flags,
        )
    );
    let entity = PrimedTntEntity::new(
        &vanilla_entities::TNT,
        next_entity_id(),
        test_tnt_position(),
        Arc::downgrade(&world),
    );

    entity.tick();
    assert!(entity.velocity().x > 0.0);

    entity.tick();
    assert!(entity.position().x > test_tnt_position().x);
}

#[test]
fn teleported_tnt_preserves_nether_portal_but_explodes_other_blocks() {
    let world = ready_tnt_world("primed_tnt_portal_explosion");
    let portal_pos = TEST_TNT_POS;
    let glass_pos = portal_pos.south();
    let flags = UpdateFlags::UPDATE_NONE
        | UpdateFlags::UPDATE_KNOWN_SHAPE
        | UpdateFlags::UPDATE_SKIP_ON_PLACE;
    assert!(world.set_block(
        portal_pos,
        vanilla_blocks::NETHER_PORTAL.default_state(),
        flags,
    ));
    assert!(world.set_block(glass_pos, vanilla_blocks::GLASS.default_state(), flags,));
    assert_eq!(
        world.get_block_state(portal_pos).get_block(),
        &vanilla_blocks::NETHER_PORTAL
    );
    let entity = PrimedTntEntity::new(
        &vanilla_entities::TNT,
        next_entity_id(),
        test_tnt_position(),
        Arc::downgrade(&world),
    );

    entity.on_teleported();
    entity.explode(&world, None, None);

    assert_eq!(
        world.get_block_state(portal_pos).get_block(),
        &vanilla_blocks::NETHER_PORTAL
    );
    assert!(world.get_block_state(glass_pos).is_air());
}

#[test]
fn horizontally_aligned_explosion_does_not_push_primed_tnt_upward() {
    let world = ready_tnt_world("primed_tnt_explosion_origin");
    let position = test_tnt_position() + DVec3::X;
    let entity = Arc::new(PrimedTntEntity::new(
        &vanilla_entities::TNT,
        next_entity_id(),
        position,
        Arc::downgrade(&world),
    ));
    let shared: SharedEntity = entity.clone();
    world
        .try_add_entity(shared)
        .expect("primed TNT should enter the loaded test chunk");

    world.explode(ExplosionOptions::new(
        test_tnt_position(),
        TEST_EXPLOSION_POWER,
        ExplosionInteraction::None,
    ));

    assert!(entity.velocity().x > 0.0);
    assert!(entity.velocity().y.abs() <= f64::EPSILON);
}
