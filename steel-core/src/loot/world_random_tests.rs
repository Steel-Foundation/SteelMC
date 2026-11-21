use super::*;
use crate::{
    loot::{
        random_sequences::{DomainRandomSequences, RandomSequences},
        vanilla_loot_tables,
    },
    test_support::test_domain,
};
use std::task::Waker;
use steel_utils::Identifier;
use tokio::runtime::Builder;

fn poll(request: &mut LootRandomRequest) -> Poll<OperationResult<OwnedLootRandom>> {
    Pin::new(request).poll(&mut Context::from_waker(Waker::noop()))
}

fn acquire(request: &mut LootRandomRequest) -> OwnedLootRandom {
    let Poll::Ready(result) = poll(request) else {
        panic!("stream should be available");
    };
    result.expect("stream acquisition")
}

#[test]
fn world_requests_preserve_seed_priority_and_share_named_domain_ownership() {
    let worlds = test_domain("queued_loot", &["overworld", "nether"]);
    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let _domains = DomainRandomSequences::load(&worlds)
            .await
            .expect("domain sequences");
        let world = worlds.default_world("queued_loot").expect("primary world");
        let nether = worlds
            .get(&Identifier::new_static("queued_loot", "nether"))
            .expect("nether world");
        let table = &vanilla_loot_tables::CHESTS_SPAWN_BONUS_CHEST;
        let key = table.random_sequence.as_ref().expect("named sequence");
        let expected = RandomSequences::new_for_test(world.seed());
        let mut first = world.request_loot_random(Some(table), 0);
        let mut second = nether.request_loot_random(Some(table), 0);
        let mut first = acquire(&mut first);
        assert!(poll(&mut second).is_pending());
        assert_eq!(
            first.next_i64(),
            expected
                .with_random(key, |random| Ok(random.next_i64()))
                .expect("reference draw")
        );

        let mut seeded = acquire(&mut nether.request_loot_random(Some(table), 42));
        assert_eq!(seeded.next_i64(), LegacyRandom::from_seed(42).next_i64());
        assert!(poll(&mut second).is_pending());

        let live = acquire(&mut world.request_loot_random(None, 0));
        assert!(poll(&mut second).is_pending());
        drop(live);
        drop(first);
        let mut second = acquire(&mut second);
        assert_eq!(
            second.next_i64(),
            expected
                .with_random(key, |random| Ok(random.next_i64()))
                .expect("next reference draw")
        );

        let mut live_first = world.request_loot_random(None, 0);
        let mut live_second = world.request_loot_random(None, 0);
        let live_first = acquire(&mut live_first);
        assert!(poll(&mut live_second).is_pending());
        drop(live_first);
        drop(acquire(&mut live_second));
    });
}
