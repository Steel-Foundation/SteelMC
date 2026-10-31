use crate::{ApiResult, Hooks};
use core::mem::transmute;
use stabby::{
    IStable,
    host::{Bootstrap, ErasedFunction, Export, RegistrationHook, RegistrationStatus},
    str::Str,
};

/// Pre-init binds the API and exposes only hook subscriptions.
pub type ExtensionPreInit = extern "C" fn(Bootstrap, Hooks<'_>) -> ApiResult<()>;
/// Init runs after registration, with configuration and frozen static registries.
pub type ExtensionInit = extern "C" fn(Str<'_>) -> ApiResult<()>;
type PreInitReport = extern "C" fn(
    extern "C" fn(RegistrationHook<'static>) -> RegistrationStatus,
    Hooks<'static>,
) -> ApiResult<()>;
type InitReport = extern "C" fn(Str<'static>) -> ApiResult<()>;

/// Descriptor returned by the single `steel_extension_v2` bootstrap symbol.
#[stabby::stabby]
pub struct ExtensionDescriptor {
    /// Diagnostic extension name.
    pub name: Str<'static>,
    pre_init: Export,
    initialize: Export,
}

impl ExtensionDescriptor {
    /// Describes distinct, typed lifecycle entrypoints.
    pub const fn new(
        name: &'static str,
        pre_init: ExtensionPreInit,
        initialize: ExtensionInit,
    ) -> Self {
        Self {
            name: Str::new(name),
            pre_init: Self::entry::<PreInitReport>(
                "steel:extension/pre_init/v2",
                pre_init as *const (),
            ),
            initialize: Self::entry::<InitReport>(
                "steel:extension/init/v2",
                initialize as *const (),
            ),
        }
    }
    const fn entry<Signature: IStable>(key: &'static str, pointer: *const ()) -> Export {
        // SAFETY: the private constructor supplies a non-null function pointer;
        // the erased form is restored only after its exact signature is checked.
        let erased = unsafe { transmute::<*const (), ErasedFunction>(pointer) };
        // SAFETY: the typed constructor pairs each pointer with its canonical
        // lifetime-erased signature report.
        unsafe { Export::new(key, Signature::REPORT, erased) }
    }
    #[cfg(feature = "host")]
    pub(crate) fn pre_initializer(&'static self) -> Result<ExtensionPreInit, String> {
        if !PreInitReport::REPORT.is_compatible(self.pre_init.report()) {
            return Err(format!("incompatible pre-init ABI for {}", self.name));
        }
        // SAFETY: the report checks the exact signature, and the loader pins this library.
        Ok(unsafe { transmute::<ErasedFunction, ExtensionPreInit>(self.pre_init.erased()) })
    }
    #[cfg(feature = "host")]
    pub(crate) fn initializer(&'static self) -> Result<ExtensionInit, String> {
        if !InitReport::REPORT.is_compatible(self.initialize.report()) {
            return Err(format!("incompatible init ABI for {}", self.name));
        }
        // SAFETY: the report checks the exact signature, and the loader pins this library.
        Ok(unsafe { transmute::<ErasedFunction, ExtensionInit>(self.initialize.erased()) })
    }
}
