//! Chiseled bookshelf block entity implementation.

use std::io;
use std::mem;
use std::sync::{Arc, Weak};

use simdnbt::ToNbtTag;
use simdnbt::borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView};
use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use steel_registry::ItemStackTemplate;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, BoolProperty};
use steel_registry::data_components::components::ItemContainerContents;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_block_entity_types;
use steel_registry::vanilla_item_tags::ItemTag;
use steel_registry::{vanilla_blocks, vanilla_game_events};
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId, DowncastType, DowncastTypeKey, locks::SyncMutex};

use crate::block_entity::{BlockEntity, BlockEntityBase};
use crate::inventory::container::Container;
use crate::inventory::lock::{ContainerLockGuard, ContainerRef, SharedContainer};
use crate::world::{LevelReader as _, World, game_event::GameEventContext};

/// Number of slots in a chiseled bookshelf.
pub const CHISELED_BOOKSHELF_SLOTS: usize = 6;

const DEFAULT_LAST_INTERACTED_SLOT: i32 = -1;
const ITEMS_NBT_KEY: &str = "Items";
const ITEM_SLOT_NBT_KEY: &str = "Slot";
const LAST_INTERACTED_SLOT_NBT_KEY: &str = "last_interacted_slot";
const MAX_BOOKS_PER_SLOT: i32 = 1;

const OCCUPIED_PROPERTIES: [&BoolProperty; CHISELED_BOOKSHELF_SLOTS] = [
    &BlockStateProperties::SLOT_0_OCCUPIED,
    &BlockStateProperties::SLOT_1_OCCUPIED,
    &BlockStateProperties::SLOT_2_OCCUPIED,
    &BlockStateProperties::SLOT_3_OCCUPIED,
    &BlockStateProperties::SLOT_4_OCCUPIED,
    &BlockStateProperties::SLOT_5_OCCUPIED,
];

struct ChiseledBookShelfContainer {
    items: Vec<ItemStack>,
    last_interacted_slot: i32,
    state_update_pending: bool,
}

/// Six-slot storage for a chiseled bookshelf.
pub struct ChiseledBookShelfBlockEntity {
    base: Arc<BlockEntityBase>,
    container: Arc<SyncMutex<ChiseledBookShelfContainer>>,
    container_ref: ContainerRef,
}

// SAFETY: This key is owned by Steel and uniquely identifies
// `ChiseledBookShelfBlockEntity`.
unsafe impl DowncastType for ChiseledBookShelfBlockEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:block_entity/chiseled_bookshelf");
}

// SAFETY: This key is owned by Steel and uniquely identifies the independently
// lockable inventory data used by a chiseled bookshelf block entity.
unsafe impl DowncastType for ChiseledBookShelfContainer {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:container/chiseled_bookshelf");
}

impl ChiseledBookShelfBlockEntity {
    /// Creates a chiseled bookshelf block entity.
    #[must_use]
    pub fn new(level: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
        let callback_level = Weak::clone(&level);
        let base = Arc::new(BlockEntityBase::new(
            &vanilla_block_entity_types::CHISELED_BOOKSHELF,
            level,
            pos,
            state,
        ));
        let container = Arc::new(SyncMutex::new(ChiseledBookShelfContainer {
            items: (0..CHISELED_BOOKSHELF_SLOTS)
                .map(|_| ItemStack::empty())
                .collect(),
            last_interacted_slot: DEFAULT_LAST_INTERACTED_SLOT,
            state_update_pending: false,
        }));
        let callback_container = Arc::clone(&container);
        let after_changed = Arc::new(move || {
            Self::publish_pending_state(&callback_level, pos, &callback_container);
        });
        let shared_container: SharedContainer = container.clone();

        Self {
            container_ref: ContainerRef::owned_by_block_entity_with_callback(
                shared_container,
                Arc::clone(&base),
                after_changed,
            ),
            base,
            container,
        }
    }

