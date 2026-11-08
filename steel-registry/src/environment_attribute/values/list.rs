use std::borrow::Cow;
use std::ops::Deref;

use steel_utils::{DowncastType, DowncastTypeKey};

use super::AmbientParticle;

/// A list-valued attribute value, as vanilla's `List<Element>` attribute types.
///
/// Rust's orphan rules prevent keying `Vec<T>` for downcasting, so list values
/// use this wrapper; each element type gets its own downcast key.
#[derive(Debug, Clone)]
pub struct AttributeList<T: Clone + 'static>(pub Cow<'static, [T]>);

impl<T: Clone + 'static> AttributeList<T> {
    pub const EMPTY: Self = Self(Cow::Borrowed(&[]));

    #[must_use]
    pub const fn new(elements: &'static [T]) -> Self {
        Self(Cow::Borrowed(elements))
    }

    #[must_use]
    pub const fn owned(elements: Vec<T>) -> Self {
        Self(Cow::Owned(elements))
    }
}

impl<T: Clone + 'static> Deref for AttributeList<T> {
    type Target = [T];

    fn deref(&self) -> &[T] {
        &self.0
    }
}

/// Vanilla `LerpFunction.ofListCrossFade`: fades `from` out and `to` in by
/// scaling each element with `scale`.
#[must_use]
#[expect(
    clippy::float_cmp,
    reason = "vanilla returns an endpoint for an exact alpha of 0 or 1"
)]
pub fn lerp_list_cross_fade<T: Clone + 'static>(
    alpha: f32,
    from: &AttributeList<T>,
    to: &AttributeList<T>,
    scale: fn(&T, f32) -> T,
) -> AttributeList<T> {
    if alpha == 0.0 {
        return from.clone();
    }
    if alpha == 1.0 {
        return to.clone();
    }
    let elements = from
        .iter()
        .map(|element| scale(element, 1.0 - alpha))
        .chain(to.iter().map(|element| scale(element, alpha)))
        .collect();
    AttributeList::owned(elements)
}

// SAFETY: This Steel-owned key uniquely identifies `AttributeList<AmbientParticle>`
// within the process.
unsafe impl DowncastType for AttributeList<AmbientParticle> {
    const TYPE_KEY: DowncastTypeKey =
        DowncastTypeKey::new("steel:environment_attribute/list/ambient_particle");
}
