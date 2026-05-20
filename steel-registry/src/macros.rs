/// Implements `RegistryExt` for a registry type.
///
/// Expects `$id_field` to be `Vec<&'static $Entry>`.
#[macro_export]
macro_rules! impl_registry_ext {
    ($Registry:ty, $Entry:ty, $id_field:ident, $key_field:ident) => {
        impl $crate::RegistryExt for $Registry {
            type Entry = $Entry;

            fn freeze(&mut self) {
                self.allows_registering = false;
            }

            fn by_id(&self, id: usize) -> Option<&'static $Entry> {
                self.$id_field.get(id).copied()
            }

            fn by_key(&self, key: &steel_utils::Identifier) -> Option<&'static $Entry> {
                self.$key_field
                    .get(key)
                    .and_then(|&id| self.$id_field.get(id).copied())
            }

            fn id_from_key(&self, key: &steel_utils::Identifier) -> Option<usize> {
                self.$key_field.get(key).copied()
            }

            fn len(&self) -> usize {
                self.$id_field.len()
            }

            fn is_empty(&self) -> bool {
                self.$id_field.is_empty()
            }
        }
    };
}

/// Implements `RegistryEntry` for an entry type via hash map lookup.
#[macro_export]
macro_rules! impl_registry_entry {
    ($Entry:ty, $global_field:ident) => {
        impl $crate::RegistryEntry for $Entry {
            fn key(&self) -> &steel_utils::Identifier {
                &self.key
            }

            fn try_id(&self) -> Option<usize> {
                use $crate::RegistryExt;
                $crate::REGISTRY.$global_field.id_from_key(&self.key)
            }
        }
    };
}

/// Implements the default register, replace, and iter methods in the registries
#[macro_export]
macro_rules! impl_standard_methods {
    ($Registry:ty, $Entry:ty, $id_field:ident, $key_field:ident, $allow_registering:ident) => {
        impl $Registry {
            pub fn register(&mut self, entry: $Entry) -> usize {
                assert!(
                    self.$allow_registering,
                    concat!(
                        "Cannot register ",
                        stringify!($Entry),
                        " after registry has been frozen"
                    )
                );
                let id = self.$id_field.len();
                self.$id_field.push(entry);
                self.$key_field.insert(entry.key.clone(), id);
                id
            }

            pub fn iter(&self) -> impl Iterator<Item = (usize, $Entry)> + '_ {
                self.$id_field
                    .iter()
                    .enumerate()
                    .map(|(id, &entry)| (id, entry))
            }
        }

        impl Default for $Registry {
            fn default() -> Self {
                Self::new()
            }
        }
    };
}

/// Implements both `RegistryExt` and `RegistryEntry` for a standard registry.
#[macro_export]
macro_rules! impl_registry {
    ($Registry:ty, $Entry:ty, $id_field:ident, $key_field:ident, $global_field:ident) => {
        $crate::impl_registry_ext!($Registry, $Entry, $id_field, $key_field);
        $crate::impl_registry_entry!($Entry, $global_field);
    };
}

/// Implements `TaggedRegistryExt` for a registry with tag support.
#[macro_export]
macro_rules! impl_tagged_registry {
    ($Registry:ty, $key_field:ident, $entity_name:literal) => {
        impl $crate::TaggedRegistryExt for $Registry {
            fn register_tag(&mut self, tag: steel_utils::Identifier, keys: &[&'static str]) {
                assert!(
                    self.allows_registering,
                    "Cannot register tags after registry has been frozen"
                );

                let identifiers: Vec<steel_utils::Identifier> = keys
                    .iter()
                    .filter_map(|key| {
                        let ident = steel_utils::registry::registry_vanilla_or_custom_tag(key);
                        if self.$key_field.contains_key(&ident) {
                            Some(ident)
                        } else {
                            None
                        }
                    })
                    .collect();

                self.tags.insert(tag, identifiers);
            }

            fn modify_tag(
                &mut self,
                tag: &steel_utils::Identifier,
                f: impl FnOnce(Vec<steel_utils::Identifier>) -> Vec<steel_utils::Identifier>,
            ) {
                let existing = self.tags.remove(tag).unwrap_or_default();
                let entries = f(existing)
                    .into_iter()
                    .filter(|key| {
                        let exists = self.$key_field.contains_key(key);
                        if !exists {
                            tracing::error!(
                                "{} {} not found in registry, skipping from tag {}",
                                $entity_name,
                                key,
                                tag,
                            );
                        }
                        exists
                    })
                    .collect();
                self.tags.insert(tag.clone(), entries);
            }

            fn is_in_tag(
                &self,
                entry: &'static Self::Entry,
                tag: &steel_utils::Identifier,
            ) -> bool {
                self.tags
                    .get(tag)
                    .is_some_and(|entries| entries.contains(&entry.key))
            }

            fn get_tag(&self, tag: &steel_utils::Identifier) -> Option<Vec<&'static Self::Entry>> {
                use $crate::RegistryExt;
                self.tags.get(tag).map(|idents| {
                    idents
                        .iter()
                        .filter_map(|ident| self.by_key(ident))
                        .collect()
                })
            }

            fn iter_tag(
                &self,
                tag: &steel_utils::Identifier,
            ) -> impl Iterator<Item = &'static Self::Entry> + '_ {
                use $crate::RegistryExt;
                self.tags
                    .get(tag)
                    .into_iter()
                    .flat_map(|v| v.iter().filter_map(|ident| self.by_key(ident)))
            }

            fn tag_keys(&self) -> impl Iterator<Item = &steel_utils::Identifier> + '_ {
                self.tags.keys()
            }
        }
    };
}
