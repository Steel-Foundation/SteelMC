//! Vanilla `Fireball`: a hurting projectile shown as an item.
//!
//! The item lives in the entity's generated synced data, which starts as a
//! fire charge; the [`FireballItem`] component saves it.

use simdnbt::ToNbtTag as _;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_entity_data::FireballEntityData;
use steel_registry::vanilla_items;
use steel_utils::{DowncastType, DowncastTypeKey};

use crate::entity::component::{Component, Components as _, Has};
use crate::entity::projectile::hurting;
use crate::entity::{Entity, EntityProps};

const ITEM_TAG: &str = "Item";

/// Marks a vanilla `Fireball` type and saves the item it is shown as.
pub struct FireballItem;

// SAFETY: This key is owned by Steel and uniquely identifies `FireballItem`.
unsafe impl DowncastType for FireballItem {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity_component/fireball_item");
}

impl Component for FireballItem {
    fn save(&self, owner: &dyn Entity, nbt: &mut NbtCompound) {
        nbt.insert(ITEM_TAG, Fireball { entity: owner }.item().to_nbt_tag());
    }

    fn load(&self, owner: &dyn Entity, nbt: BorrowedNbtCompoundView<'_, '_>) {
        let item = nbt
            .compound(ITEM_TAG)
            .and_then(|tag| ItemStack::from_borrowed_compound(&tag))
            .unwrap_or_else(default_stack);
        Fireball { entity: owner }.set_item(&item);
    }
}

fn default_stack() -> ItemStack {
    ItemStack::new(&vanilla_items::FIRE_CHARGE)
}

/// An entity seen as a vanilla `Fireball`.
#[derive(Clone, Copy)]
pub struct Fireball<'a> {
    entity: &'a dyn Entity,
}

impl<'a> Fireball<'a> {
    /// Returns `entity` as a fireball, if it is one.
    #[must_use]
    pub fn of(entity: &'a dyn Entity) -> Option<Self> {
        entity.query::<FireballItem>().map(|_| Self { entity })
    }

    fn with_synced<R>(&self, f: impl FnOnce(&mut FireballEntityData) -> R) -> Option<R> {
        let synced = self.entity.synced_data()?;
        let mut data = synced.lock_data();
        data.fireball_layer_mut().map(f)
    }

    /// The item this fireball is shown as.
    #[must_use]
    pub fn item(&self) -> ItemStack {
        self.with_synced(|data| data.item_stack.get().clone())
            .unwrap_or_else(default_stack)
    }

    /// Shows this fireball as a single `item`, or as a fire charge when
    /// `item` is empty.
    pub fn set_item(&self, item: &ItemStack) {
        let item = if item.is_empty() {
            default_stack()
        } else {
            item.copy_with_count(1)
        };
        self.with_synced(|data| data.item_stack.set(item));
    }
}

/// A concrete vanilla `Fireball` type.
pub trait Parts: hurting::Parts + Has<FireballItem> {
    /// Returns this entity as a fireball.
    fn fireball(&self) -> Fireball<'_>;
}

impl<E: hurting::Parts + Has<FireballItem>> Parts for E {
    fn fireball(&self) -> Fireball<'_> {
        Fireball { entity: self }
    }
}

/// `Fireball`'s overrides of the entity properties.
#[must_use]
pub const fn entity_props<E: Parts>() -> EntityProps<E> {
    EntityProps {
        play_entity_on_fire_extinguished_sound: |_| {},
        ..hurting::entity_props()
    }
}
