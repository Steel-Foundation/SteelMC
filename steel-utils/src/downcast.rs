//! Deterministic concrete-type downcasting for erased Steel objects.

use std::fmt::{self, Display, Formatter};
use std::ptr::{from_mut, from_ref};

/// A process-wide key identifying one concrete Rust type for downcasting.
///
/// Keys conventionally use `<owner>:<kind>/<name>`, such as
/// `steel:entity/item`. They identify Rust implementations, not Minecraft
/// registry entries or translation keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DowncastTypeKey(&'static str);

impl DowncastTypeKey {
    /// Creates a non-empty downcast type key.
    ///
    /// # Panics
    ///
    /// Panics if `key` is empty.
    #[must_use]
    pub const fn new(key: &'static str) -> Self {
        assert!(!key.is_empty(), "downcast type keys cannot be empty");
        Self(key)
    }

    /// Returns the key as a string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

impl Display for DowncastTypeKey {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
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
pub unsafe trait DowncastType: 'static {
    /// The process-wide key for this concrete type.
    const TYPE_KEY: DowncastTypeKey;
}

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
