//! Downstream-crate coverage for keyed downcasting.

use steel_utils::{Downcast as _, DowncastType, DowncastTypeKey, ErasedType};

struct PluginOwnedType(u32);

// SAFETY: This test crate owns the key and concrete type, and no other type in
// the test process uses this key.
unsafe impl DowncastType for PluginOwnedType {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel_utils_test:entity/plugin_owned");
}

#[test]
fn downstream_type_receives_sealed_erasure_implementation() {
    let value = PluginOwnedType(42);
    let erased: &dyn ErasedType = &value;

    assert_eq!(
        erased
            .downcast_ref::<PluginOwnedType>()
            .map(|value| value.0),
        Some(42)
    );
}
