#[stabby::host_api]
/// Content registration capabilities borrowed during startup.
pub mod api {
    #[cfg(feature = "host")]
    use crate::host::content::{self, ContentRegistration};
    use crate::{
        ApiResult, BlockState,
        content::{
            BedDefinition, BlockDefinition, BlockItemDefinition, ItemDefinition, SlabMaterial,
        },
    };
    use stabby::{string::String as ApiString, vec::Vec as ApiVec};

    /// Exclusive access before the real item and block registries freeze.
    #[opaque]
    pub struct ContentRegistry<'a>(&'a mut ContentRegistration<'a>);

    impl<'a> ContentRegistry<'a> {
        /// Discovers every registered vanilla slab and its placing item.
        #[must_use]
        pub fn slab_materials(&self) -> ApiVec<SlabMaterial> {
            content::slab_materials(self.host().registry)
        }

        /// Registers a complete bed state family and its placing item.
        pub fn register_bed(&mut self, definition: BedDefinition) -> ApiResult<BlockState> {
            content::register_bed(self.host_mut().registry, definition)
                .map_err(ApiString::from)
                .into()
        }

        /// This extension's TOML configuration.
        #[must_use]
        pub fn config(&self) -> ApiString {
            self.host().config.into()
        }

        /// Registers a non-placing item with default item behavior.
        pub fn register_item(&mut self, definition: ItemDefinition) -> ApiResult<()> {
            content::register_item(self.host_mut().registry, definition)
                .map_err(ApiString::from)
                .into()
        }

        /// Registers a block without a placing item or default loot table.
        pub fn register_block(&mut self, definition: BlockDefinition) -> ApiResult<BlockState> {
            content::register_block(self.host_mut().registry, definition)
                .map_err(ApiString::from)
                .into()
        }

        /// Registers a block, its placing item, and optionally its self-drop.
        pub fn register_block_item(
            &mut self,
            definition: BlockItemDefinition,
        ) -> ApiResult<BlockState> {
            content::register_block_item(self.host_mut().registry, definition)
                .map_err(ApiString::from)
                .into()
        }
    }

    impl<'a> crate::Hooks<'a> {
        /// Subscribes during pre-init. Content is registered before behaviors,
        /// commands, extension init, and world loading.
        pub fn register_content_registration(
            &mut self,
            callback: extern "C" fn(ContentRegistry<'_>) -> ApiResult<()>,
        ) -> ApiResult<()> {
            content::subscribe(self.host_mut(), callback)
                .map_err(ApiString::from)
                .into()
        }
    }
}
