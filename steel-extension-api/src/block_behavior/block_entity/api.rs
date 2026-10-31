#[stabby::host_api]
pub mod api {
    #[cfg(feature = "host")]
    use crate::block_behavior::core_types;
    use crate::block_behavior::types::{
        BlockEntityRef, BorrowedNbt, Container, GameEventListener, ItemStackRef, Nbt, NbtMut,
    };
    use crate::{BlockPos, BlockState, World};
    use stabby::option::Option as StableOption;
    impl<'entity> BlockEntityRef<'entity> {
        /// Calls the captured entity implementation directly.
        pub fn on_block_state_changed(&self, state: BlockState) {
            self.host()
                .on_block_state_changed(core_types::BlockStateId(state.id));
        }
        /// Calls the captured entity implementation directly.
        pub fn on_set_removed(&self) {
            self.host().on_set_removed();
        }
        /// Calls the captured entity implementation directly.
        pub fn on_clear_removed(&self) {
            self.host().on_clear_removed();
        }
        /// Calls the captured entity implementation directly.
        #[must_use]
        pub fn trigger_event(&self, param_a: i32, param_b: i32) -> bool {
            self.host().trigger_event(param_a, param_b)
        }
        /// Calls the captured entity implementation directly.
        pub fn pre_remove_side_effects(&self, pos: BlockPos, state: BlockState) {
            self.host().pre_remove_side_effects(
                core_types::BlockPos::new(pos.x, pos.y, pos.z),
                core_types::BlockStateId(state.id),
            );
        }
        /// Calls the captured entity implementation directly.
        pub fn load_additional(&self, nbt: BorrowedNbt<'_>) {
            self.host().load_additional(nbt.into_host());
        }
        /// Calls the captured entity implementation directly.
        pub fn save_additional(&self, nbt: NbtMut<'_>) {
            self.host().save_additional(nbt.into_host());
        }
        /// Calls the captured entity implementation directly.
        #[must_use]
        pub fn get_update_tag(&self) -> StableOption<Nbt> {
            let result = self.host().get_update_tag();
            result.map(Nbt::from_host).into()
        }
        /// Calls the captured entity implementation directly.
        pub fn tick(&self, world: World<'_>) {
            self.host().tick(world.host());
        }
        /// Calls the captured entity implementation directly.
        #[must_use]
        pub fn container_ref(&self) -> StableOption<Container> {
            let result = self.host().container_ref();
            result.map(Container::from_host).into()
        }
        /// Calls the captured entity implementation directly.
        #[must_use]
        pub fn game_event_listener(&self) -> StableOption<GameEventListener> {
            let result = self.host().game_event_listener();
            result.map(GameEventListener::from_host).into()
        }
        /// Calls the captured entity implementation directly.
        pub fn apply_components_from_item(&self, item: ItemStackRef<'_>) {
            self.host().apply_components_from_item(item.into_host());
        }
    }
}
