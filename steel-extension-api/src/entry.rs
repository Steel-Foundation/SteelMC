use crate::{ApiResult, ExtensionContext, Hooks};
use core::mem::transmute;
use stabby::{
    IStable,
    host::{Bootstrap, ErasedFunction, Export, RegistrationHook, RegistrationStatus},
    str::Str,
};

/// Pre-init binds the API and exposes only hook subscriptions.
pub type ExtensionPreInit = extern "C" fn(Bootstrap, Hooks<'_>) -> ApiResult<()>;
/// Init runs after registration, with configuration, data directory and frozen registries.
pub type ExtensionInit = extern "C" fn(ExtensionContext<'_>) -> ApiResult<()>;
type PreInitReport = extern "C" fn(
    extern "C" fn(RegistrationHook<'static>) -> RegistrationStatus,
    Hooks<'static>,
) -> ApiResult<()>;
type InitReport = extern "C" fn(ExtensionContext<'static>) -> ApiResult<()>;

/// Descriptor returned by the single `steel_extension_v3` bootstrap symbol.
///
/// Dependency lists are comma-separated extension names so the descriptor stays a `const`
/// value; the host splits and validates them before running any extension code.
#[stabby::stabby]
pub struct ExtensionDescriptor {
    /// Unique extension name, also its data directory name and package manifest name.
    pub name: Str<'static>,
    /// Extension version, for diagnostics; must match the package manifest.
    pub version: Str<'static>,
    /// `steel-extension-api` package version the extension was built with.
    pub api_version: Str<'static>,
    /// Extensions that must be loaded and initialized first.
    pub depends: Str<'static>,
    /// Extensions initialized first when present.
    pub soft_depends: Str<'static>,
    pre_init: Export,
    initialize: Export,
}

impl ExtensionDescriptor {
    /// Describes distinct, typed lifecycle entrypoints.
    pub const fn new(
        name: &'static str,
        version: &'static str,
        api_version: &'static str,
        depends: &'static str,
        soft_depends: &'static str,
        pre_init: ExtensionPreInit,
        initialize: ExtensionInit,
    ) -> Self {
        Self {
            name: Str::new(name),
            version: Str::new(version),
            api_version: Str::new(api_version),
            depends: Str::new(depends),
            soft_depends: Str::new(soft_depends),
            pre_init: Self::entry::<PreInitReport>(
                "steel:extension/pre_init/v3",
                pre_init as *const (),
            ),
            initialize: Self::entry::<InitReport>(
                "steel:extension/init/v3",
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

/// Converts an extension entrypoint result into the ABI result type.
///
/// Implemented for `ApiResult<()>` and any `Result<(), E: Display>`, so entrypoints can use `?`.
pub trait IntoApiResult {
    /// Performs the conversion.
    fn into_api_result(self) -> ApiResult<()>;
}

impl IntoApiResult for ApiResult<()> {
    fn into_api_result(self) -> ApiResult<()> {
        self
    }
}

impl<E: core::fmt::Display> IntoApiResult for Result<(), E> {
    fn into_api_result(self) -> ApiResult<()> {
        self.map_err(|error| stabby::string::String::from(error.to_string()))
            .into()
    }
}
