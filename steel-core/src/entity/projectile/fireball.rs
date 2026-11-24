//! Vanilla `Fireball` — hurting projectiles that render a fire-charge item.
//!
//! Vanilla stores the item in `DATA_ITEM_STACK`; Steel keeps it in the generated
//! `*EntityData` layer, so concrete fireballs implement [`Fireball::get_item`] /
//! [`Fireball::set_item`] against their synced data.

use simdnbt::ToNbtTag as _;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_items;

use crate::entity::projectile::AbstractHurtingProjectile;

const ITEM_TAG: &str = "Item";

/// Behavior shared by fireballs that carry a rendered item stack.
pub trait Fireball: AbstractHurtingProjectile {
    /// Returns the rendered item stack (vanilla `Fireball.getItem`).
    fn get_item(&self) -> ItemStack;

    /// Stores the rendered item stack in synced data.
    fn set_item(&self, item: ItemStack);

    /// Vanilla `Fireball.setItem`: empty stacks restore the default fire charge,
    /// otherwise the stack is copied with count 1.
    fn set_item_clamped(&self, item: ItemStack) {
        let item = if item.is_empty() {
            ItemStack::new(&vanilla_items::FIRE_CHARGE)
        } else {
            item.copy_with_count(1)
        };
        self.set_item(item);
    }

    /// Saves hurting-projectile state and the rendered item.
    fn save_fireball(&self, nbt: &mut NbtCompound) {
        self.save_hurting_projectile(nbt);
        nbt.insert(ITEM_TAG, self.get_item().to_nbt_tag());
    }

    /// Loads hurting-projectile state and the rendered item, defaulting to a
    /// fire charge when the tag is absent.
    fn load_fireball(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.load_hurting_projectile(nbt);
        let item = nbt
            .compound(ITEM_TAG)
            .and_then(|tag| ItemStack::from_borrowed_compound(&tag))
            .unwrap_or_else(|| ItemStack::new(&vanilla_items::FIRE_CHARGE));
        self.set_item_clamped(item);
    }
}