    fn publish_pending_state(
        level: &Weak<World>,
        pos: BlockPos,
        container: &SyncMutex<ChiseledBookShelfContainer>,
    ) {
        let occupied = {
            let mut container = container.lock();
            if !container.state_update_pending {
                return;
            }
            container.state_update_pending = false;
            container
                .items
                .iter()
                .map(|item| !item.is_empty())
                .collect::<Vec<_>>()
        };

        let Some(world) = level.upgrade() else {
            return;
        };
        let state = world.get_block_state(pos);
        if state.get_block() != &vanilla_blocks::CHISELED_BOOKSHELF {
            return;
        }

        let mut updated_state = state;
        for (property, occupied) in OCCUPIED_PROPERTIES.iter().zip(occupied) {
            updated_state = updated_state.set_value(*property, occupied);
        }

        world.set_block(pos, updated_state, UpdateFlags::UPDATE_ALL);
        world.game_event(
            &vanilla_game_events::BLOCK_CHANGE,
            pos,
            &GameEventContext::new(None, Some(updated_state)),
        );
    }

    /// Inserts a valid bookshelf book into an empty slot.
    #[must_use]
    pub fn insert_book(&self, slot: usize, item: ItemStack) -> bool {
        if slot >= CHISELED_BOOKSHELF_SLOTS
            || item.is_empty()
            || !item.item().has_tag(&ItemTag::BOOKSHELF_BOOKS)
        {
            return false;
        }

        let mut guard = ContainerLockGuard::lock_all(&[&self.container_ref]);
        let Some(container) = guard.get(self.container_ref.container_id()) else {
            return false;
        };
        if !container.get_item(slot).is_empty() {
            return false;
        }
        guard.set_item(self.container_ref.container_id(), slot, item)
    }

    /// Removes the entire stack from a slot, matching Vanilla's count-ignoring override.
    #[must_use]
    pub fn remove_book(&self, slot: usize) -> ItemStack {
        if slot >= CHISELED_BOOKSHELF_SLOTS {
            return ItemStack::empty();
        }
        let mut guard = ContainerLockGuard::lock_all(&[&self.container_ref]);
        guard
            .remove_item(self.container_ref.container_id(), slot, MAX_BOOKS_PER_SLOT)
            .unwrap_or_else(ItemStack::empty)
    }

    /// Returns a copy of the item stored in `slot`.
    #[must_use]
    pub fn item(&self, slot: usize) -> Option<ItemStack> {
        let container = self.container.lock();
        let item = container.items.get(slot)?;
        Some(item.copy_with_count(item.count()))
    }

    /// Returns the last slot changed by insertion, removal, or automation.
    #[must_use]
    pub fn last_interacted_slot(&self) -> i32 {
        self.container.lock().last_interacted_slot
    }

    /// Applies the `minecraft:container` component from a placed block item.
    pub fn apply_container_items(&self, items: Vec<ItemStack>) {
        let mut container = self.container.lock();
        container.items.fill_with(ItemStack::empty);
        for (destination, item) in container.items.iter_mut().zip(items) {
            *destination = item;
        }
        container.state_update_pending = false;
        drop(container);
        BlockEntity::set_changed(self);
    }

    /// Collects the inventory as Vanilla's implicit `minecraft:container` component.
    pub fn collect_container_component(&self) -> io::Result<ItemContainerContents> {
        let container = self.container.lock();
        let Some(last_non_empty) = container.items.iter().rposition(|item| !item.is_empty()) else {
            return Ok(ItemContainerContents::empty());
        };

        let mut items = Vec::with_capacity(last_non_empty + 1);
        for item in &container.items[..=last_non_empty] {
            if item.is_empty() {
                items.push(None);
            } else {
                items.push(Some(ItemStackTemplate::from_stack(item)?));
            }
        }
        ItemContainerContents::new(items)
    }
}

impl BlockEntity for ChiseledBookShelfBlockEntity {
    fn base(&self) -> &BlockEntityBase {
        &self.base
    }

    fn pre_remove_side_effects(&self, pos: BlockPos, _state: BlockStateId) {
        let items = {
            let mut container = self.container.lock();
            container.state_update_pending = false;
            mem::replace(
                &mut container.items,
                (0..CHISELED_BOOKSHELF_SLOTS)
                    .map(|_| ItemStack::empty())
                    .collect(),
            )
        };
        let Some(world) = self.get_level() else {
            return;
        };
        for item in items {
            world.drop_item_stack(pos, item);
        }
    }

