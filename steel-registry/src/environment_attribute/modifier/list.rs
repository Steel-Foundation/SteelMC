use steel_math::keyframe_track::LerpFunction;

use super::{AttributeModifier, OperationId};
use crate::environment_attribute::{AttributeList, AttributeValue, EnvironmentAttribute};

/// Appends the argument list, as vanilla's `ListModifier.Append`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListAppendModifier;

impl<T: Clone + Send + Sync + 'static> AttributeModifier<AttributeList<T>> for ListAppendModifier
where
    AttributeList<T>: AttributeValue,
{
    type Argument = AttributeList<T>;

    fn operation_id(&self) -> OperationId {
        OperationId::Append
    }

    fn apply(&self, subject: AttributeList<T>, argument: &AttributeList<T>) -> AttributeList<T> {
        if argument.is_empty() {
            return subject;
        }
        if subject.is_empty() {
            return argument.clone();
        }
        AttributeList::owned(subject.iter().chain(argument.iter()).cloned().collect())
    }

    fn argument_keyframe_lerp(
        &self,
        attribute: &EnvironmentAttribute<AttributeList<T>>,
    ) -> LerpFunction<AttributeList<T>> {
        attribute.attribute_type().keyframe_lerp()
    }
}
