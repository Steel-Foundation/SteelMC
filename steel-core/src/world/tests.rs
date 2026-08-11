use super::*;
use std::{
    sync::{Arc, Weak},
    thread,
};

use steel_registry::entity_type::EntityTypeRef;
use steel_registry::{
    init_vanilla_registry, sound_events, vanilla_entities, vanilla_fluids, vanilla_items,
};
use uuid::Uuid;

use crate::behavior::init_behaviors;
use crate::chunk::chunk_ticket_manager::{ChunkTicket, ChunkTicketLevel};
use crate::entity::{EntityBase, entities::PigEntity};
use crate::test_support::{fresh_test_world, insert_ready_full_chunk, test_world};

const FIRST_HALF: BlockLocalAabb = BlockLocalAabb::new(0.0, 0.0, 0.0, 0.5, 1.0, 1.0);
const SECOND_HALF: BlockLocalAabb = BlockLocalAabb::new(0.5, 0.0, 0.0, 1.0, 1.0, 1.0);
static SPLIT_BLOCK: &[BlockLocalAabb] = &[FIRST_HALF, SECOND_HALF];

fn advance_scheduling_until(world: &Arc<World>, mut ready: impl FnMut() -> bool) {
    for _ in 0..10_000 {
        world.chunk_map.advance_scheduling();
        if ready() {
            return;
        }
        thread::sleep(Duration::from_millis(1));
    }
    panic!("chunk scheduling condition did not become ready");
}

#[test]
fn sound_range_uses_event_range_and_strict_vanilla_boundary() {
    init_vanilla_registry();
    let sound = &sound_events::ENTITY_PLAYER_LEVELUP;

    assert!(sound_is_within_range(sound, 0.75, 255.0));
    assert!(!sound_is_within_range(sound, 0.75, 256.0));
}

#[test]
fn generic_shape_update_does_not_schedule_non_source_fluid() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("shape_update_fluid_ownership");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let pos = BlockPos::new(0, 64, 0);
    let flowing_water = vanilla_blocks::WATER
        .default_state()
        .set_value(&BlockStateProperties::LEVEL, 1);

    assert!(world.set_block(pos, flowing_water, UpdateFlags::UPDATE_SKIP_ON_PLACE));
    assert!(!world.has_scheduled_fluid_tick(pos, &vanilla_fluids::FLOWING_WATER));

    world.execute_neighbor_shape_update(
        Direction::North,
        pos,
        pos.north(),
        flowing_water,
        UpdateFlags::UPDATE_NONE,
        512,
    );

    assert!(!world.has_scheduled_fluid_tick(pos, &vanilla_fluids::FLOWING_WATER));
}

#[test]
fn closest_portal_candidate_filters_then_tiebreaks_by_y() {
    let center = BlockPos::new(0, 64, 0);
    let candidates = [
        BlockPos::new(1, 64, 0),
        BlockPos::new(0, 67, 0),
        BlockPos::new(0, 61, 0),
    ];

    assert_eq!(
        closest_portal_candidate(candidates, center, |pos| pos.x() != 1),
        Some(BlockPos::new(0, 61, 0))
    );
}

#[test]
fn nether_portal_frame_offsets_match_vanilla_create_portal_axes() {
    let origin = BlockPos::new(10, 70, 20);

    assert_eq!(
        nether_portal_frame_offset_pos(origin, Direction::East, 1, 2, -1),
        BlockPos::new(11, 72, 19)
    );
    assert_eq!(
        nether_portal_frame_offset_pos(origin, Direction::South, 1, 2, -1),
        BlockPos::new(11, 72, 21)
    );
    assert_eq!(
        nether_portal_frame_offset_pos(origin, Direction::East, 0, -1, 1),
        BlockPos::new(10, 69, 21)
    );
}

#[test]
fn nether_portal_creation_scan_origin_matches_vanilla_column_shift() {
    let column = BlockPos::new(10, 0, 20);

    assert_eq!(
        nether_portal_creation_scan_origin(column, Direction::East, 70),
        BlockPos::new(9, 70, 20)
    );
    assert_eq!(
        nether_portal_creation_scan_origin(column, Direction::South, 70),
        BlockPos::new(10, 70, 19)
    );
}

struct TrackerTestEntity {
    base: EntityBase,
}

impl TrackerTestEntity {
    fn shared(id: i32) -> SharedEntity {
        Arc::new(Self {
            base: EntityBase::with_uuid(
                id,
                Uuid::from_u128(id as u128),
                DVec3::ZERO,
                vanilla_entities::ITEM.dimensions,
                Weak::new(),
            ),
        })
    }
}

crate::entity::impl_test_downcast_type!(TrackerTestEntity);

