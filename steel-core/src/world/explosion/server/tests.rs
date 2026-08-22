use std::sync::{
    Arc, Weak,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

use glam::DVec3;
use steel_protocol::packet_traits::{CompressionInfo, EncodedPacket};
use steel_registry::blocks::properties::{BlockStateProperties, PistonType};
use steel_registry::{
    entity_type::EntityTypeRef,
    fluid::FluidState,
    init_vanilla_registry, vanilla_block_entity_types, vanilla_blocks, vanilla_damage_types,
    vanilla_entities,
    vanilla_game_rules::{
        BLOCK_EXPLOSION_DROP_DECAY, MOB_EXPLOSION_DROP_DECAY, MOB_GRIEFING,
        TNT_EXPLOSION_DROP_DECAY,
    },
    vanilla_items,
};
use steel_utils::locks::SyncMutex;
use steel_utils::types::{GameType, UpdateFlags};
use steel_utils::{
    BlockPos, BlockStateId, ChunkPos, Direction, Downcast as _, DowncastType, DowncastTypeKey,
};
use text_components::TextComponent;

use super::*;
use crate::behavior::{BlockLootContext, FLUID_BEHAVIORS, init_behaviors};
use crate::block_entity::{
    SharedBlockEntity, entities::PistonMovingBlockEntity, init_block_entities,
};
use crate::entity::entities::{
    ChestMinecartEntity, ItemFrameEntity, LeashFenceKnotEntity, PigEntity, PrimedTntEntity,
};
use crate::entity::{EntityBase, EntityFluidContact, LivingEntity as _, next_entity_id};
use crate::player::connection::NetworkConnection;
use crate::player::{Player, PlayerConnection, ResetReason};
use crate::test_support::{TestPlayerBuilder, fresh_test_world, insert_ready_full_chunk};
use crate::world::explosion::default_block_explosion_resistance;
use crate::world::{
    DefaultExplosionDamageCalculator, ExplosionDamageCalculator, ExplosionInteraction,
    ExplosionOptions, ExplosionOutcome,
};

const TEST_BLOCK_BOTTOM_CENTER: DVec3 = DVec3::new(0.5, 64.0, 0.5);
const TEST_BLOCK_CENTER: DVec3 = DVec3::new(0.5, 64.5, 0.5);
const TEST_LOW_EXPLOSION_CENTER: DVec3 = DVec3::new(0.5, 64.125, 0.5);
const TEST_WALL_POS: BlockPos = BlockPos::new(1, 64, 0);
const TEST_LEASH_POS: BlockPos = BlockPos::new(0, 64, 1);
const PACKET_VIEW_DISTANCE: f64 = 64.0;
const PACKET_CUTOFF_EPSILON: f64 = 0.001;
const NEAR_EXPOSURE_TARGET_DISTANCE: f64 = 2.0;
const FAR_EXPOSURE_TARGET_DISTANCE: f64 = 6.0;
const ENTITY_EFFECT_TEST_RADIUS: f32 = 2.0;
const STANDARD_TNT_EXPLOSION_POWER: f32 = 4.0;
const VANILLA_SMALL_EXPLOSION_RADIUS: f32 = 2.0;
const VANILLA_COMBINED_DROP_STACK_LIMIT: i32 = 16;
const FULL_EXPOSURE: f32 = 1.0;
const NO_EXPOSURE: f32 = 0.0;
const FIXED_RAY_RANDOM_SAMPLE: f32 = 0.5;
const DOES_NOT_CREATE_FIRE: bool = false;

struct VetoExplosionSource {
    base: EntityBase,
    resistance_calls: AtomicUsize,
    decision_calls: AtomicUsize,
}

struct BlockMutatingExposureEntity {
    base: EntityBase,
    wall_pos: BlockPos,
    place_wall_on_hit: bool,
}

// SAFETY: This test-only key uniquely identifies `BlockMutatingExposureEntity`.
unsafe impl DowncastType for BlockMutatingExposureEntity {
    const TYPE_KEY: DowncastTypeKey =
        DowncastTypeKey::new("steel:test/block_mutating_exposure_entity");
}

impl BlockMutatingExposureEntity {
    fn new(
        id: i32,
        position: DVec3,
        world: &Arc<World>,
        wall_pos: BlockPos,
        place_wall_on_hit: bool,
    ) -> Self {
        Self {
            base: EntityBase::new(
                id,
                position,
                vanilla_entities::ITEM.dimensions,
                Arc::downgrade(world),
            ),
            wall_pos,
            place_wall_on_hit,
        }
    }
}

impl Entity for BlockMutatingExposureEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        &vanilla_entities::ITEM
    }

    fn on_explosion_hit(&self, _explosion_source: Option<&dyn Entity>) {
        if !self.place_wall_on_hit {
            return;
        }
        let Some(world) = self.level() else {
            return;
        };
        let _ = world.set_block(
            self.wall_pos,
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_NONE,
        );
    }
}

struct RecordingConnection {
    packets: Arc<SyncMutex<Vec<EncodedPacket>>>,
    closed: AtomicBool,
}

impl NetworkConnection for RecordingConnection {
    fn compression(&self) -> Option<CompressionInfo> {
        None
    }

    fn send_encoded(&self, packet: EncodedPacket) {
        self.packets.lock().push(packet);
    }

    fn send_encoded_bundle(&self, packets: Vec<EncodedPacket>) {
        self.packets.lock().extend(packets);
    }

    fn disconnect_with_reason(&self, _reason: TextComponent) {
        self.close();
    }

    fn tick(&self) {}

    fn latency(&self) -> i32 {
        0
    }

