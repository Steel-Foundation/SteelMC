use glam::DVec3;
use steel_macros::item_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, RailShape};
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_game_events;

use crate::behavior::{BLOCK_BEHAVIORS, InteractionResult, ItemBehavior, UseOnContext};
use crate::entity::entities::objects::vehicles::create_minecart;
use crate::world::game_event::GameEventContext;

const RAIL_HEIGHT_OFFSET: f64 = 0.0625;
const SLOPED_RAIL_HEIGHT_OFFSET: f64 = 0.5;

/// Item behavior for minecart entity.
#[item_behavior(class = "MinecartItem")]
pub struct MinecartItem {
    #[json_arg(vanilla_entities, json = "type")]
    entity_type: EntityTypeRef,
}

impl MinecartItem {
    /// Creates a minecart item behavior.
    #[must_use]
    pub const fn new(entity_type: EntityTypeRef) -> Self {
        Self { entity_type }
    }
}

impl ItemBehavior for MinecartItem {
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        let pos = context.hit_result.block_pos;
        let state = context.world.get_block_state(pos);
        let block = state.get_block();

        if !block.has_tag(&BlockTag::RAILS) {
            return InteractionResult::Fail;
        }

        let shape = if BLOCK_BEHAVIORS.get_behavior(block).as_rail().is_some() {
            state.get_value(&BlockStateProperties::RAIL_SHAPE)
        } else {
            RailShape::NorthSouth
        };

        let offset = if shape.is_slope() {
            SLOPED_RAIL_HEIGHT_OFFSET
        } else {
            0.0
        };
        let position = DVec3::new(
            f64::from(pos.x()) + 0.5,
            f64::from(pos.y()) + RAIL_HEIGHT_OFFSET + offset,
            f64::from(pos.z()) + 0.5,
        );

        let stack = context.inv.with_item(|stack| stack.clone());
        let Some(cart) = create_minecart(
            context.world,
            position,
            self.entity_type,
            &stack,
            Some(context.player),
        ) else {
            return InteractionResult::Fail;
        };

        // TODO: When experimental movement is enabled, it checks if there's a minecart at the
        // location and fails the interaction if there is one.

        if let Err(error) = context.world.try_add_entity(cart) {
            log::warn!("Failed to add minecart {}: {error}", self.entity_type.key);
            return InteractionResult::Fail;
        }

        context.world.game_event(
            &vanilla_game_events::ENTITY_PLACE,
            pos,
            &GameEventContext::new(
                Some(context.player.as_ref()),
                Some(context.world.get_block_state(pos.below())),
            ),
        );

        context.inv.with_item(ItemStack::shrink_one);

        InteractionResult::Success
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use glam::DVec3;
    use steel_registry::blocks::{
        BlockRef,
        block_state_ext::BlockStateExt as _,
        properties::{BlockStateProperties, RailShape},
    };
    use steel_registry::data_components::vanilla_components::CUSTOM_NAME;
    use steel_registry::item_stack::ItemStack;
    use steel_registry::stat::vanilla_stat_types;
    use steel_registry::{vanilla_blocks, vanilla_entities, vanilla_items};
    use steel_utils::types::{GameType, InteractionHand, UpdateFlags};
    use steel_utils::{BlockPos, ChunkPos, Direction, WorldAabb};
    use text_components::TextComponent;

    use crate::behavior::{BlockHitResult, InteractionResult};
    use crate::bootstrap::init_globals;
    use crate::entity::{Entity, SharedEntity, next_entity_id};
    use crate::player::{Player, game_mode::use_item_on};
    use crate::test_support::{
        TestPlayerBuilder, TestWorld, fresh_test_world, insert_ready_full_chunk,
    };

    struct Fixture {
        world_fixture: TestWorld,
        player: Arc<Player>,
        pos: BlockPos,
    }

