use std::sync::{
    Arc, Weak,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

use glam::DVec3;
use steel_protocol::packet_traits::{CompressionInfo, EncodedPacket};
use steel_registry::blocks::properties::{
    AttachFace, BlockStateProperties, DoubleBlockHalf, PistonType,
};
use steel_registry::{
    entity_type::EntityTypeRef,
    fluid::FluidState,
    init_vanilla_registry, vanilla_blocks, vanilla_damage_types, vanilla_entities,
    vanilla_game_rules::{
        BLOCK_EXPLOSION_DROP_DECAY, MOB_EXPLOSION_DROP_DECAY, MOB_GRIEFING,
        TNT_EXPLOSION_DROP_DECAY,
    },
    vanilla_items,
};
use steel_utils::locks::SyncMutex;
use steel_utils::random::{Random, legacy_random::LegacyRandom};
use steel_utils::types::{GameType, UpdateFlags};
use steel_utils::{
    BlockPos, BlockStateId, ChunkPos, Direction, Downcast as _, DowncastType, DowncastTypeKey,
    PackedBlockPos,
};
use text_components::TextComponent;
use uuid::Uuid;

use super::*;
use crate::behavior::{FLUID_BEHAVIORS, init_behaviors};
use crate::block_entity::{
    SharedBlockEntity, entities::PistonMovingBlockEntity, init_block_entities,
};
use crate::entity::entities::{
    ChestMinecartEntity, ItemFrameEntity, LeashFenceKnotEntity, PigEntity, PrimedTntEntity,
};
use crate::entity::{EntityBase, EntityFluidContact, LivingEntity as _};
use crate::player::connection::NetworkConnection;
use crate::player::{Player, PlayerConnection, ResetReason};
use crate::test_support::{TestPlayerBuilder, fresh_test_world, insert_ready_full_chunk};
use crate::world::explosion::default_block_explosion_resistance;
use crate::world::{
    DefaultExplosionDamageCalculator, ExplosionDamageCalculator, ExplosionInteraction,
    ExplosionOptions, ExplosionOutcome,
};

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

#[derive(Default)]
struct CountingImmutableCalculator {
    resistance_calls: AtomicUsize,
    decision_calls: AtomicUsize,
    cache_resistance: bool,
    always_allows_block_explosion: bool,
    bounded_read_radius: Option<u32>,
}

impl ImmutableExplosionBlockCalculator for CountingImmutableCalculator {
    fn bounded_block_read_radius(&self) -> Option<u32> {
        self.bounded_read_radius
    }

    fn can_cache_explosion_resistance(&self) -> bool {
        self.cache_resistance
    }

    fn always_allows_block_explosion(&self) -> bool {
        self.always_allows_block_explosion
    }

    fn explosion_resistance(
        &self,
        _reader: &dyn ExplosionBlockReader,
        _pos: BlockPos,
        state: BlockStateId,
        fluid: FluidState,
    ) -> Option<f32> {
        self.resistance_calls.fetch_add(1, Ordering::Relaxed);
        default_block_explosion_resistance(state, fluid)
    }

    fn should_explode(
        &self,
        _reader: &dyn ExplosionBlockReader,
        _pos: BlockPos,
        _state: BlockStateId,
        _power: f32,
    ) -> bool {
        self.decision_calls.fetch_add(1, Ordering::Relaxed);
        true
    }
}

struct CountingBlockReader<'a> {
    world: &'a World,
    calls: AtomicUsize,
}

impl ExplosionBlockReader for CountingBlockReader<'_> {
    fn block_state(&self, pos: BlockPos) -> Option<BlockStateId> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        Some(self.world.get_block_state(pos))
    }
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
    entity_id: i32,
    uuid: Uuid,
    name: &'static str,
    position: DVec3,
) -> (Arc<Player>, Arc<SyncMutex<Vec<EncodedPacket>>>) {
    let packets = Arc::new(SyncMutex::new(Vec::new()));
    let connection = Arc::new(PlayerConnection::Other(Box::new(RecordingConnection {
        packets: Arc::clone(&packets),
        closed: AtomicBool::new(false),
    })));
    let player = TestPlayerBuilder::new(Arc::clone(world), uuid, name, entity_id)
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
            4.0
        }

        fn center(&self) -> DVec3 {
            DVec3::ZERO
        }

        fn can_trigger_blocks(&self) -> bool {
            false
        }

        fn should_affect_blocklike_entities(&self) -> bool {
            false
        }
    }

    init_vanilla_registry();
    let entity = ItemEntity::new(
        &vanilla_entities::ITEM,
        1,
        DVec3::new(4.0, 0.0, 0.0),
        Weak::new(),
    );
    let damage =
        DefaultExplosionDamageCalculator.entity_damage_amount(&TestExplosion, &entity, 1.0);
    assert_eq!(damage.to_bits(), 22.0_f32.to_bits());
}

#[test]
fn default_damage_source_preserves_direct_source_position() {
    init_vanilla_registry();
    let position = DVec3::new(1.25, 64.0, -3.5);
    let entity = ItemEntity::new(&vanilla_entities::ITEM, 17, position, Weak::new());

    let source = default_explosion_damage_source(Some(&entity), None);

    assert_eq!(source.direct_entity_id, Some(entity.id()));
    assert_eq!(source.causing_entity_id, None);
    assert_eq!(source.source_position, Some(position));
    assert_eq!(source.damage_type.key, vanilla_damage_types::EXPLOSION.key);
}

