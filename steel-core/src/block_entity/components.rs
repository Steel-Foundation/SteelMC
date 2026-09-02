//! Vanilla block-entity data-component storage and item exchange.
//!
//! Block entities hold two kinds of components: implicit components that
//! concrete entities derive from their own fields (a container's items, name,
//! and lock), and explicit components carried over from the placing item that
//! no field consumed. Both are collected back onto items by pick-block with
//! data and the `copy_components` loot function.

use std::cell::RefCell;
use std::io::Cursor;

use simdnbt::borrow::read_compound as read_borrowed_compound;
use simdnbt::owned::NbtCompound;
use steel_registry::block_entity_type::BlockEntityTypeRef;
use steel_registry::data_components::vanilla_components::{
    BLOCK_ENTITY_DATA, BLOCK_STATE, BlockEntityData,
};
use steel_registry::data_components::{Component, CustomData, DataComponentMap, DataComponentType};
use steel_registry::item_stack::ItemStack;
use steel_utils::nbt::{merge_nbt_compounds, nbt_compounds_equal};
use steel_utils::{BlockPos, DowncastType, Identifier};

use super::BlockEntity;

/// Item components read while applying implicit components.
///
/// Mirrors the recording `DataComponentGetter` in Vanilla
/// `BlockEntity.applyComponents`: every component an implementation reads is
/// consumed and not retained as an explicit block-entity component.
pub struct BlockEntityComponentInput<'a> {
    stack: &'a ItemStack,
    consumed: RefCell<Vec<Identifier>>,
}

impl<'a> BlockEntityComponentInput<'a> {
    fn new(stack: &'a ItemStack) -> Self {
        Self {
            stack,
            consumed: RefCell::new(vec![
                BLOCK_ENTITY_DATA.key().clone(),
                BLOCK_STATE.key().clone(),
            ]),
        }
    }

    /// Returns the effective item value and marks the component consumed.
    #[must_use]
    pub fn get<T: Component + DowncastType>(
        &self,
        component: DataComponentType<T>,
    ) -> Option<&'a T> {
        self.consumed.borrow_mut().push(component.key().clone());
        self.stack.get(component)
    }

    /// Returns the effective item value or `default`, marking the component consumed.
    #[must_use]
    pub fn get_or_default<T: Component + DowncastType + Clone>(
        &self,
        component: DataComponentType<T>,
        default: T,
    ) -> T {
        self.get(component).cloned().unwrap_or(default)
    }

    fn into_consumed(self) -> Vec<Identifier> {
        self.consumed.into_inner()
    }
}

/// Final component exchange between block entities and items.
///
/// Mirrors the `final` component methods of Vanilla `BlockEntity`; concrete
/// entities customize the exchange only through
/// [`BlockEntity::apply_implicit_components`],
/// [`BlockEntity::collect_implicit_components`], and
/// [`BlockEntity::remove_components_from_tag`].
pub trait BlockEntityComponentsExt: BlockEntity {
    /// Applies the placing item's components.
    ///
    /// Mirrors Vanilla `BlockEntity.applyComponentsFromItemStack`: implicit
    /// components are consumed by the entity, and the remaining set values of
    /// the item's patch become this entity's explicit components.
    fn apply_components_from_item_stack(&self, stack: &ItemStack) {
        let input = BlockEntityComponentInput::new(stack);
        self.apply_implicit_components(&input);
        let consumed = input.into_consumed();
        let retained = stack
            .components_patch()
            .forget(|key| consumed.contains(key));
        self.base().set_components(retained.split().added);
    }

    /// Returns the explicit components overlaid with the implicit ones.
    ///
    /// Mirrors Vanilla `BlockEntity.collectComponents`.
    fn collect_components(&self) -> DataComponentMap {
        let mut components = self.base().components();
        self.collect_implicit_components(&mut components);
        components
    }

    /// Copies this entity onto `stack` like Vanilla's pick-block-with-data path.
    ///
    /// Mirrors `ServerGamePacketListenerImpl.addBlockDataToItem`: entity data that
    /// is not represented by components becomes `BLOCK_ENTITY_DATA`, and the
    /// collected components are applied to the stack.
    fn add_block_data_to_item(&self, stack: &mut ItemStack) {
        let mut nbt = self.save_custom_only();
        self.remove_components_from_tag(&mut nbt);
        set_block_entity_data(stack, self.get_type(), self.get_block_pos(), nbt);
        stack.apply_components(&self.collect_components());
    }

