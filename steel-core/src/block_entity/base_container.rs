//! Shared storage and persistence for Vanilla base container block entities.

use std::mem;

use glam::DVec3;
use simdnbt::borrow::NbtCompound as NbtCompoundView;
use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use simdnbt::{FromNbtTag as _, ToNbtTag as _};
use steel_protocol::packets::game::SoundSource;
use steel_registry::data_components::DataComponentMap;
use steel_registry::data_components::vanilla_components::{
    CONTAINER, CUSTOM_NAME, ItemContainerContents, LOCK,
};
use steel_registry::{item_predicate::LockCode, item_stack::ItemStack, sound_events};
use steel_utils::translations::CONTAINER_IS_LOCKED;
use text_components::TextComponent;

use crate::block_entity::BlockEntityComponentInput;
use crate::entity::Entity as _;
use crate::player::Player;
use crate::world::World;

/// Inventory, custom-name, and lock data shared by container block entities.
pub(crate) struct BaseContainer {
    items: Vec<ItemStack>,
    custom_name: Option<TextComponent>,
    lock: Option<LockCode>,
}

impl BaseContainer {
    #[must_use]
    pub(crate) fn new(size: usize) -> Self {
        Self {
            items: vec![ItemStack::empty(); size],
            custom_name: None,
            lock: None,
        }
    }

    pub(crate) fn load_metadata(&mut self, nbt: &NbtCompoundView<'_, '_>) {
        self.custom_name = nbt
            .get("CustomName")
            .and_then(|tag| TextComponent::from_nbt(&tag.to_owned()));
        self.lock = nbt.get("lock").and_then(LockCode::from_nbt_tag);
    }

    pub(crate) fn load_items(&mut self, nbt: &NbtCompoundView<'_, '_>) {
        self.items = Self::items_from_nbt(nbt, self.items.len());
    }

    pub(crate) fn items_from_nbt(nbt: &NbtCompoundView<'_, '_>, size: usize) -> Vec<ItemStack> {
        let mut result = vec![ItemStack::empty(); size];
        let Some(items) = nbt.list("Items").and_then(|items| items.compounds()) else {
            return result;
        };
        for compound in items {
            let Some(slot) = compound.byte("Slot").map(|slot| slot as u8 as usize) else {
                continue;
            };
            if slot < result.len()
                && let Some(item) = ItemStack::from_borrowed_compound(&compound)
            {
                result[slot] = item;
            }
        }
        result
    }

    pub(crate) fn save_metadata(&self, nbt: &mut NbtCompound) {
        if let Some(custom_name) = &self.custom_name {
            nbt.insert("CustomName", custom_name.to_nbt_tag());
        }
        if let Some(lock) = &self.lock {
            nbt.insert("lock", lock.to_nbt_tag_ref());
        }
    }

    pub(crate) fn save_items(&self, nbt: &mut NbtCompound) {
        Self::save_item_slice(nbt, &self.items);
    }

    pub(crate) fn save_item_slice(nbt: &mut NbtCompound, item_slice: &[ItemStack]) {
        let mut items = Vec::new();
        for (slot, item) in item_slice.iter().enumerate() {
            if item.is_empty() {
                continue;
            }
            let NbtTag::Compound(mut item_nbt) = item.to_nbt_tag_ref() else {
                continue;
            };
            item_nbt.insert("Slot", slot as i8);
            items.push(item_nbt);
        }
        nbt.insert("Items", NbtList::Compound(items));
    }

    /// Mirrors `BaseContainerBlockEntity.applyImplicitComponents`.
    pub(crate) fn apply_implicit_components(&mut self, components: &BlockEntityComponentInput<'_>) {
        self.custom_name = components.get(CUSTOM_NAME).cloned();
        self.lock = components.get(LOCK).cloned();
        components
            .get_or_default(CONTAINER, ItemContainerContents::empty())
            .copy_into(&mut self.items);
    }

    /// Mirrors `BaseContainerBlockEntity.collectImplicitComponents`.
    ///
    /// Items that cannot be represented by Steel's validated persistent templates
    /// are reported and omitted, like Vanilla's save-time problem reporting.
    pub(crate) fn collect_implicit_components(&self, components: &mut DataComponentMap) {
        components.set(CUSTOM_NAME, self.custom_name.clone());
        if self.has_lock() {
            components.set(LOCK, self.lock.clone());
        }
        match ItemContainerContents::from_items(&self.items) {
            Ok(contents) => components.set(CONTAINER, Some(contents)),
            Err(error) => log::warn!("Skipping container component of block entity: {error}"),
        }
    }

    /// Mirrors `BaseContainerBlockEntity.removeComponentsFromTag`.
    pub(crate) fn remove_components_from_tag(nbt: &mut NbtCompound) {
        nbt.remove("CustomName");
        nbt.remove("lock");
        nbt.remove("Items");
    }

    pub(crate) fn replace_items(&mut self, items: Vec<ItemStack>) -> Result<(), Vec<ItemStack>> {
        if items.len() != self.items.len() {
            return Err(items);
        }
        self.items = items;
        Ok(())
    }

    #[must_use]
    pub(crate) fn items(&self) -> &[ItemStack] {
        &self.items
    }

    pub(crate) fn items_mut(&mut self) -> &mut [ItemStack] {
        &mut self.items
    }

    pub(crate) fn set_item(&mut self, slot: usize, mut stack: ItemStack) {
        if slot >= self.items.len() {
            return;
        }
        let max_stack_size = 99.min(stack.max_stack_size());
        if !stack.is_empty() && stack.count() > max_stack_size {
            stack.set_count(max_stack_size);
        }
        self.items[slot] = stack;
    }

    pub(crate) fn clear_items(&mut self) {
        self.items.fill(ItemStack::empty());
    }

    /// Removes every item while retaining the fixed slot count.
    pub(crate) fn take_items(&mut self) -> Vec<ItemStack> {
        let size = self.items.len();
        mem::replace(&mut self.items, vec![ItemStack::empty(); size])
    }

    #[must_use]
    pub(crate) fn display_name(&self, default: TextComponent) -> TextComponent {
        self.custom_name.clone().unwrap_or(default)
    }

    #[must_use]
    pub(crate) const fn has_custom_name(&self) -> bool {
        self.custom_name.is_some()
    }

    #[must_use]
    pub(crate) fn has_lock(&self) -> bool {
        self.lock
            .as_ref()
            .is_some_and(|lock| lock != &LockCode::NO_LOCK)
    }

    /// Mirrors `BaseContainerBlockEntity.canOpen` and `LockCode.canUnlock`:
    /// spectators bypass the lock, everyone else needs a matching main-hand item.
    ///
    /// Callers snapshot the main hand before locking the container so the
    /// inventory lock is never nested inside a container lock.
    #[must_use]
    pub(crate) fn can_open(&self, player: &Player, main_hand: &ItemStack) -> bool {
        player.is_spectator()
            || self
                .lock
                .as_ref()
                .is_none_or(|lock| lock.unlocks_with(main_hand))
    }

    /// Mirrors `BaseContainerBlockEntity.sendChestLockedNotifications`.
    pub(crate) fn send_chest_locked_notifications(
        world: &World,
        pos: DVec3,
        player: &Player,
        display_name: TextComponent,
    ) {
        player.send_overlay_message(&CONTAINER_IS_LOCKED.message([display_name]).component());
        world.play_sound_at(
            &sound_events::BLOCK_CHEST_LOCKED,
            SoundSource::Blocks,
            pos,
            1.0,
            1.0,
            None,
        );
    }
}