#[test]
fn primed_tnt_damage_source_preserves_direct_and_owner_attribution() {
    init_vanilla_registry();
    let world = fresh_test_world("explosion_tnt_damage_attribution");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let player =
        TestPlayerBuilder::new(Arc::clone(&world), Uuid::from_u128(0xD4A6), "Owner", 72).build();
    assert!(world.add_player(Arc::clone(&player), ResetReason::InitialJoin));
    let tnt = PrimedTntEntity::primed(
        &vanilla_entities::TNT,
        73,
        DVec3::new(0.5, 64.0, 0.5),
        &world,
        Some(player.as_ref()),
    );

    let explosion = ServerExplosion::new(
        &world,
        Some(&tnt),
        None,
        None,
        None,
        DVec3::new(0.5, 64.0, 0.5),
        4.0,
        false,
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
    let survival =
        TestPlayerBuilder::new(Arc::clone(&world), Uuid::from_u128(0x5101), "Survival", 74).build();
    survival
        .base()
        .set_position_local(DVec3::new(1.5, 64.0, 0.5));
    survival.set_client_loaded(true);
    assert!(world.add_player(Arc::clone(&survival), ResetReason::InitialJoin));
    let creative =
        TestPlayerBuilder::new(Arc::clone(&world), Uuid::from_u128(0xC8EA), "Creative", 75).build();
    creative
        .base()
        .set_position_local(DVec3::new(0.5, 64.0, 1.5));
    creative.restore_game_modes(GameType::Creative, None);
    creative
        .abilities
        .lock()
        .update_for_game_mode(GameType::Creative);
    creative.abilities.lock().flying = true;
    creative.set_client_loaded(true);
    assert!(world.add_player(Arc::clone(&creative), ResetReason::InitialJoin));
    let spectator =
        TestPlayerBuilder::new(Arc::clone(&world), Uuid::from_u128(0x5EEC), "Spectator", 76)
            .build();
    spectator
        .base()
        .set_position_local(DVec3::new(-0.5, 64.0, 0.5));
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
        DVec3::new(0.5, 64.0, 0.5),
        2.0,
        false,
        BlockInteraction::Keep,
    );

    explosion.hurt_entities();

    let expected_damage =
        DefaultExplosionDamageCalculator.entity_damage_amount(&explosion, survival.as_ref(), 1.0);
    assert_eq!(
        survival.get_health().to_bits(),
        (20.0_f32 - expected_damage).to_bits()
    );
    let delta = survival.explosion_damage_origin() - explosion.center;
    let expected_knockback = delta / delta.length() * 0.75;
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
    let center = DVec3::new(0.5, 64.0, 0.5);
    insert_ready_full_chunk(&world, ChunkPos::from_block_pos(BlockPos::from(center)));
    insert_ready_full_chunk(&world, ChunkPos::new(4, 0));
    let (_near, near_packets) = recording_player(
        &world,
        77,
        Uuid::from_u128(0x64A),
        "Near",
        center + DVec3::new(63.999, 0.0, 0.0),
    );
    let (_boundary, boundary_packets) = recording_player(
        &world,
        78,
        Uuid::from_u128(0x64B),
        "Boundary",
        center + DVec3::new(64.0, 0.0, 0.0),
    );
    near_packets.lock().clear();
    boundary_packets.lock().clear();

    world.explode(ExplosionOptions::new(
        center,
        -1.0,
        ExplosionInteraction::None,
    ));

    assert_eq!(near_packets.lock().len(), 1);
    assert!(boundary_packets.lock().is_empty());
}

#[test]
fn small_particle_selection_matches_radius_and_block_interaction() {
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
            false,
            interaction,
        )
    };

    assert!(explosion(1.999, BlockInteraction::Destroy).is_small());
    assert!(explosion(2.0, BlockInteraction::Keep).is_small());
    assert!(!explosion(2.0, BlockInteraction::Destroy).is_small());
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
        world.resolve_block_interaction(ExplosionInteraction::Trigger),
        BlockInteraction::TriggerBlock
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
    let center = DVec3::new(0.5, 64.5, 0.5);
    assert!(world.set_block(
        BlockPos::from(center),
        vanilla_blocks::STONE.default_state(),
        UpdateFlags::UPDATE_NONE,
    ));
    let source = VetoExplosionSource {
        base: EntityBase::new(
            71,
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
        2.0,
        false,
        BlockInteraction::Destroy,
    );

    let source_affected = source_explosion.calculate_exploded_positions(|| 0.5);

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
        2.0,
        false,
        BlockInteraction::Destroy,
    );
    let custom_affected = custom_explosion.calculate_exploded_positions(|| 0.5);

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
fn exposure_matches_sampler_for_clear_and_obstructed_paths() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("pinned_explosion_exposure");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let center = DVec3::new(0.5, 64.125, 0.5);
    let entity = ItemEntity::new(
        &vanilla_entities::ITEM,
        18,
        DVec3::new(2.5, 64.0, 0.5),
        Arc::downgrade(&world),
    );

    assert_eq!(
        assert_exposure_matches_seen_percent(&world, center, &entity).to_bits(),
        1.0_f32.to_bits()
    );

    assert!(world.set_block(
        BlockPos::new(1, 64, 0),
        vanilla_blocks::STONE.default_state(),
        UpdateFlags::UPDATE_ALL,
    ));
    assert_eq!(
        assert_exposure_matches_seen_percent(&world, center, &entity).to_bits(),
        0.0_f32.to_bits()
    );
}

#[test]
fn player_exposure_distinguishes_partial_and_full_occlusion() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("explosion_partial_exposure");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let center = DVec3::new(0.5, 64.5, 0.5);
    let player =
        TestPlayerBuilder::new(Arc::clone(&world), Uuid::from_u128(0x0CC1), "Occluded", 79).build();
    player.base().set_position_local(DVec3::new(2.5, 64.0, 0.5));

    assert_eq!(
        seen_percent(&world, center, player.as_ref()).to_bits(),
        1.0_f32.to_bits()
    );
    assert!(world.set_block(
        BlockPos::new(1, 64, 0),
        vanilla_blocks::STONE_SLAB.default_state(),
        UpdateFlags::UPDATE_NONE,
    ));
    let partial = seen_percent(&world, center, player.as_ref());
    assert!(partial > 0.0 && partial < 1.0, "partial exposure={partial}");
    assert!(world.set_block(
        BlockPos::new(1, 64, 0),
        vanilla_blocks::STONE.default_state(),
        UpdateFlags::UPDATE_NONE,
    ));
    assert_eq!(
        seen_percent(&world, center, player.as_ref()).to_bits(),
        0.0_f32.to_bits()
    );
}

#[test]
fn cached_exposure_raycast_matches_clear_partial_and_blocked_paths() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("cached_explosion_exposure");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let center = DVec3::new(0.5, 64.5, 0.5);
    let player = TestPlayerBuilder::new(
        Arc::clone(&world),
        Uuid::from_u128(0xCA_C4_ED),
        "Cached",
        80,
    )
    .build();
    player.base().set_position_local(DVec3::new(2.5, 64.0, 0.5));

    let compare = || {
        let exposure = EntityExplosionExposure::capture(player.as_ref());
        let uncached = exposure.calculate_uncached(world.as_ref(), center);
        let mut raycast = ExplosionExposureRaycast::new(world.as_ref(), exposure.collision_context);
        let cached = exposure.calculate_with_visibility(|from| raycast.is_path_clear(from, center));
        assert_eq!(cached.to_bits(), uncached.to_bits());
        (cached, raycast.stats())
    };

    let (clear, stats) = compare();
    assert_eq!(clear.to_bits(), 1.0_f32.to_bits());
    assert!(stats.cache_hits > 0, "stats={stats:?}");
    assert!(
        stats.state_lookups * 2 < stats.block_visits,
        "stats={stats:?}"
    );
    assert!(
        stats.collision_lookups < stats.block_visits,
        "stats={stats:?}"
    );

    assert!(world.set_block(
        BlockPos::new(1, 64, 0),
        vanilla_blocks::STONE_SLAB.default_state(),
        UpdateFlags::UPDATE_NONE,
    ));
    let (partial, _) = compare();
    assert!(partial > 0.0 && partial < 1.0, "partial exposure={partial}");

    assert!(world.set_block(
        BlockPos::new(1, 64, 0),
        vanilla_blocks::STONE.default_state(),
        UpdateFlags::UPDATE_NONE,
    ));
    let (blocked, _) = compare();
    assert_eq!(blocked.to_bits(), 0.0_f32.to_bits());
}

