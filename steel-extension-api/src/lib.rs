//! Native extension contracts. Enable `host` only when compiling the server.
// The complete behavior vtable contains all core block callbacks.
#![recursion_limit = "512"]

mod bindings;
pub mod block_behavior;
mod callbacks;
pub mod command;
pub mod content;
pub mod effects;
mod entry;
pub mod events;
pub mod fill;
pub mod hooks;
pub mod hud;
pub mod items;
pub mod log;
pub mod menus;
pub mod players;
pub mod runtime;
pub mod scheduler;
pub mod services;
pub mod text;
pub mod worlds;
pub use runtime::{
    BotInput, Difficulty, GameMode, Operation, OperationState, OperationStatus, PlayerControls,
    PlayerId, PlayerInfo, Position, WorldSettings,
    api::{Command, Commands, Server},
};
#[cfg(feature = "host")]
pub mod host;

pub use bindings::api::*;
pub use entry::{ExtensionDescriptor, ExtensionInit, ExtensionPreInit, IntoApiResult};
pub use stabby;
#[cfg(not(feature = "host"))]
use stabby::host::{self as host_abi, BindError, Bootstrap};
use stabby::{result::Result as StableResult, string::String as StableString};

/// Version of this API crate, recorded in every extension descriptor.
pub const API_VERSION: &str = env!("CARGO_PKG_VERSION");

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
        .chain(events::api::imports())
        .chain(events::more::api::imports())
        .chain(players::api::imports())
        .chain(command::api::imports())
        .chain(services::api::imports())
        .chain(items::api::imports())
        .chain(effects::api::imports())
        .chain(hud::api::imports())
        .chain(worlds::api::imports())
        .chain(menus::api::imports())
        .chain(fill::api::imports())
        .chain(content::api::imports())
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

/// How a block change notifies neighbors and clients.
#[stabby::stabby]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockUpdateFlags {
    /// Vanilla `Block.UPDATE_*` bits.
    pub bits: u16,
}

impl BlockUpdateFlags {
    /// Notify neighbors so they can react (redstone, falling blocks, fluids).
    pub const NEIGHBORS: Self = Self { bits: 1 };
    /// Send the change to clients.
    pub const CLIENTS: Self = Self { bits: 1 << 1 };
    /// Neighbor updates and client sync, as used by normal gameplay.
    pub const ALL: Self = Self { bits: 0b11 };
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

/// Severity of a extension log message.
#[stabby::stabby]
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    /// A failure.
    Error,
    /// A recoverable problem.
    Warn,
    /// Normal operation.
    Info,
    /// Diagnostic detail.
    Debug,
}

/// Exports the extension descriptor and lifecycle entrypoints behind one checked bootstrap.
///
/// `pre_init` receives [`Hooks`] and `init` receives [`ExtensionContext`]; both may return
/// either [`ApiResult<()>`] or any `Result<(), E: Display>`. The name must match the
/// `[package]` name of the extension's `extension.toml`.
///
// Host builds have no extension bootstrap to export into, so the example only compiles for
// extensions.
#[cfg_attr(feature = "host", doc = "```ignore")]
#[cfg_attr(not(feature = "host"), doc = "```")]
/// mod extension {
///     use steel_extension_api::{ExtensionContext, Hooks};
///     fn pre_init(hooks: Hooks<'_>) -> Result<(), String> {
///         Ok(())
///     }
///     fn init(context: ExtensionContext<'_>) -> Result<(), String> {
///         Ok(())
///     }
///     steel_extension_api::export_extension! {
///         name: "example",
///         version: "1.0.0",
///         depends: [],
///         soft_depends: ["vault"],
///         pre_init: pre_init,
///         init: init,
///     }
/// }
/// ```
///
/// The short form `export_extension!("name", pre_init, init)` declares version `0.0.0` and no
/// dependencies.
#[macro_export]
macro_rules! export_extension {
    ($name:literal, $pre_init:path, $init:path) => {
        $crate::export_extension! {
            name: $name,
            version: "0.0.0",
            depends: [],
            soft_depends: [],
            pre_init: $pre_init,
            init: $init,
        }
    };
    (
        name: $name:literal,
        version: $version:literal,
        depends: [$($depends:literal),* $(,)?],
        soft_depends: [$($soft:literal),* $(,)?],
        pre_init: $pre_init:path,
        init: $init:path $(,)?
    ) => {
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
            $crate::log::set_extension_name($name);
            $crate::IntoApiResult::into_api_result($pre_init(hooks))
        }
        #[$crate::stabby::export(callback, crate = $crate::stabby)]
        fn __steel_extension_init(
            context: $crate::ExtensionContext<'_>,
        ) -> $crate::ApiResult<()> {
            $crate::IntoApiResult::into_api_result($init(context))
        }
        #[unsafe(no_mangle)]
        #[$crate::stabby::export(callback, crate = $crate::stabby)]
        pub fn steel_extension_v3() -> &'static $crate::ExtensionDescriptor {
            static DESCRIPTOR: $crate::ExtensionDescriptor = $crate::ExtensionDescriptor::new(
                $name,
                $version,
                $crate::API_VERSION,
                concat!($($depends, ","),*),
                concat!($($soft, ","),*),
                __steel_extension_pre_init,
                __steel_extension_init,
            );
            &DESCRIPTOR
        }
    };
}
