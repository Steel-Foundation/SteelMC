//! Enchanting table block entity.

use std::sync::{Arc, Weak};

use simdnbt::borrow::{
    BaseNbtCompound as BorrowedNbtCompound, NbtCompound as BorrowedNbtCompoundView,
};
use simdnbt::owned::NbtCompound;
use steel_registry::data_components::{DataComponentMap, vanilla_components::CUSTOM_NAME};
use steel_registry::vanilla_block_entity_types;
use steel_utils::{
    BlockPos, BlockStateId, DowncastType, DowncastTypeKey, locks::SyncMutex, translations,
};
use text_components::TextComponent;

use crate::block_entity::{BlockEntity, BlockEntityBase, ImplicitComponentGetter};
use crate::world::World;

/// Enchanting table block entity: holds the optional custom name.
pub struct EnchantingTableBlockEntity {
    base: Arc<BlockEntityBase>,
    name: SyncMutex<Option<TextComponent>>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `EnchantingTableBlockEntity`.
unsafe impl DowncastType for EnchantingTableBlockEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:block_entity/enchanting_table");
}

impl EnchantingTableBlockEntity {
    /// Creates a new enchanting table block entity.
    #[must_use]
    pub fn new(level: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
        Self {
            base: Arc::new(BlockEntityBase::new(
                &vanilla_block_entity_types::ENCHANTING_TABLE,
                level,
                pos,
                state,
            )),
            name: SyncMutex::new(None),
        }
    }

    /// Returns the custom name or the default enchanting table name
    #[must_use]
    pub fn display_name(&self) -> TextComponent {
        self.name
            .lock()
            .clone()
            .unwrap_or_else(|| TextComponent::translated(translations::CONTAINER_ENCHANT.msg()))
    }

    /// Returns the custom name if set
    #[must_use]
    pub fn custom_name(&self) -> Option<TextComponent> {
        self.name.lock().clone()
    }

    /// Sets the custom name
    pub fn set_custom_name(&self, name: Option<TextComponent>) {
        *self.name.lock() = name;
    }
}

impl BlockEntity for EnchantingTableBlockEntity {
    fn base(&self) -> &BlockEntityBase {
        &self.base
    }

    fn load_additional(&self, nbt: &BorrowedNbtCompound<'_>) {
        let nbt_view: BorrowedNbtCompoundView<'_, '_> = nbt.into();
        *self.name.lock() = nbt_view
            .get("CustomName")
            .and_then(|tag| TextComponent::from_nbt(&tag.to_owned()));
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        if let Some(name) = self.name.lock().as_ref() {
            nbt.insert("CustomName", name.to_codec_nbt());
        }
    }

    fn apply_implicit_components(&self, components: &mut ImplicitComponentGetter<'_>) {
        *self.name.lock() = components.get(CUSTOM_NAME);
    }

    fn collect_implicit_components(&self, components: &mut DataComponentMap) {
        components.set(CUSTOM_NAME, self.custom_name());
    }
}

#[cfg(test)]
mod tests {
    use simdnbt::owned::NbtCompound;
    use steel_registry::data_components::components::{BlockEntityData, CustomData};
    use steel_registry::data_components::vanilla_components::{
        BLOCK_ENTITY_DATA, CUSTOM_NAME, REPAIR_COST,
    };
    use steel_registry::item_stack::ItemStack;
    use steel_registry::{init_vanilla_registry, vanilla_blocks, vanilla_items};

    use super::*;
    use crate::test_support::fresh_test_world;

    fn table(world: &Arc<World>) -> EnchantingTableBlockEntity {
        EnchantingTableBlockEntity::new(
            Arc::downgrade(world),
            BlockPos::new(8, 64, 8),
            vanilla_blocks::ENCHANTING_TABLE.default_state(),
        )
    }

    #[test]
    fn applying_item_components_consumes_the_custom_name_and_stores_the_rest() {
        init_vanilla_registry();
        let world_fixture = fresh_test_world("enchanting_table_apply_components");
        let world = &world_fixture.world;
        let table = table(world);
        let name = TextComponent::from("Arcane Desk".to_string());
        let mut stack = ItemStack::new(&vanilla_items::ENCHANTING_TABLE);
        stack.set(CUSTOM_NAME, name.clone());
        stack.set(REPAIR_COST, 3);

        table.apply_components_from_item_stack(&stack);

        assert_eq!(table.custom_name(), Some(name.clone()));
        let stored = table.base().stored_components();
        assert!(
            !stored.has(CUSTOM_NAME),
            "an implicit component must not also be stored"
        );
        assert_eq!(stored.get(REPAIR_COST), Some(3));
        assert_eq!(table.collect_components().get(CUSTOM_NAME), Some(name));
    }

    #[test]
    fn applying_item_components_skips_placement_only_components() {
        init_vanilla_registry();
        let world_fixture = fresh_test_world("enchanting_table_apply_placement_components");
        let world = &world_fixture.world;
        let table = table(world);
        let Some(custom_data) = CustomData::try_from_compound(NbtCompound::new()) else {
            panic!("an empty compound should be valid custom data");
        };
        let mut stack = ItemStack::new(&vanilla_items::ENCHANTING_TABLE);
        stack.set(
            BLOCK_ENTITY_DATA,
            BlockEntityData::new(&vanilla_block_entity_types::ENCHANTING_TABLE, custom_data),
        );

        table.apply_components_from_item_stack(&stack);

        assert!(table.base().stored_components().is_empty());
        assert_eq!(table.custom_name(), None);
    }

    #[test]
    fn applying_an_unnamed_item_clears_a_previous_custom_name() {
        init_vanilla_registry();
        let world_fixture = fresh_test_world("enchanting_table_apply_clears_name");
        let world = &world_fixture.world;
        let table = table(world);
        table.set_custom_name(Some(TextComponent::from("Old".to_string())));

        table.apply_components_from_item_stack(&ItemStack::new(&vanilla_items::ENCHANTING_TABLE));

        assert_eq!(table.custom_name(), None);
    }
}