#[test]
fn exposure_cache_reuses_static_shapes_across_identical_entity_samples() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("shared_cached_explosion_exposure");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let center = DVec3::new(0.5, 64.5, 0.5);
    let entity = ItemEntity::new(
        &vanilla_entities::ITEM,
        81,
        DVec3::new(6.5, 64.0, 0.5),
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
fn cached_exposure_matches_across_chunk_and_section_boundaries() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("cached_exposure_boundaries");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    insert_ready_full_chunk(&world, ChunkPos::new(1, 0));
    assert!(world.set_block(
        BlockPos::new(15, 79, 0),
        vanilla_blocks::STONE_SLAB.default_state(),
        UpdateFlags::UPDATE_NONE,
    ));
    let entity = ItemEntity::new(
        &vanilla_entities::ITEM,
        81,
        DVec3::new(16.5, 80.0, 0.5),
        Arc::downgrade(&world),
    );

    assert_exposure_matches_seen_percent(&world, DVec3::new(14.5, 78.5, 0.5), &entity);
}

#[test]
fn exposure_cache_does_not_retain_air_from_a_missing_chunk() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("explosion_exposure_missing_chunk");
    let from = DVec3::new(2.5, 64.5, 0.5);
    let to = DVec3::new(0.5, 64.5, 0.5);
    let mut raycast = ExplosionExposureRaycast::new(world.as_ref(), BlockCollisionContext::empty());

    assert!(raycast.is_path_clear(from, to));
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    assert!(world.set_block(
        BlockPos::new(1, 64, 0),
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
    let powder_snow_pos = BlockPos::new(1, 64, 0);
    assert!(world.set_block(
        powder_snow_pos,
        vanilla_blocks::POWDER_SNOW.default_state(),
        UpdateFlags::UPDATE_ALL,
    ));
    let entity = ItemEntity::new(
        &vanilla_entities::ITEM,
        18,
        DVec3::new(2.5, 64.0, 0.5),
        Arc::downgrade(&world),
    );
    let center = DVec3::new(0.5, 64.125, 0.5);

    let walking_exposure = EntityExplosionExposure::capture(&entity);
    let mut shared_raycast =
        ExplosionExposureRaycast::new(world.as_ref(), walking_exposure.collision_context);
    let walking = walking_exposure.calculate_cached_with(&mut shared_raycast, center);
    assert_eq!(walking.to_bits(), 1.0_f32.to_bits());
    assert_eq!(
        walking.to_bits(),
        walking_exposure
            .calculate_uncached(world.as_ref(), center)
            .to_bits()
    );

    entity.set_fall_distance(3.0);
    entity.set_shared_shift_key_down(true);
    let falling_exposure = EntityExplosionExposure::capture(&entity);
    assert!(falling_exposure.collision_context.is_descending());
    let falling = falling_exposure.calculate_cached_with(&mut shared_raycast, center);
    assert_eq!(falling.to_bits(), 0.0_f32.to_bits());
    assert_eq!(
        falling.to_bits(),
        falling_exposure
            .calculate_uncached(world.as_ref(), center)
            .to_bits()
    );
}

#[test]
fn exposure_cache_is_cleared_before_block_mutating_entity_callbacks() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("explosion_exposure_callback_mutation");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let wall_pos = BlockPos::new(1, 64, 0);
    let position = DVec3::new(2.5, 64.0, 0.5);
    let mutator = Arc::new(BlockMutatingExposureEntity::new(
        82, position, &world, wall_pos, true,
    ));
    let observer = Arc::new(BlockMutatingExposureEntity::new(
        83, position, &world, wall_pos, false,
    ));
    let mutator_entity: SharedEntity = mutator.clone();
    let observer_entity: SharedEntity = observer.clone();
    assert!(world.try_add_entity(mutator_entity).is_ok());
    assert!(world.try_add_entity(observer_entity).is_ok());

    world.explode(ExplosionOptions::new(
        DVec3::new(0.5, 64.125, 0.5),
        2.0,
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
    let piston_pos = BlockPos::new(1, 64, 0);
    let moving_state = vanilla_blocks::MOVING_PISTON
        .default_state()
        .set_value(&BlockStateProperties::FACING, Direction::East)
        .set_value(&BlockStateProperties::PISTON_TYPE, PistonType::Normal);
    let moved_state = vanilla_blocks::PISTON
        .default_state()
        .set_value(&BlockStateProperties::FACING, Direction::East)
        .set_value(&BlockStateProperties::EXTENDED, true);
    assert!(world.set_block(piston_pos, moving_state, UpdateFlags::UPDATE_NONE));
    let block_entity: SharedBlockEntity = Arc::new(PistonMovingBlockEntity::new_moving(
        Arc::downgrade(&world),
        piston_pos,
        moving_state,
        moved_state,
        Direction::East,
        false,
        true,
    ));
    assert!(world.set_block_entity(block_entity));
    let center = DVec3::new(0.5, 64.125, 0.5);
    let entity = ItemEntity::new(
        &vanilla_entities::ITEM,
        20,
        DVec3::new(2.5, 64.0, 0.5),
        Arc::downgrade(&world),
    );
    let exposure = EntityExplosionExposure::capture(&entity);
    let live = exposure.calculate_uncached(world.as_ref(), center);
    assert!(exposure.sample_positions().into_iter().any(|from| {
        !world.is_block_collision_path_clear(from, center, exposure.collision_context)
    }));
    assert_eq!(
        seen_percent(&world, center, &entity).to_bits(),
        live.to_bits()
    );
}

#[test]
fn explosion_applies_damage_and_impulse_to_nearby_entities() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("explosion_entity_effects");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let item = Arc::new(ItemEntity::new(
        &vanilla_entities::ITEM,
        19,
        DVec3::new(1.5, 64.0, 0.5),
        Arc::downgrade(&world),
    ));
    item.set_item(ItemStack::with_count(&vanilla_items::STONE, 1));
    let entity: SharedEntity = item.clone();
    let Ok(()) = world.try_add_entity(entity) else {
        panic!("test item must be added to its loaded chunk");
    };

    let ExplosionOutcome {
        affected_block_count: _,
    } = world.explode(ExplosionOptions::new(
        DVec3::new(0.5, 64.0, 0.5),
        2.0,
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
        20,
        DVec3::new(1.5, 64.0, 0.5),
        Arc::downgrade(&world),
    ));
    item.set_item(ItemStack::with_count(&vanilla_items::STONE, 1));
    let entity: SharedEntity = item.clone();
    let Ok(()) = world.try_add_entity(entity) else {
        panic!("test item must be added to its loaded chunk");
    };

    world.explode(ExplosionOptions::new(
        DVec3::new(0.5, 64.0, 0.5),
        2.0,
        ExplosionInteraction::None,
    ));

    assert!(!item.is_removed());
    assert_eq!(item.get_health(), 5);
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
        21,
        DVec3::new(1.5, 64.0, 0.5),
        Arc::downgrade(&world),
    ));
    let entity: SharedEntity = minecart.clone();
    let Ok(()) = world.try_add_entity(entity) else {
        panic!("test chest minecart must be added to its loaded chunk");
    };
    let pig = PigEntity::new(
        &vanilla_entities::PIG,
        22,
        DVec3::new(0.5, 64.0, 0.5),
        Arc::downgrade(&world),
    );
    let mut options =
        ExplosionOptions::new(DVec3::new(0.5, 64.0, 0.5), 2.0, ExplosionInteraction::Mob);
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
    let (item_frame, leash_knot) = add_block_attached_targets(&world, 23);

    world.explode(ExplosionOptions::new(
        DVec3::new(0.5, 64.0, 0.5),
        2.0,
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
        25,
        DVec3::new(0.5, 64.0, 0.5),
        Arc::downgrade(&world),
    );
    source
        .base()
        .set_fluid_contact(EntityFluidContact::from_parts(1.0, 0.0, false, false));
    assert!(source.is_in_water());
    let (item_frame, leash_knot) = add_block_attached_targets(&world, 26);
    let mut options =
        ExplosionOptions::new(DVec3::new(0.5, 64.0, 0.5), 2.0, ExplosionInteraction::Block);
    options.source = Some(&source);

    world.explode(options);

    assert_eq!(item_frame.velocity(), DVec3::ZERO);
    assert_eq!(leash_knot.velocity(), DVec3::ZERO);
}

