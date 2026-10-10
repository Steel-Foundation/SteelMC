use steel_protocol::packets::game::RelativeMovement;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::{init_vanilla_registry, vanilla_damage_types, vanilla_entities};
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::types::UpdateFlags;
use steel_utils::{ChunkPos, Identifier};

use super::*;
use crate::behavior::init_behaviors;
use crate::entity::entities::PigEntity;
use crate::entity::{LivingEntity, change_entity_world, init_entities, next_entity_id};
use crate::player::ResetReason;
use crate::portal::{TeleportPostTransition, TeleportTransition};
use crate::test_support::{
    TestPlayerBuilder, TestWorld, fresh_test_world, insert_ready_full_chunk, test_domain,
};

// Center the fixture in the only loaded chunk so movement stays in Full chunk data.
const TEST_TNT_BLOCK_POS: BlockPos = BlockPos::new(8, 64, 8);

fn test_tnt_position() -> DVec3 {
    let (x, y, z) = TEST_TNT_BLOCK_POS.get_bottom_center();
    DVec3::new(x, y, z)
}

fn ready_tnt_world(key: &'static str) -> TestWorld {
    init_vanilla_registry();
    init_behaviors();
    let fixture = fresh_test_world(key);
    insert_ready_full_chunk(&fixture.world, ChunkPos::from_block_pos(TEST_TNT_BLOCK_POS));
    fixture
}

fn primed_tnt(world: &Arc<World>, position: DVec3) -> Arc<PrimedTntEntity> {
    Arc::new(PrimedTntEntity::new(
        &vanilla_entities::TNT,
        next_entity_id(),
        position,
        Arc::downgrade(world),
    ))
}

#[test]
fn priming_applies_vanilla_float_tau_launch_motion() {
    const SEED: i64 = 0x7A71;
    const EXPECTED_INITIAL_MOTION_X: f64 = -0.003_337_386_497_296_714_6;
    const EXPECTED_INITIAL_MOTION_Z: f64 = -0.019_719_580_405_466_58;
    // Java's `Math.sin` may differ from Rust's in the final bits.
    const INITIAL_MOTION_TOLERANCE: f64 = 1.0e-12;

    let fixture = fresh_test_world("primed_tnt_initial_motion");
    let world = &fixture.world;
    world.set_random_seed_for_test(SEED);

    let entity = PrimedTntEntity::primed(
        &vanilla_entities::TNT,
        next_entity_id(),
        test_tnt_position(),
        world,
        None,
    );

    let velocity = entity.velocity();
    assert!((velocity.x - EXPECTED_INITIAL_MOTION_X).abs() <= INITIAL_MOTION_TOLERANCE);
    assert_eq!(velocity.y.to_bits(), f64::from(0.2_f32).to_bits());
    assert!((velocity.z - EXPECTED_INITIAL_MOTION_Z).abs() <= INITIAL_MOTION_TOLERANCE);
    let mut expected = LegacyRandom::from_seed(SEED as u64);
    let _ = expected.next_f64();
    assert_eq!(world.with_random(Random::next_i64), expected.next_i64());
}