    fn create_fixture(
        key: &'static str,
        pos: BlockPos,
        block: BlockRef,
        shape: Option<RailShape>,
        game_mode: GameType,
    ) -> Fixture {
        init_globals();

        let world_fixture = fresh_test_world(key);
        let world = &world_fixture.world;
        insert_ready_full_chunk(world, ChunkPos::from_block_pos(pos));

        let flags = UpdateFlags::UPDATE_NONE | UpdateFlags::UPDATE_SKIP_ON_PLACE;

        for support in [
            pos.below(),
            pos.north(),
            pos.south(),
            pos.east(),
            pos.west(),
        ] {
            assert!(world.set_block(support, vanilla_blocks::STONE.default_state(), flags,));
        }

        let state = match shape {
            Some(shape) => block
                .default_state()
                .set_value(&BlockStateProperties::RAIL_SHAPE, shape),
            None => block.default_state(),
        };
        assert!(world.set_block(pos, state, flags));
        assert_eq!(world.get_block_state(pos), state);

        let player =
            TestPlayerBuilder::new(Arc::clone(world), "CartTester", next_entity_id()).build();

        player
            .try_set_position(DVec3::new(
                f64::from(pos.x()) + 0.5,
                f64::from(pos.y()) + 1.0,
                f64::from(pos.z()) + 2.5,
            ))
            .expect("test player should move into the loaded chunk");

        player.restore_game_modes(game_mode, None);
        player.abilities.lock().update_for_game_mode(game_mode);

        Fixture {
            world_fixture,
            player,
            pos,
        }
    }

    fn other_hand(hand: InteractionHand) -> InteractionHand {
        match hand {
            InteractionHand::MainHand => InteractionHand::OffHand,
            InteractionHand::OffHand => InteractionHand::MainHand,
        }
    }

    fn prepare_hand(player: &Player, hand: InteractionHand, stack: ItemStack) {
        let mut inventory = player.inventory.lock();
        inventory.set_item_in_hand(hand, stack);
        inventory.set_item_in_hand(other_hand(hand), ItemStack::new(&vanilla_items::STONE));
    }

    fn held_stack(player: &Player, hand: InteractionHand) -> ItemStack {
        player.inventory.lock().get_item_in_hand(hand).clone()
    }

    fn use_cart(fixture: &Fixture, hand: InteractionHand) -> InteractionResult {
        let hit = BlockHitResult {
            location: DVec3::new(
                f64::from(fixture.pos.x()) + 0.5,
                f64::from(fixture.pos.y()) + 1.0,
                f64::from(fixture.pos.z()) + 0.5,
            ),
            direction: Direction::Up,
            block_pos: fixture.pos,
            miss: false,
            inside: false,
            world_border_hit: false,
        };

        use_item_on(&fixture.player, &fixture.world_fixture.world, hand, &hit)
    }

    fn minecart_use_count(player: &Player) -> i32 {
        let target = vanilla_stat_types::ITEM_USED.get(&vanilla_items::MINECART);

        player
            .stats()
            .into_iter()
            .find_map(|(stat, count)| (stat == target).then_some(count))
            .unwrap_or(0)
    }

    fn carts_at(fixture: &Fixture) -> Vec<SharedEntity> {
        let pos = fixture.pos;
        let bounds = WorldAabb::new(
            f64::from(pos.x()) - 2.0,
            f64::from(pos.y()) - 1.0,
            f64::from(pos.z()) - 2.0,
            f64::from(pos.x()) + 3.0,
            f64::from(pos.y()) + 3.0,
            f64::from(pos.z()) + 3.0,
        );

        fixture
            .world_fixture
            .world
            .get_entities_in_aabb_matching(&bounds, |entity| {
                entity.entity_type() == &vanilla_entities::MINECART
            })
    }

    fn single_cart(fixture: &Fixture) -> SharedEntity {
        let mut carts = carts_at(fixture);
        assert_eq!(carts.len(), 1, "placement should create exactly one cart");
        carts.pop().expect("the single cart should be present")
    }

