use std::collections::BTreeMap;

use super::*;
use crate::maps::structures::MapStructureSearch;

struct SearchResult {
    target: BlockPos,
    claims: BTreeMap<(i32, i32, String), i32>,
    elapsed: Duration,
}

fn run_search(fixture: &VanillaFixture, ahead: bool) -> SearchResult {
    let test = MapWorld::new(fixture.seed, 2);
    let empty = fixture
        .overworld
        .chunks
        .iter()
        .find(|chunk| chunk.starts.is_empty())
        .expect("extracted empty candidate forces a search beyond radius zero");
    let origin = BlockPos::new(empty.x * 16 + 8, test.world.sea_level, empty.z * 16 + 8);
    let runtime = Arc::clone(&test.runtime);
    let result = runtime.block_on(async {
        let search = MapStructureSearch::new(
            &test.world,
            &[&vanilla_structures::STRUCTURE_BURIED_TREASURE],
            origin,
            true,
        )
        .expect("treasure placement");
        let limit = if ahead {
            test.world.chunk_map.max_running_generation_tasks()
        } else {
            1
        };
        let start = Instant::now();
        let target = test
            .drive(search.find(50, limit))
            .await
            .expect("structure search")
            .expect("nearby treasure");
        let elapsed = start.elapsed();
        assert_ne!(
            ChunkPos::from_block_pos(target),
            ChunkPos::new(empty.x, empty.z)
        );
        test.stop_generation().await;
        let mut holders = Vec::new();
        for chunks in [
            &test.world.chunk_map.chunks,
            &test.world.chunk_map.unloading_chunks,
        ] {
            chunks.iter_sync(|_, holder| {
                holders.push(Arc::clone(holder));
                true
            });
        }
        let mut claims = BTreeMap::new();
        for holder in holders {
            if let Some(chunk) = holder.try_chunk(ChunkStatus::StructureStarts) {
                let pos = holder.get_pos();
                for (key, start) in chunk.structure_starts().iter() {
                    if start.references != 0 {
                        claims.insert((pos.0.x, pos.0.y, key.to_string()), start.references);
                    }
                }
            }
        }
        SearchResult {
            target,
            claims,
            elapsed,
        }
    });
    let root = test.root.clone();
    drop(test);
    fs::remove_dir_all(root).expect("remove search world");
    result
}

#[test]
#[expect(
    clippy::print_stdout,
    reason = "reports real-generation comparison timings under --nocapture"
)]
fn lookahead_preserves_sequential_target_and_reference_claims() {
    thread::Builder::new()
        .stack_size(DEBUG_STACK_SIZE)
        .spawn(|| {
            let fixture: VanillaFixture =
                serde_json::from_str(include_str!("../../../test_assets/structure_starts.json"))
                    .expect("extracted vanilla structures");
            let sequential = run_search(&fixture, false);
            let ahead = run_search(&fixture, true);
            assert_eq!(ahead.target, sequential.target);
            assert_eq!(ahead.claims, sequential.claims);
            assert_eq!(
                ahead.claims.len(),
                1,
                "speculation must not claim unused starts"
            );
            assert_eq!(ahead.claims.values().next(), Some(&1));
            println!(
                "[map-lookahead] sequential: {:?}; lookahead: {:?}; target: {:?}",
                sequential.elapsed, ahead.elapsed, ahead.target
            );
        })
        .expect("search comparison thread")
        .join()
        .expect("search comparison");
}
