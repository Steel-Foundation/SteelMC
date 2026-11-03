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
use steel_utils::types::{GameType, InteractionHand, SignTextSlot, UpdateFlags};
use steel_utils::{BlockPos, BlockStateId, ChunkPos, Direction, Downcast as _};
use text_components::TextComponent;
use uuid::Uuid;

use crate::behavior::{
    BlockHitResult, InteractionResult, blocks::facing_text_slot, init_behaviors,
};
use crate::block_entity::SharedBlockEntity;
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

    fn sign(&self, state: BlockStateId, with_text: bool, glowing: bool) -> SharedBlockEntity {
        for direction in [
            Direction::Down,
            Direction::Up,
            Direction::North,
            Direction::South,
            Direction::West,
            Direction::East,
        ] {
            self.world_fixture.world.set_block(
                direction.relative(self.pos),
                vanilla_blocks::STONE.default_state(),
                UpdateFlags::UPDATE_ALL,
            );
        }
        self.block(state);
        let entity = self
            .world_fixture
            .world
            .get_block_entity(self.pos)
            .expect("sign entity");
        let sign = entity.downcast_ref::<SignBlockEntity>().expect("sign");
        for slot in [SignTextSlot::Front, SignTextSlot::Back] {
            let mut text = SignText::new();
            if with_text {
                text.set_message(0, TextComponent::plain("Read me"));
            }
            text.has_glowing_text = glowing;
            sign.set_text(text, slot);
        }
        entity
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
    for block in [
        &vanilla_blocks::OAK_SIGN,
        &vanilla_blocks::OAK_WALL_SIGN,
        &vanilla_blocks::OAK_HANGING_SIGN,
        &vanilla_blocks::OAK_WALL_HANGING_SIGN,
    ] {
        for (item, hand) in applicators.into_iter().flat_map(|item| {
            [InteractionHand::MainHand, InteractionHand::OffHand]
                .into_iter()
                .map(move |hand| (item, hand))
        }) {
            let interaction = Interaction::new("use_on_sign_side_effects");
            let entity = interaction.sign(
                block.default_state(),
                true,
                item == &*vanilla_items::INK_SAC,
            );
            let sign = entity.downcast_ref::<SignBlockEntity>().expect("sign");
            let mut stack = ItemStack::with_count(item, 1);
            stack.set(USE_COOLDOWN, UseCooldown::new(1.0, None));
            stack.set(
                USE_REMAINDER,
                UseRemainder::new(ItemStackTemplate::new(&vanilla_items::DIAMOND)),
            );
            let before_use = stack.clone();
            interaction.hold(hand, stack);

            let result = interaction.use_on(hand);

            assert_eq!(result, InteractionResult::Success);
            assert!(result.should_swing());
            assert_eq!(sign.get_player_who_may_edit(), None);
            if item == &*vanilla_items::HONEYCOMB {
                assert!(sign.is_waxed());
            } else {
                let slot =
                    facing_text_slot(block.default_state(), interaction.pos, &interaction.player);
                assert_eq!(
                    sign.get_text(slot).has_glowing_text,
                    item == &*vanilla_items::GLOW_INK_SAC
                );
            }
            assert_eq!(interaction.used(item), 1);
            assert_eq!(interaction.count(item), 0);
            assert_eq!(interaction.count(&vanilla_items::DIAMOND), 0);
            assert!(!interaction.player.is_item_on_cooldown(&before_use));
        }
    }
}

#[test]
fn sneaking_bypasses_sign_applicators_in_both_hands() {
    for (item, hand) in [
        &*vanilla_items::GLOW_INK_SAC,
        &*vanilla_items::INK_SAC,
        &*vanilla_items::HONEYCOMB,
    ]
    .into_iter()
    .flat_map(|item| {
        [InteractionHand::MainHand, InteractionHand::OffHand]
            .into_iter()
            .map(move |hand| (item, hand))
    }) {
        let interaction = Interaction::new("sneaking_sign_applicator");
        let glowing = item == &*vanilla_items::INK_SAC;
        let entity = interaction.sign(vanilla_blocks::OAK_SIGN.default_state(), true, glowing);
        let sign = entity.downcast_ref::<SignBlockEntity>().expect("sign");
        interaction.player.set_crouching(true);
        interaction.hold(hand, ItemStack::with_count(item, 2));

        assert_eq!(interaction.use_on(hand), InteractionResult::Pass);

        assert_eq!(interaction.count(item), 2);
        assert_eq!(interaction.used(item), 0);
        assert!(!sign.is_waxed());
        assert_eq!(sign.get_player_who_may_edit(), None);
        for slot in [SignTextSlot::Front, SignTextSlot::Back] {
            assert_eq!(sign.get_text(slot).has_glowing_text, glowing);
        }
    }
}

#[test]
fn glow_ink_rejections_do_not_consume_or_change_the_sign() {
    for (with_text, waxed, other_editor) in [
        (false, false, false),
        (true, true, false),
        (true, false, true),
    ] {
        let interaction = Interaction::new("rejected_sign_applicator");
        let entity = interaction.sign(vanilla_blocks::OAK_SIGN.default_state(), with_text, false);
        let sign = entity.downcast_ref::<SignBlockEntity>().expect("sign");
        if waxed {
            assert!(sign.wax());
        }
        if other_editor {
            sign.set_player_who_may_edit(Some(Uuid::nil()));
        }
        interaction.hold(
            InteractionHand::MainHand,
            ItemStack::with_count(&vanilla_items::GLOW_INK_SAC, 2),
        );

        interaction.use_on(InteractionHand::MainHand);

        assert_eq!(interaction.count(&vanilla_items::GLOW_INK_SAC), 2);
        assert_eq!(interaction.used(&vanilla_items::GLOW_INK_SAC), 0);
        for slot in [SignTextSlot::Front, SignTextSlot::Back] {
            assert!(!sign.get_text(slot).has_glowing_text);
        }
    }
}

#[test]
fn honeycomb_can_wax_blank_signs_without_consuming_in_creative() {
    let interaction = Interaction::new("creative_blank_sign_waxing");
    let entity = interaction.sign(vanilla_blocks::OAK_SIGN.default_state(), false, false);
    let sign = entity.downcast_ref::<SignBlockEntity>().expect("sign");
    interaction
        .player
        .restore_game_modes(GameType::Creative, None);
    interaction.hold(
        InteractionHand::MainHand,
        ItemStack::new(&vanilla_items::HONEYCOMB),
    );

    let result = interaction.use_on(InteractionHand::MainHand);

    assert_eq!(result, InteractionResult::Success);
    assert!(result.should_swing());
    assert!(sign.is_waxed());
    assert_eq!(sign.get_player_who_may_edit(), None);
    assert_eq!(interaction.count(&vanilla_items::HONEYCOMB), 1);
    assert_eq!(interaction.used(&vanilla_items::HONEYCOMB), 1);
}