    fn close(&self) {
        self.closed.store(true, Ordering::Release);
    }

    fn closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }
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

fn recording_player(
    world: &Arc<World>,
    name: &'static str,
    position: DVec3,
) -> (Arc<Player>, Arc<SyncMutex<Vec<EncodedPacket>>>) {
    let packets = Arc::new(SyncMutex::new(Vec::new()));
    let connection = Arc::new(PlayerConnection::Other(Box::new(RecordingConnection {
        packets: Arc::clone(&packets),
        closed: AtomicBool::new(false),
    })));
    let player = TestPlayerBuilder::new(Arc::clone(world), name, next_entity_id())
        .connection(connection)
        .build();
    player.base().set_position_local(position);
    assert!(world.add_player(Arc::clone(&player), ResetReason::InitialJoin));
    (player, packets)
}

#[test]
fn vanilla_damage_formula_uses_distance_and_exposure() {
    struct TestExplosion;

    impl Explosion for TestExplosion {
        fn world(&self) -> &Arc<World> {
            panic!("test formula does not access the world")
        }

        fn damage_source(&self) -> &DamageSource {
            panic!("test formula does not access the damage source")
        }

        fn block_interaction(&self) -> BlockInteraction {
            BlockInteraction::Keep
        }

        fn indirect_source_entity(&self) -> Option<&dyn Entity> {
            None
        }

        fn direct_source_entity(&self) -> Option<&dyn Entity> {
            None
        }

        fn radius(&self) -> f32 {
            STANDARD_TNT_EXPLOSION_POWER
        }

        fn center(&self) -> DVec3 {
            DVec3::ZERO
        }

        fn should_affect_blocklike_entities(&self) -> bool {
            false
        }
    }

    init_vanilla_registry();
    let half_diameter_distance = f64::from(STANDARD_TNT_EXPLOSION_POWER);
    let expected_damage_at_half_diameter = 22.0_f32;
    let entity = ItemEntity::new(
        &vanilla_entities::ITEM,
        next_entity_id(),
        DVec3::X * half_diameter_distance,
        Weak::new(),
    );
    let damage = DefaultExplosionDamageCalculator.entity_damage_amount(
        &TestExplosion,
        &entity,
        FULL_EXPOSURE,
    );
    assert_eq!(damage.to_bits(), expected_damage_at_half_diameter.to_bits());
}

#[test]
fn default_damage_source_preserves_direct_source_position() {
    init_vanilla_registry();
    let world = fresh_test_world("explosion_damage_source_position");
    let position = DVec3::new(1.25, 64.0, -3.5);
    let entity = ItemEntity::new(
        &vanilla_entities::ITEM,
        next_entity_id(),
        position,
        Weak::new(),
    );

    let source = default_explosion_damage_source(Some(&entity), None);

    assert_eq!(source.direct_entity_id, Some(entity.id()));
    assert_eq!(source.causing_entity_id, None);
    assert_eq!(source.effective_source_position(&world), Some(position));
    assert_eq!(source.source_position_raw(), None);
    assert_eq!(source.damage_type.key, vanilla_damage_types::EXPLOSION.key);
}

#[test]
fn explicit_position_explosion_damage_source_retains_raw_position() {
    init_vanilla_registry();
    let world = fresh_test_world("explicit_explosion_damage_source_position");
    let position = DVec3::new(1.25, 64.0, -3.5);
    let damage_source = DamageSource::environment(&vanilla_damage_types::BAD_RESPAWN_POINT)
        .with_source_position(position);
    let explosion = ServerExplosion::new(
        &world,
        None,
        Some(damage_source),
        None,
        None,
        position,
        STANDARD_TNT_EXPLOSION_POWER,
        DOES_NOT_CREATE_FIRE,
        BlockInteraction::Keep,
    );

    assert_eq!(
        explosion.damage_source.effective_source_position(&world),
        Some(position),
    );
    assert_eq!(
        explosion.damage_source.source_position_raw(),
        Some(position),
    );
}

#[test]
fn primed_tnt_damage_source_preserves_direct_and_owner_attribution() {
    init_vanilla_registry();
    let world = fresh_test_world("explosion_tnt_damage_attribution");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let player = TestPlayerBuilder::new(Arc::clone(&world), "Owner", next_entity_id()).build();
    assert!(world.add_player(Arc::clone(&player), ResetReason::InitialJoin));
    let tnt = PrimedTntEntity::primed(
        &vanilla_entities::TNT,
        next_entity_id(),
        TEST_BLOCK_BOTTOM_CENTER,
        &world,
        Some(player.as_ref()),
    );

    let explosion = ServerExplosion::new(
        &world,
        Some(&tnt),
        None,
        None,
        None,
        TEST_BLOCK_BOTTOM_CENTER,
        STANDARD_TNT_EXPLOSION_POWER,
        DOES_NOT_CREATE_FIRE,
        BlockInteraction::Destroy,
    );

    assert_eq!(explosion.damage_source.direct_entity_id, Some(tnt.id()));
    assert_eq!(explosion.damage_source.causing_entity_id, Some(player.id()));
    assert_eq!(
        explosion.damage_source.damage_type.key,
        vanilla_damage_types::PLAYER_EXPLOSION.key
    );
}