impl Entity for TrackerTestEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        &vanilla_entities::ITEM
    }
}

#[test]
fn entity_breaker_is_available_to_chorus_flower_loot() {
    init_vanilla_registry();
    init_behaviors();

    let state = vanilla_blocks::CHORUS_FLOWER.default_state();
    let pos = BlockPos::new(1_312, 64, 1_312);
    let breaker = TrackerTestEntity::shared(987_654);
    let world = fresh_test_world("entity_breaker_loot");
    let context = BlockLootContext::new(&world, pos).with_entity(Some(breaker.as_ref()));
    let drops = context.get_drops(state);

    assert_eq!(drops.len(), 1);
    assert_eq!(drops[0].item(), &*vanilla_items::CHORUS_FLOWER);
    assert_eq!(drops[0].count(), 1);
    assert!(
        BlockLootContext::new(&world, pos)
            .get_drops(state)
            .is_empty()
    );
}

fn assert_vec3_close(left: DVec3, right: DVec3) {
    let diff = left - right;
    assert!(
        diff.length_squared() < 1.0e-24,
        "expected {left:?} to equal {right:?}"
    );
}

#[test]
fn nearest_player_range_uses_vanilla_strict_boundary() {
    assert!(nearest_player_distance_in_range(63.999, 8.0, 64.0));
    assert!(!nearest_player_distance_in_range(64.0, 8.0, 64.0));
}

#[test]
fn nearest_player_negative_range_is_unbounded() {
    assert!(nearest_player_distance_in_range(1_000_000.0, -1.0, 1.0));
}

#[test]
fn level_event_recipient_range_uses_block_corner_and_strict_boundary() {
    let event_pos = BlockPos::ZERO;

    assert!(World::recipient_within_64_blocks(
        DVec3::new(-63.999, 0.0, 0.0),
        event_pos,
    ));
    assert!(!World::recipient_within_64_blocks(
        DVec3::new(-64.0, 0.0, 0.0),
        event_pos,
    ));
    assert!(!World::recipient_within_64_blocks(
        DVec3::new(64.25, 0.0, 0.0),
        event_pos,
    ));
}

#[test]
fn level_event_range_is_independent_of_chunk_tracking_view() {
    let player_pos = DVec3::new(15.9, 64.0, 0.0);
    let event_pos = BlockPos::new(64, 64, 0);
    let view = PlayerChunkView::new(ChunkPos::new(0, 0), 2);

    assert!(!view.contains(ChunkPos::from_block_pos(event_pos)));
    assert!(World::recipient_within_64_blocks(player_pos, event_pos));
}

#[test]
fn particle_recipient_range_uses_block_center_and_strict_boundary() {
    let player_pos = BlockPos::ZERO;

    assert!(World::particle_recipient_in_range(
        player_pos,
        DVec3::new(32.499, 0.5, 0.5),
        false,
    ));
    assert!(!World::particle_recipient_in_range(
        player_pos,
        DVec3::new(32.5, 0.5, 0.5),
        false,
    ));
    assert!(World::particle_recipient_in_range(
        player_pos,
        DVec3::new(512.499, 0.5, 0.5),
        true,
    ));
    assert!(!World::particle_recipient_in_range(
        player_pos,
        DVec3::new(512.5, 0.5, 0.5),
        true,
    ));
}

#[test]
fn spawnable_bounds_match_vanilla_teleport_command_bounds() {
    assert!(World::is_in_spawnable_bounds(BlockPos::new(0, 320, 0)));
    assert!(World::is_in_spawnable_bounds(BlockPos::new(
        29_999_999,
        19_999_999,
        -30_000_000
    )));
    assert!(!World::is_in_spawnable_bounds(BlockPos::new(
        30_000_000, 0, 0
    )));
    assert!(!World::is_in_spawnable_bounds(BlockPos::new(
        0,
        -20_000_001,
        0
    )));
    assert!(!World::is_in_spawnable_bounds(BlockPos::new(
        0, 20_000_000, 0
    )));
}

