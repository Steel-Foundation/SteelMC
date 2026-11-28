use std::fmt::{self, Debug, Formatter};
use std::marker::PhantomData;
use std::ptr;
use std::sync::OnceLock;

use rustc_hash::{FxHashMap, FxHashSet};
use steel_utils::Identifier;

use super::{RegistryEntry, RegistryExt};

/// A named set of `T` registry entries, such as `BlockTag::CLIMBABLE`.
///
/// Tags are declared as statics and passed by reference, so a registry finds a
/// tag it registered by address instead of hashing the key. The key stays the
/// tag's identity: an unregistered `Tag` with the same key names the same tag.
pub struct Tag<T> {
    key: Identifier,
    _entry: PhantomData<fn() -> T>,
}

/// A tag declared as a static.
pub type TagRef<T> = &'static Tag<T>;

impl<T> Tag<T> {
    #[must_use]
    pub const fn new(key: Identifier) -> Self {
        Self {
            key,
            _entry: PhantomData,
        }
    }

    #[must_use]
    pub const fn vanilla_static(path: &'static str) -> Self {
        Self::new(Identifier::vanilla_static(path))
    }

    #[must_use]
    pub const fn key(&self) -> &Identifier {
        &self.key
    }
}

/// Tags are equal when their keys are, wherever they are declared.
impl<T> PartialEq for Tag<T> {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl<T> Eq for Tag<T> {}

impl<T> Debug for Tag<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Tag").field(&self.key).finish()
    }
}

/// Names a tag of `T` entries for a registry lookup.
///
/// A [`Tag`] is the typed form used by code. An [`Identifier`] names a tag by
/// key, for tags that come from data.
pub trait TagQuery<T> {
    #[doc(hidden)]
    fn find_in<'tags>(&self, tags: &'tags RegistryTags) -> Option<&'tags RegistryTag>;
}

impl<T> TagQuery<T> for Tag<T> {
    fn find_in<'tags>(&self, tags: &'tags RegistryTags) -> Option<&'tags RegistryTag> {
        let slot = tags
            .slots_by_address
            .get(&ptr::from_ref(self).addr())
            .or_else(|| tags.slots_by_key.get(&self.key))?;
        tags.tags.get(*slot)
    }
}

impl<T> TagQuery<T> for Identifier {
    fn find_in<'tags>(&self, tags: &'tags RegistryTags) -> Option<&'tags RegistryTag> {
        tags.tags.get(*tags.slots_by_key.get(self)?)
    }
}

impl<T, Q: TagQuery<T> + ?Sized> TagQuery<T> for &Q {
    fn find_in<'tags>(&self, tags: &'tags RegistryTags) -> Option<&'tags RegistryTag> {
        (**self).find_in(tags)
    }
}

#[derive(Debug, Default)]
pub struct RegistryTags {
    tags: Vec<RegistryTag>,
    slots_by_key: FxHashMap<Identifier, usize>,
    /// Slots of tags registered through a [`Tag`] static, by that static's address.
    slots_by_address: FxHashMap<usize, usize>,
}

#[derive(Debug)]
pub struct RegistryTag {
    key: Identifier,
    ordered_keys: Vec<&'static Identifier>,
    member_keys: FxHashSet<&'static Identifier>,
    /// Addresses of the frozen registry's entries whose key is a member.
    ///
    /// Membership follows entry keys, so an entry replaced under the same key
    /// stays a member. Entries can only change before the registry freezes,
    /// which is why this is built on the first lookup after that.
    frozen_members: OnceLock<FxHashSet<usize>>,
}

impl RegistryTag {
    fn new(key: Identifier, ordered_keys: Vec<&'static Identifier>) -> Self {
        let member_keys = ordered_keys.iter().copied().collect();
        Self {
            key,
            ordered_keys,
            member_keys,
            frozen_members: OnceLock::new(),
        }
    }

    fn frozen_members<R: RegistryExt>(&self, registry: &R) -> &FxHashSet<usize> {
        self.frozen_members.get_or_init(|| {
            (0..registry.len())
                .filter_map(|id| registry.by_id(id))
                .filter(|entry| self.member_keys.contains(entry.key()))
                .map(|entry| ptr::from_ref(entry).addr())
                .collect()
        })
    }
}

impl RegistryTags {
    /// Sets the entries of the tag named `key`, creating it if needed.
    #[doc(hidden)]
    pub fn insert(&mut self, key: Identifier, ordered_keys: Vec<&'static Identifier>) {
        self.set(key, ordered_keys);
    }

    /// Sets the entries of `tag` and lets later lookups find it by address.
    #[doc(hidden)]
    pub fn insert_tag<T>(&mut self, tag: TagRef<T>, ordered_keys: Vec<&'static Identifier>) {
        let slot = self.set(tag.key.clone(), ordered_keys);
        self.slots_by_address
            .insert(ptr::from_ref(tag).addr(), slot);
    }

    fn set(&mut self, key: Identifier, ordered_keys: Vec<&'static Identifier>) -> usize {
        if let Some(&slot) = self.slots_by_key.get(&key) {
            self.tags[slot] = RegistryTag::new(key, ordered_keys);
            return slot;
        }
        let slot = self.tags.len();
        self.slots_by_key.insert(key.clone(), slot);
        self.tags.push(RegistryTag::new(key, ordered_keys));
        slot
    }

    /// Returns whether `entry` is in `tag`.
    ///
    /// `frozen_registry` is the owning registry once it can no longer change.
    #[doc(hidden)]
    #[must_use]
    pub fn contains<R: RegistryExt>(
        &self,
        tag: &(impl TagQuery<R::Entry> + ?Sized),
        entry: &'static R::Entry,
        frozen_registry: Option<&R>,
    ) -> bool {
        let Some(tag) = tag.find_in(self) else {
            return false;
        };
        let Some(registry) = frozen_registry else {
            return tag.member_keys.contains(entry.key());
        };
        tag.frozen_members(registry)
            .contains(&ptr::from_ref(entry).addr())
    }

    #[doc(hidden)]
    #[must_use]
    pub fn get<T>(&self, tag: &(impl TagQuery<T> + ?Sized)) -> Option<&[&'static Identifier]> {
        tag.find_in(self).map(|tag| tag.ordered_keys.as_slice())
    }

    #[doc(hidden)]
    pub fn keys(&self) -> impl Iterator<Item = &Identifier> + '_ {
        self.tags.iter().map(|tag| &tag.key)
    }
}
