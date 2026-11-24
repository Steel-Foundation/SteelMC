use futures::executor::block_on;
use glam::DVec3;
use std::{
    future::{Future, poll_fn},
    pin::pin,
    task::{Context, Poll, Waker},
};

use steel_registry::{init_vanilla_registry, item_stack::ItemStack, vanilla_items};
use steel_utils::random::{Random, legacy_random::LegacyRandom};

use super::{vanilla::fixtures, *};
use crate::{bootstrap::init_globals, test_support::fresh_test_world};

fn tables() -> LootTableBehaviorRegistry {
    init_vanilla_registry();
    let mut tables = LootTableBehaviorRegistry::new();
    fixtures::register(&mut tables);
    tables
}

#[test]
fn exploration_destination_holders_compile_and_missing_origin_preserves_the_stack_and_rng() {
    let tables = tables();
    for table in [
        &fixtures::EXPLORATION_DIRECT_DESTINATION,
        &fixtures::EXPLORATION_DESTINATION_LIST,
        &fixtures::EXPLORATION_EMPTY_DESTINATION_LIST,
    ] {
        let mut random = LegacyRandom::from_seed(17);
        let mut untouched = LegacyRandom::from_seed(17);
        let mut context = LootContext::new(&mut random);
        context.tables = Some(&tables);
        assert!(matches!(
            table.get_random_items(&mut context),
            Err(OperationError::RequiresResumption(_))
        ));
        let items =
            block_on(table.get_random_items_async(&mut context)).expect("map without origin");
        assert_eq!(items, [ItemStack::new(&vanilla_items::MAP)]);
        assert_eq!(random.next_i64(), untouched.next_i64());
    }
}

#[test]
fn direct_and_resumable_codegen_preserve_items_and_random_state() {
    let tables = tables();
    for table in [
        &fixtures::MODIFIER_ORDER,
        &fixtures::PARTIAL_COMPOSITE_MODIFIERS,
        &fixtures::RAW_NESTED,
        &fixtures::GROUP_WEIGHT,
        &fixtures::TAG_WEIGHT,
        &fixtures::BOOK_ENCHANTMENT,
        &fixtures::PROVIDER_CHANCE,
    ] {
        for seed in 0..32 {
            let mut direct = LegacyRandom::from_seed(seed);
            let mut resumable = LegacyRandom::from_seed(seed);
            let expected = {
                let mut context = LootContext::new(&mut direct);
                context.tables = Some(&tables);
                table.get_random_items(&mut context).expect("direct loot")
            };
            let actual = {
                let mut context = LootContext::new(&mut resumable);
                context.tables = Some(&tables);
                block_on(table.get_random_items_async(&mut context)).expect("resumable loot")
            };
            assert_eq!(actual, expected, "table={}, seed={seed}", table.key);
            assert_eq!(
                direct.next_i64(),
                resumable.next_i64(),
                "table={}, seed={seed}",
                table.key
            );
        }
    }
}

struct PausingOutput {
    items: Vec<ItemStack>,
}

impl LootAsyncOutput for PausingOutput {
    fn emit<'a>(&'a mut self, _: &'a mut LootContext<'_>, item: ItemStack) -> LootFuture<'a> {
        Box::pin(async move {
            let mut paused = false;
            poll_fn(|task| {
                if paused {
                    return Poll::Ready(());
                }
                paused = true;
                task.waker().wake_by_ref();
                Poll::Pending
            })
            .await;
            self.items.push(item);
            Ok(())
        })
    }
}

#[test]
fn suspension_keeps_modifier_order_and_cancellation_restores_recursion_stack() {
    let tables = tables();
    let mut direct = LegacyRandom::from_seed(42);
    let mut context = LootContext::new(&mut direct);
    context.tables = Some(&tables);
    let mut expected = Vec::new();
    fixtures::MODIFIER_ORDER
        .emit_raw(&mut context, &mut |_, item| {
            expected.push(item);
            Ok(())
        })
        .expect("direct loot");

    let mut random = LegacyRandom::from_seed(42);
    let mut context = LootContext::new(&mut random);
    context.tables = Some(&tables);
    let mut output = PausingOutput { items: Vec::new() };
    {
        let mut future = pin!(fixtures::MODIFIER_ORDER.emit_raw_async(&mut context, &mut output));
        let mut task = Context::from_waker(Waker::noop());
        assert!(future.as_mut().poll(&mut task).is_pending());
        assert!(future.as_mut().poll(&mut task).is_pending());
        assert!(matches!(
            future.as_mut().poll(&mut task),
            Poll::Ready(Ok(()))
        ));
    }
    assert_eq!(output.items, expected);
    assert!(context.visited.current().is_none());
    drop(context);
    assert_eq!(random.next_i64(), direct.next_i64());

    let mut context = LootContext::new(&mut random);
    context.tables = Some(&tables);
    {
        let mut future = pin!(fixtures::RAW_NESTED.emit_raw_async(&mut context, &mut output));
        assert!(
            future
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
    }
    assert!(context.visited.current().is_none());
}

#[test]
fn exploration_tables_require_resumption_before_any_random_draws() {
    init_globals();
    let mut random = LegacyRandom::from_seed(17);
    let mut untouched = LegacyRandom::from_seed(17);
    let mut context = LootContext::new(&mut random);
    let table = &vanilla_loot_tables::CHESTS_SHIPWRECK_MAP;
    assert!(
        matches!(table.get_random_items(&mut context), Err(OperationError::RequiresResumption(key)) if key == table.key)
    );
    assert_eq!(random.next_i64(), untouched.next_i64());
}

#[test]
fn missing_origin_and_a_world_without_structures_discard_the_exploration_target_without_extra_rng()
{
    init_globals();
    let fixture = fresh_test_world("exploration_without_structures");
    for seed in 0..16 {
        let mut without_origin = LegacyRandom::from_seed(seed);
        let expected = block_on(
            vanilla_loot_tables::CHESTS_SHIPWRECK_MAP
                .get_random_items_async(&mut LootContext::new(&mut without_origin)),
        )
        .expect("missing origin skips lookup");
        let mut without_structures = LegacyRandom::from_seed(seed);
        let mut context = LootContext::new(&mut without_structures)
            .with_world(&fixture.world)
            .with_origin(DVec3::ZERO);
        let actual = block_on(
            vanilla_loot_tables::CHESTS_SHIPWRECK_MAP.get_random_items_async(&mut context),
        )
        .expect("empty generator has no targets");
        assert_eq!(actual, expected);
        assert!(
            actual
                .iter()
                .all(|item| !item.is(&vanilla_items::BURIED_TREASURE_MAP))
        );
        assert_eq!(without_origin.next_i64(), without_structures.next_i64());
    }
}
