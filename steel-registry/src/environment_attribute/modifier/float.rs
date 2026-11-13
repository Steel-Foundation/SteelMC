use steel_math::keyframe_track::{LerpFunction, lerp_float};

use super::{AttributeModifier, OperationId};
use crate::environment_attribute::EnvironmentAttribute;

/// Float arithmetic modifiers taking a plain float argument: vanilla's
/// `FloatModifier.ADD`, `SUBTRACT`, `MULTIPLY`, `MINIMUM`, and `MAXIMUM`.
///
/// Vanilla's `FloatModifier.ALPHA_BLEND` takes a different argument type and is
/// [`FloatAlphaBlendModifier`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloatModifier {
    Add,
    Subtract,
    Multiply,
    Minimum,
    Maximum,
}

impl AttributeModifier<f32> for FloatModifier {
    type Argument = f32;

    fn operation_id(&self) -> OperationId {
        match self {
            Self::Add => OperationId::Add,
            Self::Subtract => OperationId::Subtract,
            Self::Multiply => OperationId::Multiply,
            Self::Minimum => OperationId::Minimum,
            Self::Maximum => OperationId::Maximum,
        }
    }

    fn apply(&self, subject: f32, argument: &f32) -> f32 {
        let argument = *argument;
        match self {
            Self::Add => subject + argument,
            Self::Subtract => subject - argument,
            Self::Multiply => subject * argument,
            Self::Minimum => subject.min(argument),
            Self::Maximum => subject.max(argument),
        }
    }

    fn argument_keyframe_lerp(&self, _attribute: &EnvironmentAttribute<f32>) -> LerpFunction<f32> {
        lerp_float
    }
}

/// Argument of [`FloatAlphaBlendModifier`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FloatWithAlpha {
    pub value: f32,
    pub alpha: f32,
}

/// Blends the value toward the argument's value by the argument's alpha.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FloatAlphaBlendModifier;

impl AttributeModifier<f32> for FloatAlphaBlendModifier {
    type Argument = FloatWithAlpha;

    fn operation_id(&self) -> OperationId {
        OperationId::AlphaBlend
    }

    fn apply(&self, subject: f32, argument: &FloatWithAlpha) -> f32 {
        lerp_float(argument.alpha, &subject, &argument.value)
    }

    fn argument_keyframe_lerp(
        &self,
        _attribute: &EnvironmentAttribute<f32>,
    ) -> LerpFunction<FloatWithAlpha> {
        |alpha, from, to| FloatWithAlpha {
            value: lerp_float(alpha, &from.value, &to.value),
            alpha: lerp_float(alpha, &from.alpha, &to.alpha),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AttributeModifier as _, FloatAlphaBlendModifier, FloatWithAlpha};

    #[test]
    fn alpha_blend_moves_toward_argument_value() {
        let rain = FloatWithAlpha {
            value: 4.0,
            alpha: 0.3125,
        };
        assert_eq!(FloatAlphaBlendModifier.apply(15.0, &rain), 11.5625);
    }
}
