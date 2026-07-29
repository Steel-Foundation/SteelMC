//! The most common imports needed when working with inventories and menus
pub use crate::inventory::{
    click::{Click, ClickOutcome, DragKind, MouseButton, QuickCraft, SwapTarget},
    container::{Container, SimpleContainer},
    equipment::{EntityEquipment, EquipmentSlot, EquipmentSlotType, OwnedEntityEquipment},
    lock::{ContainerId, ContainerLockGuard, ContainerRef},
    menu::{
        ContainerSlots, DataSlot, FakeResultRemainderPolicy, FillDirection, GridPlacer, Menu,
        MenuBehavior, MenuBuilder, MenuKind, PlayerInventorySections, Rect, Region, Section,
        SectionKind, SectionSource, SlotFactory,
    },
    slots::{NormalSlot, RestrictedSlot, ResultHandler, Slot},
};

pub use crate::player::Player;
pub use steel_registry::item_stack::ItemStack;
pub use steel_utils::locks::{IntoShared, Shared};
