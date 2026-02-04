//! Handler for the "execute" command.
//!
//! TODO: This is a partial implementation. Missing subcommands include:
//! - `as` (execute as another entity)
//! - `at` (execute at another entity's position)
//! - `positioned` (execute at specific coordinates)
//! - `if`/`unless` (conditional execution)
//! - `store` (store command results)
//! - `facing` (face towards entity or coordinates)
//! - `align` (align position to block grid)
//! - `dimension` (execute in another dimension)
//! - `summon` (execute as newly summoned entity)
//! - `on` (execute on related entities)
use crate::command::arguments::anchor::AnchorArgument;
use crate::command::arguments::rotation::RotationArgument;
use crate::command::commands::{
    CommandExecutor, CommandHandlerBuilder, CommandHandlerDyn, CommandRedirectTarget, argument,
    literal, redirect,
};
use crate::command::context::{CommandContext, EntityAnchor};
use crate::command::error::CommandError;

/// Handler for the "execute" command.
#[must_use]
pub fn command_handler() -> impl CommandHandlerDyn {
    CommandHandlerBuilder::new(
        &["execute"],
        "Executes another command with extra options.",
        "minecraft:command.execute",
    )
    .then(
        literal("anchored").then(
            argument("anchor", AnchorArgument)
                .then(redirect(CommandRedirectTarget::Current, AnchorExecutor)),
        ),
    )
    .then(
        literal("rotated").then(
            argument("rot", RotationArgument)
                .then(redirect(CommandRedirectTarget::Current, RotationExecutor)),
        ),
    )
    .then(literal("run").then(redirect(CommandRedirectTarget::All, RunExecutor)))
}

struct AnchorExecutor;
impl CommandExecutor<((), EntityAnchor)> for AnchorExecutor {
    fn execute(
        &self,
        args: ((), EntityAnchor),
        context: &mut CommandContext,
    ) -> Result<(), CommandError> {
        context.anchor = args.1;
        Ok(())
    }
}

struct RotationExecutor;
impl CommandExecutor<((), (f32, f32))> for RotationExecutor {
    fn execute(
        &self,
        args: ((), (f32, f32)),
        context: &mut CommandContext,
    ) -> Result<(), CommandError> {
        context.rotation = Some(args.1);
        Ok(())
    }
}

struct RunExecutor;
impl CommandExecutor<()> for RunExecutor {
    fn execute(&self, _args: (), _context: &mut CommandContext) -> Result<(), CommandError> {
        Ok(())
    }
}
