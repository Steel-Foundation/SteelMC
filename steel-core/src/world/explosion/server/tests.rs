use std::sync::{
    Arc, Weak,
    atomic::{AtomicUsize, Ordering},
};

use glam::DVec3;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, PistonType};
use steel_registry::item_stack::ItemStack;
use steel_registry::{
    entity_type::EntityTypeRef, fluid::FluidState, init_vanilla_registry, vanilla_blocks,
    vanilla_entities, vanilla_game_rules::MOB_GRIEFING, vanilla_items,
};
use steel_utils::types::{GameType, UpdateFlags};
use steel_utils::{
    BlockPos, BlockStateId, ChunkPos, Direction, Downcast as _, DowncastType, DowncastTypeKey,
    WorldAabb,
};

use super::*;
use crate::behavior::{BlockCollisionContext, init_behaviors};
use crate::block_entity::{
    SharedBlockEntity, entities::PistonMovingBlockEntity, init_block_entities,
};
use crate::entity::entities::{
    ChestMinecartEntity, ItemEntity, ItemFrameEntity, LeashFenceKnotEntity, PigEntity,
    PrimedTntEntity,
};
use crate::entity::{EntityBase, EntityFluidContact, next_entity_id};
use crate::player::ResetReason;
use crate::test_support::{
    TestPlayerBuilder, TestWorld, fresh_test_world, insert_ready_full_chunk,
};
use crate::world::raycast::ExplosionExposureRaycast;
use crate::world::{ExplosionInteraction, ExplosionOptions, World};

const TEST_BLOCK_BOTTOM_CENTER: DVec3 = DVec3::new(0.5, 64.0, 0.5);
const TEST_BLOCK_CENTER: DVec3 = DVec3::new(0.5, 64.5, 0.5);
const TEST_LOW_EXPLOSION_CENTER: DVec3 = DVec3::new(0.5, 64.125, 0.5);
const TEST_WALL_POS: BlockPos = BlockPos::new(1, 64, 0);
const NEAR_EXPOSURE_TARGET_DISTANCE: f64 = 2.0;
const FAR_EXPOSURE_TARGET_DISTANCE: f64 = 6.0;
const ENTITY_EFFECT_TEST_RADIUS: f32 = 2.0;

struct VetoExplosionSource {
    base: EntityBase,
    resistance_calls: AtomicUsize,
    decision_calls: AtomicUsize,
}

// SAFETY: This test-only key uniquely identifies `VetoExplosionSource` in Steel's test build.
unsafe impl DowncastType for VetoExplosionSource {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:test/explosion_veto_source");
}

impl Entity for VetoExplosionSource {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        &vanilla_entities::ITEM
    }

    fn block_explosion_resistance(
        &self,
        _explosion: &dyn Explosion,
        _world: &World,
        _pos: BlockPos,
        _state: BlockStateId,
        _fluid: FluidState,
        resistance: f32,
    ) -> f32 {
        self.resistance_calls.fetch_add(1, Ordering::Relaxed);
        resistance
    }

    fn should_block_explode(
        &self,
        _explosion: &dyn Explosion,
        _world: &World,
        _pos: BlockPos,
        _state: BlockStateId,
        _power: f32,
    ) -> bool {
        self.decision_calls.fetch_add(1, Ordering::Relaxed);
        false
    }
}

#[derive(Default)]
struct VetoCustomCalculator {
    resistance_calls: AtomicUsize,
    decision_calls: AtomicUsize,
}

impl ExplosionDamageCalculator for VetoCustomCalculator {
    fn block_explosion_resistance(
        &self,
        _explosion: &dyn Explosion,
        _world: &World,
        _pos: BlockPos,
        _state: BlockStateId,
        _fluid: FluidState,
    ) -> Option<f32> {
        self.resistance_calls.fetch_add(1, Ordering::Relaxed);
        None
    }

    fn should_block_explode(
        &self,
        _explosion: &dyn Explosion,
        _world: &World,
        _pos: BlockPos,
        _state: BlockStateId,
        _power: f32,
    ) -> bool {
        self.decision_calls.fetch_add(1, Ordering::Relaxed);
        false
    }
}

struct BlockMutatingExposureEntity {
    base: EntityBase,
    place_wall_on_hit: bool,
}

