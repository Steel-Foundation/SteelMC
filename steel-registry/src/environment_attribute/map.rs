use super::{
    AttributeModifier, AttributeModifierEntry, AttributeValue, EnvironmentAttribute,
    ErasedAttributeModifierEntry, ErasedEnvironmentAttribute, ErasedEnvironmentAttributeRef,
    TypedAttributeModifierEntry,
};

/// An attribute paired with the modifier entry a map applies to it.
#[derive(Debug, Clone, Copy)]
pub struct EnvironmentAttributeMapEntry {
    attribute: ErasedEnvironmentAttributeRef,
    modifier: &'static dyn ErasedAttributeModifierEntry,
}

impl EnvironmentAttributeMapEntry {
    /// Pairs an attribute with a modifier entry of the same value type.
    #[must_use]
    pub const fn new<V: AttributeValue, M: AttributeModifier<V>>(
        attribute: &'static EnvironmentAttribute<V>,
        modifier: &'static AttributeModifierEntry<V, M>,
    ) -> Self {
        Self {
            attribute,
            modifier,
        }
    }

    #[must_use]
    pub const fn attribute(&self) -> ErasedEnvironmentAttributeRef {
        self.attribute
    }

    #[must_use]
    pub const fn modifier(&self) -> &'static dyn ErasedAttributeModifierEntry {
        self.modifier
    }
}

/// Attribute modifiers attached to a dimension type, biome, or weather state,
/// as vanilla's `EnvironmentAttributeMap`.
///
/// Holds at most one entry per attribute.
#[derive(Debug, Clone, Copy)]
pub struct EnvironmentAttributeMap {
    entries: &'static [EnvironmentAttributeMapEntry],
}

impl EnvironmentAttributeMap {
    pub const EMPTY: Self = Self { entries: &[] };

    #[must_use]
    pub const fn new(entries: &'static [EnvironmentAttributeMapEntry]) -> Self {
        Self { entries }
    }

    #[must_use]
    pub const fn entries(&self) -> &'static [EnvironmentAttributeMapEntry] {
        self.entries
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns the modifier entry for `attribute`, if this map has one.
    #[must_use]
    pub fn get(
        &self,
        attribute: &(impl ErasedEnvironmentAttribute + ?Sized),
    ) -> Option<&'static dyn ErasedAttributeModifierEntry> {
        self.entries
            .iter()
            .find(|entry| entry.attribute.key() == attribute.key())
            .map(|entry| entry.modifier)
    }

    /// Returns the typed modifier entry for `attribute`, if this map has one.
    ///
    /// Entries are paired with their attribute by
    /// [`EnvironmentAttributeMapEntry::new`], so the entry always has the
    /// attribute's value type.
    #[must_use]
    pub fn get_typed<V: AttributeValue>(
        &self,
        attribute: &EnvironmentAttribute<V>,
    ) -> Option<&'static dyn TypedAttributeModifierEntry<V>> {
        self.get(attribute)?.downcast::<V>()
    }

    #[must_use]
    pub fn contains(&self, attribute: &(impl ErasedEnvironmentAttribute + ?Sized)) -> bool {
        self.get(attribute).is_some()
    }

    /// Applies this map's modifier for `attribute` to `base`, or returns `base`
    /// unchanged when the map has no entry for it.
    #[must_use]
    pub fn apply_modifier<V: AttributeValue>(
        &self,
        attribute: &EnvironmentAttribute<V>,
        base: V,
    ) -> V {
        match self.get_typed(attribute) {
            Some(modifier) => modifier.apply(base),
            None => base,
        }
    }

    /// Iterates over the attributes this map modifies.
    pub fn attributes(&self) -> impl Iterator<Item = ErasedEnvironmentAttributeRef> {
        self.entries.iter().map(|entry| entry.attribute)
    }
}
