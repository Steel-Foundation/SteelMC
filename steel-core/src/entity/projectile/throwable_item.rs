//! Vanilla `ThrowableItemProjectile` — carries a synced rendered item stack.
//!
//! Vanilla stores the item in `DATA_ITEM_STACK` (synced entity data). Steel keeps
//! it in the generated `*EntityData` layer, so this trait has no base struct: the
//! concrete entity implements [`ThrowableItemProjectile::set_item`] /
//! [`ThrowableItemProjectile::get_item`] against its synced data.

use simdnbt::ToNbtTag;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_registry::item_stack::ItemStack;
use steel_registry::items::ItemRef;

use crate::entity::projectile::ThrowableProjectile;

/// Vanilla-shaped behavior shared by entities that extend `ThrowableItemProjectile`.
pub trait ThrowableItemProjectile: ThrowableProjectile {
    /// Vanilla `ThrowableItemProjectile.getDefaultItem`.
    fn get_default_item(&self) -> ItemRef;

    /// Sets the rendered item stack (vanilla `setItem`, count clamped to 1).
    ///
    /// The concrete entity stores this in its synced data layer.
    fn set_item(&self, item: ItemStack);

    /// Returns the rendered item stack (vanilla `getItem`).
    fn get_item(&self) -> ItemStack;

    /// Vanilla `ThrowableItemProjectile.setItem` count-clamping helper.
    fn set_item_clamped(&self, item: ItemStack) {
        self.set_item(item.copy_with_count(1));
    }

    /// Saves the item stack (vanilla `addAdditionalSaveData`).
    fn save_throwable_item(&self, nbt: &mut NbtCompound) {
        let item = self.get_item();
        if !item.is_empty() {
            nbt.insert("Item", item.to_nbt_tag());
        }
    }

    /// Loads the item stack, falling back to the default item (vanilla
    /// `readAdditionalSaveData`).
    fn load_throwable_item(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        let item = nbt
            .compound("Item")
            .and_then(|tag| ItemStack::from_borrowed_compound(&tag))
            .unwrap_or_else(|| ItemStack::new(self.get_default_item()));
        self.set_item_clamped(item);
    }
}
