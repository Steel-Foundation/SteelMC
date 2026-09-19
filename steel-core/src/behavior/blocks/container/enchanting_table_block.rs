//! Enchanting table block behavior.

use std::sync::{Arc, Weak};

use glam::IVec3;
use steel_macros::block_behavior;
use steel_registry::{
    REGISTRY, TaggedRegistryExt,
    blocks::{BlockRef, block_state_ext::BlockStateExt},
    vanilla_block_entity_types,
    vanilla_block_tags::BlockTag,
};
use steel_utils::{BlockPos, BlockStateId, Downcast as _};

use crate::behavior::InventoryAccess;
use crate::behavior::block::{BlockBehavior, BlockEntityCreation};
use crate::behavior::context::{BlockHitResult, BlockPlaceContext, InteractionResult};
use crate::block_entity::{BLOCK_ENTITIES, entities::EnchantingTableBlockEntity};
use crate::entity::ai::path::PathComputationType;
use crate::inventory::menu::kinds::enchantment;
use crate::player::Player;
use crate::world::World;

/// Behavior for the enchanting table.
#[block_behavior]
pub struct EnchantingTableBlock {
    block: BlockRef,
}

/// Every offset two blocks out from the table, at table height and one above.
const fn bookshelf_offsets() -> [IVec3; 32] {
    let mut offsets = [IVec3::ZERO; 32];
    let mut count = 0;
    let mut y: i32 = 0;
    while y <= 1 {
        let mut x: i32 = -2;
        while x <= 2 {
            let mut z: i32 = -2;
            while z <= 2 {
                if x.abs() == 2 || z.abs() == 2 {
                    offsets[count] = IVec3::new(x, y, z);
                    count += 1;
                }
                z += 1;
            }
            x += 1;
        }
        y += 1;
    }
    offsets
}

impl EnchantingTableBlock {
    /// Vanilla `EnchantingTableBlock.BOOKSHELF_OFFSETS`.
    pub const BOOKSHELF_OFFSETS: [IVec3; 32] = bookshelf_offsets();

    /// Creates a new enchanting table block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    /// Vanilla `isValidBookShelf`: a power provider at `offset` with a power
    /// transmitter (air and other replaceables) halfway between it and the table.
    #[must_use]
    pub fn is_valid_book_shelf(world: &World, pos: BlockPos, offset: IVec3) -> bool {
        let provider = world
            .get_block_state(pos.offset(offset.x, offset.y, offset.z))
            .get_block();
        if !REGISTRY
            .blocks
            .is_in_tag(provider, &BlockTag::ENCHANTMENT_POWER_PROVIDER)
        {
            return false;
        }
        let transmitter = world
            .get_block_state(pos.offset(offset.x / 2, offset.y, offset.z / 2))
            .get_block();
        REGISTRY
            .blocks
            .is_in_tag(transmitter, &BlockTag::ENCHANTMENT_POWER_TRANSMITTER)
    }
}

