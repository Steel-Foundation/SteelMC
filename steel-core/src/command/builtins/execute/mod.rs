//! Vanilla command execution context composition.

mod condition;
mod source;
mod store;

use steel_utils::{Identifier, translations};
use text_components::TextComponent;

use super::super::{
    brigadier::{CommandNodeBuilder, CommandSyntaxError, NodeId},
    execution::{CommandSource, SteelCommandContext, SteelCommandRuntime, literal},
    registration::CommandRegistration,
};
use crate::command::storage::CommandStorage;
use crate::scoreboard::{Scoreboard, ScoreboardObjective};

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
        .then(
            literal("store")
                .then(store::target("result", true))
                .then(store::target("success", false)),
        )
        .then(source::positioned_operation())
        .then(source::rotated_operation())
        .then(source::facing_operation())
        .then(source::align_operation())
        .then(source::anchored_operation())
        .then(source::in_operation())
        .then(source::summon_operation())
        .then(source::on_relations())
}

fn source_scoreboard(
    context: &SteelCommandContext<CommandSource>,
) -> Result<&Scoreboard, CommandSyntaxError> {
    let source = context.source();
    source
        .server()
        .scoreboards
        .get(source.world().domain())
        .ok_or_else(|| {
            CommandSyntaxError::dynamic(format!(
                "Domain '{}' has no command scoreboard",
                source.world().domain()
            ))
        })
}

fn source_command_storage(
    context: &SteelCommandContext<CommandSource>,
) -> Result<&CommandStorage, CommandSyntaxError> {
    let source = context.source();
    source
        .server()
        .command_storage
        .get(source.world().domain())
        .ok_or_else(|| {
            CommandSyntaxError::dynamic(format!(
                "Domain '{}' has no command storage",
                source.world().domain()
            ))
        })
}

fn objective(
    context: &SteelCommandContext<CommandSource>,
    scoreboard: &Scoreboard,
    name: &str,
) -> Result<ScoreboardObjective, CommandSyntaxError> {
    let objective_name = context.objective_name(name).ok_or_else(|| {
        CommandSyntaxError::dynamic(format!(
            "Parsed value for {name} is missing from the command context"
        ))
    })?;
    scoreboard.objective(objective_name).ok_or_else(|| {
        let message = translations::ARGUMENTS_OBJECTIVE_NOT_FOUND
            .message([TextComponent::from(objective_name.to_owned())])
            .component();
        CommandSyntaxError::dynamic(message)
    })
}