#[test]
fn player_knockback_map_excludes_creative_flying_and_spectator_players() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("explosion_player_knockback_rules");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let survival = TestPlayerBuilder::new(Arc::clone(&world), "Survival", next_entity_id()).build();
    survival
        .base()
        .set_position_local(TEST_BLOCK_BOTTOM_CENTER + DVec3::X);
    survival.set_client_loaded(true);
    assert!(world.add_player(Arc::clone(&survival), ResetReason::InitialJoin));
    let creative = TestPlayerBuilder::new(Arc::clone(&world), "Creative", next_entity_id()).build();
    creative
        .base()
        .set_position_local(TEST_BLOCK_BOTTOM_CENTER + DVec3::Z);
    creative.restore_game_modes(GameType::Creative, None);
    creative
        .abilities
        .lock()
        .update_for_game_mode(GameType::Creative);
    creative.abilities.lock().flying = true;
    creative.set_client_loaded(true);
    assert!(world.add_player(Arc::clone(&creative), ResetReason::InitialJoin));
    let spectator =
        TestPlayerBuilder::new(Arc::clone(&world), "Spectator", next_entity_id()).build();
    spectator
        .base()
        .set_position_local(TEST_BLOCK_BOTTOM_CENTER - DVec3::X);
    spectator.restore_game_modes(GameType::Spectator, None);
    spectator
        .abilities
        .lock()
        .update_for_game_mode(GameType::Spectator);
    spectator.set_client_loaded(true);
    assert!(world.add_player(Arc::clone(&spectator), ResetReason::InitialJoin));
    let mut explosion = ServerExplosion::new(
        &world,
        None,
        None,
        None,
        None,
        TEST_BLOCK_BOTTOM_CENTER,
        ENTITY_EFFECT_TEST_RADIUS,
        DOES_NOT_CREATE_FIRE,
        BlockInteraction::Keep,
    );
    let initial_survival_health = survival.get_health();

    explosion.hurt_entities();

    let expected_damage = DefaultExplosionDamageCalculator.entity_damage_amount(
        &explosion,
        survival.as_ref(),
        FULL_EXPOSURE,
    );
    assert_eq!(
        survival.get_health().to_bits(),
        (initial_survival_health - expected_damage).to_bits()
    );
    let delta = survival.explosion_damage_origin() - explosion.center;
    let blast_diameter = f64::from(ENTITY_EFFECT_TEST_RADIUS) * 2.0;
    let normalized_distance = survival.position().distance(explosion.center) / blast_diameter;
    let expected_knockback =
        delta / delta.length() * (1.0 - normalized_distance) * f64::from(FULL_EXPOSURE);
    assert_eq!(survival.velocity(), expected_knockback);
    assert_eq!(
        explosion.hit_players.get(&survival.id()),
        Some(&expected_knockback)
    );
    assert_ne!(creative.velocity(), DVec3::ZERO);
    assert!(!explosion.hit_players.contains_key(&creative.id()));
    assert_eq!(spectator.velocity(), DVec3::ZERO);
    assert!(!explosion.hit_players.contains_key(&spectator.id()));
}

#[test]
fn packet_delivery_uses_vanillas_strict_sixty_four_block_cutoff() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("explosion_packet_cutoff");
    let center = TEST_BLOCK_BOTTOM_CENTER;
    insert_ready_full_chunk(&world, ChunkPos::from_block_pos(BlockPos::from(center)));
    let near_position = center + DVec3::X * (PACKET_VIEW_DISTANCE - PACKET_CUTOFF_EPSILON);
    let boundary_position = center + DVec3::X * PACKET_VIEW_DISTANCE;
    insert_ready_full_chunk(
        &world,
        ChunkPos::from_block_pos(BlockPos::from(boundary_position)),
    );
    let (_near, near_packets) = recording_player(&world, "Near", near_position);
    let (_boundary, boundary_packets) = recording_player(&world, "Boundary", boundary_position);
    near_packets.lock().clear();
    boundary_packets.lock().clear();

    let packet_only_radius = -1.0;
    world.explode(ExplosionOptions::new(
        center,
        packet_only_radius,
        ExplosionInteraction::None,
    ));

    assert_eq!(near_packets.lock().len(), 1);
    assert!(boundary_packets.lock().is_empty());
}

#[test]
fn small_particle_selection_matches_radius_and_block_interaction() {
    const RADIUS_EPSILON: f32 = 0.001;

    init_vanilla_registry();
    let world = fresh_test_world("explosion_particle_selection");
    let explosion = |radius, interaction| {
        ServerExplosion::new(
            &world,
            None,
            None,
            None,
            None,
            DVec3::ZERO,
            radius,
            DOES_NOT_CREATE_FIRE,
            interaction,
        )
    };

    assert!(
        explosion(
            VANILLA_SMALL_EXPLOSION_RADIUS - RADIUS_EPSILON,
            BlockInteraction::Destroy
        )
        .is_small()
    );
    assert!(explosion(VANILLA_SMALL_EXPLOSION_RADIUS, BlockInteraction::Keep).is_small());
    assert!(!explosion(VANILLA_SMALL_EXPLOSION_RADIUS, BlockInteraction::Destroy).is_small());
}