    /// Merges a placing item's `BLOCK_ENTITY_DATA` into this entity's saved data.
    ///
    /// Mirrors Vanilla `TypedEntityData.loadInto(BlockEntity)`; returns whether
    /// the merged data changed anything.
    fn load_custom_data(&self, data: &CustomData) -> bool {
        let previous = self.save_custom_only();
        let mut merged = previous.clone();
        merge_nbt_compounds(&mut merged, data.as_compound());
        if nbt_compounds_equal(&merged, &previous) {
            return false;
        }
        let mut bytes = Vec::new();
        merged.write(&mut bytes);
        match read_borrowed_compound(&mut Cursor::new(bytes.as_slice())) {
            Ok(borrowed) => self.load_additional(&borrowed),
            Err(error) => {
                log::warn!(
                    "Failed to reload block entity {} at {:?} with custom data: {error:?}",
                    self.get_type().key,
                    self.get_block_pos()
                );
                return false;
            }
        }
        self.set_changed();
        true
    }
}

impl<T: BlockEntity + ?Sized> BlockEntityComponentsExt for T {}

/// Mirrors Vanilla `BlockItem.setBlockEntityData`.
fn set_block_entity_data(
    stack: &mut ItemStack,
    block_entity_type: BlockEntityTypeRef,
    pos: BlockPos,
    mut nbt: NbtCompound,
) {
    nbt.remove("id");
    if nbt.is_empty() {
        stack.remove(BLOCK_ENTITY_DATA);
        return;
    }
    let Some(data) = CustomData::try_from_compound(nbt) else {
        log::warn!(
            "Block entity {} at {pos:?} saved malformed custom data",
            block_entity_type.key
        );
        return;
    };
    stack.set(
        BLOCK_ENTITY_DATA,
        BlockEntityData::new(block_entity_type, data),
    );
}

#[cfg(test)]
mod tests {
    use std::sync::Weak;

    use steel_registry::data_components::vanilla_components::{CUSTOM_DATA, CUSTOM_NAME};
    use steel_registry::{test_support::init_test_registry, vanilla_blocks, vanilla_items};
    use text_components::TextComponent;

    use super::*;
    use crate::block_entity::entities::ChestBlockEntity;

    #[test]
    fn explicit_components_round_trip_through_block_entity_nbt() {
        init_test_registry();
        let name = TextComponent::plain("Named");
        let mut custom_data = NbtCompound::new();
        custom_data.insert("steel_test", 1_i32);
        let mut stack = ItemStack::new(&vanilla_items::CHEST);
        stack.set(CUSTOM_NAME, name.clone());
        stack.set(
            CUSTOM_DATA,
            CustomData::try_from_compound(custom_data).expect("test custom data should be valid"),
        );
        let chest = ChestBlockEntity::new(
            Weak::new(),
            BlockPos::new(1, 2, 3),
            vanilla_blocks::CHEST.default_state(),
        );

        chest.apply_components_from_item_stack(&stack);
        let saved = chest.save_without_metadata();

        let Some(components) = saved.compound("components") else {
            panic!("block entities always save their components compound");
        };
        assert!(components.get("minecraft:custom_data").is_some());
        assert!(
            components.get("minecraft:custom_name").is_none(),
            "consumed implicit components are saved by the entity's own fields"
        );
        assert!(saved.get("CustomName").is_some());

        let mut bytes = Vec::new();
        saved.write(&mut bytes);
        let borrowed = read_borrowed_compound(&mut Cursor::new(bytes.as_slice()))
            .expect("saved chest NBT should reborrow");
        let loaded = ChestBlockEntity::new(
            Weak::new(),
            BlockPos::new(1, 2, 3),
            vanilla_blocks::CHEST.default_state(),
        );
        loaded.load_with_components(&borrowed);

        assert!(loaded.base().components().has(CUSTOM_DATA));
        assert_eq!(loaded.display_name(), name);
    }
}
