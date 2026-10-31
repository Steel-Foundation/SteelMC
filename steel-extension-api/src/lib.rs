//! Native extension contracts. Enable `host` only when compiling the server.
// The complete behavior vtable contains all core block callbacks.
#![recursion_limit = "512"]

mod bindings;
pub mod block_behavior;
mod entry;
pub mod hooks;
pub mod runtime;
pub use runtime::{
    BotInput, Difficulty, GameMode, Operation, OperationState, OperationStatus, PlayerControls,
    PlayerId, PlayerInfo, Position, WorldSettings,
    api::{Command, Commands, Server},
};
#[cfg(feature = "host")]
pub mod host;

pub use bindings::api::*;
pub use entry::{ExtensionDescriptor, ExtensionInit, ExtensionPreInit};
pub use stabby;
#[cfg(not(feature = "host"))]
use stabby::host::{self as host_abi, BindError, Bootstrap};
use stabby::{result::Result as StableResult, string::String as StableString};

/// An owned ABI-safe error result. Convert with `into()` to use ordinary Rust `?`.
pub type ApiResult<T> = StableResult<T, StableString>;

/// Binds the entire API atomically before any extension callback is installed.
#[cfg(not(feature = "host"))]
pub fn bind(bootstrap: Bootstrap) -> Result<(), BindError> {
    let imports: Vec<_> = bindings::api::imports()
        .iter()
        .chain(block_behavior::original::api::imports())
        .chain(block_behavior::values_api::api::imports())
        .chain(block_behavior::capability_api::api::imports())
        .chain(block_behavior::block_entity::api::api::imports())
        .chain(block_behavior::block_entity::factory::api::imports())
        .chain(runtime::api::imports())
        .chain(hooks::api::imports())
        .copied()
        .collect();
    host_abi::bind(bootstrap, &imports)
}

/// A block state resolved by the host registry. Extensions cannot construct raw IDs.
#[stabby::stabby]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockState {
    pub(crate) id: u16,
}

/// A world-space block position.
#[stabby::stabby]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockPos {
    /// X coordinate.
    pub x: i32,
    /// Y coordinate.
    pub y: i32,
    /// Z coordinate.
    pub z: i32,
}

/// One terrain write, validated against the current generation capability.
#[stabby::stabby]
#[derive(Debug, Clone, Copy)]
pub struct BlockChange {
    /// World-space position.
    pub pos: BlockPos,
    /// Host-resolved state.
    pub state: BlockState,
}

/// Runs synchronously at the end of each unfrozen world tick.
///
/// Export macros check the types and select the calling convention; the extension
/// still registers this function explicitly through `Hooks` during pre-init.
///
/// ```
/// use steel_extension_api::{stabby, World, WorldTickCallback};
/// #[stabby::export(callback)]
/// fn tick(world: World<'_>, tick: u64) {}
/// let callback: WorldTickCallback = tick;
/// ```
///
/// Ordinary Rust-owned types cannot be exported across the native boundary:
///
/// ```compile_fail
/// use steel_extension_api::stabby;
/// #[stabby::export(callback)]
/// fn invalid(value: std::string::String) {}
/// ```
pub type WorldTickCallback = extern "C" fn(World<'_>, u64);
/// Fills terrain for the current chunk. Biomes and dimension rules are registered separately.
pub type TerrainCallback = extern "C" fn(Terrain<'_>);

/// Exports distinct pre-init and init entrypoints behind one checked bootstrap.
///
/// ```
/// mod extension {
///     fn pre_init(hooks: steel_extension_api::Hooks<'_>) -> steel_extension_api::ApiResult<()> {
///         Ok(()).into()
///     }
///     fn init(config: steel_extension_api::stabby::str::Str<'_>) -> steel_extension_api::ApiResult<()> {
///         Ok(()).into()
///     }
///     steel_extension_api::export_extension!("example", pre_init, init);
/// }
/// ```
#[macro_export]
macro_rules! export_extension {
    ($name:literal, $pre_init:path, $init:path) => {
        #[$crate::stabby::export(callback, crate = $crate::stabby)]
        fn __steel_extension_pre_init(
            bootstrap: extern "C" fn(
                $crate::stabby::host::RegistrationHook<'_>,
            ) -> $crate::stabby::host::RegistrationStatus,
            hooks: $crate::Hooks<'_>,
        ) -> $crate::ApiResult<()> {
            if let Err(error) = $crate::bind(bootstrap) {
                return Err($crate::stabby::string::String::from(error.to_string())).into();
            }
            $pre_init(hooks)
        }
        #[$crate::stabby::export(callback, crate = $crate::stabby)]
        fn __steel_extension_init(config: $crate::stabby::str::Str<'_>) -> $crate::ApiResult<()> {
            $init(config)
        }
        #[unsafe(no_mangle)]
        #[$crate::stabby::export(callback, crate = $crate::stabby)]
        pub fn steel_extension_v2() -> &'static $crate::ExtensionDescriptor {
            static DESCRIPTOR: $crate::ExtensionDescriptor = $crate::ExtensionDescriptor::new(
                $name,
                __steel_extension_pre_init,
                __steel_extension_init,
            );
            &DESCRIPTOR
        }
    };
}