    fn load_additional(&self, nbt: &BorrowedNbtCompound<'_>) {
        let nbt: NbtCompoundView<'_, '_> = nbt.into();
        let mut container = self.container.lock();
        container.items.fill_with(ItemStack::empty);
        container.state_update_pending = false;

        if let Some(items) = nbt.list(ITEMS_NBT_KEY)
            && let Some(compounds) = items.compounds()
        {
            for compound in compounds {
                let Some(slot) = compound.byte(ITEM_SLOT_NBT_KEY) else {
                    continue;
                };
                let Ok(slot) = usize::try_from(slot) else {
                    continue;
                };
                if slot < CHISELED_BOOKSHELF_SLOTS
                    && let Some(item) = ItemStack::from_borrowed_compound(&compound)
                {
                    container.items[slot] = item;
                }
            }
        }
        container.last_interacted_slot = nbt
            .int(LAST_INTERACTED_SLOT_NBT_KEY)
            .unwrap_or(DEFAULT_LAST_INTERACTED_SLOT);
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        let container = self.container.lock();
        let mut items = Vec::new();
        for (slot, item) in container.items.iter().enumerate() {
            if item.is_empty() {
                continue;
            }
            if let NbtTag::Compound(mut item_nbt) = item.copy_with_count(item.count()).to_nbt_tag()
            {
                item_nbt.insert(ITEM_SLOT_NBT_KEY, slot as i8);
                items.push(item_nbt);
            }
        }
        nbt.insert(ITEMS_NBT_KEY, NbtList::Compound(items));
        nbt.insert(LAST_INTERACTED_SLOT_NBT_KEY, container.last_interacted_slot);
    }

    fn container_ref(&self) -> Option<ContainerRef> {
        Some(self.container_ref.clone())
    }
}

impl Container for ChiseledBookShelfContainer {
    fn items(&self) -> &[ItemStack] {
        &self.items
    }

    fn items_mut(&mut self) -> &mut [ItemStack] {
        &mut self.items
    }

    fn set_item(&mut self, slot: usize, stack: ItemStack) {
        if slot >= CHISELED_BOOKSHELF_SLOTS {
            return;
        }
        if stack.is_empty() {
            let _ = self.remove_item(slot, MAX_BOOKS_PER_SLOT);
            return;
        }
        if !stack.item().has_tag(&ItemTag::BOOKSHELF_BOOKS) {
            return;
        }

        self.items[slot] = stack;
        self.last_interacted_slot = slot as i32;
        self.state_update_pending = true;
    }

    fn remove_item(&mut self, slot: usize, _count: i32) -> ItemStack {
        let Some(item) = self.items.get_mut(slot) else {
            return ItemStack::empty();
        };
        let removed = mem::take(item);
        if !removed.is_empty() {
            self.last_interacted_slot = slot as i32;
            self.state_update_pending = true;
        }
        removed
    }

    fn get_max_stack_size(&self) -> i32 {
        MAX_BOOKS_PER_SLOT
    }

    fn set_changed(&mut self) {}

    fn can_place_item(&self, _slot: usize, stack: &ItemStack) -> bool {
        stack.item().has_tag(&ItemTag::BOOKSHELF_BOOKS)
    }