#[test]
fn interaction_categories_follow_their_individual_gamerules() {
    init_vanilla_registry();
    let world = fresh_test_world("explosion_interaction_rules");

    assert_eq!(
        world.resolve_block_interaction(ExplosionInteraction::None),
        BlockInteraction::Keep
    );
    assert_eq!(
        world.resolve_block_interaction(ExplosionInteraction::Block),
        BlockInteraction::DestroyWithDecay
    );
    assert_eq!(
        world.resolve_block_interaction(ExplosionInteraction::Mob),
        BlockInteraction::DestroyWithDecay
    );
    assert_eq!(
        world.resolve_block_interaction(ExplosionInteraction::Tnt),
        BlockInteraction::Destroy
    );

    assert!(world.set_game_rule(&BLOCK_EXPLOSION_DROP_DECAY, false));
    assert!(world.set_game_rule(&MOB_EXPLOSION_DROP_DECAY, false));
    assert!(world.set_game_rule(&TNT_EXPLOSION_DROP_DECAY, true));
    assert_eq!(
        world.resolve_block_interaction(ExplosionInteraction::Block),
        BlockInteraction::Destroy
    );
    assert_eq!(
        world.resolve_block_interaction(ExplosionInteraction::Mob),
        BlockInteraction::Destroy
    );
    assert_eq!(
        world.resolve_block_interaction(ExplosionInteraction::Tnt),
        BlockInteraction::DestroyWithDecay
    );

    assert!(world.set_game_rule(&MOB_GRIEFING, false));
    assert_eq!(
        world.resolve_block_interaction(ExplosionInteraction::Mob),
        BlockInteraction::Keep
    );
}

#[test]
fn block_and_fluid_resistance_use_the_vanilla_maximum() {
    init_vanilla_registry();
    init_behaviors();
    let waterlogged_fence = vanilla_blocks::OAK_FENCE
        .default_state()
        .set_value(&BlockStateProperties::WATERLOGGED, true);
    let fluid = waterlogged_fence.get_fluid_state();
    let resistance = default_block_explosion_resistance(waterlogged_fence, fluid);
    let fluid_resistance = FLUID_BEHAVIORS
        .get_behavior(fluid.fluid_id)
        .explosion_resistance();

    assert_eq!(resistance, Some(fluid_resistance));
    assert!(fluid_resistance > waterlogged_fence.get_block().config.explosion_resistance);
    assert_eq!(
        default_block_explosion_resistance(
            vanilla_blocks::OBSIDIAN.default_state(),
            vanilla_blocks::OBSIDIAN.default_state().get_fluid_state(),
        ),
        Some(vanilla_blocks::OBSIDIAN.config.explosion_resistance)
    );
}

#[test]
fn source_and_custom_calculator_hooks_run_on_the_sequential_lane() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("explosion_calculator_hooks");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let center = TEST_BLOCK_CENTER;
    assert!(world.set_block(
        BlockPos::from(center),
        vanilla_blocks::STONE.default_state(),
        UpdateFlags::UPDATE_NONE,
    ));
    let source = VetoExplosionSource {
        base: EntityBase::new(
            next_entity_id(),
            center,
            vanilla_entities::ITEM.dimensions,
            Arc::downgrade(&world),
        ),
        resistance_calls: AtomicUsize::new(0),
        decision_calls: AtomicUsize::new(0),
    };
    let source_explosion = ServerExplosion::new(
        &world,
        Some(&source),
        None,
        None,
        None,
        center,
        VANILLA_SMALL_EXPLOSION_RADIUS,
        DOES_NOT_CREATE_FIRE,
        BlockInteraction::Destroy,
    );

    let source_affected = source_explosion.calculate_exploded_positions(|| FIXED_RAY_RANDOM_SAMPLE);

    assert!(source_affected.is_empty());
    assert!(source.resistance_calls.load(Ordering::Relaxed) > 0);
    assert!(source.decision_calls.load(Ordering::Relaxed) > 0);

    let custom = VetoCustomCalculator::default();
    let custom_explosion = ServerExplosion::new(
        &world,
        None,
        None,
        Some(&custom),
        None,
        center,
        VANILLA_SMALL_EXPLOSION_RADIUS,
        DOES_NOT_CREATE_FIRE,
        BlockInteraction::Destroy,
    );
    let custom_affected = custom_explosion.calculate_exploded_positions(|| FIXED_RAY_RANDOM_SAMPLE);

    assert!(custom_affected.is_empty());
    assert!(custom.resistance_calls.load(Ordering::Relaxed) > RAY_COUNT);
    assert!(custom.decision_calls.load(Ordering::Relaxed) > RAY_COUNT);
}

fn assert_exposure_matches_seen_percent(world: &World, center: DVec3, entity: &dyn Entity) -> f32 {
    let exposure = EntityExplosionExposure::capture(entity);
    let live = exposure.calculate_uncached(world, center);
    assert_eq!(
        seen_percent(world, center, entity).to_bits(),
        live.to_bits()
    );
    live
}

#[test]
fn cached_exposure_raycast_matches_clear_partial_and_blocked_paths() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("cached_explosion_exposure");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let center = TEST_BLOCK_CENTER;
    let player = TestPlayerBuilder::new(Arc::clone(&world), "Cached", next_entity_id()).build();
    player
        .base()
        .set_position_local(TEST_BLOCK_BOTTOM_CENTER + DVec3::X * NEAR_EXPOSURE_TARGET_DISTANCE);

    let compare = || {
        let exposure = EntityExplosionExposure::capture(player.as_ref());
        let uncached = exposure.calculate_uncached(world.as_ref(), center);
        let mut raycast = ExplosionExposureRaycast::new(world.as_ref(), exposure.collision_context);
        let clear_grid_min = BlockPos::new(0, 63, 0);
        let clear_grid_max = BlockPos::new(4, 67, 1);
        raycast.configure_clear_grid(clear_grid_min, clear_grid_max);
        let cached = exposure.calculate_with_visibility(|from| raycast.is_path_clear(from, center));
        assert_eq!(cached.to_bits(), uncached.to_bits());
        (cached, raycast.stats())
    };

    let (clear, stats) = compare();
    assert_eq!(clear.to_bits(), FULL_EXPOSURE.to_bits());
    assert!(
        stats.cache_hits + stats.clear_grid_hits > 0,
        "stats={stats:?}"
    );
    assert!(
        stats.state_lookups * 2 < stats.block_visits,
        "stats={stats:?}"
    );
    assert!(
        stats.collision_lookups < stats.block_visits,
        "stats={stats:?}"
    );

    assert!(world.set_block(
        TEST_WALL_POS,
        vanilla_blocks::STONE_SLAB.default_state(),
        UpdateFlags::UPDATE_NONE,
    ));
    let (partial, _) = compare();
    assert!(partial > 0.0 && partial < 1.0, "partial exposure={partial}");

    assert!(world.set_block(
        TEST_WALL_POS,
        vanilla_blocks::STONE.default_state(),
        UpdateFlags::UPDATE_NONE,
    ));
    let (blocked, _) = compare();
    assert_eq!(blocked.to_bits(), NO_EXPOSURE.to_bits());
}

