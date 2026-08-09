//! Vanilla chiseled bookshelf placement, interaction, and comparator behavior.

use std::sync::{Arc, Weak};

use glam::DVec3;
use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, Direction};
use steel_registry::data_components::vanilla_components::CONTAINER;
use steel_registry::item_stack::ItemStack;
use steel_registry::sound_event::SoundEventRef;
use steel_registry::vanilla_item_tags::ItemTag;
use steel_registry::{
    sound_events, vanilla_block_entity_types, vanilla_game_events, vanilla_items,
};
use steel_utils::types::InteractionHand;
use steel_utils::{BlockPos, BlockStateId, Downcast as _};

use crate::behavior::{
    BlockBehavior, BlockEntityCreation, BlockHitResult, BlockPlaceContext, InteractionResult,
    InventoryAccess, PlacementSource,
};
use crate::block_entity::BLOCK_ENTITIES;
use crate::block_entity::entities::{CHISELED_BOOKSHELF_SLOTS, ChiseledBookShelfBlockEntity};
use crate::player::Player;
use crate::world::game_event::GameEventContext;
use crate::world::{LevelReader, World};

/// Direct-interaction behavior for Vanilla's chiseled bookshelf.
///
/// The static full-block shape and six occupied properties come from extracted
/// block-state data. Structure rotation and mirroring use Steel's common facing
/// property transform, matching `HorizontalDirectionalBlock`.
#[block_behavior]
pub struct ChiseledBookShelfBlock {
    block: BlockRef,
}

impl ChiseledBookShelfBlock {
    /// Creates behavior for the chiseled bookshelf block.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    fn section_index(coordinate: f64, section_count: i32) -> usize {
        let targeted_pixel = coordinate as f32 * 16.0;
        let section_size = 16.0 / section_count as f32;
        ((targeted_pixel / section_size).floor() as i32).clamp(0, section_count - 1) as usize
    }

    fn hit_slot(state: BlockStateId, hit_result: &BlockHitResult) -> Option<usize> {
        let facing = state.get_value(&BlockStateProperties::HORIZONTAL_FACING);
        if hit_result.direction != facing {
            return None;
        }

        let hit_block_pos = hit_result.direction.relative(hit_result.block_pos);
        let relative_hit = hit_result.location
            - DVec3::new(
                f64::from(hit_block_pos.x()),
                f64::from(hit_block_pos.y()),
                f64::from(hit_block_pos.z()),
            );
        let horizontal = match facing {
            Direction::North => 1.0 - relative_hit.x,
            Direction::South => relative_hit.x,
            Direction::West => relative_hit.z,
            Direction::East => 1.0 - relative_hit.z,
            Direction::Down | Direction::Up => return None,
        };
        let row = Self::section_index(1.0 - relative_hit.y, 2);
        let column = Self::section_index(horizontal, 3);
        Some(column + row * 3)
    }

    fn insert_sound(enchanted: bool) -> SoundEventRef {
        if enchanted {
            &sound_events::BLOCK_CHISELED_BOOKSHELF_INSERT_ENCHANTED
        } else {
            &sound_events::BLOCK_CHISELED_BOOKSHELF_INSERT
        }
    }

    fn pickup_sound(item: &ItemStack) -> SoundEventRef {
        if item.is(&vanilla_items::ENCHANTED_BOOK) {
            &sound_events::BLOCK_CHISELED_BOOKSHELF_PICKUP_ENCHANTED
        } else {
            &sound_events::BLOCK_CHISELED_BOOKSHELF_PICKUP
        }
    }
}

