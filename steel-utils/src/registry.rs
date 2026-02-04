use crate::Identifier;

/// Creates a vanilla or a new custom tag for the generator build scripts
#[must_use]
pub fn registry_vanilla_or_custom_tag(key: &'static str) -> Identifier {
    if let Some(key) = key.strip_prefix("c:") {
        Identifier::new_static("c", key)
    } else {
        Identifier::vanilla_static(key)
    }
}
