use std::fmt::{self, Debug, Formatter};
use std::sync::LazyLock;

/// A value stored inline, or built on first access when it cannot be
/// constructed in a `const` context (for example particles, which box their
/// options).
pub enum LazyValue<T: 'static> {
    /// Built at compile time.
    Value(T),
    /// Built by the function on first access.
    Lazy(LazyLock<T>),
}

impl<T> LazyValue<T> {
    #[must_use]
    pub const fn new(value: T) -> Self {
        Self::Value(value)
    }

    #[must_use]
    pub const fn lazy(init: fn() -> T) -> Self {
        Self::Lazy(LazyLock::new(init))
    }

    #[must_use]
    pub fn get(&self) -> &T {
        match self {
            Self::Value(value) => value,
            Self::Lazy(value) => value,
        }
    }
}

impl<T: Debug> Debug for LazyValue<T> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        Debug::fmt(self.get(), formatter)
    }
}