// SAFETY: This test-only key uniquely identifies `BlockMutatingExposureEntity`.
unsafe impl DowncastType for BlockMutatingExposureEntity {
    const TYPE_KEY: DowncastTypeKey =
        DowncastTypeKey::new("steel:test/block_mutating_exposure_entity");
}

impl Entity for BlockMutatingExposureEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        &vanilla_entities::ITEM
    }

    fn on_explosion_hit(&self, _explosion_source: Option<&dyn Entity>) {
        if self.place_wall_on_hit
            && let Some(world) = self.level()
        {
            world.set_block(
                TEST_WALL_POS,
                vanilla_blocks::STONE.default_state(),
                UpdateFlags::UPDATE_NONE,
            );
        }
    }
}

fn ready_world(key: &'static str) -> TestWorld {
    init_vanilla_registry();
    init_behaviors();
    let fixture = fresh_test_world(key);
    insert_ready_full_chunk(&fixture.world, ChunkPos::new(0, 0));
    fixture
}

fn item_entity(world: &Arc<World>, position: DVec3) -> ItemEntity {
    ItemEntity::new(
        &vanilla_entities::ITEM,
        next_entity_id(),
        position,
        Arc::downgrade(world),
    )
}

fn add_entity<E: Entity>(world: &Arc<World>, entity: E) -> Arc<E> {
    let entity = Arc::new(entity);
    let Ok(()) = world.try_add_entity(Arc::<E>::clone(&entity)) else {
        panic!("test entity must be added to its loaded chunk");
    };
    entity
}

fn add_block_attached_targets(
    world: &Arc<World>,
) -> (Arc<ItemFrameEntity>, Arc<LeashFenceKnotEntity>) {
    let item_frame = add_entity(
        world,
        ItemFrameEntity::new_attached(
            &vanilla_entities::ITEM_FRAME,
            next_entity_id(),
            TEST_WALL_POS,
            Direction::West,
            Arc::downgrade(world),
        ),
    );
    let leash_knot = add_entity(
        world,
        LeashFenceKnotEntity::new_attached(
            &vanilla_entities::LEASH_KNOT,
            next_entity_id(),
            BlockPos::new(0, 64, 1),
            Arc::downgrade(world),
        ),
    );
    (item_frame, leash_knot)
}

fn assert_exposure_matches_seen_percent(world: &World, center: DVec3, entity: &dyn Entity) {
    let exposure = EntityExplosionExposure::capture(entity);
    assert_eq!(
        seen_percent(world, center, entity).to_bits(),
        exposure.calculate_uncached(world, center).to_bits()
    );
}

#[test]
fn player_knockback_map_excludes_creative_flying_and_spectator_players() {
    let fixture = ready_world("explosion_player_knockback_rules");
    let world = &fixture.world;
    let player = |name, offset, game_mode| {
        let player = TestPlayerBuilder::new(Arc::clone(world), name, next_entity_id()).build();
        player
            .base()
            .set_position_local(TEST_BLOCK_BOTTOM_CENTER + offset);
        player.restore_game_modes(game_mode, None);
        player.abilities.lock().update_for_game_mode(game_mode);
        player.set_client_loaded(true);
        assert!(world.add_player(Arc::clone(&player), ResetReason::InitialJoin));
        player
    };
    let survival = player("Survival", DVec3::X, GameType::Survival);
    let creative = player("Creative", DVec3::Z, GameType::Creative);
    creative.abilities.lock().flying = true;
    let spectator = player("Spectator", -DVec3::X, GameType::Spectator);
    let mut explosion = ServerExplosion::new(
        world,
        None,
        None,
        None,
        None,
        TEST_BLOCK_BOTTOM_CENTER,
        ENTITY_EFFECT_TEST_RADIUS,
        false,
        BlockInteraction::Keep,
    );

    explosion.hurt_entities();

    let delta = survival.explosion_damage_origin() - explosion.center;
    let normalized_distance = survival.position().distance(explosion.center)
        / (f64::from(ENTITY_EFFECT_TEST_RADIUS) * 2.0);
    let expected_knockback = delta / delta.length() * (1.0 - normalized_distance);
    assert_eq!(survival.velocity(), expected_knockback);
    assert_eq!(
        explosion.hit_players.get(&survival.id()),
        Some(&expected_knockback)
    );
    // Creative flyers are still pushed, but Vanilla omits their client knockback.
    assert_ne!(creative.velocity(), DVec3::ZERO);
    assert!(!explosion.hit_players.contains_key(&creative.id()));
    assert_eq!(spectator.velocity(), DVec3::ZERO);
    assert!(!explosion.hit_players.contains_key(&spectator.id()));
}