#[test]
fn cross_world_recreation_preserves_owner_and_explosion_attribution() {
    init_vanilla_registry();
    init_behaviors();
    init_entities();
    let domain = test_domain("primed_tnt_recreation", &["source", "target"]);
    let source = domain
        .get(&Identifier::new_static("primed_tnt_recreation", "source"))
        .expect("source world");
    let target = domain
        .get(&Identifier::new_static("primed_tnt_recreation", "target"))
        .expect("target world");
    let position = test_tnt_position();
    let chunk = ChunkPos::from_entity_pos(position);
    insert_ready_full_chunk(source, chunk);
    insert_ready_full_chunk(target, chunk);
    let player = TestPlayerBuilder::new(Arc::clone(source), "Owner", next_entity_id()).build();
    assert!(source.add_player(Arc::clone(&player), ResetReason::InitialJoin));
    let owner: SharedEntity = player;
    let previous = Arc::new(PrimedTntEntity::primed(
        &vanilla_entities::TNT,
        next_entity_id(),
        position,
        source,
        Some(owner.as_ref()),
    ));
    let previous_entity: SharedEntity = Arc::<PrimedTntEntity>::clone(&previous);
    source
        .try_add_entity(Arc::clone(&previous_entity))
        .expect("source TNT should enter the loaded test chunk");
    let transition = TeleportTransition {
        target_world: Arc::clone(target),
        position,
        rotation: previous.rotation(),
        velocity: DVec3::ZERO,
        relatives: RelativeMovement::NONE,
        portal_cooldown: 0,
        as_passenger: false,
        post_transition: TeleportPostTransition::do_nothing(),
    };

    let recreated = change_entity_world(previous_entity, &transition)
        .expect("primed TNT should recreate in the loaded target world");
    let recreated_tnt = steel_utils::Downcast::downcast_ref::<PrimedTntEntity>(recreated.as_ref())
        .expect("recreated entity should remain primed TNT");
    assert!(recreated_tnt.state.lock().used_portal);

    let victim = Arc::new(PigEntity::new(
        &vanilla_entities::PIG,
        next_entity_id(),
        position + DVec3::X,
        Arc::downgrade(target),
    ));
    target
        .try_add_entity(Arc::<PigEntity>::clone(&victim))
        .expect("victim should enter the loaded target chunk");
    recreated_tnt.set_fuse(1);
    Arc::clone(&recreated).tick();

    // The damage source retains the discarded TNT and the owner resolved from another world.
    let source = victim.last_damage_source().expect("TNT damage source");
    assert_eq!(source.damage_type, &vanilla_damage_types::PLAYER_EXPLOSION);
    let direct = source.direct_entity().expect("discarded primed TNT");
    assert!(Arc::ptr_eq(direct, &recreated));
    assert!(direct.is_removed());
    assert!(Arc::ptr_eq(
        source.causing_entity().expect("cross-world owner"),
        &owner
    ));
    assert!(Arc::ptr_eq(
        &victim.last_hurt_by_mob().expect("responsible owner"),
        &owner
    ));
    assert_eq!(victim.last_hurt_by_player_uuid(), Some(owner.uuid()));
}

#[test]
fn flowing_water_pushes_primed_tnt_trajectory() {
    let fixture = ready_tnt_world("primed_tnt_fluid_current");
    let world = &fixture.world;
    let flags = UpdateFlags::UPDATE_NONE
        | UpdateFlags::UPDATE_KNOWN_SHAPE
        | UpdateFlags::UPDATE_SKIP_ON_PLACE;
    assert!(world.set_block(
        TEST_TNT_BLOCK_POS.below(),
        vanilla_blocks::STONE.default_state(),
        flags,
    ));
    assert!(world.set_block(
        TEST_TNT_BLOCK_POS,
        vanilla_blocks::WATER.default_state(),
        flags,
    ));
    assert!(
        world.set_block(
            TEST_TNT_BLOCK_POS.east(),
            vanilla_blocks::WATER
                .default_state()
                .set_value(&BlockStateProperties::LEVEL, 4),
            flags,
        )
    );
    let entity = primed_tnt(world, test_tnt_position());

    // TNT skips the base tick, so currents come only from its post-move fluid update.
    Arc::clone(&entity).tick();
    assert!(entity.velocity().x > 0.0);
    Arc::clone(&entity).tick();
    assert!(entity.position().x > test_tnt_position().x);
}

#[test]
fn teleported_tnt_preserves_nether_portal_but_explodes_other_blocks() {
    let fixture = ready_tnt_world("primed_tnt_portal_explosion");
    let world = &fixture.world;
    let portal_pos = TEST_TNT_BLOCK_POS;
    let glass_pos = portal_pos.south();
    let flags = UpdateFlags::UPDATE_NONE
        | UpdateFlags::UPDATE_KNOWN_SHAPE
        | UpdateFlags::UPDATE_SKIP_ON_PLACE;
    assert!(world.set_block(
        portal_pos,
        vanilla_blocks::NETHER_PORTAL.default_state(),
        flags,
    ));
    assert!(world.set_block(glass_pos, vanilla_blocks::GLASS.default_state(), flags));
    let entity = primed_tnt(world, test_tnt_position());

    entity.on_teleported();
    entity.explode(world);

    assert_eq!(
        world.get_block_state(portal_pos).get_block(),
        &vanilla_blocks::NETHER_PORTAL
    );
    assert!(world.get_block_state(glass_pos).is_air());
}

#[test]
fn horizontally_aligned_explosion_does_not_push_primed_tnt_upward() {
    let fixture = ready_tnt_world("primed_tnt_explosion_origin");
    let world = &fixture.world;
    let entity = primed_tnt(world, test_tnt_position() + DVec3::X);
    world
        .try_add_entity(Arc::<PrimedTntEntity>::clone(&entity))
        .expect("primed TNT should enter the loaded test chunk");

    // Vanilla pushes TNT from its feet rather than its eye position.
    world.explode(ExplosionOptions::new(
        test_tnt_position(),
        2.0,
        ExplosionInteraction::None,
    ));

    assert!(entity.velocity().x > 0.0);
    assert!(entity.velocity().y.abs() <= f64::EPSILON);
}
