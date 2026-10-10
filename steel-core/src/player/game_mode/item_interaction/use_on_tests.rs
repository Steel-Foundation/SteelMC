use std::sync::Arc;

use glam::DVec3;
use steel_registry::block_transformer::BlockTransformerComponent;
use steel_registry::data_components::UseRemainder;
use steel_registry::data_components::vanilla_components::{
    BLOCK_TRANSFORMER, USE_COOLDOWN, USE_REMAINDER, UseCooldown,
};
use steel_registry::item_stack::ItemStack;
use steel_registry::items::ItemRef;
use steel_registry::stat::vanilla_stat_types;
use steel_registry::{ItemStackTemplate, init_vanilla_registry, vanilla_blocks, vanilla_items};
use steel_utils::types::{InteractionHand, SignTextSlot, UpdateFlags};
use steel_utils::{BlockPos, BlockStateId, ChunkPos, Direction, Downcast as _};
use text_components::TextComponent;

use crate::behavior::{BlockHitResult, InteractionResult, init_behaviors};
use crate::block_entity::entities::{SignBlockEntity, SignText};
use crate::entity::{Entity as _, entities::ItemEntity};
use crate::inventory::container::Container as _;
use crate::player::Player;
use crate::test_support::{
    TestPlayerBuilder, TestWorld, fresh_test_world, insert_ready_full_chunk,
};

use super::use_item_on;

struct Interaction {
    world_fixture: TestWorld,
    player: Arc<Player>,
    pos: BlockPos,
}

impl Interaction {
    fn new(name: &'static str) -> Self {
        init_vanilla_registry();
        init_behaviors();
        let world_fixture = fresh_test_world(name);
        let world = &world_fixture.world;
        insert_ready_full_chunk(world, ChunkPos::new(0, 0));
        let player = TestPlayerBuilder::new(Arc::clone(world), "ItemUse", 1).build();
        player.base().set_position_local(DVec3::new(8.5, 64.0, 6.5));
        Self {
            world_fixture,
            player,
            pos: BlockPos::new(8, 64, 8),
        }
    }

    fn block(&self, state: BlockStateId) {
        assert!(
            self.world_fixture
                .world
                .set_block(self.pos, state, UpdateFlags::UPDATE_ALL)
        );
    }

    fn hold(&self, hand: InteractionHand, stack: ItemStack) {
        self.player.inventory.lock().set_item_in_hand(hand, stack);
    }

    fn use_on(&self, hand: InteractionHand) -> InteractionResult {
        use_item_on(
            &self.player,
            &self.world_fixture.world,
            hand,
            &BlockHitResult {
                block_pos: self.pos,
                direction: Direction::Up,
                location: DVec3::new(8.5, 64.5, 8.5),
                miss: false,
                inside: false,
                world_border_hit: false,
            },
        )
    }

    fn used(&self, item: ItemRef) -> i32 {
        let stat = vanilla_stat_types::ITEM_USED.get(item);
        self.player
            .stats()
            .into_iter()
            .find_map(|(key, count)| (key == stat).then_some(count))
            .unwrap_or(0)
    }

    fn count(&self, item: ItemRef) -> i32 {
        let inv = self.player.inventory.lock();
        (0..inv.get_container_size())
            .map(|slot| inv.get_item(slot))
            .filter(|stack| stack.is(item))
            .map(ItemStack::count)
            .sum()
    }
}

fn consuming_transformer(count: i32) -> ItemStack {
    let mut stack = ItemStack::with_count(&vanilla_items::STICK, count);
    let transformer = vanilla_items::WOODEN_SHOVEL
        .components
        .get_ref(BLOCK_TRANSFORMER)
        .expect("shovel transformer");
    stack.set(
        BLOCK_TRANSFORMER,
        BlockTransformerComponent::new(transformer.block_transformer),
    );
    stack
}

#[test]
fn extra_remainder_merges_into_the_other_hand_without_losing_inventory_changes() {
    let interaction = Interaction::new("use_on_remainder_merge");
    interaction.hold(
        InteractionHand::MainHand,
        ItemStack::with_count(&vanilla_items::DIAMOND, 63),
    );
    let mut stack = consuming_transformer(3);
    stack.set(
        USE_REMAINDER,
        UseRemainder::new(ItemStackTemplate::new(&vanilla_items::DIAMOND)),
    );
    interaction.hold(InteractionHand::OffHand, stack);
    interaction.block(vanilla_blocks::DIRT.default_state());
    assert_eq!(
        interaction.use_on(InteractionHand::OffHand),
        InteractionResult::Success
    );
    let inv = interaction.player.inventory.lock();
    assert_eq!(inv.get_selected_item().count(), 64);
    assert_eq!(inv.get_item_in_hand(InteractionHand::OffHand).count(), 2);
}

