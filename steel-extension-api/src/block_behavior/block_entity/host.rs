use super::{BlockEntityBehaviorDyn, DynBlockEntityBehavior};
use crate::block_behavior::{
    core_types,
    types::{BorrowedNbt, ItemStackRef, NbtMut},
};
use crate::{BlockPos, BlockState, World};
use steel_utils::{DowncastType, DowncastTypeKey};
pub(crate) enum Base {
    New(core_types::BlockEntityBase),
    Original(core_types::SharedBlockEntity),
}
pub(crate) struct Adapter {
    pub base: Base,
    pub implementation: DynBlockEntityBehavior,
}
// SAFETY: this owner-scoped key identifies this one host adapter implementation;
// it never identifies or downcasts a extension's private payload.
unsafe impl DowncastType for Adapter {
    const TYPE_KEY: DowncastTypeKey =
        DowncastTypeKey::new("steel:extension/block_entity_adapter/v1");
}
impl core_types::BlockEntity for Adapter {
    fn base(&self) -> &core_types::BlockEntityBase {
        match &self.base {
            Base::New(base) => base,
            Base::Original(original) => original.base(),
        }
    }
    fn on_block_state_changed(&self, state: core_types::BlockStateId) {
        self.implementation
            .on_block_state_changed(BlockState { id: state.0 });
    }
    fn on_set_removed(&self) {
        self.implementation.on_set_removed();
    }
    fn on_clear_removed(&self) {
        self.implementation.on_clear_removed();
    }
    fn trigger_event(&self, param_a: i32, param_b: i32) -> bool {
        self.implementation.trigger_event(param_a, param_b)
    }
    fn pre_remove_side_effects(&self, pos: core_types::BlockPos, state: core_types::BlockStateId) {
        self.implementation.pre_remove_side_effects(
            BlockPos {
                x: pos.x(),
                y: pos.y(),
                z: pos.z(),
            },
            BlockState { id: state.0 },
        );
    }
    fn load_additional(&self, nbt: &core_types::BorrowedNbtCompound<'_>) {
        self.implementation
            .load_additional(BorrowedNbt::from_host(nbt));
    }
    fn save_additional(&self, nbt: &mut core_types::NbtCompound) {
        self.implementation.save_additional(NbtMut::from_host(nbt));
    }
    fn get_update_tag(&self) -> Option<core_types::NbtCompound> {
        let result = self.implementation.get_update_tag();
        result.match_owned(|value| Some(value.into_host()), || None)
    }
    fn tick(&self, world: &core_types::Arc<core_types::World>) {
        self.implementation.tick(World::from_host(world));
    }
    fn container_ref(&self) -> Option<core_types::ContainerRef> {
        let result = self.implementation.container_ref();
        result.match_owned(|value| Some(value.into_host()), || None)
    }
    fn game_event_listener(&self) -> Option<core_types::SharedGameEventListener> {
        let result = self.implementation.game_event_listener();
        result.match_owned(|value| Some(value.into_host()), || None)
    }
    fn apply_components_from_item(&self, item: &core_types::ItemStack) {
        self.implementation
            .apply_components_from_item(ItemStackRef::from_host(item));
    }
}
