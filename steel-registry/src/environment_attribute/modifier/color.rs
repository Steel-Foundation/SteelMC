use glam::{Vec3, Vec4};
use steel_math::keyframe_track::{LerpFunction, lerp_float};

use super::{AttributeModifier, OperationId};
use crate::environment_attribute::EnvironmentAttribute;
use crate::environment_attribute::values::argb;

/// Adds or subtracts an RGB argument, keeping any alpha: vanilla's
/// `ColorModifier.ADD_RGB`, `ADD_ARGB`, `SUBTRACT_RGB`, and `SUBTRACT_ARGB`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RgbModifier {
    Add,
    Subtract,
}

impl RgbModifier {
    const fn operation(self) -> OperationId {
        match self {
            Self::Add => OperationId::Add,
            Self::Subtract => OperationId::Subtract,
        }
    }
}

impl AttributeModifier<Vec3> for RgbModifier {
    type Argument = Vec3;

    fn operation_id(&self) -> OperationId {
        self.operation()
    }

    fn apply(&self, subject: Vec3, argument: &Vec3) -> Vec3 {
        match self {
            Self::Add => argb::add_rgb(subject, *argument),
            Self::Subtract => argb::subtract_rgb(subject, *argument),
        }
    }

    fn argument_keyframe_lerp(
        &self,
        _attribute: &EnvironmentAttribute<Vec3>,
    ) -> LerpFunction<Vec3> {
        argb::lerp_rgb
    }
}

impl AttributeModifier<Vec4> for RgbModifier {
    type Argument = Vec3;

    fn operation_id(&self) -> OperationId {
        self.operation()
    }

    fn apply(&self, subject: Vec4, argument: &Vec3) -> Vec4 {
        match self {
            Self::Add => argb::add_rgb_keep_alpha(subject, *argument),
            Self::Subtract => argb::subtract_rgb_keep_alpha(subject, *argument),
        }
    }

    fn argument_keyframe_lerp(
        &self,
        _attribute: &EnvironmentAttribute<Vec4>,
    ) -> LerpFunction<Vec3> {
        argb::lerp_rgb
    }
}

/// Multiplies by a color of the same type: vanilla's `ColorModifier.MULTIPLY_RGB`
/// and `MULTIPLY_ARGB`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorMultiplyModifier;

impl AttributeModifier<Vec3> for ColorMultiplyModifier {
    type Argument = Vec3;

    fn operation_id(&self) -> OperationId {
        OperationId::Multiply
    }

    fn apply(&self, subject: Vec3, argument: &Vec3) -> Vec3 {
        argb::multiply_rgb(subject, *argument)
    }

    fn argument_keyframe_lerp(
        &self,
        _attribute: &EnvironmentAttribute<Vec3>,
    ) -> LerpFunction<Vec3> {
        argb::lerp_rgb
    }
}

impl AttributeModifier<Vec4> for ColorMultiplyModifier {
    type Argument = Vec4;

    fn operation_id(&self) -> OperationId {
        OperationId::Multiply
    }

    fn apply(&self, subject: Vec4, argument: &Vec4) -> Vec4 {
        argb::multiply_argb(subject, *argument)
    }

    fn argument_keyframe_lerp(
        &self,
        _attribute: &EnvironmentAttribute<Vec4>,
    ) -> LerpFunction<Vec4> {
        argb::lerp_argb
    }
}

/// Blends an ARGB color over the value by its alpha: vanilla's
/// `ColorModifier.ALPHA_BLEND_RGB` and `ALPHA_BLEND_ARGB`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorAlphaBlendModifier;

impl AttributeModifier<Vec3> for ColorAlphaBlendModifier {
    type Argument = Vec4;

    fn operation_id(&self) -> OperationId {
        OperationId::AlphaBlend
    }

    fn apply(&self, subject: Vec3, argument: &Vec4) -> Vec3 {
        argb::alpha_blend_rgb(subject, *argument)
    }

    fn argument_keyframe_lerp(
        &self,
        _attribute: &EnvironmentAttribute<Vec3>,
    ) -> LerpFunction<Vec4> {
        argb::lerp_argb
    }
}

impl AttributeModifier<Vec4> for ColorAlphaBlendModifier {
    type Argument = Vec4;

    fn operation_id(&self) -> OperationId {
        OperationId::AlphaBlend
    }

    fn apply(&self, subject: Vec4, argument: &Vec4) -> Vec4 {
        argb::alpha_blend_argb(subject, *argument)
    }

    fn argument_keyframe_lerp(
        &self,
        _attribute: &EnvironmentAttribute<Vec4>,
    ) -> LerpFunction<Vec4> {
        argb::lerp_argb
    }
}

/// Argument of [`BlendToGrayModifier`], as vanilla's `ColorModifier.BlendToGray`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlendToGray {
    pub brightness: f32,
    pub factor: f32,
}

impl BlendToGray {
    fn lerp(alpha: f32, from: &Self, to: &Self) -> Self {
        Self {
            brightness: lerp_float(alpha, &from.brightness, &to.brightness),
            factor: lerp_float(alpha, &from.factor, &to.factor),
        }
    }

    fn apply_rgb(self, subject: Vec3) -> Vec3 {
        let multiplied_greyscale = argb::scale_rgb(argb::greyscale(subject), self.brightness);
        argb::lerp_rgb(self.factor, &subject, &multiplied_greyscale)
    }
}

/// Fades the value toward its scaled greyscale: vanilla's
/// `ColorModifier.BLEND_TO_GRAY_RGB` and `BLEND_TO_GRAY_ARGB`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlendToGrayModifier;

impl AttributeModifier<Vec3> for BlendToGrayModifier {
    type Argument = BlendToGray;

    fn operation_id(&self) -> OperationId {
        OperationId::BlendToGray
    }

    fn apply(&self, subject: Vec3, argument: &BlendToGray) -> Vec3 {
        argument.apply_rgb(subject)
    }

    fn argument_keyframe_lerp(
        &self,
        _attribute: &EnvironmentAttribute<Vec3>,
    ) -> LerpFunction<BlendToGray> {
        BlendToGray::lerp
    }
}

impl AttributeModifier<Vec4> for BlendToGrayModifier {
    type Argument = BlendToGray;

    fn operation_id(&self) -> OperationId {
        OperationId::BlendToGray
    }

    /// Vanilla lerps the alpha toward itself, which leaves it unchanged.
    fn apply(&self, subject: Vec4, argument: &BlendToGray) -> Vec4 {
        argument.apply_rgb(subject.truncate()).extend(subject.w)
    }

    fn argument_keyframe_lerp(
        &self,
        _attribute: &EnvironmentAttribute<Vec4>,
    ) -> LerpFunction<BlendToGray> {
        BlendToGray::lerp
    }
}
