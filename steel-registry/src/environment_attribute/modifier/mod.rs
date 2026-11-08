//! Vanilla attribute modifiers (`world.attribute.modifier`).
//!
//! Vanilla exposes one modifier instance per operation and value type, typed
//! by its argument. Rust groups operations sharing an argument type into one
//! modifier type; each type's docs name the vanilla constants it covers.

mod boolean;
mod color;
mod float;
mod integer;
mod list;
mod mob_spawn_settings;

use std::fmt::Debug;
use std::marker::PhantomData;

use steel_math::keyframe_track::LerpFunction;
use steel_utils::{Downcast as _, ErasedType};

use super::{AttributeValue, EnvironmentAttribute, LazyValue};

pub use boolean::BooleanModifier;
pub use color::{
    BlendToGray, BlendToGrayModifier, ColorAlphaBlendModifier, ColorMultiplyModifier, RgbModifier,
};
pub use float::{FloatAlphaBlendModifier, FloatModifier, FloatWithAlpha};
pub use integer::IntegerModifier;
pub use list::ListAppendModifier;
pub use mob_spawn_settings::MobSpawnSettingsOverlayModifier;

/// Serialized operation names of vanilla's attribute modifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperationId {
    Override,
    AlphaBlend,
    Add,
    Subtract,
    Multiply,
    BlendToGray,
    Minimum,
    Maximum,
    And,
    Nand,
    Or,
    Nor,
    Xor,
    Xnor,
    Append,
    Overlay,
}

impl OperationId {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Override => "override",
            Self::AlphaBlend => "alpha_blend",
            Self::Add => "add",
            Self::Subtract => "subtract",
            Self::Multiply => "multiply",
            Self::BlendToGray => "blend_to_gray",
            Self::Minimum => "minimum",
            Self::Maximum => "maximum",
            Self::And => "and",
            Self::Nand => "nand",
            Self::Or => "or",
            Self::Nor => "nor",
            Self::Xor => "xor",
            Self::Xnor => "xnor",
            Self::Append => "append",
            Self::Overlay => "overlay",
        }
    }
}

/// Transforms an attribute value with an argument, as vanilla's
/// `AttributeModifier<Subject, Argument>`.
pub trait AttributeModifier<V: AttributeValue>: Debug + Send + Sync + 'static {
    /// Argument type stored in attribute maps and timeline keyframes.
    type Argument: Clone + Debug + Send + Sync + 'static;

    fn operation_id(&self) -> OperationId;

    fn apply(&self, subject: V, argument: &Self::Argument) -> V;

    /// Lerp used between timeline keyframes of this modifier's argument.
    fn argument_keyframe_lerp(
        &self,
        attribute: &EnvironmentAttribute<V>,
    ) -> LerpFunction<Self::Argument>;
}

/// Replaces the value with the argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverrideModifier;

impl<V: AttributeValue> AttributeModifier<V> for OverrideModifier {
    type Argument = V;

    fn operation_id(&self) -> OperationId {
        OperationId::Override
    }

    fn apply(&self, _subject: V, argument: &V) -> V {
        argument.clone()
    }

    fn argument_keyframe_lerp(&self, attribute: &EnvironmentAttribute<V>) -> LerpFunction<V> {
        attribute.attribute_type().keyframe_lerp()
    }
}

/// A modifier paired with its argument, as vanilla's `EnvironmentAttributeMap.Entry`.
#[derive(Debug)]
pub struct AttributeModifierEntry<V: AttributeValue, M: AttributeModifier<V>> {
    modifier: M,
    argument: LazyValue<M::Argument>,
    value_type: PhantomData<fn() -> V>,
}

impl<V: AttributeValue, M: AttributeModifier<V>> AttributeModifierEntry<V, M> {
    #[must_use]
    pub const fn new(modifier: M, argument: M::Argument) -> Self {
        Self {
            modifier,
            argument: LazyValue::new(argument),
            value_type: PhantomData,
        }
    }

    /// Creates an entry whose argument is built on first use, for arguments
    /// that cannot be constructed in a `const` context.
    #[must_use]
    pub const fn new_lazy(modifier: M, argument: fn() -> M::Argument) -> Self {
        Self {
            modifier,
            argument: LazyValue::lazy(argument),
            value_type: PhantomData,
        }
    }

    #[must_use]
    pub fn apply(&self, subject: V) -> V {
        self.modifier.apply(subject, self.argument.get())
    }
}

/// Object-safe access to a modifier entry of any value type.
pub trait ErasedAttributeModifierEntry: Debug + Send + Sync {
    fn operation_id(&self) -> OperationId;

    /// Applies the modifier to `subject` in place. Returns `false`, leaving
    /// `subject` untouched, when its type does not match the entry.
    fn apply_erased(&self, subject: &mut dyn ErasedType) -> bool;
}

impl<V: AttributeValue, M: AttributeModifier<V>> ErasedAttributeModifierEntry
    for AttributeModifierEntry<V, M>
{
    fn operation_id(&self) -> OperationId {
        self.modifier.operation_id()
    }

    fn apply_erased(&self, subject: &mut dyn ErasedType) -> bool {
        let Some(subject) = subject.downcast_mut::<V>() else {
            return false;
        };
        *subject = self.apply(subject.clone());
        true
    }
}

impl dyn ErasedAttributeModifierEntry {
    /// Applies the modifier to a typed value.
    ///
    /// Entries are paired with their attribute by typed constructors, so a
    /// mismatch is a construction bug; the value is returned unchanged.
    #[must_use]
    pub fn apply_typed<V: AttributeValue>(&self, mut subject: V) -> V {
        let applied = self.apply_erased(&mut subject);
        debug_assert!(
            applied,
            "modifier entry {self:?} does not apply to this value type"
        );
        subject
    }
}
