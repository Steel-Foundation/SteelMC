use steel_registry::dimension_type::DimensionTypeRef;
use steel_registry::{init_vanilla_registry, vanilla_blocks, vanilla_dimension_types};
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

        let mut search = new_search(world, suggestion);
        search.sequence.offset = 0;
        assert_eq!(run_search(world, &mut search), expected);
    });
}
