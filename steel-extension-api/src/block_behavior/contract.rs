//! One declaration produces the extension trait, original imports, and native adapter.
macro_rules! block_behavior_contract {
    ($(
        $(#[$attr:meta])*
        fn $method:ident (
            $($arg:ident: $native:ty => $api:ty),* $(,)?
        ) $(-> $native_return:ty => $api_return:ty)?;
    )*) => {
        /// Extension implementation of a complete block behavior. Default methods
        /// call `original()`; overrides decide when to delegate, including capabilities.
        #[stabby::stabby(checked)]
        pub trait BlockBehavior: Send + Sync {
            /// Selects the original behavior used by default methods.
            extern "C" fn original(&self) -> OriginalBlockBehavior;
            $(
                $(#[$attr])*
                #[doc = concat!("Overrides `", stringify!($method), "`; the default calls the original.")]
                extern "C" fn $method(&self, $($arg: $api),*) $(-> $api_return)? {
                    self.original().$method($($arg),*)
                }
            )*
        }
        pub(crate) mod original {
            #[stabby::host_api]
            pub mod api {
                use crate::block_behavior::{types::*, InteractionResult, MovementResult};
                use crate::{ApiResult, BlockState, BlockPos, Registry, World};
                use stabby::{option::Option as StableOption, str::Str};
                #[cfg(feature = "host")]
                use crate::{block_behavior::{bridge::Bridge, core_types}, host};
                #[cfg(feature = "host")]
                use stabby::string::String as ApiString;

                /// A captured implementation, retained for the process lifetime.
                /// Later edits of the source registry do not change this handle.
                #[opaque]
                pub struct OriginalBlockBehavior(&'static core_types::Arc<dyn core_types::BlockBehavior>);

                #[expect(clippy::too_many_arguments, reason = "preserves the core BlockBehavior method signatures")]
                impl OriginalBlockBehavior {
                    /// Captures the implementation currently in the editable registry.
                    pub fn get(registry: &Registry<'_>, block: Str<'_>) -> ApiResult<Self> {
                        host::block_by_key(block.as_str()).map(|block| {
                            let behavior = registry.host().behaviors.get_behavior_shared(block);
                            // Captures and their borrowed capability views live as long
                            // as extension callbacks; this runtime never unloads extensions.
                            Self::from_host(Box::leak(Box::new(behavior)))
                        }).map_err(ApiString::from).into()
                    }
                    $(
                        $(#[$attr])*
                        #[doc = concat!("Calls the original `", stringify!($method), "` implementation directly.")]
                        pub fn $method(self, $($arg: $api),*) $(-> $api_return)? {
                            crate::block_behavior::contract::block_behavior_contract!(@from [$($api_return)?]
                                self.host_behavior().$method($(Bridge::into_host($arg)),*)
                            )
                        }
                    )*
                    fn host_behavior(self) -> &'static dyn core_types::BlockBehavior {
                        self.host().as_ref()
                    }
                }
            }
        }
        #[cfg(feature = "host")]
        pub(crate) mod adapter {
            use super::{BlockBehaviorDyn, DynBlockBehavior, core_types, bridge::Bridge};
            pub(crate) struct Adapter(pub(crate) DynBlockBehavior);
            impl core_types::BlockBehavior for Adapter {
                $(fn $method(&self, $($arg: $native),*) $(-> $native_return)? {
                    crate::block_behavior::contract::block_behavior_contract!(@into [$($api_return)?]
                        self.0.$method($(Bridge::from_host($arg)),*)
                    )
                })*
            }
        }
    };
    (@from [] $call:expr) => { { $call; } };
    (@from [$ty:ty] $call:expr) => { Bridge::from_host($call) };
    (@into [] $call:expr) => { { $call; } };
    (@into [$ty:ty] $call:expr) => { Bridge::into_host($call) };
}
pub(super) use block_behavior_contract;