#[test]
fn extra_remainder_drops_when_the_inventory_is_full() {
    let interaction = Interaction::new("use_on_remainder_full_inventory");
    {
        let mut inv = interaction.player.inventory.lock();
        for slot in 0..36 {
            inv.set_item(slot, ItemStack::with_count(&vanilla_items::STONE, 64));
        }
    }
    let mut stack = consuming_transformer(3);
    stack.set(
        USE_REMAINDER,
        UseRemainder::new(ItemStackTemplate::new(&vanilla_items::DIAMOND)),
    );
    interaction.hold(InteractionHand::MainHand, stack);
    interaction.block(vanilla_blocks::DIRT.default_state());
    assert_eq!(
        interaction.use_on(InteractionHand::MainHand),
        InteractionResult::Success
    );
    assert_eq!(interaction.count(&vanilla_items::DIAMOND), 0);
    let entities = interaction
        .world_fixture
        .world
        .entity_manager()
        .get_accessible_entities();
    let drops: Vec<_> = entities
        .iter()
        .filter_map(|entity| entity.downcast_ref::<ItemEntity>())
        .collect();
    assert_eq!(drops.len(), 1);
    assert!(drops[0].get_item().is(&vanilla_items::DIAMOND));
    assert_eq!(drops[0].get_item().count(), 1);
    assert_eq!(
        interaction
            .player
            .inventory
            .lock()
            .get_selected_item()
            .count(),
        2
    );
}

#[test]
fn broken_tool_uses_the_original_statistics_remainder_and_cooldown() {
    let interaction = Interaction::new("use_on_broken_tool_remainder");
    let mut axe = ItemStack::new(&vanilla_items::WOODEN_AXE);
    axe.set_damage_value(axe.get_max_damage() - 1);
    axe.set(
        USE_REMAINDER,
        UseRemainder::new(ItemStackTemplate::new(&vanilla_items::DIAMOND)),
    );
    axe.set(USE_COOLDOWN, UseCooldown::new(1.0, None));
    let before_use = axe.clone();
    interaction.hold(InteractionHand::OffHand, axe);
    interaction.block(vanilla_blocks::OAK_LOG.default_state());
    assert_eq!(
        interaction.use_on(InteractionHand::OffHand),
        InteractionResult::Success
    );
    assert!(
        interaction
            .player
            .inventory
            .lock()
            .get_item_in_hand(InteractionHand::OffHand)
            .is(&vanilla_items::DIAMOND)
    );
    assert_eq!(interaction.used(&vanilla_items::WOODEN_AXE), 1);
    assert_eq!(interaction.used(&vanilla_items::DIAMOND), 0);
    assert!(interaction.player.is_item_on_cooldown(&before_use));
}

#[test]
fn sign_applicators_count_once_without_item_use_component_side_effects() {
    let applicators: [ItemRef; 3] = [
        &vanilla_items::GLOW_INK_SAC,
        &vanilla_items::INK_SAC,
        &vanilla_items::HONEYCOMB,
    ];
    for item in applicators {
        let interaction = Interaction::new("use_on_sign_side_effects");
        assert!(interaction.world_fixture.world.set_block(
            interaction.pos.below(),
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_ALL
        ));
        interaction.block(vanilla_blocks::OAK_SIGN.default_state());
        let entity = interaction
            .world_fixture
            .world
            .get_block_entity(interaction.pos)
            .expect("sign entity");
        let sign = entity.downcast_ref::<SignBlockEntity>().expect("sign");
        for slot in [SignTextSlot::Front, SignTextSlot::Back] {
            let mut text = SignText::new();
            text.set_message(0, TextComponent::plain("Read me"));
            sign.set_text(text, slot);
            if item == &*vanilla_items::INK_SAC {
                assert!(sign.set_glowing(slot, true));
            }
        }
        let mut stack = ItemStack::with_count(item, 2);
        stack.set(USE_COOLDOWN, UseCooldown::new(1.0, None));
        stack.set(
            USE_REMAINDER,
            UseRemainder::new(ItemStackTemplate::new(&vanilla_items::DIAMOND)),
        );
        let before_use = stack.clone();
        interaction.hold(InteractionHand::OffHand, stack);
        assert_eq!(
            interaction.use_on(InteractionHand::OffHand),
            InteractionResult::SuccessWithoutItem
        );
        assert_eq!(interaction.used(item), 1);
        assert_eq!(interaction.count(item), 1);
        assert_eq!(interaction.count(&vanilla_items::DIAMOND), 0);
        assert!(!interaction.player.is_item_on_cooldown(&before_use));
    }
}
