//! Vanilla `Display.ItemDisplay`: renders an item model.

use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::{NbtCompound, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_macros::entity_behavior;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_entity_data::ItemDisplayEntityData;
use steel_utils::locks::CheckedMutex;

use super::DisplayBase;
use crate::entity::{Entity, EntityBase, EntityProps, EntityTable};

/// A display entity that renders the model of an [`ItemStack`] using an [`ItemDisplayContext`].
///
/// The state shared by all displays is reached through [`Parts::display`](super::Parts::display).
// TODO: Expose slot 0 once entity slot access is available to /item
#[entity_behavior(class = "ItemDisplay")]
#[derive(Entity)]
#[entity(save = Self::save_item_display, load = Self::load_item_display)]
#[entity(key = "steel:entity/item_display", constructors)]
pub struct ItemDisplayEntity {
    #[entity(base)]
    base: EntityBase,
    #[entity(entity_type)]
    entity_type: EntityTypeRef,
    #[entity(component)]
    display: DisplayBase,
    #[entity(synced)]
    entity_data: CheckedMutex<ItemDisplayEntityData>,
}

impl EntityTable for ItemDisplayEntity {
    const ENTITY: EntityProps<Self> = super::entity_props();
}

impl ItemDisplayEntity {
    fn from_base(base: EntityBase, entity_type: EntityTypeRef) -> Self {
        base.set_no_physics(true);
        Self {
            base,
            entity_type,
            display: DisplayBase,
            entity_data: CheckedMutex::new(ItemDisplayEntityData::new()),
        }
    }

    /// Gets the context to display the item stack of this item display.
    #[must_use]
    pub fn item_transform(&self) -> ItemDisplayContext {
        ItemDisplayContext::try_from(*self.entity_data.lock().item_display.get())
            .unwrap_or(ItemDisplayContext::None)
    }

    /// Sets the context to display the item stack of this item display to `context`.
    pub fn set_item_transform(&self, context: ItemDisplayContext) {
        self.entity_data.lock().item_display.set(context as i8);
    }

    /// Gets a copy of the item stack used in this item display.
    #[must_use]
    pub fn item_stack(&self) -> ItemStack {
        self.entity_data.lock().item_stack.get().clone()
    }

    /// Sets the item stack used in this item display to `item`.
    pub fn set_item_stack(&self, item: ItemStack) {
        self.entity_data
            .lock()
            .item_stack
            .set_and_force_dirty(item, true);
    }

    fn save_item_display(&self, nbt: &mut NbtCompound) {
        {
            let data = self.entity_data.lock();
            let stack = data.item_stack.get();
            if !stack.is_empty() {
                nbt.insert("item", stack.to_nbt_tag_ref());
            }
        }

        nbt.insert("item_display", self.item_transform());
    }

    fn load_item_display(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.set_item_stack(
            nbt.get("item")
                .and_then(ItemStack::from_nbt_tag)
                .unwrap_or_else(ItemStack::empty),
        );
        self.set_item_transform(
            nbt.get("item_display")
                .and_then(ItemDisplayContext::from_nbt_tag)
                .unwrap_or(ItemDisplayContext::None),
        );
    }
}

/// The *item model transform* to use for displaying the item
/// of an item display in the client.
///
/// Each model stores a rotation, translation and scale for each context.
#[repr(i8)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ItemDisplayContext {
    /// No special context.
    None = 0,
    /// Displays the item like how it would on the left hand in third person.
    ThirdPersonLeftHand = 1,
    /// Displays the item like how it would on the right hand in third person.
    ThirdPersonRightHand = 2,
    /// Displays the item like how it would on the left hand in first person.
    FirstPersonLeftHand = 3,
    /// Displays the item like how it would on the right hand in first person.
    FirstPersonRightHand = 4,
    /// Displays the item like how it would if a player wore it as their head slot.
    Head = 5,
    /// Displays the item like how it would in the hotbar and GUIs.
    Gui = 6,
    /// Displays the item like how it would for an item entity.
    Ground = 7,
    /// Displays the item like how an item frame would.
    Fixed = 8,
    /// Displays the item like how a shelf would.
    OnShelf = 9,
}

impl TryFrom<i8> for ItemDisplayContext {
    type Error = ();

    fn try_from(value: i8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::None),
            1 => Ok(Self::ThirdPersonLeftHand),
            2 => Ok(Self::ThirdPersonRightHand),
            3 => Ok(Self::FirstPersonLeftHand),
            4 => Ok(Self::FirstPersonRightHand),
            5 => Ok(Self::Head),
            6 => Ok(Self::Gui),
            7 => Ok(Self::Ground),
            8 => Ok(Self::Fixed),
            9 => Ok(Self::OnShelf),
            _ => Err(()),
        }
    }
}

impl ToNbtTag for ItemDisplayContext {
    fn to_nbt_tag(self) -> NbtTag {
        NbtTag::String(
            match self {
                Self::None => "none",
                Self::ThirdPersonLeftHand => "thirdperson_lefthand",
                Self::ThirdPersonRightHand => "thirdperson_righthand",
                Self::FirstPersonLeftHand => "firstperson_lefthand",
                Self::FirstPersonRightHand => "firstperson_righthand",
                Self::Head => "head",
                Self::Gui => "gui",
                Self::Ground => "ground",
                Self::Fixed => "fixed",
                Self::OnShelf => "on_shelf",
            }
            .into(),
        )
    }
}

impl FromNbtTag for ItemDisplayContext {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        match tag.string()?.to_str().as_ref() {
            "none" => Some(Self::None),
            "thirdperson_lefthand" => Some(Self::ThirdPersonLeftHand),
            "thirdperson_righthand" => Some(Self::ThirdPersonRightHand),
            "firstperson_lefthand" => Some(Self::FirstPersonLeftHand),
            "firstperson_righthand" => Some(Self::FirstPersonRightHand),
            "head" => Some(Self::Head),
            "gui" => Some(Self::Gui),
            "ground" => Some(Self::Ground),
            "fixed" => Some(Self::Fixed),
            "on_shelf" => Some(Self::OnShelf),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Weak;

    use glam::DVec3;
    use steel_registry::{init_vanilla_registry, vanilla_entities, vanilla_items};

    use super::*;
    use crate::test_support::load_additional;

    #[test]
    fn empty_item_is_not_saved_and_loads_back_empty() {
        init_vanilla_registry();
        let display =
            ItemDisplayEntity::new(&vanilla_entities::ITEM_DISPLAY, 1, DVec3::ZERO, Weak::new());

        let mut nbt = NbtCompound::new();
        display.save_additional(&mut nbt);
        assert!(!nbt.contains("item"));

        display.set_item_stack(ItemStack::new(&vanilla_items::DIAMOND));
        load_additional(&*display, &nbt);
        assert!(display.item_stack().is_empty());
    }
}
