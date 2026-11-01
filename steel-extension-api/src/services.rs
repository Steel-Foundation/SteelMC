//! Cross-plugin services resolved by key, like Bukkit's `ServicesManager`.
//!
//! A contract is an ordinary stabby trait published in a small shared crate (for example
//! `vault-api`), wrapped as `stabby::dynptr!(Box<dyn Contract + Send + Sync>)`. The provider
//! registers a value during init; consumers look it up at use time:
//!
//! ```ignore
//! services::register::<DynEconomy>(&context, "vault:economy/v1", 10, Box::new(MyEconomy).into())?;
//! let economy = services::get::<DynEconomy>("vault:economy/v1")?;
//! ```
//!
//! The host stores the provider's stabby type report and refuses lookups whose expected type has
//! a different layout, so mismatched contract versions fail with an error instead of undefined
//! behavior. Registered values are pinned for the process lifetime. Keys should carry a
//! version suffix (`/v1`) and change it when a contract changes incompatibly.

use crate::ExtensionContext;
use stabby::{IStable, abi::report::TypeReport, string::String as ApiString};

/// A registered service object with its layout report.
#[stabby::stabby]
#[derive(Clone, Copy)]
pub struct ServiceHandle {
    report: &'static TypeReport,
    object: *const (),
}

/// One provider of a service.
#[stabby::stabby]
#[derive(Debug, Clone)]
pub struct ServiceInfo {
    /// Name of the providing extension.
    pub provider: ApiString,
    /// Provider priority; the highest wins lookups.
    pub priority: i32,
}

#[stabby::host_api]
/// Service registry bindings.
pub mod api {
    use super::{ServiceHandle, ServiceInfo};
    use crate::{ApiResult, ExtensionContext};
    #[cfg(feature = "host")]
    use stabby::string::String as ApiString;
    use stabby::{option::Option as StableOption, str::Str, vec::Vec as ApiVec};

    #[cfg(feature = "host")]
    use crate::host::services as host;

    impl<'a> ExtensionContext<'a> {
        /// Registers a service object owned by this extension.
        pub fn register_service(
            &self,
            key: Str<'_>,
            priority: i32,
            handle: ServiceHandle,
        ) -> ApiResult<()> {
            host::register(self.host().state.name, key.as_str(), priority, handle)
                .map_err(ApiString::from)
                .into()
        }
    }

    /// The highest-priority provider of `key`, if any.
    #[must_use]
    pub fn service(key: Str<'_>) -> StableOption<ServiceHandle> {
        host::lookup(key.as_str()).into()
    }

    /// Every provider of `key`, highest priority first.
    #[must_use]
    pub fn service_providers(key: Str<'_>) -> ApiVec<ServiceInfo> {
        host::providers(key.as_str())
    }
}

/// Registers `value` as a provider of `key` for the process lifetime.
pub fn register<T: IStable + Send + Sync + 'static>(
    context: &ExtensionContext<'_>,
    key: &str,
    priority: i32,
    value: T,
) -> Result<(), String> {
    let object: &'static T = Box::leak(Box::new(value));
    let handle = ServiceHandle {
        report: T::REPORT,
        object: core::ptr::from_ref(object).cast(),
    };
    context
        .register_service(key.into(), priority, handle)
        .match_owned(Ok, |error| Err(error.to_string()))
}

/// The highest-priority provider of `key`, checked against the expected contract type.
pub fn get<T: IStable + Send + Sync + 'static>(key: &str) -> Result<&'static T, String> {
    let handle: Option<ServiceHandle> = api::service(key.into()).into();
    let handle = handle.ok_or_else(|| format!("no provider for service {key}"))?;
    if !T::REPORT.is_compatible(handle.report) {
        return Err(format!("incompatible provider for service {key}"));
    }
    // SAFETY: the provider registered a leaked `&'static` value whose type report matches `T`
    // exactly, and provider libraries stay loaded for the process lifetime. `T: Sync` makes the
    // shared reference safe to use from any thread.
    Ok(unsafe { &*handle.object.cast::<T>() })
}

/// Every provider of `key`, highest priority first.
#[must_use]
pub fn providers(key: &str) -> Vec<ServiceInfo> {
    api::service_providers(key.into()).iter().cloned().collect()
}

#[cfg(feature = "host")]
impl ServiceHandle {
    pub(crate) const fn report(self) -> &'static TypeReport {
        self.report
    }
}

#[cfg(all(test, feature = "host"))]
pub(crate) const fn test_handle<T: IStable>(value: &'static T) -> ServiceHandle {
    ServiceHandle {
        report: T::REPORT,
        object: core::ptr::from_ref(value).cast(),
    }
}

#[cfg(all(test, feature = "host"))]
impl ServiceHandle {
    pub(crate) fn object_address(self) -> usize {
        self.object as usize
    }
}
