#[stabby::host_api]
pub mod api {
    use crate::block_behavior::{DynBlockBehavior, types::ItemStack};
    use crate::{ApiResult, BlockChange, BlockPos, BlockState};
    use stabby::{slice::Slice, str::Str, string::String as ApiString, vec::Vec as ApiVec};

    #[cfg(feature = "host")]
    use crate::host::{self, HookRegistration, Registration, TerrainAccess, TickHandler};
    #[cfg(feature = "host")]
    use std::sync::Arc;
    #[cfg(feature = "host")]
    use steel_core::{world::World as CoreWorld, worldgen::WorldGeneratorRegistry};
    #[cfg(feature = "host")]
    use steel_utils::ChunkPos;

    /// Mutable block access supplied only during the block registration event.
    /// Host object lifetimes are invariant because their backing types can contain
    /// exclusive borrows.
    ///
    /// ```compile_fail
    /// fn shorten<'long: 'short, 'short>(
    ///     registry: steel_extension_api::Registry<'long>,
    /// ) -> steel_extension_api::Registry<'short> {
    ///     registry
    /// }
    /// ```
    ///
    /// Exclusive handles also reject manual `Copy` implementations:
    ///
    /// ```compile_fail
    /// use steel_extension_api::stabby;
    /// #[stabby::host_api]
    /// mod api {
    ///     #[opaque]
    ///     pub struct Borrowed<'a>(&'a mut ());
    /// }
    /// impl Copy for api::Borrowed<'_> {}
    /// impl Clone for api::Borrowed<'_> {
    ///     fn clone(&self) -> Self { *self }
    /// }
    /// ```
    #[opaque]
    pub struct Registry<'a>(&'a mut Registration<'a>);

    impl<'a> Registry<'a> {
        /// Returns this extension's configuration as a TOML document.
        #[must_use]
        pub fn config(&self) -> ApiString {
            self.host().config.into()
        }

        /// Replaces the complete behavior. Its default methods and explicit delegation
        /// use the original selected by the extension implementation.
        pub fn replace_block_behavior(
            &mut self,
            block: Str<'_>,
            behavior: DynBlockBehavior,
        ) -> ApiResult<()> {
            host::replace_block_behavior(self.host_mut(), block.as_str(), behavior)
                .map_err(ApiString::from)
                .into()
        }
    }

