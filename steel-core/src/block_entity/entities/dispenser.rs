//! Dispenser and dropper block entities

use simdnbt::{borrow::BaseNbtCompound, owned::NbtCompound};
use std::sync::Weak;
use steel_registry::{
    data_components::vanilla_components::SeededContainerLoot, vanilla_block_entity_types,
};
use steel_utils::{BlockPos, BlockStateId, DowncastType, DowncastTypeKey, translations};
use text_components::TextComponent;

use crate::{
    block_entity::{
        BlockEntity, BlockEntityBase, ImplicitComponentGetter, RandomizableContainer,
        container_properties::ContainerProperties,
        randomizable_container::RandomizableContainerStorage,
    },
    entity::Entity as _,
    inventory::lock::ContainerRef,
    loot::LootResult,
    nameable::Nameable,
    player::Player,
    world::World,
};

/// Number of slots in a dispenser or dropper
pub const DISPENSER_SLOTS: usize = 9;

macro_rules! impl_dispenser_like_entity {
    ($name:ident, $doc:expr, $key:expr, $type:expr, $title:ident) => {
        #[doc = $doc]
        pub struct $name {
            storage: RandomizableContainerStorage,
            properties: ContainerProperties,
        }

        // SAFETY: This steelowned key identifies the concrete block entity
        unsafe impl DowncastType for $name {
            const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new($key);
        }

        impl $name {
            /// Creates the block entity and its empty inventory
            #[must_use]
            pub fn new(level: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
                Self {
                    storage: RandomizableContainerStorage::new(
                        $type,
                        DISPENSER_SLOTS,
                        level,
                        pos,
                        state,
                    ),
                    properties: ContainerProperties::default(),
                }
            }

            pub(crate) fn can_open(&self, player: &Player) -> bool {
                (!player.is_spectator() || self.loot_table().is_none())
                    && self.properties.can_open(player)
            }
        }

        impl BlockEntity for $name {
            fn base(&self) -> &BlockEntityBase {
                &self.storage.base
            }

            fn pre_remove_side_effects(&self, pos: BlockPos, _state: BlockStateId) {
                self.storage.pre_remove_side_effects(pos);
            }

            fn load_additional(&self, nbt: &BaseNbtCompound<'_>) {
                self.properties.load(nbt);
                self.storage.load_additional(nbt);
            }

            fn save_additional(&self, nbt: &mut NbtCompound) {
                self.properties.save(nbt);
                self.storage.save_additional(nbt);
            }

            fn apply_implicit_components(&self, components: &mut ImplicitComponentGetter<'_>) {
                self.properties.apply_components(components);
                self.storage.apply_implicit_components(components);
            }

            fn get_update_tag(&self) -> Option<NbtCompound> {
                None
            }

            fn container_ref(&self) -> Option<ContainerRef> {
                Some(self.storage.container_ref.clone())
            }

            fn as_nameable(&self) -> Option<&dyn Nameable> {
                Some(self)
            }

            fn as_randomizable_container(&self) -> Option<&dyn RandomizableContainer> {
                Some(self)
            }
        }

        impl RandomizableContainer for $name {
            fn loot_table(&self) -> Option<SeededContainerLoot> {
                self.storage.loot_table()
            }

            fn set_loot_table(&self, loot: Option<SeededContainerLoot>) {
                self.storage.set_loot_table(loot);
            }

            fn unpack_loot_table(&self, player: Option<&Player>) -> LootResult<()> {
                self.storage.unpack_loot_table(player)
            }
        }

        impl Nameable for $name {
            fn custom_name(&self) -> Option<TextComponent> {
                self.properties.custom_name()
            }

            fn name(&self) -> TextComponent {
                self.custom_name()
                    .unwrap_or_else(|| translations::$title.msg().component())
            }
        }
    };
}

impl_dispenser_like_entity!(
    DropperBlockEntity,
    "Dropper block entity.",
    "steel:block_entity/dropper",
    &vanilla_block_entity_types::DROPPER,
    CONTAINER_DROPPER
);
impl_dispenser_like_entity!(
    DispenserBlockEntity,
    "Dispenser block entity.",
    "steel:block_entity/dispenser",
    &vanilla_block_entity_types::DISPENSER,
    CONTAINER_DISPENSER
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::lock::ContainerLockGuard;
    use simdnbt::owned::NbtTag;
    use steel_registry::{
        data_components::vanilla_components::MAX_STACK_SIZE, init_vanilla_registry,
        item_stack::ItemStack, vanilla_blocks, vanilla_items,
    };

    fn test_dropper() -> DropperBlockEntity {
        init_vanilla_registry();
        DropperBlockEntity::new(
            Weak::new(),
            BlockPos::new(1, 2, 3),
            vanilla_blocks::DROPPER.default_state(),
        )
    }

    #[test]
    fn dropper_removal_clears_inventory_seen_by_existing_menu() {
        let dropper = test_dropper();
        let inventory = dropper.container_ref().expect("dropper inventory");
        let mut guard = ContainerLockGuard::lock_all(&[&inventory]).expect("lock inventory");
        assert!(guard.set_item(
            inventory.container_id(),
            0,
            ItemStack::new(&vanilla_items::STONE)
        ));
        drop(guard);

        dropper.pre_remove_side_effects(
            BlockPos::new(1, 2, 3),
            vanilla_blocks::DROPPER.default_state(),
        );

        let guard = ContainerLockGuard::lock_all(&[&inventory]).expect("lock detached inventory");
        let contents = guard
            .get(inventory.container_id())
            .expect("locked inventory");
        assert_eq!(contents.get_container_size(), DISPENSER_SLOTS);
        assert!(contents.is_empty());
    }
}