fn add_block_attached_targets(
    world: &Arc<World>,
    first_id: i32,
) -> (Arc<ItemFrameEntity>, Arc<LeashFenceKnotEntity>) {
    let item_frame = Arc::new(ItemFrameEntity::new_attached(
        &vanilla_entities::ITEM_FRAME,
        first_id,
        BlockPos::new(1, 64, 0),
        Direction::West,
        Arc::downgrade(world),
    ));
    let item_frame_entity: SharedEntity = item_frame.clone();
    let Ok(()) = world.try_add_entity(item_frame_entity) else {
        panic!("test item frame must be added to its loaded chunk");
    };
    let leash_knot = Arc::new(LeashFenceKnotEntity::new_attached(
        &vanilla_entities::LEASH_KNOT,
        first_id + 1,
        BlockPos::new(0, 64, 1),
        Arc::downgrade(world),
    ));
    let leash_knot_entity: SharedEntity = leash_knot.clone();
    let Ok(()) = world.try_add_entity(leash_knot_entity) else {
        panic!("test leash knot must be added to its loaded chunk");
    };
    (item_frame, leash_knot)
}

#[test]
fn trigger_explosion_activates_controls_without_destroying_them() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("trigger_explosion_controls");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let lever_pos = BlockPos::new(2, 64, 2);
    let button_pos = BlockPos::new(8, 64, 2);
    let fence_gate_pos = BlockPos::new(14, 64, 2);
    for pos in [lever_pos, button_pos] {
        assert!(world.set_block(
            pos.below(),
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
    }
    assert!(
        world.set_block(
            lever_pos,
            vanilla_blocks::LEVER
                .default_state()
                .set_value(&BlockStateProperties::ATTACH_FACE, AttachFace::Floor),
            UpdateFlags::UPDATE_NONE,
        )
    );
    assert!(
        world.set_block(
            button_pos,
            vanilla_blocks::STONE_BUTTON
                .default_state()
                .set_value(&BlockStateProperties::ATTACH_FACE, AttachFace::Floor),
            UpdateFlags::UPDATE_NONE,
        )
    );
    assert!(world.set_block(
        fence_gate_pos,
        vanilla_blocks::OAK_FENCE_GATE.default_state(),
        UpdateFlags::UPDATE_NONE,
    ));

    trigger_block_positions(&world, &mut [lever_pos, button_pos, fence_gate_pos]);

    let lever = world.get_block_state(lever_pos);
    assert_eq!(lever.get_block(), &vanilla_blocks::LEVER);
    assert!(lever.get_value(&BlockStateProperties::POWERED));
    let button = world.get_block_state(button_pos);
    assert_eq!(button.get_block(), &vanilla_blocks::STONE_BUTTON);
    assert!(button.get_value(&BlockStateProperties::POWERED));
    let fence_gate = world.get_block_state(fence_gate_pos);
    assert_eq!(fence_gate.get_block(), &vanilla_blocks::OAK_FENCE_GATE);
    assert!(fence_gate.get_value(&BlockStateProperties::OPEN));
    assert!(!fence_gate.get_value(&BlockStateProperties::POWERED));
}

#[test]
fn trigger_explosion_respects_door_and_trapdoor_wind_charge_rules() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("trigger_explosion_doors");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let oak_door_pos = BlockPos::new(1, 64, 1);
    let iron_door_pos = BlockPos::new(4, 64, 1);
    let powered_door_pos = BlockPos::new(7, 64, 1);
    let upper_door_pos = BlockPos::new(10, 64, 1);
    let copper_door_pos = BlockPos::new(13, 64, 1);
    let oak_trapdoor_pos = BlockPos::new(1, 64, 5);
    let iron_trapdoor_pos = BlockPos::new(4, 64, 5);
    let powered_trapdoor_pos = BlockPos::new(7, 64, 5);
    let copper_trapdoor_pos = BlockPos::new(10, 64, 5);
    for (pos, state) in [
        (oak_door_pos, vanilla_blocks::OAK_DOOR.default_state()),
        (iron_door_pos, vanilla_blocks::IRON_DOOR.default_state()),
        (
            powered_door_pos,
            vanilla_blocks::OAK_DOOR
                .default_state()
                .set_value(&BlockStateProperties::POWERED, true),
        ),
        (
            upper_door_pos,
            vanilla_blocks::OAK_DOOR.default_state().set_value(
                &BlockStateProperties::DOUBLE_BLOCK_HALF,
                DoubleBlockHalf::Upper,
            ),
        ),
        (copper_door_pos, vanilla_blocks::COPPER_DOOR.default_state()),
        (
            oak_trapdoor_pos,
            vanilla_blocks::OAK_TRAPDOOR.default_state(),
        ),
        (
            iron_trapdoor_pos,
            vanilla_blocks::IRON_TRAPDOOR.default_state(),
        ),
        (
            powered_trapdoor_pos,
            vanilla_blocks::OAK_TRAPDOOR
                .default_state()
                .set_value(&BlockStateProperties::POWERED, true),
        ),
        (
            copper_trapdoor_pos,
            vanilla_blocks::COPPER_TRAPDOOR.default_state(),
        ),
    ] {
        assert!(world.set_block(pos, state, UpdateFlags::UPDATE_NONE));
    }

    trigger_block_positions(
        &world,
        &mut [
            oak_door_pos,
            iron_door_pos,
            powered_door_pos,
            upper_door_pos,
            copper_door_pos,
            oak_trapdoor_pos,
            iron_trapdoor_pos,
            powered_trapdoor_pos,
            copper_trapdoor_pos,
        ],
    );

    for pos in [oak_door_pos, copper_door_pos] {
        assert!(
            world
                .get_block_state(pos)
                .get_value(&BlockStateProperties::OPEN)
        );
    }
    for pos in [iron_door_pos, powered_door_pos, upper_door_pos] {
        assert!(
            !world
                .get_block_state(pos)
                .get_value(&BlockStateProperties::OPEN)
        );
    }
    for pos in [oak_trapdoor_pos, copper_trapdoor_pos] {
        assert!(
            world
                .get_block_state(pos)
                .get_value(&BlockStateProperties::OPEN)
        );
    }
    for pos in [iron_trapdoor_pos, powered_trapdoor_pos] {
        assert!(
            !world
                .get_block_state(pos)
                .get_value(&BlockStateProperties::OPEN)
        );
    }
}