impl BlockBehavior for ChiseledBookShelfBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state().set_value(
            &BlockStateProperties::HORIZONTAL_FACING,
            context.horizontal_direction().opposite(),
        ))
    }

    fn set_placed_by(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        source: &PlacementSource<'_>,
    ) {
        let component_items = source.with_item(|item| {
            item.get(CONTAINER).map(|contents| {
                contents
                    .items()
                    .iter()
                    .take(CHISELED_BOOKSHELF_SLOTS)
                    .map(|item| {
                        item.as_ref().map_or_else(
                            ItemStack::empty,
                            steel_registry::ItemStackTemplate::create,
                        )
                    })
                    .collect::<Vec<_>>()
            })
        });
        let Some(component_items) = component_items else {
            return;
        };
        let Some(block_entity) = world.get_block_entity(pos) else {
            return;
        };
        let Some(bookshelf) = block_entity.downcast_ref::<ChiseledBookShelfBlockEntity>() else {
            return;
        };
        bookshelf.apply_container_items(component_items);
    }

    fn use_item_on(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
        _hand: InteractionHand,
        hit_result: &BlockHitResult,
        inv: &mut InventoryAccess,
    ) -> InteractionResult {
        let is_bookshelf_book = inv
            .with_item(|item| !item.is_empty() && item.item().has_tag(&ItemTag::BOOKSHELF_BOOKS));
        if !is_bookshelf_book {
            return InteractionResult::TryEmptyHandInteraction;
        }

        let Some(slot) = Self::hit_slot(state, hit_result) else {
            return InteractionResult::Pass;
        };
        let Some(block_entity) = world.get_block_entity(pos) else {
            return InteractionResult::Pass;
        };
        let Some(bookshelf) = block_entity.downcast_ref::<ChiseledBookShelfBlockEntity>() else {
            return InteractionResult::Pass;
        };
        if bookshelf.item(slot).is_some_and(|item| !item.is_empty()) {
            return InteractionResult::TryEmptyHandInteraction;
        }

        let inserted = inv.with_item(|item| item.copy_with_count(1));
        let enchanted = inserted.is(&vanilla_items::ENCHANTED_BOOK);
        if !bookshelf.insert_book(slot, inserted) {
            return InteractionResult::Pass;
        }
        if !player.has_infinite_materials() {
            inv.with_item(|item| item.shrink(1));
        }
        world.play_block_sound(Self::insert_sound(enchanted), pos, 1.0, 1.0, None);
        // Steel does not yet have the item-used stat foundation.
        InteractionResult::Success
    }

    fn use_without_item(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
        hit_result: &BlockHitResult,
        _inv: &mut InventoryAccess,
    ) -> InteractionResult {
        let Some(slot) = Self::hit_slot(state, hit_result) else {
            return InteractionResult::Pass;
        };
        let Some(block_entity) = world.get_block_entity(pos) else {
            return InteractionResult::Pass;
        };
        let Some(bookshelf) = block_entity.downcast_ref::<ChiseledBookShelfBlockEntity>() else {
            return InteractionResult::Pass;
        };
        if bookshelf.item(slot).is_none_or(|item| item.is_empty()) {
            return InteractionResult::Consume;
        }

        let removed = bookshelf.remove_book(slot);
        if removed.is_empty() {
            return InteractionResult::Consume;
        }
        world.play_block_sound(Self::pickup_sound(&removed), pos, 1.0, 1.0, None);
        player.add_item_or_drop(removed);
        world.game_event(
            &vanilla_game_events::BLOCK_CHANGE,
            pos,
            &GameEventContext::new(Some(player), None),
        );
        InteractionResult::Success
    }

    fn affect_neighbors_after_removal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _moved_by_piston: bool,
    ) {
        world.update_neighbor_for_output_signal(pos, state.get_block());
    }

    fn new_block_entity(
        &self,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> BlockEntityCreation {
        BlockEntityCreation::from_registered_factory(BLOCK_ENTITIES.create(
            &vanilla_block_entity_types::CHISELED_BOOKSHELF,
            level,
            pos,
            state,
        ))
    }

    fn has_analog_output_signal(&self, _state: BlockStateId) -> bool {
        true
    }

    fn get_analog_output_signal(
        &self,
        _state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        world
            .get_block_entity(pos)
            .and_then(|block_entity| {
                block_entity
                    .downcast_ref::<ChiseledBookShelfBlockEntity>()
                    .map(ChiseledBookShelfBlockEntity::last_interacted_slot)
            })
            .map_or(0, |slot| slot + 1)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use steel_registry::ItemStackTemplate;
    use steel_registry::blocks::shapes::is_shape_full_block;
    use steel_registry::data_components::components::ItemContainerContents;
    use steel_registry::{vanilla_blocks, vanilla_entities};
    use steel_utils::types::{GameType, UpdateFlags};
    use steel_utils::{ChunkPos, WorldAabb};
    use uuid::Uuid;

    use super::*;
    use crate::behavior::PlacementOrientation;
    use crate::bootstrap::init_globals_once;
    use crate::entity::entities::ItemEntity;
    use crate::inventory::container::Container as _;
    use crate::test_support::{TestPlayerBuilder, fresh_test_world, insert_ready_full_chunk};

    const TEST_POS: BlockPos = BlockPos::new(8, 64, 8);

    fn state_facing(facing: Direction) -> BlockStateId {
        vanilla_blocks::CHISELED_BOOKSHELF
            .default_state()
            .set_value(&BlockStateProperties::HORIZONTAL_FACING, facing)
    }

    fn hit(facing: Direction, horizontal: f64, y: f64) -> BlockHitResult {
        let x = f64::from(TEST_POS.x());
        let block_y = f64::from(TEST_POS.y());
        let z = f64::from(TEST_POS.z());
        let location = match facing {
            Direction::North => DVec3::new(x + 1.0 - horizontal, block_y + y, z),
            Direction::South => DVec3::new(x + horizontal, block_y + y, z + 1.0),
            Direction::West => DVec3::new(x, block_y + y, z + horizontal),
            Direction::East => DVec3::new(x + 1.0, block_y + y, z + 1.0 - horizontal),
            Direction::Down | Direction::Up => unreachable!("test uses horizontal facings"),
        };
        BlockHitResult {
            location,
            direction: facing,
            block_pos: TEST_POS,
            miss: false,
            inside: false,
            world_border_hit: false,
        }
    }

    #[test]
    fn all_six_hit_regions_map_identically_for_every_horizontal_facing() {
        init_globals_once();
        let horizontal_centers = [2.5 / 16.0, 7.5 / 16.0, 13.0 / 16.0];
        for facing in Direction::HORIZONTAL {
            let state = state_facing(facing);
            for (column, horizontal) in horizontal_centers.into_iter().enumerate() {
                assert_eq!(
                    ChiseledBookShelfBlock::hit_slot(state, &hit(facing, horizontal, 0.75)),
                    Some(column),
                );
                assert_eq!(
                    ChiseledBookShelfBlock::hit_slot(state, &hit(facing, horizontal, 0.25)),
                    Some(column + 3),
                );
            }

            let mut wrong_face = hit(facing, 0.5, 0.5);
            wrong_face.direction = facing.opposite();
            assert_eq!(ChiseledBookShelfBlock::hit_slot(state, &wrong_face), None);
        }
    }

    #[test]
    fn hit_boundaries_use_vanilla_pixel_sections() {
        init_globals_once();
        let state = state_facing(Direction::South);
        let epsilon = 1.0e-6;

        assert_eq!(
            ChiseledBookShelfBlock::hit_slot(
                state,
                &hit(Direction::South, 1.0 / 3.0 - epsilon, 0.75),
            ),
            Some(0),
        );
        assert_eq!(
            ChiseledBookShelfBlock::hit_slot(state, &hit(Direction::South, 1.0 / 3.0, 0.75),),
            Some(1),
        );
        assert_eq!(
            ChiseledBookShelfBlock::hit_slot(
                state,
                &hit(Direction::South, 2.0 / 3.0 - epsilon, 0.75),
            ),
            Some(1),
        );
        assert_eq!(
            ChiseledBookShelfBlock::hit_slot(state, &hit(Direction::South, 2.0 / 3.0, 0.75),),
            Some(2),
        );
        assert_eq!(
            ChiseledBookShelfBlock::hit_slot(
                state,
                &hit(Direction::South, 2.5 / 16.0, 0.5 + epsilon),
            ),
            Some(0),
        );
        assert_eq!(
            ChiseledBookShelfBlock::hit_slot(state, &hit(Direction::South, 2.5 / 16.0, 0.5),),
            Some(3),
        );
    }

    #[test]
    fn placement_faces_the_player_and_uses_the_extracted_full_block_shape() {
        init_globals_once();
        let world = fresh_test_world("chiseled_bookshelf_placement");
        let behavior = ChiseledBookShelfBlock::new(&vanilla_blocks::CHISELED_BOOKSHELF);

        for facing in Direction::HORIZONTAL {
            let mut stack = ItemStack::new(&vanilla_items::CHISELED_BOOKSHELF);
            let source = PlacementSource::direct(
                None,
                InteractionHand::MainHand,
                &mut stack,
                PlacementOrientation::Directional { direction: facing },
                false,
            );
            let context = BlockPlaceContext::new(&world, source, &hit(Direction::North, 0.5, 0.5));
            let placed = behavior
                .get_state_for_placement(&context)
                .expect("chiseled bookshelf always has a placement state");
            assert_eq!(
                placed.get_value(&BlockStateProperties::HORIZONTAL_FACING),
                facing.opposite(),
            );
            assert!(is_shape_full_block(placed.get_static_collision_shape()));
        }
    }

    fn assert_invalid_item_is_rejected(
        behavior: &ChiseledBookShelfBlock,
        world: &Arc<World>,
        state: BlockStateId,
        player: &Player,
        inventory: &mut InventoryAccess,
    ) {
        player
            .inventory
            .lock()
            .set_selected_item(ItemStack::new(&vanilla_items::STONE));
        assert_eq!(
            behavior.use_item_on(
                state,
                world,
                TEST_POS,
                player,
                InteractionHand::MainHand,
                &hit(Direction::South, 2.5 / 16.0, 0.75),
                inventory,
            ),
            InteractionResult::TryEmptyHandInteraction,
        );
        assert!(
            player
                .inventory
                .lock()
                .get_selected_item()
                .is(&vanilla_items::STONE)
        );
    }

    fn assert_slot_zero_removal_returns_book(
        behavior: &ChiseledBookShelfBlock,
        world: &Arc<World>,
        player: &Player,
        inventory: &mut InventoryAccess,
    ) {
        player
            .inventory
            .lock()
            .set_selected_item(ItemStack::empty());
        assert_eq!(
            behavior.use_without_item(
                world.get_block_state(TEST_POS),
                world,
                TEST_POS,
                player,
                &hit(Direction::South, 2.5 / 16.0, 0.75),
                inventory,
            ),
            InteractionResult::Success,
        );
        assert!(
            !world
                .get_block_state(TEST_POS)
                .get_value(&BlockStateProperties::SLOT_0_OCCUPIED)
        );
        assert_eq!(
            behavior.get_analog_output_signal(
                world.get_block_state(TEST_POS),
                world,
                TEST_POS,
                Direction::North,
            ),
            1,
        );
        assert!(
            player
                .inventory
                .lock()
                .items()
                .iter()
                .any(|item| item.is(&vanilla_items::BOOK))
        );
    }

    #[test]
    fn interactions_update_inventory_occupied_state_and_comparator() {
        init_globals_once();
        let world = fresh_test_world("chiseled_bookshelf_interactions");
        let holder = insert_ready_full_chunk(&world, ChunkPos::from_block_pos(TEST_POS));
        let state = state_facing(Direction::South);
        assert!(world.set_block(TEST_POS, state, UpdateFlags::UPDATE_ALL));
        let behavior = ChiseledBookShelfBlock::new(&vanilla_blocks::CHISELED_BOOKSHELF);
        let player =
            TestPlayerBuilder::new(Arc::clone(&world), Uuid::from_u128(1), "BookshelfTester", 1)
                .build();
        let mut inventory =
            InventoryAccess::new(Arc::clone(&player.inventory), InteractionHand::MainHand);

        assert_invalid_item_is_rejected(&behavior, &world, state, &player, &mut inventory);

        let revision = holder.packet_content_revision();
        player
            .inventory
            .lock()
            .set_selected_item(ItemStack::with_count(&vanilla_items::BOOK, 2));
        assert_eq!(
            behavior.use_item_on(
                state,
                &world,
                TEST_POS,
                &player,
                InteractionHand::MainHand,
                &hit(Direction::South, 2.5 / 16.0, 0.75),
                &mut inventory,
            ),
            InteractionResult::Success,
        );
        assert_eq!(player.inventory.lock().get_selected_item().count(), 1);
        assert!(
            world
                .get_block_state(TEST_POS)
                .get_value(&BlockStateProperties::SLOT_0_OCCUPIED)
        );
        assert!(holder.packet_content_revision() > revision);
        assert_eq!(
            behavior.get_analog_output_signal(
                world.get_block_state(TEST_POS),
                &world,
                TEST_POS,
                Direction::North,
            ),
            1,
        );

        player.restore_game_modes(GameType::Creative, None);
        player
            .inventory
            .lock()
            .set_selected_item(ItemStack::new(&vanilla_items::ENCHANTED_BOOK));
        assert_eq!(
            behavior.use_item_on(
                world.get_block_state(TEST_POS),
                &world,
                TEST_POS,
                &player,
                InteractionHand::MainHand,
                &hit(Direction::South, 7.5 / 16.0, 0.75),
                &mut inventory,
            ),
            InteractionResult::Success,
        );
        assert_eq!(player.inventory.lock().get_selected_item().count(), 1);

        let block_entity = world
            .get_block_entity(TEST_POS)
            .expect("placed chiseled bookshelf should have a block entity");
        let bookshelf = block_entity
            .downcast_ref::<ChiseledBookShelfBlockEntity>()
            .expect("registered chiseled bookshelf should use its concrete block entity");
        let occupied_properties = [
            &BlockStateProperties::SLOT_0_OCCUPIED,
            &BlockStateProperties::SLOT_1_OCCUPIED,
            &BlockStateProperties::SLOT_2_OCCUPIED,
            &BlockStateProperties::SLOT_3_OCCUPIED,
            &BlockStateProperties::SLOT_4_OCCUPIED,
            &BlockStateProperties::SLOT_5_OCCUPIED,
        ];
        for (slot, property) in occupied_properties.iter().enumerate().skip(2) {
            assert!(bookshelf.insert_book(slot, ItemStack::new(&vanilla_items::BOOK)));
            assert!(world.get_block_state(TEST_POS).get_value(*property));
            assert_eq!(
                behavior.get_analog_output_signal(
                    world.get_block_state(TEST_POS),
                    &world,
                    TEST_POS,
                    Direction::North,
                ),
                slot as i32 + 1,
            );
        }

        assert_slot_zero_removal_returns_book(&behavior, &world, &player, &mut inventory);
    }

    #[test]
    fn enchanted_books_select_the_enchanted_insert_and_pickup_sounds() {
        init_globals_once();
        assert_eq!(
            ChiseledBookShelfBlock::insert_sound(false).key,
            sound_events::BLOCK_CHISELED_BOOKSHELF_INSERT.key,
        );
        assert_eq!(
            ChiseledBookShelfBlock::insert_sound(true).key,
            sound_events::BLOCK_CHISELED_BOOKSHELF_INSERT_ENCHANTED.key,
        );
        assert_eq!(
            ChiseledBookShelfBlock::pickup_sound(&ItemStack::new(&vanilla_items::BOOK)).key,
            sound_events::BLOCK_CHISELED_BOOKSHELF_PICKUP.key,
        );
        assert_eq!(
            ChiseledBookShelfBlock::pickup_sound(&ItemStack::new(&vanilla_items::ENCHANTED_BOOK,))
                .key,
            sound_events::BLOCK_CHISELED_BOOKSHELF_PICKUP_ENCHANTED.key,
        );
    }

    #[test]
    fn placement_applies_the_container_component_to_the_block_entity() {
        init_globals_once();
        let world = fresh_test_world("chiseled_bookshelf_component_placement");
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(TEST_POS));
        let state = state_facing(Direction::North);
        assert!(world.set_block(TEST_POS, state, UpdateFlags::UPDATE_ALL));
        let behavior = ChiseledBookShelfBlock::new(&vanilla_blocks::CHISELED_BOOKSHELF);

        let contents = ItemContainerContents::new(vec![
            Some(ItemStackTemplate::new(&vanilla_items::BOOK)),
            None,
            Some(ItemStackTemplate::new(&vanilla_items::ENCHANTED_BOOK)),
        ])
        .expect("three component slots are valid");
        let mut placed_item = ItemStack::new(&vanilla_items::CHISELED_BOOKSHELF);
        placed_item.set(CONTAINER, contents);
        let source = PlacementSource::direct(
            None,
            InteractionHand::MainHand,
            &mut placed_item,
            PlacementOrientation::Directional {
                direction: Direction::South,
            },
            false,
        );
        behavior.set_placed_by(state, &world, TEST_POS, &source);

        let block_entity = world
            .get_block_entity(TEST_POS)
            .expect("placed chiseled bookshelf should have a block entity");
        let bookshelf = block_entity
            .downcast_ref::<ChiseledBookShelfBlockEntity>()
            .expect("registered chiseled bookshelf should use its concrete block entity");
        assert!(
            bookshelf
                .item(0)
                .is_some_and(|item| item.is(&vanilla_items::BOOK))
        );
        assert!(bookshelf.item(1).is_some_and(|item| item.is_empty()));
        assert!(
            bookshelf
                .item(2)
                .is_some_and(|item| item.is(&vanilla_items::ENCHANTED_BOOK))
        );
        assert_eq!(bookshelf.last_interacted_slot(), -1);
    }

    #[test]
    fn destruction_drains_and_drops_every_stored_book() {
        init_globals_once();
        let world = fresh_test_world("chiseled_bookshelf_drops");
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(TEST_POS));
        let state = state_facing(Direction::North);
        assert!(world.set_block(TEST_POS, state, UpdateFlags::UPDATE_ALL));
        let block_entity = world
            .get_block_entity(TEST_POS)
            .expect("placed chiseled bookshelf should have a block entity");
        let bookshelf = block_entity
            .downcast_ref::<ChiseledBookShelfBlockEntity>()
            .expect("registered chiseled bookshelf should use its concrete block entity");
        for slot in 0..5 {
            assert!(bookshelf.insert_book(slot, ItemStack::new(&vanilla_items::BOOK)));
        }
        assert!(bookshelf.insert_book(5, ItemStack::new(&vanilla_items::ENCHANTED_BOOK),));

        assert!(world.set_block(
            TEST_POS,
            vanilla_blocks::AIR.default_state(),
            UpdateFlags::UPDATE_ALL,
        ));
        assert!(world.get_block_entity(TEST_POS).is_none());
        for slot in 0..CHISELED_BOOKSHELF_SLOTS {
            assert!(bookshelf.item(slot).is_some_and(|item| item.is_empty()));
        }

        let drops = world.get_entities_in_aabb_matching(
            &WorldAabb::new(
                f64::from(TEST_POS.x()) - 1.0,
                f64::from(TEST_POS.y()) - 1.0,
                f64::from(TEST_POS.z()) - 1.0,
                f64::from(TEST_POS.x()) + 2.0,
                f64::from(TEST_POS.y()) + 2.0,
                f64::from(TEST_POS.z()) + 2.0,
            ),
            |entity| entity.entity_type() == &vanilla_entities::ITEM,
        );
        assert_eq!(drops.len(), CHISELED_BOOKSHELF_SLOTS);
        let dropped_items = drops
            .iter()
            .filter_map(|entity| {
                entity
                    .downcast_ref::<ItemEntity>()
                    .map(ItemEntity::get_item)
            })
            .collect::<Vec<_>>();
        assert_eq!(dropped_items.len(), CHISELED_BOOKSHELF_SLOTS);
        assert_eq!(
            dropped_items
                .iter()
                .filter(|item| item.is(&vanilla_items::BOOK))
                .map(ItemStack::count)
                .sum::<i32>(),
            5
        );
        assert_eq!(
            dropped_items
                .iter()
                .filter(|item| item.is(&vanilla_items::ENCHANTED_BOOK))
                .map(ItemStack::count)
                .sum::<i32>(),
            1
        );
    }
}