    fn can_take_item(&self, destination: &dyn Container, _slot: usize, stack: &ItemStack) -> bool {
        destination.items().iter().any(|destination_item| {
            destination_item.is_empty()
                || (ItemStack::is_same_item_same_components(stack, destination_item)
                    && destination_item.count() + stack.count()
                        <= destination.get_max_stack_size_for_item(destination_item))
        })
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::borrow::read_compound as read_borrowed_compound;
    use steel_registry::{init_vanilla_registry, vanilla_items};

    use super::*;
    use crate::inventory::container::SimpleContainer;

    const TEST_POS: BlockPos = BlockPos::new(1, 2, 3);
    const FIRST_SLOT_INDEX: usize = 0;
    const LAST_SLOT_INDEX: usize = CHISELED_BOOKSHELF_SLOTS - 1;
    const INTERACTION_TEST_SLOT: usize = 4;
    const OVERSIZED_TEST_STACK_COUNT: i32 = MAX_BOOKS_PER_SLOT + 1;
    const DESTINATION_SLOT_COUNT: usize = 1;
    const DESTINATION_SLOT_INDEX: usize = 0;
    const WRITABLE_BOOK_COMPONENT_SLOT: usize = 0;
    const EMPTY_COMPONENT_SLOT: usize = 1;
    const ENCHANTED_BOOK_COMPONENT_SLOT: usize = 2;
    const FIRST_UNSPECIFIED_COMPONENT_SLOT: usize = 3;

    fn test_bookshelf() -> ChiseledBookShelfBlockEntity {
        init_vanilla_registry();
        ChiseledBookShelfBlockEntity::new(
            Weak::new(),
            TEST_POS,
            vanilla_blocks::CHISELED_BOOKSHELF.default_state(),
        )
    }

    #[test]
    fn bookshelf_book_tag_controls_insertion_and_container_capability() {
        let bookshelf = test_bookshelf();
        let valid_items = [
            &vanilla_items::BOOK,
            &vanilla_items::WRITABLE_BOOK,
            &vanilla_items::WRITTEN_BOOK,
            &vanilla_items::KNOWLEDGE_BOOK,
            &vanilla_items::ENCHANTED_BOOK,
        ];

        for (slot, item) in valid_items.into_iter().enumerate() {
            assert!(bookshelf.insert_book(slot, ItemStack::new(item)));
        }
        let invalid_item = ItemStack::new(&vanilla_items::STONE);
        assert!(!bookshelf.insert_book(LAST_SLOT_INDEX, invalid_item));

        let container = bookshelf.container.lock();
        assert_eq!(container.get_max_stack_size(), MAX_BOOKS_PER_SLOT);
        let valid_item = ItemStack::new(&vanilla_items::BOOK);
        let invalid_item = ItemStack::new(&vanilla_items::STONE);
        assert!(container.can_place_item(FIRST_SLOT_INDEX, &valid_item));
        assert!(!container.can_place_item(FIRST_SLOT_INDEX, &invalid_item));
    }

    #[test]
    fn insertion_and_removal_update_occupancy_and_last_slot_storage() {
        let bookshelf = test_bookshelf();
        assert_eq!(
            bookshelf.last_interacted_slot(),
            DEFAULT_LAST_INTERACTED_SLOT
        );

        assert!(bookshelf.insert_book(
            INTERACTION_TEST_SLOT,
            ItemStack::with_count(&vanilla_items::WRITTEN_BOOK, OVERSIZED_TEST_STACK_COUNT),
        ));
        assert_eq!(
            bookshelf.last_interacted_slot(),
            INTERACTION_TEST_SLOT as i32,
        );
        assert_eq!(
            bookshelf
                .item(INTERACTION_TEST_SLOT)
                .map(|item| item.count()),
            Some(OVERSIZED_TEST_STACK_COUNT),
        );
        let replacement = ItemStack::new(&vanilla_items::BOOK);
        assert!(!bookshelf.insert_book(INTERACTION_TEST_SLOT, replacement));

        let removed = bookshelf.remove_book(INTERACTION_TEST_SLOT);
        assert!(removed.is(&vanilla_items::WRITTEN_BOOK));
        assert_eq!(removed.count(), OVERSIZED_TEST_STACK_COUNT);
        assert_eq!(
            bookshelf.last_interacted_slot(),
            INTERACTION_TEST_SLOT as i32,
        );
        assert!(
            bookshelf
                .item(INTERACTION_TEST_SLOT)
                .is_some_and(|item| item.is_empty())
        );
        assert!(bookshelf.remove_book(INTERACTION_TEST_SLOT).is_empty());
    }

    #[test]
    fn extraction_requires_space_somewhere_in_destination() {
        let bookshelf = test_bookshelf();
        let source = ItemStack::new(&vanilla_items::BOOK);
        let container = bookshelf.container.lock();
        let mut destination = SimpleContainer::new(DESTINATION_SLOT_COUNT);
        let full_stack_size = source.max_stack_size();
        let one_item_short_of_full_stack = full_stack_size - 1;

        assert!(container.can_take_item(&destination, DESTINATION_SLOT_INDEX, &source));
        destination.set_item(
            DESTINATION_SLOT_INDEX,
            ItemStack::new(&vanilla_items::STONE),
        );
        assert!(!container.can_take_item(&destination, DESTINATION_SLOT_INDEX, &source));
        destination.set_item(
            DESTINATION_SLOT_INDEX,
            ItemStack::with_count(&vanilla_items::BOOK, one_item_short_of_full_stack),
        );
        assert!(container.can_take_item(&destination, DESTINATION_SLOT_INDEX, &source));
        destination.set_item(
            DESTINATION_SLOT_INDEX,
            ItemStack::with_count(&vanilla_items::BOOK, full_stack_size),
        );
        assert!(!container.can_take_item(&destination, DESTINATION_SLOT_INDEX, &source));
    }

    #[test]
    fn persistence_and_container_component_preserve_all_slots_and_last_slot() {
        let bookshelf = test_bookshelf();
        for slot in 0..LAST_SLOT_INDEX {
            assert!(bookshelf.insert_book(slot, ItemStack::new(&vanilla_items::BOOK)));
        }
        assert!(bookshelf.insert_book(
            LAST_SLOT_INDEX,
            ItemStack::new(&vanilla_items::ENCHANTED_BOOK),
        ));

        let component = bookshelf
            .collect_container_component()
            .expect("valid bookshelf items should form a container component");
        assert_eq!(component.items().len(), CHISELED_BOOKSHELF_SLOTS);
        for item in &component.items()[..LAST_SLOT_INDEX] {
            assert!(
                item.as_ref()
                    .is_some_and(|item| item.item().key == vanilla_items::BOOK.key)
            );
        }
        assert!(
            component.items()[LAST_SLOT_INDEX]
                .as_ref()
                .is_some_and(|item| item.item().key == vanilla_items::ENCHANTED_BOOK.key)
        );

        let mut nbt = NbtCompound::new();
        bookshelf.save_additional(&mut nbt);
        let mut bytes = Vec::new();
        nbt.write(&mut bytes);
        let borrowed = read_borrowed_compound(&mut Cursor::new(bytes.as_slice()))
            .expect("saved bookshelf NBT should decode");

        let loaded = test_bookshelf();
        loaded.load_additional(&borrowed);
        assert_eq!(loaded.last_interacted_slot(), LAST_SLOT_INDEX as i32);
        for slot in 0..LAST_SLOT_INDEX {
            assert!(
                loaded
                    .item(slot)
                    .is_some_and(|item| item.is(&vanilla_items::BOOK))
            );
        }
        assert!(
            loaded
                .item(LAST_SLOT_INDEX)
                .is_some_and(|item| item.is(&vanilla_items::ENCHANTED_BOOK))
        );
    }

    #[test]
    fn component_application_replaces_all_six_slots_without_changing_last_slot() {
        let bookshelf = test_bookshelf();
        let previously_inserted_book = ItemStack::new(&vanilla_items::BOOK);
        assert!(bookshelf.insert_book(INTERACTION_TEST_SLOT, previously_inserted_book));

        bookshelf.apply_container_items(vec![
            ItemStack::new(&vanilla_items::WRITABLE_BOOK),
            ItemStack::empty(),
            ItemStack::new(&vanilla_items::ENCHANTED_BOOK),
        ]);

        assert_eq!(
            bookshelf.last_interacted_slot(),
            INTERACTION_TEST_SLOT as i32,
        );
        assert!(
            bookshelf
                .item(WRITABLE_BOOK_COMPONENT_SLOT)
                .is_some_and(|item| item.is(&vanilla_items::WRITABLE_BOOK))
        );
        assert!(
            bookshelf
                .item(EMPTY_COMPONENT_SLOT)
                .is_some_and(|item| item.is_empty())
        );
        assert!(
            bookshelf
                .item(ENCHANTED_BOOK_COMPONENT_SLOT)
                .is_some_and(|item| item.is(&vanilla_items::ENCHANTED_BOOK))
        );
        for slot in FIRST_UNSPECIFIED_COMPONENT_SLOT..CHISELED_BOOKSHELF_SLOTS {
            assert!(bookshelf.item(slot).is_some_and(|item| item.is_empty()));
        }
    }
}