#[test]
fn source_and_custom_calculator_hooks_run_on_the_sequential_lane() {
    let fixture = ready_world("explosion_calculator_hooks");
    let world = &fixture.world;
    let center = TEST_BLOCK_CENTER;
    assert!(world.set_block(
        BlockPos::from(center),
        vanilla_blocks::STONE.default_state(),
        UpdateFlags::UPDATE_NONE,
    ));
    let source = Arc::new(VetoExplosionSource {
        base: EntityBase::new(
            next_entity_id(),
            center,
            vanilla_entities::ITEM.dimensions,
            Arc::downgrade(world),
        ),
        resistance_calls: AtomicUsize::new(0),
        decision_calls: AtomicUsize::new(0),
    });
    let source_explosion = ServerExplosion::new(
        world,
        Some(Arc::clone(&source) as SharedEntity),
        None,
        None,
        None,
        center,
        2.0,
        false,
        BlockInteraction::Destroy,
    );

    assert_eq!(source_explosion.calculate_exploded_positions(|| 0.5), []);
    assert!(source.resistance_calls.load(Ordering::Relaxed) > 0);
    assert!(source.decision_calls.load(Ordering::Relaxed) > 0);

    let custom = VetoCustomCalculator::default();
    let custom_explosion = ServerExplosion::new(
        world,
        None,
        None,
        Some(&custom),
        None,
        center,
        2.0,
        false,
        BlockInteraction::Destroy,
    );

    assert_eq!(custom_explosion.calculate_exploded_positions(|| 0.5), []);
    assert!(custom.resistance_calls.load(Ordering::Relaxed) > RAY_COUNT);
    assert!(custom.decision_calls.load(Ordering::Relaxed) > RAY_COUNT);
}

#[test]
fn cached_exposure_raycast_matches_clear_partial_and_blocked_paths() {
    let fixture = ready_world("cached_explosion_exposure");
    let world = &fixture.world;
    let center = TEST_BLOCK_CENTER;
    let player = TestPlayerBuilder::new(Arc::clone(world), "Cached", next_entity_id()).build();
    player
        .base()
        .set_position_local(TEST_BLOCK_BOTTOM_CENTER + DVec3::X * NEAR_EXPOSURE_TARGET_DISTANCE);

    let compare = || {
        let exposure = EntityExplosionExposure::capture(player.as_ref());
        let uncached = exposure.calculate_uncached(world, center);
        let mut raycast = ExplosionExposureRaycast::new(world, exposure.collision_context);
        raycast.configure_clear_grid(BlockPos::new(0, 63, 0), BlockPos::new(4, 67, 1));
        let cached = exposure.calculate_with_visibility(|from| raycast.is_path_clear(from, center));
        assert_eq!(cached.to_bits(), uncached.to_bits());
        cached
    };

    assert_eq!(compare().to_bits(), 1.0_f32.to_bits());
    assert!(world.set_block(
        TEST_WALL_POS,
        vanilla_blocks::STONE_SLAB.default_state(),
        UpdateFlags::UPDATE_NONE,
    ));
    let partial = compare();
    assert!(partial > 0.0 && partial < 1.0, "partial exposure={partial}");
    assert!(world.set_block(
        TEST_WALL_POS,
        vanilla_blocks::STONE.default_state(),
        UpdateFlags::UPDATE_NONE,
    ));
    assert_eq!(compare().to_bits(), 0.0_f32.to_bits());
}