#[test]
fn exposure_cache_reuses_static_shapes_across_identical_entity_samples() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("shared_cached_explosion_exposure");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let center = TEST_BLOCK_CENTER;
    let entity = ItemEntity::new(
        &vanilla_entities::ITEM,
        next_entity_id(),
        TEST_BLOCK_BOTTOM_CENTER + DVec3::X * FAR_EXPOSURE_TARGET_DISTANCE,
        Arc::downgrade(&world),
    );
    let exposure = EntityExplosionExposure::capture(&entity);
    let expected = exposure.calculate_uncached(world.as_ref(), center);
    let mut raycast = ExplosionExposureRaycast::new(world.as_ref(), exposure.collision_context);

    let first = exposure.calculate_cached_with(&mut raycast, center);
    let after_first = raycast.stats();
    let second = exposure.calculate_cached_with(&mut raycast, center);
    let after_second = raycast.stats();

    assert_eq!(first.to_bits(), expected.to_bits());
    assert_eq!(second.to_bits(), expected.to_bits());
    assert!(after_first.state_lookups > 0, "stats={after_first:?}");
    assert_eq!(
        after_second.state_lookups, after_first.state_lookups,
        "the second identical exposure should be served entirely by retained static shapes"
    );
}

#[test]
fn dense_exposure_grid_skips_repeated_static_empty_shape_resolution() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("dense_explosion_exposure");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let center = TEST_BLOCK_CENTER;
    let entity = ItemEntity::new(
        &vanilla_entities::ITEM,
        next_entity_id(),
        TEST_BLOCK_BOTTOM_CENTER + DVec3::X * FAR_EXPOSURE_TARGET_DISTANCE,
        Arc::downgrade(&world),
    );
    let exposure = EntityExplosionExposure::capture(&entity);
    let expected = exposure.calculate_uncached(world.as_ref(), center);
    let mut raycast = ExplosionExposureRaycast::new(world.as_ref(), exposure.collision_context);
    let clear_grid_min = BlockPos::new(0, 63, 0);
    let clear_grid_max = BlockPos::new(8, 66, 1);
    raycast.configure_clear_grid(clear_grid_min, clear_grid_max);

    let first = exposure.calculate_cached_with(&mut raycast, center);
    let after_first = raycast.stats();
    let second = exposure.calculate_cached_with(&mut raycast, center);
    let after_second = raycast.stats();

    assert_eq!(first.to_bits(), expected.to_bits());
    assert_eq!(second.to_bits(), expected.to_bits());
    assert!(after_first.clear_grid_hits > 0, "stats={after_first:?}");
    assert!(
        after_first.clear_grid_resolutions > 0,
        "stats={after_first:?}"
    );
    assert_eq!(
        after_second.state_lookups, after_first.state_lookups,
        "the second exposure should reuse every stable empty classification"
    );
    assert_eq!(
        after_second.collision_lookups, after_first.collision_lookups,
        "stable empty positions should not re-enter exact shape resolution"
    );

    let newly_blocked_pos = TEST_WALL_POS.offset(2, 0, 0);
    assert!(world.set_block(
        newly_blocked_pos,
        vanilla_blocks::STONE.default_state(),
        UpdateFlags::UPDATE_NONE,
    ));
    raycast.clear();
    let expected_blocked = exposure.calculate_uncached(world.as_ref(), center);
    let blocked = exposure.calculate_cached_with(&mut raycast, center);
    let after_blocked = raycast.stats();
    assert_eq!(blocked.to_bits(), expected_blocked.to_bits());
    assert!(
        after_blocked.collision_lookups > after_second.collision_lookups,
        "static non-empty positions must retain exact shape clipping: {after_blocked:?}"
    );
}

#[test]
fn cached_exposure_matches_across_chunk_and_section_boundaries() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("cached_exposure_boundaries");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    insert_ready_full_chunk(&world, ChunkPos::new(1, 0));
    let occluding_block_pos = BlockPos::new(15, 79, 0);
    let entity_position = DVec3::new(16.5, 80.0, 0.5);
    let explosion_center = DVec3::new(14.5, 78.5, 0.5);
    assert!(world.set_block(
        occluding_block_pos,
        vanilla_blocks::STONE_SLAB.default_state(),
        UpdateFlags::UPDATE_NONE,
    ));
    let entity = ItemEntity::new(
        &vanilla_entities::ITEM,
        next_entity_id(),
        entity_position,
        Arc::downgrade(&world),
    );

    assert_exposure_matches_seen_percent(&world, explosion_center, &entity);
}