#[test]
fn block_state_outside_world_bounds_is_void_air() {
    init_vanilla_registry();
    let world = test_world();

    assert_eq!(
        world.get_block_state(BlockPos::new(0, world.get_min_y() - 1, 0)),
        vanilla_blocks::VOID_AIR.default_state()
    );
    assert_eq!(
        world.get_block_state(BlockPos::new(BlockPos::MAX_HORIZONTAL_COORDINATE, 0, 0)),
        vanilla_blocks::VOID_AIR.default_state()
    );
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "one state sequence documents the Vanilla client-publication gates"
)]
fn set_block_matches_vanilla_update_limit_and_client_publication_gates() {
    init_vanilla_registry();
    init_behaviors();

    let world = fresh_test_world("set_block_publication_gates");
    let pos = BlockPos::new(1_504, 64, 1_504);
    let chunk_pos = ChunkPos::from_block_pos(pos);
    let simulation_ticket = ChunkTicket::simulated_full_chunks(1);
    let simulation_revision = world
        .chunk_map
        .add_chunk_ticket(chunk_pos, simulation_ticket);
    advance_scheduling_until(&world, || {
        world
            .chunk_map
            .is_ticket_revision_committed(simulation_revision)
            && world.chunk_map.with_full_chunk(chunk_pos, |_| ()).is_some()
            && world
                .chunk_map
                .is_block_ticking_full_chunk_loaded(chunk_pos)
    });

    // Stop background generation and wait for in-flight setup work before
    // measuring client-visible revisions. Chunk lighting can publish
    // independently of the block updates exercised below.
    world.chunk_map.stop_generation_refill_loop();
    world.chunk_map.task_tracker.close();
    world
        .chunk_map
        .chunk_runtime
        .block_on(world.chunk_map.task_tracker.wait());

    let holder = world
        .chunk_map
        .chunks
        .read_sync(&chunk_pos, |_, holder| Arc::clone(holder))
        .expect("loaded test chunk should have a holder");
    assert!(
        world
            .chunk_map
            .is_block_ticking_full_chunk_loaded(chunk_pos)
    );

    let pre_light_revision = holder.packet_content_revision();
    assert!(world.set_block_with_limit(
        pos,
        vanilla_blocks::DIRT.default_state(),
        UpdateFlags::UPDATE_NONE,
        0,
    ));
    assert_eq!(
        world.get_block_state(pos),
        vanilla_blocks::DIRT.default_state()
    );
    assert_eq!(holder.packet_content_revision(), pre_light_revision);

    // The first non-air block queues lighting independently of client updates.
    // Settle it before measuring block-publication-only revisions.
    world.chunk_map.broadcast_changed_chunks();
    let publication_revision = holder.packet_content_revision();

    assert!(world.set_block(
        pos,
        vanilla_blocks::STONE.default_state(),
        UpdateFlags::UPDATE_NONE,
    ));
    assert_eq!(holder.packet_content_revision(), publication_revision);

    assert!(world.set_block(
        pos,
        vanilla_blocks::DIRT.default_state(),
        UpdateFlags::UPDATE_CLIENTS,
    ));
    assert_eq!(holder.packet_content_revision(), publication_revision + 1);

    assert!(world.set_block(
        pos,
        vanilla_blocks::STONE.default_state(),
        UpdateFlags::UPDATE_CLIENTS | UpdateFlags::UPDATE_INVISIBLE,
    ));
    assert_eq!(holder.packet_content_revision(), publication_revision + 2);

    let unsupported_fire_pos = pos.offset(2, 0, 0);
    assert!(world.get_block_state(unsupported_fire_pos).is_air());
    assert!(world.set_block(
        unsupported_fire_pos,
        vanilla_blocks::FIRE.default_state(),
        UpdateFlags::UPDATE_CLIENTS,
    ));
    assert!(world.get_block_state(unsupported_fire_pos).is_air());
    assert_eq!(holder.packet_content_revision(), publication_revision + 3);

    let loading_ticket = ChunkTicket::loading(ChunkTicketLevel::BLOCK_TICKING_CHUNK);
    let loading_revision = world.chunk_map.add_chunk_ticket(chunk_pos, loading_ticket);
    let removal_revision = world
        .chunk_map
        .remove_chunk_ticket(chunk_pos, simulation_ticket);
    advance_scheduling_until(&world, || {
        world
            .chunk_map
            .is_ticket_revision_committed(loading_revision)
            && world
                .chunk_map
                .is_ticket_revision_committed(removal_revision)
    });

    assert!(
        world
            .chunk_map
            .is_block_ticking_full_chunk_loaded(chunk_pos),
        "client publication should remain enabled for load-only BlockTicking chunks"
    );
    let load_only_revision = holder.packet_content_revision();
    assert!(world.set_block(
        pos,
        vanilla_blocks::DIRT.default_state(),
        UpdateFlags::UPDATE_CLIENTS,
    ));
    assert_eq!(holder.packet_content_revision(), load_only_revision + 1);

    let full_only_ticket = ChunkTicket::full_chunks(0);
    let full_only_revision = world
        .chunk_map
        .add_chunk_ticket(chunk_pos, full_only_ticket);
    let loading_removal_revision = world
        .chunk_map
        .remove_chunk_ticket(chunk_pos, loading_ticket);
    advance_scheduling_until(&world, || {
        world
            .chunk_map
            .is_ticket_revision_committed(full_only_revision)
            && world
                .chunk_map
                .is_ticket_revision_committed(loading_removal_revision)
    });

    assert!(
        !world
            .chunk_map
            .is_block_ticking_full_chunk_loaded(chunk_pos)
    );
    let non_ticking_revision = holder.packet_content_revision();

    assert!(world.set_block(
        pos,
        vanilla_blocks::STONE.default_state(),
        UpdateFlags::UPDATE_CLIENTS,
    ));
    assert_eq!(holder.packet_content_revision(), non_ticking_revision);
    world.send_block_updated(pos);
    assert_eq!(holder.packet_content_revision(), non_ticking_revision);

    world
        .chunk_map
        .remove_chunk_ticket(chunk_pos, full_only_ticket);
    world.chunk_map.advance_scheduling();
    world
        .chunk_map
        .chunk_runtime
        .block_on(world.chunk_map.task_tracker.wait());
}