#[test]
fn trigger_explosion_extinguishes_candles_without_destroying_them() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("trigger_explosion_candles");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let candle_pos = BlockPos::new(2, 64, 2);
    let candle_cake_pos = BlockPos::new(6, 64, 2);
    for pos in [candle_pos, candle_cake_pos] {
        assert!(world.set_block(
            pos.below(),
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
    }
    assert!(
        world.set_block(
            candle_pos,
            vanilla_blocks::CANDLE
                .default_state()
                .set_value(&BlockStateProperties::LIT, true),
            UpdateFlags::UPDATE_NONE,
        )
    );
    assert!(
        world.set_block(
            candle_cake_pos,
            vanilla_blocks::CANDLE_CAKE
                .default_state()
                .set_value(&BlockStateProperties::LIT, true),
            UpdateFlags::UPDATE_NONE,
        )
    );

    trigger_block_positions(&world, &mut [candle_pos, candle_cake_pos]);

    let candle = world.get_block_state(candle_pos);
    assert_eq!(candle.get_block(), &vanilla_blocks::CANDLE);
    assert!(!candle.get_value(&BlockStateProperties::LIT));
    let candle_cake = world.get_block_state(candle_cake_pos);
    assert_eq!(candle_cake.get_block(), &vanilla_blocks::CANDLE_CAKE);
    assert!(!candle_cake.get_value(&BlockStateProperties::LIT));
}

fn trigger_block_positions(world: &Arc<World>, positions: &mut [BlockPos]) {
    let explosion = ServerExplosion::new(
        world,
        None,
        None,
        None,
        None,
        DVec3::ZERO,
        1.0,
        false,
        BlockInteraction::TriggerBlock,
    );
    explosion.interact_with_blocks(positions);
}

#[test]
fn resistant_center_block_stops_explosion_rays() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("explosion_ray_resistance");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let center_pos = BlockPos::new(0, 64, 0);
    assert!(world.set_block(
        center_pos,
        vanilla_blocks::OBSIDIAN.default_state(),
        UpdateFlags::UPDATE_ALL,
    ));
    let explosion = ServerExplosion::new(
        &world,
        None,
        None,
        None,
        None,
        DVec3::new(0.5, 64.5, 0.5),
        4.0,
        false,
        BlockInteraction::Destroy,
    );

    let affected = explosion.calculate_exploded_positions(|| 0.5);

    assert!(affected.is_empty());
}

#[test]
fn deterministic_empty_world_rays_match_the_java_hash_set_fixture() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("explosion_java_hash_set_fixture");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let explosion = ServerExplosion::new(
        &world,
        None,
        None,
        None,
        None,
        DVec3::new(0.5, 64.5, 0.5),
        1.0,
        false,
        BlockInteraction::Destroy,
    );
    let mut draws = 0;

    let affected = explosion.calculate_exploded_positions(|| {
        draws += 1;
        0.5
    });

    assert_eq!(draws, RAY_COUNT);
    assert_eq!(
        affected,
        [
            (0, 64, 0),
            (-1, 64, 1),
            (1, 64, -1),
            (0, 64, 1),
            (1, 64, 0),
            (1, 64, 1),
            (-1, 65, -1),
            (-1, 65, 0),
            (0, 65, -1),
            (-1, 63, -1),
            (-1, 65, 1),
            (0, 65, 0),
            (1, 65, -1),
            (-1, 63, 0),
            (0, 63, -1),
            (0, 65, 1),
            (1, 65, 0),
            (-1, 63, 1),
            (0, 63, 0),
            (1, 63, -1),
            (1, 65, 1),
            (0, 63, 1),
            (1, 63, 0),
            (1, 63, 1),
            (-1, 64, -1),
            (-1, 64, 0),
            (0, 64, -1),
        ]
        .map(|(x, y, z)| BlockPos::new(x, y, z))
    );
}

#[test]
fn precomputed_ray_steps_match_vanilla_generation_order() {
    let mut index = 0;
    for xx in 0..RAY_GRID_SIZE {
        for yy in 0..RAY_GRID_SIZE {
            for zz in 0..RAY_GRID_SIZE {
                if !is_boundary_ray(xx, yy, zz) {
                    continue;
                }
                let expected = ray_direction(xx, yy, zz) * RAY_STEP;
                let actual = RAY_STEPS[index];
                assert_eq!(actual.x.to_bits(), expected.x.to_bits());
                assert_eq!(actual.y.to_bits(), expected.y.to_bits());
                assert_eq!(actual.z.to_bits(), expected.z.to_bits());
                index += 1;
            }
        }
    }
    assert_eq!(index, RAY_COUNT);
}