    /// Mutable generator registry supplied only during its registration event.
    #[opaque]
    pub struct GeneratorRegistry<'a>(&'a mut WorldGeneratorRegistry);

    impl<'a> GeneratorRegistry<'a> {
        /// Registers a terrain-only generator with a uniform biome and no decorators.
        /// Dimension rules and all registry lookups are resolved on the host.
        pub fn register_terrain_generator(
            &mut self,
            key: Str<'_>,
            dimension_type: Str<'_>,
            biome: Str<'_>,
            sea_level: i32,
            callback: extern "C" fn(Terrain<'_>),
        ) -> ApiResult<()> {
            host::register_generator(
                self.host_mut(),
                key.as_str(),
                dimension_type.as_str(),
                biome.as_str(),
                sea_level,
                callback,
            )
            .map_err(ApiString::from)
            .into()
        }
    }

    /// Startup-only event subscriptions, separate from registry replacements.
    #[opaque]
    pub struct Hooks<'a>(&'a mut HookRegistration<'a>);

    impl<'a> Hooks<'a> {
        /// Subscribes during pre-init; registration edits the live block registry.
        pub fn register_block_registration(
            &mut self,
            callback: extern "C" fn(Registry<'_>) -> ApiResult<()>,
        ) -> ApiResult<()> {
            host::subscribe_blocks(self.host_mut(), callback)
                .map_err(ApiString::from)
                .into()
        }

        /// Subscribes during pre-init; registration edits the live generator registry.
        pub fn register_generator_registration(
            &mut self,
            callback: extern "C" fn(GeneratorRegistry<'_>) -> ApiResult<()>,
        ) -> ApiResult<()> {
            host::subscribe_generators(self.host_mut(), callback)
                .map_err(ApiString::from)
                .into()
        }

        /// Runs a callback at the end of every normal world tick.
        pub fn register_world_tick(&mut self, callback: extern "C" fn(World<'_>, u64)) {
            self.host_mut()
                .hooks
                .register_world_tick(Box::new(TickHandler(callback)));
        }
    }

    /// World access borrowed only for the current callback, on its existing tick thread.
    ///
    /// ```compile_fail
    /// fn retain(world: steel_extension_api::World<'_>) -> steel_extension_api::World<'static> {
    ///     world
    /// }
    /// ```
    ///
    /// ```compile_fail
    /// fn send_to_worker(world: steel_extension_api::World<'_>) {
    ///     std::thread::scope(|scope| {
    ///         scope.spawn(move || world.is_chunk_loaded(0, 0));
    ///     });
    /// }
    /// ```
    #[opaque]
    pub struct World<'a>(&'a Arc<CoreWorld>);

    impl<'a> World<'a> {
        /// Returns the loaded world key (`domain:world`).
        #[must_use]
        pub fn key(&self) -> ApiString {
            self.host().key.to_string().into()
        }

        /// Checks full-chunk readiness without requesting loading or generation.
        #[must_use]
        pub fn is_chunk_loaded(&self, chunk_x: i32, chunk_z: i32) -> bool {
            self.host()
                .chunk_map
                .with_full_chunk(ChunkPos::new(chunk_x, chunk_z), |_| ())
                .is_some()
        }

        /// Reads a loaded block. Unavailable chunks and out-of-bounds positions are errors.
        pub fn block(&self, pos: BlockPos) -> ApiResult<BlockState> {
            host::read_block(self.host(), pos)
                .map_err(ApiString::from)
                .into()
        }

        /// Changes a loaded block through normal gameplay updates.
        pub fn set_block(&self, pos: BlockPos, state: BlockState) -> ApiResult<()> {
            host::set_block(self.host(), pos, state)
                .map_err(ApiString::from)
                .into()
        }

        /// Drops an owned stack near a loaded block with normal pickup delay.
        /// Returns whether an entity spawned; empty stacks and disabled block
        /// drops do not spawn an entity. Unavailable positions are errors.
        #[must_use]
        pub fn pop_resource(&self, pos: BlockPos, item: ItemStack) -> ApiResult<bool> {
            if let Err(error) = host::read_block(self.host(), pos) {
                return Err(ApiString::from(error)).into();
            }
            Ok(self
                .host()
                .pop_resource(
                    steel_utils::BlockPos::new(pos.x, pos.y, pos.z),
                    item.into_host(),
                )
                .is_some())
            .into()
        }
    }

    /// Chunk access restricted to the scheduler's current terrain phase.
    #[opaque]
    pub struct Terrain<'a>(&'a TerrainAccess<'a>);

    impl<'a> Terrain<'a> {
        /// Minimum X of the current chunk in world block coordinates.
        #[must_use]
        pub fn min_x(&self) -> i32 {
            self.host().chunk.pos().0.x * 16
        }

        /// Minimum Z of the current chunk in world block coordinates.
        #[must_use]
        pub fn min_z(&self) -> i32 {
            self.host().chunk.pos().0.y * 16
        }

        /// Dimension's minimum build Y.
        #[must_use]
        pub fn min_y(&self) -> i32 {
            self.host().chunk.min_y()
        }

        /// Dimension's build height.
        #[must_use]
        pub fn height(&self) -> i32 {
            self.host().chunk.height()
        }

        /// Positional vanilla xoroshiro sampling, independent of chunk invocation order.
        #[must_use]
        pub fn random_at(&self, salt: u64, pos: BlockPos) -> u64 {
            host::random_at(self.host().seed, salt, pos)
        }

        /// Validates a whole batch before writing it through the terrain capability.
        pub fn write_blocks(&self, changes: Slice<'_, BlockChange>) -> ApiResult<()> {
            host::write_terrain(self.host(), changes.as_slice())
                .map_err(ApiString::from)
                .into()
        }
    }

    /// Resolves the default state of a registered block. Numeric IDs remain private.
    pub fn default_block_state(key: Str<'_>) -> ApiResult<BlockState> {
        host::block_by_key(key.as_str())
            .map(|block| BlockState {
                id: block.default_state().0,
            })
            .map_err(ApiString::from)
            .into()
    }

    /// Resolves block names from a host registry tag, preserving generated vanilla data.
    pub fn blocks_in_tag(key: Str<'_>) -> ApiResult<ApiVec<ApiString>> {
        host::blocks_in_tag(key.as_str())
            .map_err(ApiString::from)
            .into()
    }
}
