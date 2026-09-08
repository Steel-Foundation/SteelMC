use std::thread;

use rustc_hash::FxHashSet;

use crate::entity::{next_entity_id, reserve_entity_ids};

#[test]
fn a_reserved_block_owns_the_ids_that_follow_its_first() {
    let block = reserve_entity_ids(9);

    assert_eq!(block.len(), 9);
    assert!(!block.is_empty());
    for index in 0..8 {
        assert_eq!(block.part(index), block.first() + index as i32 + 1);
    }
}

#[test]
fn single_allocations_stay_unique() {
    let first = next_entity_id();
    let second = next_entity_id();

    assert_ne!(first, second);
}

#[test]
fn concurrent_reservations_never_overlap() {
    const THREADS: usize = 8;
    const BLOCKS_PER_THREAD: usize = 64;
    const BLOCK_SIZE: u32 = 9;

    let blocks = thread::scope(|scope| {
        let handles = (0..THREADS)
            .map(|_| {
                scope.spawn(|| {
                    (0..BLOCKS_PER_THREAD)
                        .map(|_| reserve_entity_ids(BLOCK_SIZE))
                        .collect::<Vec<_>>()
                })
            })
            .collect::<Vec<_>>();

        handles
            .into_iter()
            .flat_map(|handle| handle.join().expect("reservation thread panicked"))
            .collect::<Vec<_>>()
    });

    // A multipart entity hands its parts `first() + 1 ..= first() + n`, so an
    // interleaved allocation from another world thread would hand out an ID a part
    // already owns.
    let mut seen = FxHashSet::default();
    for block in &blocks {
        for offset in 0..BLOCK_SIZE {
            let id = block.first().wrapping_add(offset as i32);
            assert!(seen.insert(id), "entity id {id} was reserved twice");
        }
    }

    assert_eq!(
        seen.len(),
        THREADS * BLOCKS_PER_THREAD * BLOCK_SIZE as usize
    );
}