#[test]
fn precomputed_ray_steps_match_java_bit_digest() {
    let mut digest = 0xcbf2_9ce4_8422_2325_u64;
    for step in RAY_STEPS.iter() {
        for bits in [step.x.to_bits(), step.y.to_bits(), step.z.to_bits()] {
            for byte in bits.to_le_bytes() {
                digest ^= u64::from(byte);
                digest = digest.wrapping_mul(0x100_0000_01b3);
            }
        }
    }

    // Produced by the Minecraft 26.2 ServerExplosion expression under OpenJDK 25.
    assert_eq!(digest, 0x0f55_998f_8a80_904d);
}

#[test]
fn block_cache_index_uses_the_full_fastutil_long_mix() {
    let fixtures = [
        (BlockPos::new(0, 64, 0), 94),
        (BlockPos::new(1, 64, 0), 61),
        (BlockPos::new(0, 64, 1), 14),
        (BlockPos::new(50, 200, 53), 464),
        (BlockPos::new(50, 200, 68), 48),
        (BlockPos::new(-1, 64, -1), 436),
    ];

    for (pos, expected) in fixtures {
        let tag = PackedBlockPos::from(pos).as_raw();
        assert_eq!(explosion_block_cache_index(tag), expected);
    }

    // These positions collide under the truncated multiply-high variant.
    assert_ne!(
        explosion_block_cache_index(PackedBlockPos::from(BlockPos::new(50, 200, 53)).as_raw()),
        explosion_block_cache_index(PackedBlockPos::from(BlockPos::new(50, 200, 68)).as_raw()),
    );
}

#[test]
fn ray_sampling_consumes_the_level_random_in_vanilla_order() {
    const SEED: i64 = 0x1E71_0DE5;

    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("explosion_level_random");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    world.set_random_seed_for_test(SEED);
    let mut expected = LegacyRandom::from_seed(SEED as u64);
    for _ in 0..RAY_COUNT {
        expected.next_f32();
    }

    world.explode(ExplosionOptions::new(
        DVec3::new(0.5, 64.5, 0.5),
        -1.0,
        ExplosionInteraction::None,
    ));

    let actual_next = world.with_random(Random::next_i64);
    assert_eq!(actual_next, expected.next_i64());
}

#[test]
fn immutable_ray_sampling_preserves_the_level_random_sequence() {
    const SEED: i64 = 0x1E71_0DE6;

    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("immutable_explosion_level_random");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    world.set_random_seed_for_test(SEED);
    let mut expected = LegacyRandom::from_seed(SEED as u64);
    for _ in 0..RAY_COUNT {
        expected.next_f32();
    }

    let calculator = DefaultExplosionDamageCalculator;
    let mut options =
        ExplosionOptions::new(DVec3::new(0.5, 64.5, 0.5), 0.0, ExplosionInteraction::None);
    options.immutable_block_calculator = Some(&calculator);
    world.explode(options);

    let actual_next = world.with_random(Random::next_i64);
    assert_eq!(actual_next, expected.next_i64());
}

#[test]
fn unusual_radii_retain_vanilla_ray_sampling_and_bounds_behavior() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("explosion_unusual_radii");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));

    for radius in [f32::NEG_INFINITY, -1.0, -0.0, 0.0, f32::NAN] {
        let explosion = ServerExplosion::new(
            &world,
            None,
            None,
            None,
            None,
            DVec3::new(0.5, 64.5, 0.5),
            radius,
            false,
            BlockInteraction::Destroy,
        );
        let mut draws = 0;
        let affected = explosion.calculate_exploded_positions(|| {
            draws += 1;
            0.5
        });
        assert_eq!(draws, RAY_COUNT, "radius={radius:?}");
        assert!(affected.is_empty(), "radius={radius:?}");
    }

    let tiny = ServerExplosion::new(
        &world,
        None,
        None,
        None,
        None,
        DVec3::new(0.5, 64.5, 0.5),
        f32::MIN_POSITIVE,
        false,
        BlockInteraction::Destroy,
    );
    let tiny_affected = tiny.calculate_exploded_positions(|| 0.5);
    assert_eq!(tiny_affected, [BlockPos::new(0, 64, 0)]);

    let positive_infinity = ServerExplosion::new(
        &world,
        None,
        None,
        None,
        None,
        DVec3::new(30_000_000.5, 64.5, 0.5),
        f32::INFINITY,
        false,
        BlockInteraction::Destroy,
    );
    let mut draws = 0;
    let affected = positive_infinity.calculate_exploded_positions(|| {
        draws += 1;
        0.5
    });
    assert_eq!(draws, RAY_COUNT);
    assert!(affected.is_empty());
}

#[test]
fn immutable_rays_match_compatibility_lane_at_radius_boundaries() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("immutable_explosion_radius_boundaries");
    let calculator = DefaultExplosionDamageCalculator;
    let center = DVec3::new(15.999, 79.999, -15.999);

    for radius in [-0.0, f32::MIN_POSITIVE, 4.0, 32.0] {
        let compatibility = ServerExplosion::new(
            &world,
            None,
            None,
            None,
            None,
            center,
            radius,
            false,
            BlockInteraction::Destroy,
        );
        let immutable = ServerExplosion::new(
            &world,
            None,
            None,
            None,
            Some(&calculator),
            center,
            radius,
            false,
            BlockInteraction::Destroy,
        );

        assert_eq!(
            immutable.calculate_exploded_positions(|| 0.5),
            compatibility.calculate_exploded_positions(|| 0.5),
            "radius={radius:?}"
        );
    }
}

#[test]
fn vanilla_shuffle_uses_descending_bounded_draws() {
    let mut values = [0, 1, 2, 3];
    let mut bounds = Vec::new();
    let indexes = [1, 0, 1];
    let mut draw = 0;

    vanilla_shuffle(&mut values, |bound| {
        bounds.push(bound);
        let index = indexes[draw];
        draw += 1;
        index
    });

    assert_eq!(bounds, [4, 3, 2]);
    assert_eq!(values, [2, 3, 0, 1]);
}