#[test]
fn light_packet_tracking_border_matches_vanilla_pending_chunk_rule() {
    let view = PlayerChunkView::new(ChunkPos::new(0, 0), 2);
    let center = ChunkPos::new(0, 0);

    assert!(!World::chunk_is_on_packet_tracked_border(
        view,
        center,
        &|pos| view.contains(pos)
    ));
    assert!(World::chunk_is_on_packet_tracked_border(
        view,
        ChunkPos::new(3, 0),
        &|_| true
    ));
    assert!(World::chunk_is_on_packet_tracked_border(
        view,
        center,
        &|pos| pos != ChunkPos::new(1, 0)
    ));
    assert!(!World::chunk_is_on_packet_tracked_border(
        view,
        center,
        &|pos| pos != center
    ));
}

#[test]
fn navigating_mob_tracker_tracks_only_pathfinder_mobs() {
    init_vanilla_registry();

    let tracker = NavigatingMobTracker::new();
    let non_pathfinder = TrackerTestEntity::shared(1);
    let pig: SharedEntity = Arc::new(PigEntity::new(
        &vanilla_entities::PIG,
        2,
        DVec3::ZERO,
        Weak::new(),
    ));

    tracker.track(&non_pathfinder);
    assert!(tracker.ids().is_empty());

    tracker.track(&pig);
    tracker.track(&pig);
    assert_eq!(tracker.ids(), [2]);

    tracker.untrack(2);
    assert!(tracker.ids().is_empty());
}

#[test]
fn clip_shape_hits_closest_block_face() {
    let Some(hit) = World::clip_shape(
        BlockPos::ZERO,
        DVec3::new(-1.0, 0.5, 0.5),
        DVec3::new(1.0, 0.5, 0.5),
        OffsetVoxelShape::without_offset(VoxelShape::from_boxes(SPLIT_BLOCK)),
    ) else {
        panic!("expected shape hit");
    };

    assert!(!hit.inside);
    assert_eq!(hit.direction, Direction::West);
    assert_eq!(hit.block_pos, BlockPos::ZERO);
    assert_vec3_close(hit.location, DVec3::new(0.0, 0.5, 0.5));
}

#[test]
fn clip_shape_reports_inside_start_like_vanilla_voxel_shape() {
    let Some(hit) = World::clip_shape(
        BlockPos::ZERO,
        DVec3::new(0.5, 0.5, 0.5),
        DVec3::new(2.5, 0.5, 0.5),
        OffsetVoxelShape::without_offset(VoxelShape::FULL_BLOCK),
    ) else {
        panic!("expected inside shape hit");
    };

    assert!(hit.inside);
    assert_eq!(hit.direction, Direction::West);
    assert_vec3_close(hit.location, DVec3::new(0.502, 0.5, 0.5));
}

#[test]
fn clip_local_aabb_supports_runtime_fluid_heights() {
    let Some(hit) = World::clip_local_aabb(
        BlockPos::ZERO,
        DVec3::new(0.5, 2.0, 0.5),
        DVec3::new(0.5, 0.0, 0.5),
        BlockLocalAabb::new(0.0, 0.0, 0.0, 1.0, 0.5, 1.0),
    ) else {
        panic!("expected fluid shape hit");
    };

    assert_eq!(hit.direction, Direction::Up);
    assert_vec3_close(hit.location, DVec3::new(0.5, 0.5, 0.5));
}

#[test]
fn fluid_clip_height_treats_source_and_flowing_variants_as_same_fluid_above() {
    init_vanilla_registry();
    init_behaviors();

    let height = World::fluid_clip_height_from_above(
        FluidState::source(&vanilla_fluids::WATER),
        FluidState::flowing(&vanilla_fluids::FLOWING_WATER, 4, false),
    );

    assert_eq!(height.to_bits(), 1.0_f64.to_bits());
}
