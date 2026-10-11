use steel_registry::dimension_type::DimensionTypeRef;
use steel_registry::{
    init_vanilla_registry, vanilla_blocks, vanilla_dimension_types, vanilla_game_rules,
};
use steel_utils::types::UpdateFlags;
use tokio::runtime::Builder;

use super::*;
use crate::behavior::init_behaviors;
use crate::test_support::{fresh_test_world_with_dimension_type, insert_ready_full_chunk};

/// Runs `test` in a fresh world of `dimension_type` with a runtime for its chunk map.
fn with_search_world<R>(
    key: &'static str,
    dimension_type: DimensionTypeRef,
    test: impl FnOnce(&Arc<World>) -> R,
) -> R {
    init_vanilla_registry();
    init_behaviors();
    let Ok(runtime) = Builder::new_current_thread().enable_all().build() else {
        panic!("test runtime should initialize");
    };
    let _guard = runtime.enter();
    let fixture = fresh_test_world_with_dimension_type("spawn_search", key, dimension_type);
    let world = &fixture.world;
    let result = test(world);

    world.chunk_map.stop_generation_refill_loop();
    world.chunk_map.task_tracker.close();
    runtime.block_on(world.chunk_map.task_tracker.wait());
    result
}

fn insert_chunks(world: &Arc<World>, radius: i32) {
    for z in -radius..=radius {
        for x in -radius..=radius {
            insert_ready_full_chunk(world, ChunkPos::new(x, z));
        }
    }
}

/// Polls `search` until it is ready, advancing chunk scheduling between polls.
fn run_search(world: &Arc<World>, search: &mut PlayerSpawnSearch) -> DVec3 {
    for _ in 0..100 {
        world.chunk_map.advance_scheduling();
        if let PlayerSpawnSearchPoll::Ready(position) = search.poll(world) {
            return position;
        }
    }
    panic!("search should finish");
}

fn new_search(world: &Arc<World>, suggestion: BlockPos) -> PlayerSpawnSearch {
    let Ok(search) = PlayerSpawnSearch::new(world, suggestion, GameType::Survival) else {
        panic!("search should start");
    };
    search
}

fn set_block(world: &Arc<World>, pos: BlockPos, state: steel_utils::BlockStateId) {
    assert!(world.set_block(pos, state, UpdateFlags::UPDATE_NONE));
}

#[test]
fn candidates_are_consumed_in_vanilla_order_across_chunks() {
    with_search_world("order", &vanilla_dimension_types::OVERWORLD, |world| {
        insert_chunks(world, 3);
        let suggestion = BlockPos::new(0, 64, 0);
        let mut sequence = new_search(world, suggestion).sequence;
        sequence.offset = 0;
        let chunk_of = |index| {
            let (x, z) = sequence.column(index);
            block_to_chunk(x, z)
        };

        // `later` shares a chunk with the first candidate, so the first chunk's batch
        // evaluates it. `earlier` comes before it in the order but lies in another chunk.
        let first_chunk = chunk_of(0);
        let Some(later) = (50..sequence.count()).find(|&index| chunk_of(index) == first_chunk)
        else {
            panic!("the first chunk should hold a later candidate");
        };
        let Some(earlier) = (1..later).find(|&index| chunk_of(index) != first_chunk) else {
            panic!("an earlier candidate should lie in another chunk");
        };
        let stone = vanilla_blocks::STONE.default_state();
        for index in [earlier, later] {
            let (x, z) = sequence.column(index);
            assert!(world.set_block(BlockPos::new(x, 64, z), stone, UpdateFlags::UPDATE_ALL));
        }
        let (x, z) = sequence.column(earlier);
        let expected = DVec3::new(f64::from(x) + 0.5, 65.0, f64::from(z) + 0.5);

        for prefetch in [false, true] {
            let mut search = new_search(world, suggestion);
            search.sequence.offset = 0;
            if prefetch {
                search = search.with_spawn_area_prefetch(world);
            }
            assert_eq!(run_search(world, &mut search), expected);
        }
    });
}

#[test]
fn prefetched_chunks_are_read_directly_once_their_tickets_are_committed() {
    with_search_world("prefetch", &vanilla_dimension_types::OVERWORLD, |world| {
        insert_chunks(world, 3);
        let mut search = new_search(world, BlockPos::new(0, 64, 0)).with_spawn_area_prefetch(world);
        let inside = ChunkPos::new(2, -2);
        let outside = ChunkPos::new(10, 10);
        assert!(!search.chunk_is_held(inside), "nothing is committed yet");

        for _ in 0..100 {
            world.chunk_map.advance_scheduling();
            // The empty world has no valid spawn, so the search keeps going.
            let _ = search.poll_with_time_budget(world, POLL_TIME_BUDGET);
            if search.prefetch_committed {
                break;
            }
        }
        assert!(search.prefetch_committed);
        assert!(search.chunk_is_held(inside));
        assert!(!search.chunk_is_held(outside));
    });
}

#[test]
fn safe_spawn_in_a_ceiling_dimension_avoids_fluid_and_the_roof() {
    with_search_world(
        "safe_spawn",
        &vanilla_dimension_types::THE_NETHER,
        |world| {
            insert_ready_full_chunk(world, ChunkPos::new(0, 0));
            let set = |y: i32, state| set_block(world, BlockPos::new(0, y, 0), state);
            let bedrock = vanilla_blocks::BEDROCK.default_state();
            let lava = vanilla_blocks::LAVA.default_state();

            // An open cavern: bedrock floor under a lava sea, bedrock roof at y=127. The only
            // free standing spot is on top of the roof, which must not be chosen.
            set(0, bedrock);
            for y in 1..=31 {
                set(y, lava);
            }
            set(127, bedrock);
            assert_eq!(world.safe_spawn_in_column(0, 0), None);

            // A ledge above the lava is the highest standing spot under the roof.
            set(100, vanilla_blocks::NETHERRACK.default_state());
            assert_eq!(
                world.safe_spawn_in_column(0, 0),
                Some(BlockPos::new(0, 101, 0))
            );
        },
    );
}

#[test]
fn failed_search_in_a_lava_cavern_falls_back_to_a_ledge_only_when_asked() {
    with_search_world("cavern", &vanilla_dimension_types::THE_NETHER, |world| {
        assert!(world.set_game_rule(&vanilla_game_rules::RESPAWN_RADIUS, 0));
        insert_chunks_around_origin(world);
        let set = |y: i32, state| set_block(world, BlockPos::new(0, y, 0), state);
        set(0, vanilla_blocks::BEDROCK.default_state());
        for y in 1..=31 {
            set(y, vanilla_blocks::LAVA.default_state());
        }
        set(100, vanilla_blocks::NETHERRACK.default_state());
        set(127, vanilla_blocks::BEDROCK.default_state());

        let run = |safe_fallback: bool| {
            let mut search = new_search(world, BlockPos::new(0, 64, 0));
            if safe_fallback {
                search = search.with_safe_fallback();
            }
            run_search(world, &mut search)
        };

        // The vanilla fixup walks down to the lava sea; the safe fallback uses the ledge.
        assert_eq!(run(false).y, 32.0);
        assert_eq!(run(true).y, 101.0);
    });
}

/// The four chunks around the origin, which the radius-0 search and its fixup read.
fn insert_chunks_around_origin(world: &Arc<World>) {
    for z in -1..=0 {
        for x in -1..=0 {
            insert_ready_full_chunk(world, ChunkPos::new(x, z));
        }
    }
}
