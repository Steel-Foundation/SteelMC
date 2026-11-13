//! Deterministic concrete-type downcasting for erased Steel objects.

use std::fmt::{self, Display, Formatter};
use std::ptr::{from_mut, from_ref};

/// A process-wide key identifying one concrete Rust type for downcasting.
///
/// Keys conventionally use `<owner>:<kind>/<name>`, such as
/// `steel:entity/item`. They identify Rust implementations, not Minecraft
/// registry entries or translation keys.
///
/// A generic type keys each instantiation with [`DowncastTypeKey::generic`],
/// combining its own name with the keys of its type parameters, so
/// `Wrapper<A>` and `Wrapper<B>` never share a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DowncastTypeKey {
    name: &'static str,
    parameters: &'static [Self],
}

impl DowncastTypeKey {
    /// Creates a non-empty downcast type key.
    ///
    /// # Panics
    ///
    /// Panics if `name` is empty.
    #[must_use]
    pub const fn new(name: &'static str) -> Self {
        Self::generic(name, &[])
    }

    /// Creates the key of one instantiation of a generic type from the type's
    /// own name and the keys of its type parameters.
    ///
    /// # Panics
    ///
    /// Panics if `name` is empty.
    #[must_use]
    pub const fn generic(name: &'static str, parameters: &'static [Self]) -> Self {
        assert!(!name.is_empty(), "downcast type keys cannot be empty");
        Self { name, parameters }
    }

    /// Returns the type's own name, without its type parameters.
    #[must_use]
    pub const fn name(self) -> &'static str {
        self.name
    }

    /// Returns the keys of the type parameters of a generic instantiation.
    #[must_use]
    pub const fn parameters(self) -> &'static [Self] {
        self.parameters
    }
}

impl Display for DowncastTypeKey {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name)?;
        let Some((first, rest)) = self.parameters.split_first() else {
            return Ok(());
        };
        write!(formatter, "<{first}")?;
        for parameter in rest {
            write!(formatter, ", {parameter}")?;
        }
        formatter.write_str(">")
    }
}

/// Assigns a deterministic downcast key to a concrete Rust type.
///
/// This trait does not assert that the type has a stable ABI. A plugin may use
/// it to recover types that plugin owns, but a key alone never permits one
/// compilation unit to reinterpret another compilation unit's private type.
///
/// # Safety
///
/// Implementors must guarantee that:
///
/// - `TYPE_KEY` uniquely identifies this exact concrete Rust type among all
///   [`DowncastType`] implementations that may coexist in the process.
/// - No separately defined type uses the same key, even if it currently has an
///   identical layout.
/// - Versions of a type that may coexist use different keys unless they are the
///   same concrete Rust type to every caller that can perform the downcast.
/// - A generic type includes the key of every type parameter that changes the
///   concrete type, through [`DowncastTypeKey::generic`].
pub unsafe trait DowncastType: 'static {
    /// The process-wide key for this concrete type.
    const TYPE_KEY: DowncastTypeKey;
}

macro_rules! impl_steel_downcast_type {
    ($type:ty, $key:literal) => {
        // SAFETY: This Steel-owned key uniquely identifies the concrete Rust
        // type within the process.
        unsafe impl DowncastType for $type {
            const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new($key);
        }
    };
}

impl_steel_downcast_type!((), "steel:rust_primitive/unit");
impl_steel_downcast_type!(bool, "steel:rust_primitive/bool");
impl_steel_downcast_type!(i32, "steel:rust_primitive/i32");
impl_steel_downcast_type!(f32, "steel:rust_primitive/f32");
impl_steel_downcast_type!(glam::Vec3, "steel:glam/vec3");
impl_steel_downcast_type!(glam::Vec4, "steel:glam/vec4");
impl_steel_downcast_type!(crate::Identifier, "steel:value/identifier");
impl_steel_downcast_type!(text_components::TextComponent, "steel:value/text_component");

mod private {
    pub trait Sealed {}

    impl<T: super::DowncastType> Sealed for T {}
}

/// Object-safe type erasure implemented for every [`DowncastType`].
///
/// This trait is sealed so erased pointers can only be produced by the blanket
/// implementation below.
pub trait ErasedType: private::Sealed {
    /// Returns the concrete type's deterministic key.
    #[doc(hidden)]
    fn downcast_type_key(&self) -> DowncastTypeKey;

