use std::fmt::Debug;

use steel_math::keyframe_track::KeyframeTrack;
use steel_utils::{Downcast as _, DowncastType, DowncastTypeKey, ErasedType};

use crate::environment_attribute::{
    AttributeModifier, AttributeValue, EnvironmentAttribute, ErasedEnvironmentAttributeRef,
};

/// A timeline track driving one attribute's modifier argument over time, as
/// vanilla's `AttributeTrack`.
///
/// Vanilla keys tracks by attribute in the timeline's map; Steel stores the
/// attribute on the track so generated data stays typed.
#[derive(Debug)]
pub struct AttributeTrack<V: AttributeValue, M: AttributeModifier<V>> {
    attribute: &'static EnvironmentAttribute<V>,
    modifier: M,
    argument_track: KeyframeTrack<M::Argument>,
}

impl<V: AttributeValue, M: AttributeModifier<V>> AttributeTrack<V, M> {
    #[must_use]
    pub const fn new(
        attribute: &'static EnvironmentAttribute<V>,
        modifier: M,
        argument_track: KeyframeTrack<M::Argument>,
    ) -> Self {
        Self {
            attribute,
            modifier,
            argument_track,
        }
    }

    /// Samples the argument at `total_ticks` of the timeline's clock and
    /// applies the modifier to `subject`.
    #[must_use]
    pub fn apply(&self, subject: V, period_ticks: Option<i32>, total_ticks: i64) -> V {
        let lerp = self.modifier.argument_keyframe_lerp(self.attribute);
        let argument = self.argument_track.sample(period_ticks, total_ticks, lerp);
        self.modifier.apply(subject, &argument)
    }
}

/// An attribute track of value type `V`, with its modifier type erased.
pub trait TypedAttributeTrack<V: AttributeValue>: Debug + Send + Sync {
    /// Samples the argument at `total_ticks` of the timeline's clock and
    /// applies the modifier to `subject`.
    fn apply(&self, subject: V, period_ticks: Option<i32>, total_ticks: i64) -> V;
}

impl<V: AttributeValue, M: AttributeModifier<V>> TypedAttributeTrack<V> for AttributeTrack<V, M> {
    fn apply(&self, subject: V, period_ticks: Option<i32>, total_ticks: i64) -> V {
        Self::apply(self, subject, period_ticks, total_ticks)
    }
}

/// An attribute track with both its value and modifier types erased, as
/// stored in a timeline's track list.
///
/// Recover the typed track with [`downcast`](#method.downcast).
pub trait ErasedAttributeTrack: Debug + Send + Sync {
    fn attribute(&self) -> ErasedEnvironmentAttributeRef;

    /// Stores this track in `slot` when `slot` is the `TypedTrackSlot` of the
    /// track's value type.
    #[doc(hidden)]
    fn fill_typed_slot(&'static self, slot: &mut dyn ErasedType);
}

impl<V: AttributeValue, M: AttributeModifier<V>> ErasedAttributeTrack for AttributeTrack<V, M> {
    fn attribute(&self) -> ErasedEnvironmentAttributeRef {
        self.attribute
    }

    fn fill_typed_slot(&'static self, slot: &mut dyn ErasedType) {
        if let Some(slot) = slot.downcast_mut::<TypedTrackSlot<V>>() {
            slot.0 = Some(self);
        }
    }
}

impl dyn ErasedAttributeTrack {
    /// Returns this track typed by its value type, or `None` when its value
    /// type is not `V`.
    #[must_use]
    pub fn downcast<V: AttributeValue>(
        &'static self,
    ) -> Option<&'static dyn TypedAttributeTrack<V>> {
        let mut slot = TypedTrackSlot::<V>(None);
        self.fill_typed_slot(&mut slot);
        slot.0
    }
}

/// Carries a typed track out of [`ErasedAttributeTrack::fill_typed_slot`].
struct TypedTrackSlot<V: AttributeValue>(Option<&'static dyn TypedAttributeTrack<V>>);

// SAFETY: The key includes `V`'s key, so every instantiation of this
// Steel-owned type has its own key.
unsafe impl<V: AttributeValue> DowncastType for TypedTrackSlot<V> {
    const TYPE_KEY: DowncastTypeKey =
        DowncastTypeKey::generic("steel:timeline/typed_track_slot", &[V::TYPE_KEY]);
}