impl BlockBehavior for EnchantingTableBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn new_block_entity(
        &self,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> BlockEntityCreation {
        BlockEntityCreation::from_registered_factory(BLOCK_ENTITIES.create(
            &vanilla_block_entity_types::ENCHANTING_TABLE,
            level,
            pos,
            state,
        ))
    }

    fn is_pathfindable(
        &self,
        _state: BlockStateId,
        _computation_type: PathComputationType,
    ) -> bool {
        false
    }

    /// Vanilla `useWithoutItem`: opens the menu titled with the table's display name.
    fn use_without_item(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
        _hit_result: &BlockHitResult,
        _inv: &mut InventoryAccess,
    ) -> InteractionResult {
        // Vanilla's menu provider is null without the block entity, so the use succeeds but opens nothing.
        let Some(block_entity) = world.get_block_entity(pos) else {
            return InteractionResult::Success;
        };
        let Some(table) = block_entity.downcast_ref::<EnchantingTableBlockEntity>() else {
            return InteractionResult::Success;
        };
        let title = table.display_name();
        let inventory = player.inventory.clone();
        let enchantment_seed = player.experience.lock().enchantment_seed();
        player.open_menu(title, move |context| {
            enchantment(
                inventory,
                context.container_id,
                pos,
                context.world,
                enchantment_seed,
            )
        });
        InteractionResult::Success
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use glam::DVec3;
    use steel_registry::{
        data_components::vanilla_components::CUSTOM_NAME, init_vanilla_registry,
        item_stack::ItemStack, vanilla_blocks, vanilla_items,
    };
    use steel_utils::{
        ChunkPos, Direction,
        types::{InteractionHand, UpdateFlags},
    };
    use text_components::TextComponent;

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::behavior::items::BlockItem;
    use crate::behavior::{PlacementOrientation, PlacementSource};
    use crate::block_entity::init_block_entities;
    use crate::entity::Entity as _;
    use crate::test_support::{TestPlayerBuilder, fresh_test_world, insert_ready_full_chunk};

    fn placed_table(key: &'static str) -> (Arc<World>, BlockPos) {
        init_vanilla_registry();
        init_behaviors();
        init_block_entities();
        let world = fresh_test_world(key);
        let pos = BlockPos::new(8, 64, 8);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
        assert!(world.set_block(
            pos,
            vanilla_blocks::ENCHANTING_TABLE.default_state(),
            UpdateFlags::UPDATE_ALL,
        ));
        (world, pos)
    }

    #[test]
    fn bookshelf_offsets_form_the_vanilla_ring() {
        assert_eq!(EnchantingTableBlock::BOOKSHELF_OFFSETS.len(), 32);
        for offset in EnchantingTableBlock::BOOKSHELF_OFFSETS {
            assert!(offset.x.abs() == 2 || offset.z.abs() == 2);
            assert!((0..=1).contains(&offset.y));
            assert!((-2..=2).contains(&offset.x) && (-2..=2).contains(&offset.z));
        }
    }

    #[test]
    fn placing_the_block_creates_its_block_entity() {
        let (world, pos) = placed_table("enchanting_table_block_entity");
        let block_entity = world
            .get_block_entity(pos)
            .expect("enchanting table should create a block entity");
        assert!(
            block_entity
                .downcast_ref::<EnchantingTableBlockEntity>()
                .is_some()
        );
    }

    #[test]
    fn bookshelves_count_only_with_a_clear_transmitter_block() {
        let (world, pos) = placed_table("enchanting_table_bookshelf_power");
        let offset = IVec3::new(2, 0, 0);
        assert!(!EnchantingTableBlock::is_valid_book_shelf(
            &world, pos, offset
        ));

        assert!(world.set_block(
            pos.offset(2, 0, 0),
            vanilla_blocks::BOOKSHELF.default_state(),
            UpdateFlags::UPDATE_ALL,
        ));
        assert!(EnchantingTableBlock::is_valid_book_shelf(
            &world, pos, offset
        ));

        assert!(world.set_block(
            pos.offset(1, 0, 0),
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_ALL,
        ));
        assert!(!EnchantingTableBlock::is_valid_book_shelf(
            &world, pos, offset
        ));
    }

    #[test]
    fn using_the_table_opens_a_menu_titled_with_its_name() {
        let (world, pos) = placed_table("enchanting_table_use");
        let player = TestPlayerBuilder::new(Arc::clone(&world), "TableUser", 1).build();
        player.base().set_position_local(DVec3::new(8.5, 64.0, 8.5));
        let table = world
            .get_block_entity(pos)
            .expect("enchanting table should create a block entity");
        table
            .downcast_ref::<EnchantingTableBlockEntity>()
            .expect("block entity should be an enchanting table")
            .set_custom_name(Some(TextComponent::from("Arcane Desk".to_string())));

        let behavior = EnchantingTableBlock::new(&vanilla_blocks::ENCHANTING_TABLE);
        let hit = BlockHitResult {
            location: DVec3::new(8.5, 65.0, 8.5),
            direction: Direction::Up,
            block_pos: pos,
            miss: false,
            inside: false,
            world_border_hit: false,
        };
        let mut inventory_access =
            InventoryAccess::new(Arc::clone(&player.inventory), InteractionHand::MainHand);
        let result = behavior.use_without_item(
            vanilla_blocks::ENCHANTING_TABLE.default_state(),
            &world,
            pos,
            &player,
            &hit,
            &mut inventory_access,
        );
        assert_eq!(result, InteractionResult::Success);
        assert!(player.has_container_open());
    }

    #[test]
    fn placing_a_named_table_item_names_the_block_entity() {
        init_vanilla_registry();
        init_behaviors();
        init_block_entities();
        let world = fresh_test_world("enchanting_table_place_named_item");
        let support = BlockPos::new(8, 63, 8);
        let pos = support.offset(0, 1, 0);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
        assert!(world.set_block(
            support,
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_ALL,
        ));

        let name = TextComponent::from("Arcane Desk".to_string());
        let mut stack = ItemStack::new(&vanilla_items::ENCHANTING_TABLE);
        stack.set(CUSTOM_NAME, name.clone());
        let source = PlacementSource::direct(
            None,
            InteractionHand::MainHand,
            &mut stack,
            PlacementOrientation::Directional {
                direction: Direction::North,
            },
            false,
        );
        let context = BlockPlaceContext::new(
            &world,
            source,
            &BlockHitResult {
                location: DVec3::new(8.5, 64.0, 8.5),
                direction: Direction::Up,
                block_pos: support,
                miss: false,
                inside: false,
                world_border_hit: false,
            },
        );

        assert_eq!(
            BlockItem::new(&vanilla_blocks::ENCHANTING_TABLE).place(context),
            InteractionResult::Success
        );

        let block_entity = world
            .get_block_entity(pos)
            .expect("placing the table should create its block entity");
        let table = block_entity
            .downcast_ref::<EnchantingTableBlockEntity>()
            .expect("block entity should be an enchanting table");
        assert_eq!(table.custom_name(), Some(name));
    }
}
