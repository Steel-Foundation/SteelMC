use steel_math::keyframe_track::{LerpFunction, lerp_constant};

use super::{AttributeModifier, OperationId};
use crate::environment_attribute::EnvironmentAttribute;

/// Boolean logic modifiers, vanilla's `BooleanModifier`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BooleanModifier {
    And,
    Nand,
    Or,
    Nor,
    Xor,
    Xnor,
}

impl AttributeModifier<bool> for BooleanModifier {
    type Argument = bool;

    fn operation_id(&self) -> OperationId {
        match self {
            Self::And => OperationId::And,
            Self::Nand => OperationId::Nand,
            Self::Or => OperationId::Or,
            Self::Nor => OperationId::Nor,
            Self::Xor => OperationId::Xor,
            Self::Xnor => OperationId::Xnor,
        }
    }

    fn apply(&self, subject: bool, argument: &bool) -> bool {
        let argument = *argument;
        match self {
            Self::And => argument && subject,
            Self::Nand => !argument || !subject,
            Self::Or => argument || subject,
            Self::Nor => !argument && !subject,
            Self::Xor => argument ^ subject,
            Self::Xnor => argument == subject,
        }
    }

    fn argument_keyframe_lerp(
        &self,
        _attribute: &EnvironmentAttribute<bool>,
    ) -> LerpFunction<bool> {
        lerp_constant
    }
}

#[cfg(test)]
mod tests {
    use super::{AttributeModifier as _, BooleanModifier};

    #[test]
    fn boolean_modifiers_follow_vanilla_truth_tables() {
        let cases = [
            (BooleanModifier::And, [false, false, false, true]),
            (BooleanModifier::Nand, [true, true, true, false]),
            (BooleanModifier::Or, [false, true, true, true]),
            (BooleanModifier::Nor, [true, false, false, false]),
            (BooleanModifier::Xor, [false, true, true, false]),
            (BooleanModifier::Xnor, [true, false, false, true]),
        ];
        for (modifier, expected) in cases {
            let results = [(false, false), (false, true), (true, false), (true, true)]
                .map(|(subject, argument)| modifier.apply(subject, &argument));
            assert_eq!(results, expected, "{modifier:?}");
        }
    }
}
