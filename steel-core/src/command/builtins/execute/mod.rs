//! Vanilla command execution context composition.

mod condition;
mod source;

use steel_utils::Identifier;

use super::super::{
    brigadier::{CommandNodeBuilder, NodeId},
    execution::{CommandSource, SteelCommandRuntime, literal},
    registration::CommandRegistration,
};

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("execute"), command)
}

fn command(dispatcher_root: NodeId) -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("execute")
        .then(literal("run").redirects(dispatcher_root))
        .then(condition::conditionals("if", true))
        .then(condition::conditionals("unless", false))
        .then(source::as_operation())
        .then(source::at_operation())
        .then(source::positioned_operation())
        .then(source::rotated_operation())
        .then(source::facing_operation())
        .then(source::align_operation())
        .then(source::anchored_operation())
        .then(source::summon_operation())
}
