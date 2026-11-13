use steel_math::keyframe_track::{LerpFunction, lerp_int};

use super::{AttributeModifier, OperationId};
use crate::environment_attribute::EnvironmentAttribute;

/// Integer arithmetic modifiers: vanilla's `IntegerModifier.ADD`, `SUBTRACT`,
/// `MULTIPLY`, `MINIMUM`, and `MAXIMUM`, with Java's wrapping overflow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegerModifier {
    Add,
    Subtract,
    Multiply,
    Minimum,
    Maximum,
}

impl AttributeModifier<i32> for IntegerModifier {
    type Argument = i32;

    fn operation_id(&self) -> OperationId {
        match self {
            Self::Add => OperationId::Add,
            Self::Subtract => OperationId::Subtract,
            Self::Multiply => OperationId::Multiply,
            Self::Minimum => OperationId::Minimum,
            Self::Maximum => OperationId::Maximum,
        }
    }

    fn apply(&self, subject: i32, argument: &i32) -> i32 {
        let argument = *argument;
        match self {
            Self::Add => subject.wrapping_add(argument),
            Self::Subtract => subject.wrapping_sub(argument),
            Self::Multiply => subject.wrapping_mul(argument),
            Self::Minimum => subject.min(argument),
            Self::Maximum => subject.max(argument),
        }
    }

    fn argument_keyframe_lerp(&self, _attribute: &EnvironmentAttribute<i32>) -> LerpFunction<i32> {
        lerp_int
    }
}