    #[test]
    fn non_rails_fail_without_consumption_spawning_or_statistics() {
        init_globals();

        for hand in [InteractionHand::MainHand, InteractionHand::OffHand] {
            let fixture = create_fixture(
                "minecart_item_nonrail",
                BlockPos::new(8, 64, 8),
                &vanilla_blocks::STONE,
                None,
                GameType::Survival,
            );

            let mut stack = ItemStack::new(&vanilla_items::MINECART);
            stack.set(CUSTOM_NAME, TextComponent::plain("Unused Cart"));
            let original = stack.clone();
            prepare_hand(&fixture.player, hand, stack);

            assert_eq!(use_cart(&fixture, hand), InteractionResult::Fail);
            assert_eq!(held_stack(&fixture.player, hand), original);
            assert_eq!(
                held_stack(&fixture.player, other_hand(hand)),
                ItemStack::new(&vanilla_items::STONE),
            );
            assert!(carts_at(&fixture).is_empty());
            assert_eq!(minecart_use_count(&fixture.player), 0);
        }
    }

    #[test]
    fn placement_uses_the_clicked_rail_height_position_and_hand() {
        init_globals();

        let cases: &[(BlockRef, RailShape, f64)] = &[
            (&vanilla_blocks::RAIL, RailShape::NorthSouth, 0.0625),
            (&vanilla_blocks::RAIL, RailShape::EastWest, 0.0625),
            (&vanilla_blocks::RAIL, RailShape::AscendingEast, 0.5625),
            (&vanilla_blocks::RAIL, RailShape::AscendingWest, 0.5625),
            (&vanilla_blocks::RAIL, RailShape::AscendingNorth, 0.5625),
            (&vanilla_blocks::RAIL, RailShape::AscendingSouth, 0.5625),
            (&vanilla_blocks::POWERED_RAIL, RailShape::EastWest, 0.0625),
            (
                &vanilla_blocks::POWERED_RAIL,
                RailShape::AscendingWest,
                0.5625,
            ),
            (&vanilla_blocks::DETECTOR_RAIL, RailShape::EastWest, 0.0625),
            (
                &vanilla_blocks::DETECTOR_RAIL,
                RailShape::AscendingWest,
                0.5625,
            ),
            (&vanilla_blocks::ACTIVATOR_RAIL, RailShape::EastWest, 0.0625),
            (
                &vanilla_blocks::ACTIVATOR_RAIL,
                RailShape::AscendingWest,
                0.5625,
            ),
        ];

        for pos in [BlockPos::new(8, 64, 8), BlockPos::new(-8, 64, -8)] {
            for &(block, shape, height_offset) in cases {
                for hand in [InteractionHand::MainHand, InteractionHand::OffHand] {
                    let fixture = create_fixture(
                        "minecart_item_placement",
                        pos,
                        block,
                        Some(shape),
                        GameType::Survival,
                    );
                    prepare_hand(
                        &fixture.player,
                        hand,
                        ItemStack::new(&vanilla_items::MINECART),
                    );

                    assert_eq!(use_cart(&fixture, hand), InteractionResult::Success);

                    let cart = single_cart(&fixture);
                    assert_eq!(
                        cart.position(),
                        DVec3::new(
                            f64::from(pos.x()) + 0.5,
                            f64::from(pos.y()) + height_offset,
                            f64::from(pos.z()) + 0.5,
                        ),
                    );
                    assert!(held_stack(&fixture.player, hand).is_empty());
                    assert_eq!(
                        held_stack(&fixture.player, other_hand(hand)),
                        ItemStack::new(&vanilla_items::STONE),
                    );
                    assert_eq!(minecart_use_count(&fixture.player), 1);
                }
            }
        }
    }

    #[test]
    fn placing_the_last_item_transfers_its_custom_name_before_consumption() {
        init_globals();

        let fixture = create_fixture(
            "minecart_item_custom_name",
            BlockPos::new(8, 64, 8),
            &vanilla_blocks::RAIL,
            Some(RailShape::NorthSouth),
            GameType::Survival,
        );
        let name = TextComponent::plain("Rail Runner");
        let mut stack = ItemStack::new(&vanilla_items::MINECART);
        stack.set(CUSTOM_NAME, name.clone());
        prepare_hand(&fixture.player, InteractionHand::MainHand, stack);

        assert_eq!(
            use_cart(&fixture, InteractionHand::MainHand),
            InteractionResult::Success,
        );

        let cart = single_cart(&fixture);
        assert_eq!(cart.custom_name(), Some(name));
        assert!(held_stack(&fixture.player, InteractionHand::MainHand).is_empty());
        assert_eq!(minecart_use_count(&fixture.player), 1);
    }
}