#[test]
fn exposure_cache_does_not_retain_air_from_a_missing_chunk() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("explosion_exposure_missing_chunk");
    let from = TEST_BLOCK_CENTER + DVec3::X * NEAR_EXPOSURE_TARGET_DISTANCE;
    let to = TEST_BLOCK_CENTER;
    let mut raycast = ExplosionExposureRaycast::new(world.as_ref(), BlockCollisionContext::empty());
    let clear_grid_min = BlockPos::new(0, 64, 0);
    let clear_grid_max = BlockPos::new(2, 64, 0);
    raycast.configure_clear_grid(clear_grid_min, clear_grid_max);

    assert!(raycast.is_path_clear(from, to));
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    assert!(world.set_block(
        TEST_WALL_POS,
        vanilla_blocks::STONE.default_state(),
        UpdateFlags::UPDATE_NONE,
    ));
    assert!(!raycast.is_path_clear(from, to));
}

#[test]
fn exposure_clipping_uses_the_entity_collision_context() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("explosion_entity_collision_context");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let powder_snow_pos = TEST_WALL_POS;
    assert!(world.set_block(
        powder_snow_pos,
        vanilla_blocks::POWDER_SNOW.default_state(),
        UpdateFlags::UPDATE_ALL,
    ));
    let entity = ItemEntity::new(
        &vanilla_entities::ITEM,
        next_entity_id(),
        TEST_BLOCK_BOTTOM_CENTER + DVec3::X * NEAR_EXPOSURE_TARGET_DISTANCE,
        Arc::downgrade(&world),
    );
    let center = TEST_LOW_EXPLOSION_CENTER;

    let walking_exposure = EntityExplosionExposure::capture(&entity);
    let mut shared_raycast =
        ExplosionExposureRaycast::new(world.as_ref(), walking_exposure.collision_context);
    let clear_grid_min = BlockPos::new(0, 63, 0);
    let clear_grid_max = BlockPos::new(3, 66, 1);
    shared_raycast.configure_clear_grid(clear_grid_min, clear_grid_max);
    let walking = walking_exposure.calculate_cached_with(&mut shared_raycast, center);
    let after_walking = shared_raycast.stats();
    assert_eq!(walking.to_bits(), FULL_EXPOSURE.to_bits());
    assert_eq!(
        walking.to_bits(),
        walking_exposure
            .calculate_uncached(world.as_ref(), center)
            .to_bits()
    );

    let descending_fall_distance = 3.0;
    entity.set_fall_distance(descending_fall_distance);
    entity.set_shared_shift_key_down(true);
    let falling_exposure = EntityExplosionExposure::capture(&entity);
    assert!(falling_exposure.collision_context.is_descending());
    let falling = falling_exposure.calculate_cached_with(&mut shared_raycast, center);
    let after_falling = shared_raycast.stats();
    assert_eq!(falling.to_bits(), NO_EXPOSURE.to_bits());
    assert_eq!(
        falling.to_bits(),
        falling_exposure
            .calculate_uncached(world.as_ref(), center)
            .to_bits()
    );
    assert!(
        after_falling.state_lookups > after_walking.state_lookups,
        "dynamic powder-snow state must remain live: before={after_walking:?}, after={after_falling:?}"
    );
}

#[test]
fn exposure_cache_is_cleared_before_block_mutating_entity_callbacks() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("explosion_exposure_callback_mutation");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let wall_pos = TEST_WALL_POS;
    let position = TEST_BLOCK_BOTTOM_CENTER + DVec3::X * NEAR_EXPOSURE_TARGET_DISTANCE;
    let mutator_places_wall_on_hit = true;
    let mutator = Arc::new(BlockMutatingExposureEntity::new(
        next_entity_id(),
        position,
        &world,
        wall_pos,
        mutator_places_wall_on_hit,
    ));
    let observer_places_wall_on_hit = false;
    let observer = Arc::new(BlockMutatingExposureEntity::new(
        next_entity_id(),
        position,
        &world,
        wall_pos,
        observer_places_wall_on_hit,
    ));
    let mutator_entity: SharedEntity = mutator.clone();
    let observer_entity: SharedEntity = observer.clone();
    assert!(world.try_add_entity(mutator_entity).is_ok());
    assert!(world.try_add_entity(observer_entity).is_ok());

    world.explode(ExplosionOptions::new(
        TEST_LOW_EXPLOSION_CENTER,
        ENTITY_EFFECT_TEST_RADIUS,
        ExplosionInteraction::None,
    ));

    assert_eq!(
        world.get_block_state(wall_pos),
        vanilla_blocks::STONE.default_state()
    );
    assert!(mutator.velocity().x > 0.0);
    assert_eq!(observer.velocity(), DVec3::ZERO);
}

#[test]
fn moving_piston_exposure_uses_live_block_entities() {
    init_vanilla_registry();
    init_behaviors();
    init_block_entities();
    let world = fresh_test_world("moving_piston_explosion_exposure");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let piston_pos = TEST_WALL_POS;
    let moving_state = vanilla_blocks::MOVING_PISTON
        .default_state()
        .set_value(&BlockStateProperties::FACING, Direction::East)
        .set_value(&BlockStateProperties::PISTON_TYPE, PistonType::Normal);
    let moved_state = vanilla_blocks::PISTON
        .default_state()
        .set_value(&BlockStateProperties::FACING, Direction::East)
        .set_value(&BlockStateProperties::EXTENDED, true);
    let is_extending = false;
    let is_source_piston = true;
    assert!(world.set_block(piston_pos, moving_state, UpdateFlags::UPDATE_NONE));
    let block_entity: SharedBlockEntity = Arc::new(PistonMovingBlockEntity::new_moving(
        Arc::downgrade(&world),
        piston_pos,
        moving_state,
        moved_state,
        Direction::East,
        is_extending,
        is_source_piston,
    ));
    assert!(world.set_block_entity(block_entity));
    let center = TEST_LOW_EXPLOSION_CENTER;
    let entity = ItemEntity::new(
        &vanilla_entities::ITEM,
        next_entity_id(),
        TEST_BLOCK_BOTTOM_CENTER + DVec3::X * NEAR_EXPOSURE_TARGET_DISTANCE,
        Arc::downgrade(&world),
    );
    let exposure = EntityExplosionExposure::capture(&entity);
    let live = exposure.calculate_uncached(world.as_ref(), center);
    assert!(exposure.sample_positions().into_iter().any(|from| {
        !world.is_block_collision_path_clear(from, center, exposure.collision_context)
    }));
    let mut raycast = ExplosionExposureRaycast::new(world.as_ref(), exposure.collision_context);
    let clear_grid_min = BlockPos::new(0, 63, 0);
    let clear_grid_max = BlockPos::new(3, 66, 1);
    raycast.configure_clear_grid(clear_grid_min, clear_grid_max);
    assert_eq!(
        exposure
            .calculate_cached_with(&mut raycast, center)
            .to_bits(),
        live.to_bits()
    );
}

