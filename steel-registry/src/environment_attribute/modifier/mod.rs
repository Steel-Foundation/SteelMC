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
use steel_utils::{Downcast as _, DowncastType, DowncastTypeKey, ErasedType};

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

/// A modifier entry of value type `V`, with its modifier type erased.
pub trait TypedAttributeModifierEntry<V: AttributeValue>: Debug + Send + Sync {
    fn operation_id(&self) -> OperationId;

    fn apply(&self, subject: V) -> V;
}

impl<V: AttributeValue, M: AttributeModifier<V>> TypedAttributeModifierEntry<V>
    for AttributeModifierEntry<V, M>
{
    fn operation_id(&self) -> OperationId {
        self.modifier.operation_id()
    }

    fn apply(&self, subject: V) -> V {
        Self::apply(self, subject)
    }
}

/// A modifier entry with both its value and modifier types erased, as stored
/// in attribute maps of mixed value types.
///
/// Recover the typed entry with [`downcast`](#method.downcast).
pub trait ErasedAttributeModifierEntry: Debug + Send + Sync {
    fn operation_id(&self) -> OperationId;

    /// Downcast key of the entry's value type.
    fn value_type_key(&self) -> DowncastTypeKey;

    /// Stores this entry in `slot` when `slot` is the
    /// `TypedModifierEntrySlot` of the entry's value type.
    #[doc(hidden)]
    fn fill_typed_slot(&'static self, slot: &mut dyn ErasedType);
}

impl<V: AttributeValue, M: AttributeModifier<V>> ErasedAttributeModifierEntry
    for AttributeModifierEntry<V, M>
{
    fn operation_id(&self) -> OperationId {
        self.modifier.operation_id()
    }

    fn value_type_key(&self) -> DowncastTypeKey {
        V::TYPE_KEY
    }

    fn fill_typed_slot(&'static self, slot: &mut dyn ErasedType) {
        if let Some(slot) = slot.downcast_mut::<TypedModifierEntrySlot<V>>() {
            slot.0 = Some(self);
        }
    }
}

impl dyn ErasedAttributeModifierEntry {
    /// Returns this entry typed by its value type, or `None` when its value
    /// type is not `V`.
    #[must_use]
    pub fn downcast<V: AttributeValue>(
        &'static self,
    ) -> Option<&'static dyn TypedAttributeModifierEntry<V>> {
        let mut slot = TypedModifierEntrySlot::<V>(None);
        self.fill_typed_slot(&mut slot);
        slot.0
    }
}

/// Carries a typed entry out of [`ErasedAttributeModifierEntry::fill_typed_slot`].
struct TypedModifierEntrySlot<V: AttributeValue>(
    Option<&'static dyn TypedAttributeModifierEntry<V>>,
);

// SAFETY: The key includes `V`'s key, so every instantiation of this
// Steel-owned type has its own key.
unsafe impl<V: AttributeValue> DowncastType for TypedModifierEntrySlot<V> {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::generic(
        "steel:environment_attribute/typed_modifier_entry_slot",
        &[V::TYPE_KEY],
    );
}