#[test]
fn stable_air_certificate_skips_rays_and_observes_later_block_writes() {
    let fixture = ready_world("stable_air_explosion_exposure");
    let world = &fixture.world;
    let center = TEST_BLOCK_CENTER;
    let entity = item_entity(
        world,
        TEST_BLOCK_BOTTOM_CENTER + DVec3::X * FAR_EXPOSURE_TARGET_DISTANCE,
    );
    let exposure = EntityExplosionExposure::capture(&entity);
    let mut raycast = ExplosionExposureRaycast::new(world, exposure.collision_context);

    assert_eq!(
        exposure
            .calculate_cached_with(&mut raycast, center)
            .to_bits(),
        1.0_f32.to_bits()
    );
    let after_air = raycast.stats();
    assert_eq!(after_air.stable_air_hits, 1);
    assert_eq!(after_air.block_visits, 0);

    assert!(world.set_block(
        TEST_WALL_POS,
        vanilla_blocks::STONE.default_state(),
        UpdateFlags::UPDATE_NONE,
    ));
    let expected = exposure.calculate_uncached(world, center);
    assert_eq!(
        exposure
            .calculate_cached_with(&mut raycast, center)
            .to_bits(),
        expected.to_bits()
    );
    assert_eq!(raycast.stats().stable_air_hits, 1);
}

#[test]
fn resting_tnt_certificate_includes_vanilla_adjusted_source_block() {
    init_vanilla_registry();
    let explosion_position = DVec3::new(0.5, 200.0, 0.5);
    let target = PrimedTntEntity::new(
        &vanilla_entities::TNT,
        next_entity_id(),
        explosion_position + DVec3::X * FAR_EXPOSURE_TARGET_DISTANCE,
        Weak::new(),
    );
    let center = explosion_position
        + DVec3::Y * (f64::from(vanilla_entities::TNT.dimensions.height) * 0.0625);
    let exposure = EntityExplosionExposure::capture(&target);
    let Some(bounds) = exposure.stable_air_certificate_bounds(center) else {
        panic!("resting TNT exposure must have finite certificate bounds");
    };
    let (min, max) = bounds.corners();

    // Vanilla extrapolates the bottom sample by -1e-7 before flooring it. Tightening this to the
    // raw entity hull at y=200 would miss tall collision shapes sourced from the block below.
    assert_eq!(min.y(), 199);
    assert_eq!(max.y(), 200);
}

#[test]
fn cached_exposure_matches_across_chunk_and_section_boundaries() {
    let fixture = ready_world("cached_exposure_boundaries");
    let world = &fixture.world;
    insert_ready_full_chunk(world, ChunkPos::new(1, 0));
    assert!(world.set_block(
        BlockPos::new(15, 79, 0),
        vanilla_blocks::STONE_SLAB.default_state(),
        UpdateFlags::UPDATE_NONE,
    ));
    let entity = item_entity(world, DVec3::new(16.5, 80.0, 0.5));

    assert_exposure_matches_seen_percent(world, DVec3::new(14.5, 78.5, 0.5), &entity);
}

#[test]
fn exposure_cache_does_not_retain_air_from_a_missing_chunk() {
    init_vanilla_registry();
    init_behaviors();
    let fixture = fresh_test_world("explosion_exposure_missing_chunk");
    let world = &fixture.world;
    let from = TEST_BLOCK_CENTER + DVec3::X * NEAR_EXPOSURE_TARGET_DISTANCE;
    let to = TEST_BLOCK_CENTER;
    let mut raycast = ExplosionExposureRaycast::new(world, BlockCollisionContext::empty());
    raycast.configure_clear_grid(BlockPos::new(0, 64, 0), BlockPos::new(2, 64, 0));

    assert!(raycast.is_path_clear(from, to));
    insert_ready_full_chunk(world, ChunkPos::new(0, 0));
    assert!(world.set_block(
        TEST_WALL_POS,
        vanilla_blocks::STONE.default_state(),
        UpdateFlags::UPDATE_NONE,
    ));
    assert!(!raycast.is_path_clear(from, to));
}

