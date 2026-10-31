//! Extension-defined block entity data and lifecycle callbacks.
use crate::block_behavior::types::{
    BorrowedNbt, Container, GameEventListener, ItemStackRef, Nbt, NbtMut,
};
use crate::{BlockPos, BlockState, World};
use stabby::option::Option as StableOption;
/// Entity-specific data and callbacks. Steel owns identity, lifecycle dispatch,
/// component storage, and persistence orchestration. Serialization is mandatory.
#[stabby::stabby(checked)]
pub trait BlockEntityBehavior: Send + Sync {
    /// On block state changed.
    extern "C" fn on_block_state_changed(&self, _state: BlockState) {}
    /// On set removed.
    extern "C" fn on_set_removed(&self) {}
    /// On clear removed.
    extern "C" fn on_clear_removed(&self) {}
    /// Trigger event.
    extern "C" fn trigger_event(&self, _param_a: i32, _param_b: i32) -> bool {
        false
    }
    /// Pre remove side effects.
    extern "C" fn pre_remove_side_effects(&self, _pos: BlockPos, _state: BlockState) {}
    /// Load additional.
    extern "C" fn load_additional(&self, nbt: BorrowedNbt<'_>);
    /// Save additional.
    extern "C" fn save_additional(&self, nbt: NbtMut<'_>);
    /// Get update tag.
    extern "C" fn get_update_tag(&self) -> StableOption<Nbt> {
        StableOption::None()
    }
    /// Tick.
    extern "C" fn tick(&self, _world: World<'_>) {}
    /// Container ref.
    extern "C" fn container_ref(&self) -> StableOption<Container> {
        StableOption::None()
    }
    /// Game event listener.
    extern "C" fn game_event_listener(&self) -> StableOption<GameEventListener> {
        StableOption::None()
    }
    /// Apply components from item.
    extern "C" fn apply_components_from_item(&self, _item: ItemStackRef<'_>) {}
}
/// Owning, checked block entity implementation.
pub type DynBlockEntityBehavior =
    stabby::dynptr!(stabby::boxed::Box<dyn BlockEntityBehavior + Send + Sync>);
pub(crate) mod api;
pub(crate) mod factory;
#[cfg(feature = "host")]
pub(crate) mod host;
