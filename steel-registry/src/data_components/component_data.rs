//! Type-erased data component values.

use std::fmt::{self, Debug, Formatter};

use steel_utils::{
    Downcast as _, DowncastType, DowncastTypeKey, ErasedType,
    hash::{ComponentHasher, HashComponent},
};

use super::components::{
    AttackRange, DamageTypeComponent, Equippable, ItemAttributeModifiers, ItemEnchantments,
    PiercingWeapon, Tool, UseCooldown, Weapon,
};

/// Behavior required from a value stored in a [`ComponentData`].
///
/// Concrete type recovery is provided by Steel's deterministic keyed
/// downcasting foundation. A value is eligible for the blanket implementation
/// when it also supports cloning, comparison, debugging, hashing, and shared
/// server access.
pub trait Component: ErasedType + Debug + Send + Sync + 'static {
    #[doc(hidden)]
    fn clone_component(&self) -> Box<dyn Component>;

    #[doc(hidden)]
    fn component_eq(&self, other: &dyn Component) -> bool;

    #[doc(hidden)]
    fn hash_component_value(&self, hasher: &mut ComponentHasher);
}

impl<T> Component for T
where
    T: DowncastType + Clone + Debug + PartialEq + HashComponent + Send + Sync,
{
    fn clone_component(&self) -> Box<dyn Component> {
        Box::new(self.clone())
    }

    fn component_eq(&self, other: &dyn Component) -> bool {
        other.downcast_ref::<T>() == Some(self)
    }

    fn hash_component_value(&self, hasher: &mut ComponentHasher) {
        self.hash_component(hasher);
    }
}

/// A type-erased component value.
///
/// Implemented component values retain their concrete Rust type and can be
/// recovered with [`Self::downcast_ref`]. The unimplemented state is kept only
/// for the existing vanilla prototype placeholders until their real component
/// types and codecs are ported.
pub struct ComponentData {
    value: Option<Box<dyn Component>>,
}

impl ComponentData {
    /// Erases a typed component value.
    #[must_use]
    pub fn new(value: impl Component) -> Self {
        Self {
            value: Some(Box::new(value)),
        }
    }

    pub(crate) const fn unimplemented() -> Self {
        Self { value: None }
    }

    /// Returns the concrete value when it has type `T`.
    #[must_use]
    pub fn downcast_ref<T: DowncastType>(&self) -> Option<&T> {
        self.value.as_deref()?.downcast_ref::<T>()
    }

    /// Returns the concrete type key, or `None` for an unimplemented value.
    #[must_use]
    pub fn type_key(&self) -> Option<DowncastTypeKey> {
        self.value.as_deref().map(ErasedType::downcast_type_key)
    }

    /// Returns whether this contains a real typed value.
    #[must_use]
    pub const fn is_implemented(&self) -> bool {
        self.value.is_some()
    }

    /// Computes the vanilla validation hash for this component value.
    #[must_use]
    pub fn compute_hash(&self) -> i32 {
        let mut hasher = ComponentHasher::new();

        if let Some(value) = &self.value {
            value.hash_component_value(&mut hasher);
        } else {
            // Existing unimplemented prototype values retain their old hash
            // until their concrete vanilla codecs are ported.
            hasher.start_map();
            hasher.end_map();
        }

        hasher.finish()
    }
}

impl Clone for ComponentData {
    fn clone(&self) -> Self {
        Self {
            value: self.value.as_ref().map(|value| value.clone_component()),
        }
    }
}

impl Debug for ComponentData {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match &self.value {
            Some(value) => formatter.debug_tuple("ComponentData").field(value).finish(),
            None => formatter.write_str("ComponentData(Unimplemented)"),
        }
    }
}

impl PartialEq for ComponentData {
    fn eq(&self, other: &Self) -> bool {
        match (&self.value, &other.value) {
            (Some(value), Some(other)) => value.component_eq(other.as_ref()),
            (None, None) => true,
            _ => false,
        }
    }
}

macro_rules! impl_component_downcast_type {
    ($type:ty, $key:literal) => {
        // SAFETY: This Steel-owned key uniquely identifies the concrete
        // component implementation within the process.
        unsafe impl DowncastType for $type {
            const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new($key);
        }
    };
}

impl_component_downcast_type!(DamageTypeComponent, "steel:item_component/damage_type");
impl_component_downcast_type!(Tool, "steel:item_component/tool");
impl_component_downcast_type!(Weapon, "steel:item_component/weapon");
impl_component_downcast_type!(AttackRange, "steel:item_component/attack_range");
impl_component_downcast_type!(UseCooldown, "steel:item_component/use_cooldown");
impl_component_downcast_type!(PiercingWeapon, "steel:item_component/piercing_weapon");
impl_component_downcast_type!(Equippable, "steel:item_component/equippable");
impl_component_downcast_type!(
    ItemAttributeModifiers,
    "steel:item_component/attribute_modifiers"
);
impl_component_downcast_type!(ItemEnchantments, "steel:item_component/enchantments");

#[cfg(test)]
mod tests {
    use super::ComponentData;

    #[test]
    fn typed_values_downcast_by_deterministic_key() {
        let value = ComponentData::new(17_i32);

        assert_eq!(value.downcast_ref::<i32>(), Some(&17));
        assert_eq!(value.downcast_ref::<bool>(), None);
    }

    #[test]
    fn equality_requires_the_same_concrete_type() {
        assert_eq!(ComponentData::new(17_i32), ComponentData::new(17_i32));
        assert_ne!(ComponentData::new(17_i32), ComponentData::new(17.0_f32));
    }
}