#[test]
fn explosion_block_entity_context_preserves_the_live_loot_type() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("explosion_block_entity_loot_context");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let pos = TEST_WALL_POS;
    let moving_state = vanilla_blocks::MOVING_PISTON
        .default_state()
        .set_value(&BlockStateProperties::FACING, Direction::East)
        .set_value(&BlockStateProperties::PISTON_TYPE, PistonType::Normal);
    let moved_state = vanilla_blocks::PISTON
        .default_state()
        .set_value(&BlockStateProperties::FACING, Direction::East)
        .set_value(&BlockStateProperties::EXTENDED, true);
    let is_extending = false;
    let is_source_piston = true;
    let block_entity: SharedBlockEntity = Arc::new(PistonMovingBlockEntity::new_moving(
        Arc::downgrade(&world),
        pos,
        moving_state,
        moved_state,
        Direction::East,
        is_extending,
        is_source_piston,
    ));
    assert!(world.set_block(pos, moving_state, UpdateFlags::UPDATE_NONE));
    assert!(world.set_block_entity(block_entity));
    let block_entity = world
        .get_block_entity(pos)
        .expect("the moving-piston block entity should be live at the loot position");
    let context = BlockLootContext::new(&world, pos).with_block_entity(Some(&block_entity));
    let mut rng = rand::rng();
    let loot_context = context.create_loot_context(moving_state, &mut rng);
    let block_entity_ref = loot_context
        .block_entity
        .expect("the live block entity should reach loot evaluation");

    assert_eq!(loot_context.origin, Some(TEST_WALL_POS.get_center()));
    assert_eq!(
        block_entity_ref.block_entity_type,
        Some(&vanilla_block_entity_types::PISTON.key)
    );
    assert!(block_entity_ref.custom_name.is_none());
    assert!(block_entity_ref.inventory.is_none());
}

#[test]
fn explosion_applies_damage_and_impulse_to_nearby_entities() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("explosion_entity_effects");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let item = Arc::new(ItemEntity::new(
        &vanilla_entities::ITEM,
        next_entity_id(),
        TEST_BLOCK_BOTTOM_CENTER + DVec3::X,
        Arc::downgrade(&world),
    ));
    item.set_item(ItemStack::new(&vanilla_items::STONE));
    let entity: SharedEntity = item.clone();
    let Ok(()) = world.try_add_entity(entity) else {
        panic!("test item must be added to its loaded chunk");
    };

    let ExplosionOutcome {
        affected_block_count: _,
    } = world.explode(ExplosionOptions::new(
        TEST_BLOCK_BOTTOM_CENTER,
        ENTITY_EFFECT_TEST_RADIUS,
        ExplosionInteraction::None,
    ));

    assert!(item.is_removed());
    assert!(item.velocity().x > 0.0);
}

#[test]
fn non_destructive_explosion_ignores_items_when_mob_griefing_is_disabled() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("explosion_ignores_blocklike_entities");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    assert!(world.set_game_rule(&MOB_GRIEFING, false));
    let item = Arc::new(ItemEntity::new(
        &vanilla_entities::ITEM,
        next_entity_id(),
        TEST_BLOCK_BOTTOM_CENTER + DVec3::X,
        Arc::downgrade(&world),
    ));
    item.set_item(ItemStack::new(&vanilla_items::STONE));
    let entity: SharedEntity = item.clone();
    let Ok(()) = world.try_add_entity(entity) else {
        panic!("test item must be added to its loaded chunk");
    };

    let initial_health = item.get_health();
    world.explode(ExplosionOptions::new(
        TEST_BLOCK_BOTTOM_CENTER,
        ENTITY_EFFECT_TEST_RADIUS,
        ExplosionInteraction::None,
    ));

    assert!(!item.is_removed());
    assert_eq!(item.get_health(), initial_health);
    assert_eq!(item.velocity(), DVec3::ZERO);
}

#[test]
fn mob_explosion_does_not_push_vehicles_when_mob_griefing_is_disabled() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("explosion_ignores_vehicles_without_mob_griefing");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    assert!(world.set_game_rule(&MOB_GRIEFING, false));
    let minecart = Arc::new(ChestMinecartEntity::new(
        &vanilla_entities::CHEST_MINECART,
        next_entity_id(),
        TEST_BLOCK_BOTTOM_CENTER + DVec3::X,
        Arc::downgrade(&world),
    ));
    let entity: SharedEntity = minecart.clone();
    let Ok(()) = world.try_add_entity(entity) else {
        panic!("test chest minecart must be added to its loaded chunk");
    };
    let pig = PigEntity::new(
        &vanilla_entities::PIG,
        next_entity_id(),
        TEST_BLOCK_BOTTOM_CENTER,
        Arc::downgrade(&world),
    );
    let mut options = ExplosionOptions::new(
        TEST_BLOCK_BOTTOM_CENTER,
        ENTITY_EFFECT_TEST_RADIUS,
        ExplosionInteraction::Mob,
    );
    options.source = Some(&pig);

    world.explode(options);

    assert_eq!(minecart.velocity(), DVec3::ZERO);
}