#[test]
fn fire_creation_draws_before_testing_air_and_support() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("explosion_fire_order");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let supported = BlockPos::new(1, 64, 1);
    let unsupported = supported.east();
    let occupied = unsupported.east();
    assert!(world.set_block(
        supported.below(),
        vanilla_blocks::STONE.default_state(),
        UpdateFlags::UPDATE_NONE,
    ));
    assert!(world.set_block(
        occupied,
        vanilla_blocks::STONE.default_state(),
        UpdateFlags::UPDATE_NONE,
    ));
    let explosion = ServerExplosion::new(
        &world,
        None,
        None,
        None,
        None,
        DVec3::ZERO,
        1.0,
        true,
        BlockInteraction::Destroy,
    );
    let mut draws = 0;

    explosion.create_fire_with(&[unsupported, occupied, supported], || {
        draws += 1;
        0
    });

    assert_eq!(draws, 3);
    assert!(world.get_block_state(unsupported).is_air());
    assert_eq!(
        world.get_block_state(occupied).get_block(),
        &vanilla_blocks::STONE
    );
    assert!(!world.get_block_state(supported).is_air());
}

#[test]
fn explosion_rays_use_vanilla_world_bounds_in_both_lanes() {
    init_vanilla_registry();
    let world = fresh_test_world("explosion_world_bounds");
    let bounds = ExplosionWorldBounds::from_world(&world);
    let min_y = world.get_min_y();
    let max_y = world.get_max_y();
    let cases = [
        (BlockPos::new(-30_000_000, min_y, -30_000_000), true),
        (BlockPos::new(29_999_999, max_y, 29_999_999), true),
        (BlockPos::new(-30_000_001, min_y, 0), false),
        (BlockPos::new(30_000_000, min_y, 0), false),
        (BlockPos::new(0, min_y, -30_000_001), false),
        (BlockPos::new(0, min_y, 30_000_000), false),
        (BlockPos::new(0, min_y - 1, 0), false),
        (BlockPos::new(0, max_y + 1, 0), false),
    ];

    for (pos, expected) in cases {
        assert_eq!(bounds.contains(pos), expected, "pos={pos:?}");
    }
}

#[test]
fn immutable_block_rays_match_sequential_order_and_repeat() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("immutable_explosion_rays");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let center = DVec3::new(8.5, 64.5, 8.5);
    assert!(world.set_block(
        BlockPos::from(center),
        vanilla_blocks::STONE.default_state(),
        UpdateFlags::UPDATE_ALL,
    ));
    let immutable_calculator = DefaultExplosionDamageCalculator;
    let immutable = ServerExplosion::new(
        &world,
        None,
        None,
        None,
        Some(&immutable_calculator),
        center,
        4.0,
        false,
        BlockInteraction::Destroy,
    );
    let sequential = ServerExplosion::new(
        &world,
        None,
        None,
        None,
        None,
        center,
        4.0,
        false,
        BlockInteraction::Destroy,
    );
    let calculate = |explosion: &ServerExplosion<'_>| {
        let mut index = 0_u32;
        explosion.calculate_exploded_positions(|| {
            let value = ((index * 37 + 11) % 101) as f32 / 100.0;
            index += 1;
            value
        })
    };

    let sequential_positions = calculate(&sequential);
    let first_immutable_positions = calculate(&immutable);
    let second_immutable_positions = calculate(&immutable);

    assert_eq!(first_immutable_positions, sequential_positions);
    assert_eq!(second_immutable_positions, first_immutable_positions);

    let mut ray_index = 0_u32;
    let rays = immutable.draw_immutable_rays(|| {
        let value = ((ray_index * 37 + 11) % 101) as f32 / 100.0;
        ray_index += 1;
        value
    });
    assert_eq!(ray_index as usize, RAY_COUNT);
    let context = ExplosionRayContext {
        center,
        bounds: ExplosionWorldBounds::from_world(&world),
    };
    let reader = world.as_ref();
    let pure_sequential =
        calculate_immutable_rays_sequential(&rays, context, reader, &immutable_calculator);
    let sequential_set = pure_sequential.iter().copied().collect::<FxHashSet<_>>();
    for worker_threads in [1, 4, 8, 12, 16] {
        let worker_pool = rayon::ThreadPoolBuilder::new()
            .num_threads(worker_threads)
            .build()
            .expect("explosion test pool should start");
        let target_shards = worker_threads * 2;
        let pure_parallel = worker_pool.install(|| {
            calculate_immutable_rays_sharded(
                &rays,
                context,
                reader,
                &immutable_calculator,
                target_shards,
            )
        });
        let repeated_parallel = worker_pool.install(|| {
            calculate_immutable_rays_sharded(
                &rays,
                context,
                reader,
                &immutable_calculator,
                target_shards,
            )
        });

        assert_eq!(
            pure_parallel.iter().copied().collect::<FxHashSet<_>>(),
            sequential_set
        );
        assert_eq!(repeated_parallel, pure_parallel);
    }
}

#[test]
fn immutable_parallel_fixture_preserves_calculator_call_counts() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("immutable_explosion_calculator_calls");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let center = DVec3::new(8.5, 64.5, 8.5);
    let explosion = ServerExplosion::new(
        &world,
        None,
        None,
        None,
        None,
        center,
        4.0,
        false,
        BlockInteraction::Destroy,
    );
    let rays = explosion.draw_immutable_rays(|| 0.5);
    let context = ExplosionRayContext {
        center,
        bounds: ExplosionWorldBounds::from_world(&world),
    };
    let reader = world.as_ref();
    let calculator = CountingImmutableCalculator::default();

    let sequential = calculate_immutable_rays_sequential(&rays, context, reader, &calculator);
    let sequential_counts = (
        calculator.resistance_calls.swap(0, Ordering::Relaxed),
        calculator.decision_calls.swap(0, Ordering::Relaxed),
    );
    let parallel = calculate_immutable_rays_sharded(&rays, context, reader, &calculator, 16);
    let parallel_counts = (
        calculator.resistance_calls.load(Ordering::Relaxed),
        calculator.decision_calls.load(Ordering::Relaxed),
    );

    assert_eq!(parallel_counts, sequential_counts);
    assert_eq!(
        parallel.iter().copied().collect::<FxHashSet<_>>(),
        sequential.iter().copied().collect::<FxHashSet<_>>()
    );
}