#[test]
fn exposure_clipping_uses_the_entity_collision_context() {
    let fixture = ready_world("explosion_entity_collision_context");
    let world = &fixture.world;
    assert!(world.set_block(
        TEST_WALL_POS,
        vanilla_blocks::POWDER_SNOW.default_state(),
        UpdateFlags::UPDATE_ALL,
    ));
    let entity = item_entity(
        world,
        TEST_BLOCK_BOTTOM_CENTER + DVec3::X * NEAR_EXPOSURE_TARGET_DISTANCE,
    );
    let center = TEST_LOW_EXPLOSION_CENTER;
    let walking_exposure = EntityExplosionExposure::capture(&entity);
    let mut shared_raycast =
        ExplosionExposureRaycast::new(world, walking_exposure.collision_context);
    shared_raycast.configure_clear_grid(BlockPos::new(0, 63, 0), BlockPos::new(3, 66, 1));

    let walking = walking_exposure.calculate_cached_with(&mut shared_raycast, center);
    assert_eq!(walking.to_bits(), 1.0_f32.to_bits());

    // Powder snow only collides with entities that are not descending.
    entity.set_fall_distance(3.0);
    entity.set_shared_shift_key_down(true);
    let falling_exposure = EntityExplosionExposure::capture(&entity);
    assert!(falling_exposure.collision_context.is_descending());
    let falling = falling_exposure.calculate_cached_with(&mut shared_raycast, center);
    assert_eq!(falling.to_bits(), 0.0_f32.to_bits());
    assert_eq!(
        falling.to_bits(),
        falling_exposure.calculate_uncached(world, center).to_bits()
    );
}

#[test]
fn exposure_cache_is_cleared_before_block_mutating_entity_callbacks() {
    let fixture = ready_world("explosion_exposure_callback_mutation");
    let world = &fixture.world;
    let position = TEST_BLOCK_BOTTOM_CENTER + DVec3::X * NEAR_EXPOSURE_TARGET_DISTANCE;
    let entity = |place_wall_on_hit| BlockMutatingExposureEntity {
        base: EntityBase::new(
            next_entity_id(),
            position,
            vanilla_entities::ITEM.dimensions,
            Arc::downgrade(world),
        ),
        place_wall_on_hit,
    };
    let mutator = add_entity(world, entity(true));
    let observer = add_entity(world, entity(false));

    world.explode(ExplosionOptions::new(
        TEST_LOW_EXPLOSION_CENTER,
        ENTITY_EFFECT_TEST_RADIUS,
        ExplosionInteraction::None,
    ));

    assert_eq!(
        world.get_block_state(TEST_WALL_POS),
        vanilla_blocks::STONE.default_state()
    );
    assert!(mutator.velocity().x > 0.0);
    assert_eq!(observer.velocity(), DVec3::ZERO);
}

#[test]
fn moving_piston_exposure_uses_live_block_entities() {
    let fixture = ready_world("moving_piston_explosion_exposure");
    let world = &fixture.world;
    init_block_entities();
    let moving_state = vanilla_blocks::MOVING_PISTON
        .default_state()
        .set_value(&BlockStateProperties::FACING, Direction::East)
        .set_value(&BlockStateProperties::PISTON_TYPE, PistonType::Normal);
    let moved_state = vanilla_blocks::PISTON
        .default_state()
        .set_value(&BlockStateProperties::FACING, Direction::East)
        .set_value(&BlockStateProperties::EXTENDED, true);
    assert!(world.set_block(TEST_WALL_POS, moving_state, UpdateFlags::UPDATE_NONE));
    let block_entity: SharedBlockEntity = Arc::new(PistonMovingBlockEntity::new_moving(
        Arc::downgrade(world),
        TEST_WALL_POS,
        moving_state,
        moved_state,
        Direction::East,
        false,
        true,
    ));
    assert!(world.set_block_entity(block_entity));
    let center = TEST_LOW_EXPLOSION_CENTER;
    let entity = item_entity(
        world,
        TEST_BLOCK_BOTTOM_CENTER + DVec3::X * NEAR_EXPOSURE_TARGET_DISTANCE,
    );
    let exposure = EntityExplosionExposure::capture(&entity);
    let live = exposure.calculate_uncached(world, center);
    assert!(live < 1.0, "the moving piston should occlude a sample");
    let mut raycast = ExplosionExposureRaycast::new(world, exposure.collision_context);
    raycast.configure_clear_grid(BlockPos::new(0, 63, 0), BlockPos::new(3, 66, 1));

    assert_eq!(
        exposure
            .calculate_cached_with(&mut raycast, center)
            .to_bits(),
        live.to_bits()
    );
}