    /// Returns a pointer to the concrete value.
    #[doc(hidden)]
    fn downcast_data(&self) -> *const ();

    /// Returns a mutable pointer to the concrete value.
    #[doc(hidden)]
    fn downcast_data_mut(&mut self) -> *mut ();
}

impl<T: DowncastType> ErasedType for T {
    fn downcast_type_key(&self) -> DowncastTypeKey {
        T::TYPE_KEY
    }

    fn downcast_data(&self) -> *const () {
        from_ref(self).cast()
    }

    fn downcast_data_mut(&mut self) -> *mut () {
        from_mut(self).cast()
    }
}

/// Extension methods for deterministic concrete-type downcasting.
pub trait Downcast: ErasedType {
    /// Returns whether the erased value has concrete type `T`.
    #[must_use]
    fn is<T: DowncastType>(&self) -> bool {
        self.downcast_type_key() == T::TYPE_KEY
    }

    /// Returns the erased value as `T` when its key matches.
    #[must_use]
    fn downcast_ref<T: DowncastType>(&self) -> Option<&T> {
        if !self.is::<T>() {
            return None;
        }

        // SAFETY: `DowncastType` requires equal keys to identify the exact same
        // concrete Rust type, and `ErasedType` is sealed so its pointer always
        // points to the concrete implementor.
        Some(unsafe { &*self.downcast_data().cast::<T>() })
    }

    /// Returns the erased value as mutable `T` when its key matches.
    #[must_use]
    fn downcast_mut<T: DowncastType>(&mut self) -> Option<&mut T> {
        if !self.is::<T>() {
            return None;
        }

        // SAFETY: `DowncastType` requires equal keys to identify the exact same
        // concrete Rust type, and the exclusive borrow guarantees that the
        // erased pointer can be reborrowed mutably for the returned lifetime.
        Some(unsafe { &mut *self.downcast_data_mut().cast::<T>() })
    }
}

impl<T: ErasedType + ?Sized> Downcast for T {}

#[cfg(test)]
mod tests {
    use super::{Downcast as _, DowncastType, DowncastTypeKey, ErasedType};

    struct First(u32);
    struct Second;

    // SAFETY: These test-only keys are distinct and identify their respective
    // concrete types within the test process.
    unsafe impl DowncastType for First {
        const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:test/downcast/first");
    }

    // SAFETY: These test-only keys are distinct and identify their respective
    // concrete types within the test process.
    unsafe impl DowncastType for Second {
        const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:test/downcast/second");
    }

    struct Wrapper<T>(T);

    // SAFETY: The key includes `T`'s key, so every instantiation of this
    // test-only type has its own key.
    unsafe impl<T: DowncastType> DowncastType for Wrapper<T> {
        const TYPE_KEY: DowncastTypeKey =
            DowncastTypeKey::generic("steel:test/downcast/wrapper", &[T::TYPE_KEY]);
    }

    #[test]
    fn generic_keys_distinguish_instantiations() {
        let value = Wrapper(First(7));
        let erased: &dyn ErasedType = &value;

        assert!(erased.is::<Wrapper<First>>());
        assert!(!erased.is::<Wrapper<Second>>());
        assert!(!erased.is::<First>());
        assert_eq!(
            Wrapper::<First>::TYPE_KEY.to_string(),
            "steel:test/downcast/wrapper<steel:test/downcast/first>"
        );
    }

    #[test]
    fn downcasts_matching_shared_reference() {
        let value = First(7);
        let erased: &dyn ErasedType = &value;

        assert!(erased.is::<First>());
        assert!(!erased.is::<Second>());
        assert_eq!(erased.downcast_ref::<First>().map(|first| first.0), Some(7));
        assert!(erased.downcast_ref::<Second>().is_none());
    }

    #[test]
    fn downcasts_matching_mutable_reference() {
        let mut value = First(7);
        let erased: &mut dyn ErasedType = &mut value;
        let Some(first) = erased.downcast_mut::<First>() else {
            panic!("matching type key should downcast");
        };
        first.0 = 11;

        assert_eq!(value.0, 11);
    }
}