#[test]
fn non_blocklike_explosion_does_not_push_block_attached_entities() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("explosion_ignores_block_attached_entities");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    assert!(world.set_game_rule(&MOB_GRIEFING, false));
    let (item_frame, leash_knot) = add_block_attached_targets(&world);

    world.explode(ExplosionOptions::new(
        TEST_BLOCK_BOTTOM_CENTER,
        ENTITY_EFFECT_TEST_RADIUS,
        ExplosionInteraction::None,
    ));

    assert_eq!(item_frame.velocity(), DVec3::ZERO);
    assert_eq!(leash_knot.velocity(), DVec3::ZERO);
}

#[test]
fn submerged_source_explosion_does_not_push_block_attached_entities() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("submerged_explosion_ignores_block_attached_entities");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let source = ItemEntity::new(
        &vanilla_entities::ITEM,
        next_entity_id(),
        TEST_BLOCK_BOTTOM_CENTER,
        Arc::downgrade(&world),
    );
    let water_height = 1.0;
    let lava_height = 0.0;
    let eye_in_water = false;
    let eye_in_lava = false;
    source
        .base()
        .set_fluid_contact(EntityFluidContact::from_parts(
            water_height,
            lava_height,
            eye_in_water,
            eye_in_lava,
        ));
    assert!(source.is_in_water());
    let (item_frame, leash_knot) = add_block_attached_targets(&world);
    let mut options = ExplosionOptions::new(
        TEST_BLOCK_BOTTOM_CENTER,
        ENTITY_EFFECT_TEST_RADIUS,
        ExplosionInteraction::Block,
    );
    options.source = Some(&source);

    world.explode(options);

    assert_eq!(item_frame.velocity(), DVec3::ZERO);
    assert_eq!(leash_knot.velocity(), DVec3::ZERO);
}

fn add_block_attached_targets(
    world: &Arc<World>,
) -> (Arc<ItemFrameEntity>, Arc<LeashFenceKnotEntity>) {
    let item_frame = Arc::new(ItemFrameEntity::new_attached(
        &vanilla_entities::ITEM_FRAME,
        next_entity_id(),
        TEST_WALL_POS,
        Direction::West,
        Arc::downgrade(world),
    ));
    let item_frame_entity: SharedEntity = item_frame.clone();
    let Ok(()) = world.try_add_entity(item_frame_entity) else {
        panic!("test item frame must be added to its loaded chunk");
    };
    let leash_knot = Arc::new(LeashFenceKnotEntity::new_attached(
        &vanilla_entities::LEASH_KNOT,
        next_entity_id(),
        TEST_LEASH_POS,
        Arc::downgrade(world),
    ));
    let leash_knot_entity: SharedEntity = leash_knot.clone();
    let Ok(()) = world.try_add_entity(leash_knot_entity) else {
        panic!("test leash knot must be added to its loaded chunk");
    };
    (item_frame, leash_knot)
}

#[test]
fn destructive_explosion_removes_stone_and_spawns_its_loot() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("explosion_block_destruction");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let center_pos = BlockPos::new(0, 64, 0);
    assert!(world.set_block(
        center_pos,
        vanilla_blocks::STONE.default_state(),
        UpdateFlags::UPDATE_ALL,
    ));
    let mut explosion = ServerExplosion::new(
        &world,
        None,
        None,
        None,
        None,
        TEST_BLOCK_CENTER,
        STANDARD_TNT_EXPLOSION_POWER,
        DOES_NOT_CREATE_FIRE,
        BlockInteraction::Destroy,
    );

    explosion.explode();

    assert!(world.get_block_state(center_pos).is_air());
    let drop_search_bounds = WorldAabb::new(-1.0, 63.0, -1.0, 2.0, 67.0, 2.0);
    let drops = world.get_entities_in_aabb_matching(&drop_search_bounds, |entity| {
        entity.entity_type() == &vanilla_entities::ITEM
    });
    assert!(drops.iter().any(|entity| {
        entity
            .as_ref()
            .downcast_ref::<ItemEntity>()
            .is_some_and(|item| item.get_item().is(&vanilla_items::COBBLESTONE))
    }));
}

#[test]
fn combined_explosion_drops_never_exceed_vanilla_stack_limit() {
    const INPUT_STACK_SIZE: i32 = 10;
    const EXPECTED_STACK_COUNT: usize = 2;

    init_vanilla_registry();
    let stack = ItemStack::with_count(&vanilla_items::STONE, INPUT_STACK_SIZE);
    let mut stacks = Vec::new();

    add_or_append_stack(&mut stacks, stack.clone(), BlockPos::ZERO);
    add_or_append_stack(&mut stacks, stack, BlockPos::ZERO);

    assert_eq!(stacks.len(), EXPECTED_STACK_COUNT);
    assert_eq!(stacks[0].stack.count(), VANILLA_COMBINED_DROP_STACK_LIMIT);
    assert_eq!(
        stacks[1].stack.count(),
        INPUT_STACK_SIZE * 2 - VANILLA_COMBINED_DROP_STACK_LIMIT
    );
}
