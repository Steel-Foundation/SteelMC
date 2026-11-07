//! Vanilla `ThrowableItemProjectile`: a throwable shown as an item.
//!
//! The item lives in the entity's generated synced data; the [`ThrownItem`]
//! component knows the type's default item and saves the item.

use simdnbt::ToNbtTag;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_registry::item_stack::ItemStack;
use steel_registry::items::ItemRef;
use steel_registry::vanilla_entity_data::ThrowableItemProjectileEntityData;
use steel_utils::{DowncastType, DowncastTypeKey};

use crate::entity::Entity;
use crate::entity::component::{Component, Components as _, Has};
use crate::entity::projectile;

/// The default item of a vanilla `ThrowableItemProjectile` type.
pub struct ThrownItem {
    default_item: ItemRef,
}

impl ThrownItem {
    /// A throwable shown as `default_item` unless given another item.
    #[must_use]
    pub const fn new(default_item: ItemRef) -> Self {
        Self { default_item }
    }

    /// The synced item a new throwable of this type starts with.
    #[must_use]
    pub fn default_stack(&self) -> ItemStack {
        ItemStack::new(self.default_item)
    }
}

// SAFETY: This key is owned by Steel and uniquely identifies `ThrownItem`.
unsafe impl DowncastType for ThrownItem {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity_component/thrown_item");
}

impl Component for ThrownItem {
    fn save(&self, owner: &dyn Entity, nbt: &mut NbtCompound) {
        let item = ThrowableItem::new(owner, self).item();
        if !item.is_empty() {
            nbt.insert("Item", item.to_nbt_tag());
        }
    }

    fn load(&self, owner: &dyn Entity, nbt: BorrowedNbtCompoundView<'_, '_>) {
        let item = nbt
            .compound("Item")
            .and_then(|tag| ItemStack::from_borrowed_compound(&tag))
            .unwrap_or_else(|| self.default_stack());
        ThrowableItem::new(owner, self).set_item(&item);
    }
}

/// An entity seen as a vanilla `ThrowableItemProjectile`.
#[derive(Clone, Copy)]
pub struct ThrowableItem<'a> {
    entity: &'a dyn Entity,
    thrown_item: &'a ThrownItem,
}

impl<'a> ThrowableItem<'a> {
    /// Pairs `entity` with its thrown item component.
    #[must_use]
    pub const fn new(entity: &'a dyn Entity, thrown_item: &'a ThrownItem) -> Self {
        Self {
            entity,
            thrown_item,
        }
    }

    /// Returns `entity` as a throwable item projectile, if it is one.
    #[must_use]
    pub fn of(entity: &'a dyn Entity) -> Option<Self> {
        Some(Self::new(entity, entity.query::<ThrownItem>()?))
    }

    fn with_synced<R>(
        &self,
        f: impl FnOnce(&mut ThrowableItemProjectileEntityData) -> R,
    ) -> Option<R> {
        let synced = self.entity.synced_data()?;
        let mut data = synced.lock_data();
        data.throwable_item_projectile_layer_mut().map(f)
    }

    /// The item this projectile is shown as.
    #[must_use]
    pub fn item(&self) -> ItemStack {
        self.with_synced(|data| data.item_stack.get().clone())
            .unwrap_or_else(ItemStack::empty)
    }

    /// Shows this projectile as a single `item`.
    pub fn set_item(&self, item: &ItemStack) {
        self.with_synced(|data| data.item_stack.set(item.copy_with_count(1)));
    }

    /// The item a new throwable of this type starts with.
    #[must_use]
    pub fn default_stack(&self) -> ItemStack {
        self.thrown_item.default_stack()
    }
}

/// A concrete vanilla `ThrowableItemProjectile` type.
pub trait Parts: projectile::Parts + Has<ThrownItem> {
    /// Returns this entity as a throwable item projectile.
    fn throwable_item(&self) -> ThrowableItem<'_>;
}

impl<E: projectile::Parts + Has<ThrownItem>> Parts for E {
    fn throwable_item(&self) -> ThrowableItem<'_> {
        ThrowableItem::new(self, self.get::<ThrownItem>())
    }
}
