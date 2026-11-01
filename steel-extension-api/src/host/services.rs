//! Process-wide service registry.
use crate::services::{ServiceHandle, ServiceInfo};
use stabby::vec::Vec as ApiVec;
use steel_utils::locks::SyncMutex;

struct Entry {
    key: String,
    provider: &'static str,
    priority: i32,
    handle: ServiceHandle,
}

/// Registered services in registration order. Entries are never removed because providers
/// are never unloaded.
struct Registry(Vec<Entry>);

// SAFETY: every handle points at a leaked `Send + Sync` value of a pinned library and is only
// dereferenced by consumers after their type report check.
unsafe impl Send for Registry {}

static SERVICES: SyncMutex<Registry> = SyncMutex::new(Registry(Vec::new()));

pub(crate) fn register(
    provider: &'static str,
    key: &str,
    priority: i32,
    handle: ServiceHandle,
) -> Result<(), String> {
    if key.is_empty() {
        return Err("service key must not be empty".into());
    }
    let mut services = SERVICES.lock();
    if services
        .0
        .iter()
        .any(|entry| entry.key == key && entry.provider == provider)
    {
        return Err(format!(
            "extension {provider} already provides service {key}"
        ));
    }
    if let Some(existing) = services.0.iter().find(|entry| entry.key == key)
        && !existing.handle.report().is_compatible(handle.report())
    {
        log::warn!(
            "Extension {provider} provides service {key} with a layout different from {}",
            existing.provider
        );
    }
    services.0.push(Entry {
        key: key.to_owned(),
        provider,
        priority,
        handle,
    });
    Ok(())
}

/// Highest priority wins; among equal priorities the first registration wins.
fn ranked(services: &Registry, key: &str) -> Vec<usize> {
    let mut matches: Vec<usize> = services
        .0
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.key == key)
        .map(|(index, _)| index)
        .collect();
    matches.sort_by_key(|index| core::cmp::Reverse(services.0[*index].priority));
    matches
}

pub(crate) fn lookup(key: &str) -> Option<ServiceHandle> {
    let services = SERVICES.lock();
    ranked(&services, key)
        .first()
        .map(|index| services.0[*index].handle)
}

pub(crate) fn providers(key: &str) -> ApiVec<ServiceInfo> {
    let services = SERVICES.lock();
    ranked(&services, key)
        .into_iter()
        .map(|index| ServiceInfo {
            provider: services.0[index].provider.into(),
            priority: services.0[index].priority,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use stabby::IStable;

    fn handle<T: IStable>(value: &'static T) -> ServiceHandle {
        crate::services::test_handle(value)
    }

    #[test]
    fn highest_priority_then_first_registration_wins() {
        static LOW: u32 = 1;
        static HIGH: u32 = 2;
        static TIE: u32 = 3;
        let key = "test:ranking/v1";
        assert!(register("low", key, 0, handle(&LOW)).is_ok());
        assert!(register("high", key, 10, handle(&HIGH)).is_ok());
        assert!(register("tie", key, 10, handle(&TIE)).is_ok());
        let providers: Vec<_> = providers(key)
            .iter()
            .map(|info| (info.provider.to_string(), info.priority))
            .collect();
        assert_eq!(
            providers,
            [
                ("high".to_owned(), 10),
                ("tie".to_owned(), 10),
                ("low".to_owned(), 0)
            ]
        );
        let winner = lookup(key).map(ServiceHandle::object_address);
        assert_eq!(
            winner,
            Some(core::ptr::from_ref(&HIGH).cast::<()>() as usize)
        );
    }

    #[test]
    fn duplicate_provider_is_rejected_and_unknown_keys_are_empty() {
        static VALUE: u8 = 0;
        assert!(register("dup", "test:dup/v1", 0, handle(&VALUE)).is_ok());
        assert!(register("dup", "test:dup/v1", 5, handle(&VALUE)).is_err());
        assert!(lookup("test:missing/v1").is_none());
        assert!(providers("test:missing/v1").is_empty());
    }

    #[test]
    fn typed_lookup_rejects_incompatible_reports() {
        static VALUE: u32 = 7;
        assert!(register("typed", "test:typed/v1", 0, handle(&VALUE)).is_ok());
        assert_eq!(crate::services::get::<u32>("test:typed/v1").copied(), Ok(7));
        assert!(
            crate::services::get::<u64>("test:typed/v1")
                .is_err_and(|error| error.contains("incompatible"))
        );
        assert!(crate::services::get::<u32>("test:none/v1").is_err());
    }
}
