//! Vanilla's item display implementation.

use crate::entity::damage::DamageSource;
use crate::entity::entities::objects::technical::display::{modify_display_entity_base, Display, DisplayView, PrivateDisplayView};
use crate::entity::{Entity, EntityBase, EntityBaseLoad, EntitySyncedData};
use crate::world::World;
use glam::DVec3;
use parking_lot::MutexGuard;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::{NbtCompound, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use std::sync::Weak;
use steel_macros::entity_behavior;
use steel_registry::blocks::behavior::PushReaction;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_entity_data::{DisplayEntityData, ItemDisplayEntityData};
use steel_utils::locks::SyncMutex;
use steel_utils::{DowncastType, DowncastTypeKey};

/// The Vanilla item display entity.
///
/// In addition to having the common display entity properties, this entity
/// also stores an [`ItemStack`] and the context to display it (an [`ItemDisplayContext`]).
///
/// Like any display entity, to **access** or **modify** the data of an item display,
/// you will need to use [`Display::with_view`]. This method takes a function with a
/// [`ItemDisplayView`] as a parameter, which can be used within the function.
#[entity_behavior(class = "ItemDisplay")]
pub struct ItemDisplayEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    entity_data: SyncMutex<ItemDisplayEntityData>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `ItemDisplayEntity`.
unsafe impl DowncastType for ItemDisplayEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/item_display");
}

impl ItemDisplayEntity {
    /// Creates a new item display entity.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::new_with_base(
            EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
        )
    }

    /// Creates an item display entity from saved base data.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self::new_with_base(
            EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
        )
    }

    #[must_use]
    fn new_with_base(base: EntityBase, entity_type: EntityTypeRef) -> Self {
        Self {
            base: modify_display_entity_base(base),
            entity_type,
            entity_data: SyncMutex::new(ItemDisplayEntityData::new()),
        }
    }
}

impl Entity for ItemDisplayEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
        Some(&self.entity_data)
    }

    fn tick(&self) {
        self.tick_display();
    }
    fn hurt(&self, world: &World, source: &DamageSource, amount: f32) -> bool {
        self.hurt_display(world, source, amount)
    }
    fn piston_push_reaction(&self) -> PushReaction {
        self.piston_push_reaction_display()
    }
    fn is_ignoring_block_triggers(&self) -> bool {
        self.is_ignoring_block_triggers_display()
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.with_view(|view| {
            <Self as Display>::save_display(&view, nbt);

            let stack = view.item_stack_ref();
            if !stack.is_empty() {
                nbt.insert("item", stack.to_nbt_tag_ref());
            }

            nbt.insert("item_display", view.item_transform());
        });
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.with_view(|mut view| {
            <Self as Display>::load_display(&mut view, nbt);

            view.set_item_stack(
                nbt.get("item")
                    .and_then(ItemStack::from_nbt_tag)
                    .unwrap_or_else(ItemStack::empty),
            );
            view.set_item_transform(
                nbt.get("item_display")
                    .and_then(ItemDisplayContext::from_nbt_tag)
                    .unwrap_or(ItemDisplayContext::None),
            );
        });
    }

    // TODO: Add `getItemStack` and `setItemStack` overrides.
}

impl Display for ItemDisplayEntity {
    type View<'a> = ItemDisplayView<'a>;

    fn with_view(&self, f: impl FnOnce(Self::View<'_>)) {
        f(ItemDisplayView(self.entity_data.lock()));
    }
}

/// A view to the data of an item display.
///
/// Along with having the methods in [`DisplayView`], this view also has additional methods
/// to access and manipulate the item stack and display context of the item display.
pub struct ItemDisplayView<'a>(MutexGuard<'a, ItemDisplayEntityData>);

impl<'a> PrivateDisplayView<'a> for ItemDisplayView<'a> {
    fn display_data(&self) -> &DisplayEntityData {
        self.0.display()
    }

    fn display_data_mut(&mut self) -> &mut DisplayEntityData {
        self.0.display_mut()
    }
}

impl<'a> DisplayView<'a> for ItemDisplayView<'a> {}

impl ItemDisplayView<'_> {
    /// Gets the context to display the item stack of this item display.
    #[must_use]
    pub fn item_transform(&self) -> ItemDisplayContext {
        ItemDisplayContext::try_from(*self.0.item_display.get()).unwrap_or(ItemDisplayContext::None)
    }

    /// Sets the context to display the item stack of this item display to `context`.
    pub fn set_item_transform(&mut self, context: ItemDisplayContext) {
        self.0.item_display.set(context as i8);
    }

    /// Gets a clone of the item stack used in this item display.
    #[must_use]
    pub fn item_stack(&self) -> ItemStack {
        self.item_stack_ref().clone()
    }

    /// Gets a reference to the item stack used in this item display.
    #[must_use]
    pub fn item_stack_ref(&self) -> &ItemStack {
        self.0.item_stack.get()
    }

    /// Sets the item stack used in this item display to `item`.
    pub fn set_item_stack(&mut self, item: ItemStack) {
        self.0.item_stack.set(item);
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
        match tag.string()?.to_string().as_str() {
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
