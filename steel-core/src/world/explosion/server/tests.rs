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
}

impl ImmutableExplosionBlockCalculator for CountingImmutableCalculator {
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
    let live = exposure.calculate(world, center);
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

    assert_eq!(
        assert_exposure_matches_seen_percent(&world, center, &entity).to_bits(),
        1.0_f32.to_bits()
    );

    entity.set_fall_distance(3.0);
    assert_eq!(
        assert_exposure_matches_seen_percent(&world, center, &entity).to_bits(),
        0.0_f32.to_bits()
    );
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
    let live = exposure.calculate(world.as_ref(), center);
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
