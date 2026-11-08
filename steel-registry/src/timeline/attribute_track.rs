use std::fmt::Debug;

use steel_math::keyframe_track::KeyframeTrack;
use steel_utils::{Downcast as _, ErasedType};

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

/// Object-safe access to an attribute track of any value type.
pub trait ErasedAttributeTrack: Debug + Send + Sync {
    fn attribute(&self) -> ErasedEnvironmentAttributeRef;

    /// Applies the track to `subject` in place. Returns `false`, leaving
    /// `subject` untouched, when its type does not match the track.
    fn apply_erased(
        &self,
        subject: &mut dyn ErasedType,
        period_ticks: Option<i32>,
        total_ticks: i64,
    ) -> bool;
}

impl<V: AttributeValue, M: AttributeModifier<V>> ErasedAttributeTrack for AttributeTrack<V, M> {
    fn attribute(&self) -> ErasedEnvironmentAttributeRef {
        self.attribute
    }

    fn apply_erased(
        &self,
        subject: &mut dyn ErasedType,
        period_ticks: Option<i32>,
        total_ticks: i64,
    ) -> bool {
        let Some(subject) = subject.downcast_mut::<V>() else {
            return false;
        };
        *subject = self.apply(subject.clone(), period_ticks, total_ticks);
        true
    }
}

impl dyn ErasedAttributeTrack {
    /// Applies the track to a typed value.
    ///
    /// Tracks are typed by their attribute at construction, so a mismatch is a
    /// lookup bug; the value is returned unchanged.
    #[must_use]
    pub fn apply_typed<V: AttributeValue>(
        &self,
        mut subject: V,
        period_ticks: Option<i32>,
        total_ticks: i64,
    ) -> V {
        let applied = self.apply_erased(&mut subject, period_ticks, total_ticks);
        debug_assert!(
            applied,
            "attribute track {self:?} does not apply to this value type"
        );
        subject
    }
}