#[test]
fn non_destructive_explosion_ignores_blocklike_entities_without_mob_griefing() {
    let fixture = ready_world("explosion_ignores_blocklike_entities");
    let world = &fixture.world;
    assert!(world.set_game_rule(&MOB_GRIEFING, false));
    let item = item_entity(world, TEST_BLOCK_BOTTOM_CENTER + DVec3::X);
    item.set_item(ItemStack::new(&vanilla_items::STONE));
    let item = add_entity(world, item);
    let (item_frame, leash_knot) = add_block_attached_targets(world);

    world.explode(ExplosionOptions::new(
        TEST_BLOCK_BOTTOM_CENTER,
        ENTITY_EFFECT_TEST_RADIUS,
        ExplosionInteraction::None,
    ));

    assert!(!item.is_removed());
    assert_eq!(item.velocity(), DVec3::ZERO);
    assert_eq!(item_frame.velocity(), DVec3::ZERO);
    assert_eq!(leash_knot.velocity(), DVec3::ZERO);
}

#[test]
fn mob_explosion_does_not_push_vehicles_when_mob_griefing_is_disabled() {
    let fixture = ready_world("explosion_ignores_vehicles_without_mob_griefing");
    let world = &fixture.world;
    assert!(world.set_game_rule(&MOB_GRIEFING, false));
    let minecart = add_entity(
        world,
        ChestMinecartEntity::new(
            &vanilla_entities::CHEST_MINECART,
            next_entity_id(),
            TEST_BLOCK_BOTTOM_CENTER + DVec3::X,
            Arc::downgrade(world),
        ),
    );
    let pig: SharedEntity = Arc::new(PigEntity::new(
        &vanilla_entities::PIG,
        next_entity_id(),
        TEST_BLOCK_BOTTOM_CENTER,
        Arc::downgrade(world),
    ));
    let mut options = ExplosionOptions::new(
        TEST_BLOCK_BOTTOM_CENTER,
        ENTITY_EFFECT_TEST_RADIUS,
        ExplosionInteraction::Mob,
    );
    options.source = Some(pig);

    world.explode(options);

    assert_eq!(minecart.velocity(), DVec3::ZERO);
}

#[test]
fn submerged_source_explosion_does_not_push_block_attached_entities() {
    let fixture = ready_world("submerged_explosion_ignores_block_attached_entities");
    let world = &fixture.world;
    let source = item_entity(world, TEST_BLOCK_BOTTOM_CENTER);
    source
        .base()
        .set_fluid_contact(EntityFluidContact::from_parts(1.0, 0.0, false, false));
    assert!(source.is_in_water());
    let (item_frame, leash_knot) = add_block_attached_targets(world);
    let mut options = ExplosionOptions::new(
        TEST_BLOCK_BOTTOM_CENTER,
        ENTITY_EFFECT_TEST_RADIUS,
        ExplosionInteraction::Block,
    );
    options.source = Some(Arc::new(source) as SharedEntity);

    world.explode(options);

    assert_eq!(item_frame.velocity(), DVec3::ZERO);
    assert_eq!(leash_knot.velocity(), DVec3::ZERO);
}

#[test]
fn destructive_explosion_removes_stone_and_spawns_its_loot() {
    let fixture = ready_world("explosion_block_destruction");
    let world = &fixture.world;
    let center_pos = BlockPos::new(0, 64, 0);
    assert!(world.set_block(
        center_pos,
        vanilla_blocks::STONE.default_state(),
        UpdateFlags::UPDATE_ALL,
    ));
    let mut explosion = ServerExplosion::new(
        world,
        None,
        None,
        None,
        None,
        TEST_BLOCK_CENTER,
        4.0,
        false,
        BlockInteraction::Destroy,
    );

    explosion.explode();

    assert!(world.get_block_state(center_pos).is_air());
    let drops = world.get_entities_in_aabb_matching(
        &WorldAabb::new(-1.0, 63.0, -1.0, 2.0, 67.0, 2.0),
        |entity| entity.entity_type() == &vanilla_entities::ITEM,
    );
    assert!(drops.iter().any(|entity| {
        entity
            .as_ref()
            .downcast_ref::<ItemEntity>()
            .is_some_and(|item| item.get_item().is(&vanilla_items::COBBLESTONE))
    }));
}