#[test]
fn immutable_cache_preserves_order_and_extensible_hook_calls() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("immutable_explosion_cache");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let center = DVec3::new(8.5, 64.5, 8.5);
    let explosion = ServerExplosion::new(
        &world,
        None,
        None,
        None,
        None,
        center,
        4.0,
        false,
        BlockInteraction::Destroy,
    );
    let rays = explosion.draw_immutable_rays(|| 0.5);
    let context = ExplosionRayContext {
        center,
        bounds: ExplosionWorldBounds::from_world(&world),
    };

    let cached_reader = CountingBlockReader {
        world: world.as_ref(),
        calls: AtomicUsize::new(0),
    };
    let cached_calculator = CountingImmutableCalculator {
        cache_resistance: true,
        ..CountingImmutableCalculator::default()
    };
    let cached =
        calculate_immutable_rays_sequential(&rays, context, &cached_reader, &cached_calculator);

    let uncached_reader = CountingBlockReader {
        world: world.as_ref(),
        calls: AtomicUsize::new(0),
    };
    let uncached_calculator = CountingImmutableCalculator {
        cache_resistance: true,
        ..CountingImmutableCalculator::default()
    };
    let mut uncached_set = JavaBlockPosSet::default();
    for &ray in &rays {
        visit_immutable_ray_positions(
            ray,
            context,
            &uncached_reader,
            &uncached_calculator,
            |pos| {
                uncached_set.insert(pos);
            },
        );
    }
    let uncached = uncached_set.into_iter().collect::<Vec<_>>();

    assert_eq!(cached, uncached);
    assert_eq!(
        cached_calculator.decision_calls.load(Ordering::Relaxed),
        uncached_calculator.decision_calls.load(Ordering::Relaxed)
    );
    assert!(
        cached_calculator.resistance_calls.load(Ordering::Relaxed)
            < uncached_calculator.resistance_calls.load(Ordering::Relaxed)
    );
    assert!(
        cached_reader.calls.load(Ordering::Relaxed) < uncached_reader.calls.load(Ordering::Relaxed)
    );

    let always_allows_calculator = CountingImmutableCalculator {
        cache_resistance: true,
        always_allows_block_explosion: true,
        ..CountingImmutableCalculator::default()
    };
    let always_allows = calculate_immutable_rays_sequential(
        &rays,
        context,
        &cached_reader,
        &always_allows_calculator,
    );
    assert_eq!(always_allows, uncached);
    assert_eq!(
        always_allows_calculator
            .decision_calls
            .load(Ordering::Relaxed),
        0
    );
}

#[test]
fn incomplete_bounded_region_falls_back_before_calculator_hooks() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("immutable_explosion_incomplete_region");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let center = DVec3::new(15.5, 64.5, 8.5);
    let actual_calculator = CountingImmutableCalculator {
        bounded_read_radius: Some(0),
        ..CountingImmutableCalculator::default()
    };
    let actual_explosion = ServerExplosion::new(
        &world,
        None,
        None,
        None,
        Some(&actual_calculator),
        center,
        4.0,
        false,
        BlockInteraction::Destroy,
    );
    let powers = actual_explosion.draw_immutable_ray_powers(|| 0.5);

    let actual = actual_explosion.calculate_immutable_ray_powers(&powers, &actual_calculator);

    let expected_calculator = CountingImmutableCalculator::default();
    let expected_explosion = ServerExplosion::new(
        &world,
        None,
        None,
        None,
        None,
        center,
        4.0,
        false,
        BlockInteraction::Destroy,
    );
    let expected = expected_explosion.calculate_immutable_ray_powers_uncached_with_reader(
        &powers,
        &expected_calculator,
        world.as_ref(),
    );

    assert_eq!(actual, expected);
    assert_eq!(
        actual_calculator.resistance_calls.load(Ordering::Relaxed),
        expected_calculator.resistance_calls.load(Ordering::Relaxed)
    );
    assert_eq!(
        actual_calculator.decision_calls.load(Ordering::Relaxed),
        expected_calculator.decision_calls.load(Ordering::Relaxed)
    );
}

#[test]
fn bounded_immutable_reader_covers_maximum_power_rays() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("bounded_explosion_reader_coverage");
    let calculator = DefaultExplosionDamageCalculator;
    let powers = [4.0_f32 * 1.3_f32; RAY_COUNT];

    for center in [
        DVec3::new(0.0, 64.0, 0.0),
        DVec3::new(15.999, 79.999, 15.999),
        DVec3::new(-15.999, 48.001, -15.999),
    ] {
        let explosion = ServerExplosion::new(
            &world,
            None,
            None,
            None,
            Some(&calculator),
            center,
            4.0,
            false,
            BlockInteraction::Destroy,
        );
        let bounds = explosion
            .immutable_ray_region_bounds(0)
            .expect("finite radius-four explosion has bounded ray coverage");
        let affected = world
            .try_with_block_region(bounds, |region| {
                let reader = RegionExplosionBlockReader::new(region);
                explosion.calculate_immutable_ray_powers_with_reader(&powers, &calculator, &reader)
            })
            .expect("radius-four ray workset stays within the bounded-reader slot limit")
            .expect("bounded reader covers every maximum-power ray access");

        assert!(
            !affected.is_empty(),
            "maximum-power rays affect blocks at {center:?}"
        );
    }
}

#[test]
fn java_block_pos_set_matches_jdk_collision_resize_order() {
    let mut positions = JavaBlockPosSet::default();
    for x in (0..=128).step_by(16) {
        assert!(positions.insert(BlockPos::new(x, 0, 0)));
    }

    assert_eq!(positions.buckets.len(), 32);
    assert_eq!(
        positions.into_iter().collect::<Vec<_>>(),
        [0, 32, 64, 96, 128, 16, 48, 80, 112].map(|x| BlockPos::new(x, 0, 0))
    );
}

#[test]
fn explosion_entity_query_excludes_spectators() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("explosion_excludes_spectators");
    let player =
        TestPlayerBuilder::new(Arc::clone(&world), Uuid::from_u128(1), "Spectator", 27).build();
    player.restore_game_modes(GameType::Spectator, None);
    player
        .abilities
        .lock()
        .update_for_game_mode(GameType::Spectator);
    let position = player.position();
    insert_ready_full_chunk(&world, ChunkPos::from_block_pos(BlockPos::from(position)));
    assert!(world.add_player(Arc::clone(&player), ResetReason::InitialJoin));

    world.explode(ExplosionOptions::new(
        position - DVec3::X,
        2.0,
        ExplosionInteraction::None,
    ));

    assert_eq!(player.velocity(), DVec3::ZERO);
}

#[test]
fn destructive_explosion_removes_blocks_and_spawns_their_loot() {
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
        DVec3::new(0.5, 64.5, 0.5),
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

#[test]
fn combined_explosion_drops_never_exceed_sixteen() {
    init_vanilla_registry();
    let stack = ItemStack::with_count(&vanilla_items::STONE, 10);
    let mut stacks = Vec::new();

    add_or_append_stack(&mut stacks, stack.clone(), BlockPos::ZERO);
    add_or_append_stack(&mut stacks, stack, BlockPos::ZERO);

    assert_eq!(stacks.len(), 2);
    assert_eq!(stacks[0].stack.count(), 16);
    assert_eq!(stacks[1].stack.count(), 4);
}
