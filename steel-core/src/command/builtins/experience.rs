//! Vanilla experience command plus Steel's existing clear extension.

use std::sync::Arc;

use steel_utils::{Identifier, translations};
use text_components::TextComponent;

use super::super::{
    brigadier::{ArgumentType, CommandNodeBuilder, CommandSyntaxError},
    execution::{
        CommandSource, SteelArgumentType, SteelCommandContext, SteelCommandRuntime, argument,
        literal,
    },
    registration::CommandRegistration,
};
use crate::entity::Entity;
use crate::player::Player;

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("experience"), |_| command()).alias("xp")
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("experience")
        .then(experience_operation(
            "add",
            i32::MIN,
            add_points,
            add_levels,
        ))
        .then(experience_operation("set", 0, set_points, set_levels))
        .then(
            literal("query").then(
                argument("target", SteelArgumentType::player())
                    .then(literal("points").executes(query_points))
                    .then(literal("levels").executes(query_levels)),
            ),
        )
        .then(
            literal("clear")
                .executes(clear_source)
                .then(argument("target", SteelArgumentType::players()).executes(clear_targets)),
        )
}

fn experience_operation(
    name: &'static str,
    minimum: i32,
    points: fn(&SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError>,
    levels: fn(&SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError>,
) -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal(name).then(
        argument("target", SteelArgumentType::players()).then(
            argument("amount", ArgumentType::integer(minimum, i32::MAX))
                .executes(points)
                .then(literal("points").executes(points))
                .then(literal("levels").executes(levels)),
        ),
    )
}

fn query_points(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    query_experience(context, ExperienceType::Points)
}

fn query_levels(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    query_experience(context, ExperienceType::Levels)
}

fn query_experience(
    context: &SteelCommandContext<CommandSource>,
    experience_type: ExperienceType,
) -> Result<i32, CommandSyntaxError> {
    let player = context.player("target")?;
    let amount = {
        let experience = player.experience.lock();
        match experience_type {
            ExperienceType::Points => experience.points(),
            ExperienceType::Levels => experience.level(),
        }
    };
    let translation = match experience_type {
        ExperienceType::Points => &translations::COMMANDS_EXPERIENCE_QUERY_POINTS,
        ExperienceType::Levels => &translations::COMMANDS_EXPERIENCE_QUERY_LEVELS,
    };
    let message = translation
        .message([
            TextComponent::plain(player.plain_text_name()),
            TextComponent::from(amount.to_string()),
        ])
        .component();
    context.source().send_success(&message, false);
    Ok(amount)
}

fn add_points(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    add_experience(context, ExperienceType::Points)
}

fn add_levels(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    add_experience(context, ExperienceType::Levels)
}

fn add_experience(
    context: &SteelCommandContext<CommandSource>,
    experience_type: ExperienceType,
) -> Result<i32, CommandSyntaxError> {
    let players = context.players("target")?;
    let amount = required_amount(context)?;
    for player in &players {
        match experience_type {
            ExperienceType::Points => player.give_experience_points(amount),
            ExperienceType::Levels => player.give_experience_levels(amount),
        }
    }

    send_mutation_success(context, &players, amount, experience_type, Mutation::Add);
    player_count_result(&players)
}

fn set_points(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    set_experience(context, ExperienceType::Points)
}

fn set_levels(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    set_experience(context, ExperienceType::Levels)
}

fn set_experience(
    context: &SteelCommandContext<CommandSource>,
    experience_type: ExperienceType,
) -> Result<i32, CommandSyntaxError> {
    let players = context.players("target")?;
    let amount = required_amount(context)?;
    let mut success = 0usize;
    for player in &players {
        let changed = match experience_type {
            ExperienceType::Points => {
                let mut experience = player.experience.lock();
                if experience.can_set_points(amount) {
                    experience.set_points(amount);
                    true
                } else {
                    false
                }
            }
            ExperienceType::Levels => {
                player.experience.lock().set_levels(amount);
                true
            }
        };
        success += usize::from(changed);
    }

    if success == 0 {
        return Err(CommandSyntaxError::dynamic(TextComponent::from(
            &translations::COMMANDS_EXPERIENCE_SET_POINTS_INVALID,
        )));
    }

    send_mutation_success(context, &players, amount, experience_type, Mutation::Set);
    player_count_result(&players)
}

fn send_mutation_success(
    context: &SteelCommandContext<CommandSource>,
    players: &[Arc<Player>],
    amount: i32,
    experience_type: ExperienceType,
    mutation: Mutation,
) {
    let amount = TextComponent::from(amount.to_string());
    let message = if let [player] = players {
        let translation = match (mutation, experience_type) {
            (Mutation::Add, ExperienceType::Points) => {
                &translations::COMMANDS_EXPERIENCE_ADD_POINTS_SUCCESS_SINGLE
            }
            (Mutation::Add, ExperienceType::Levels) => {
                &translations::COMMANDS_EXPERIENCE_ADD_LEVELS_SUCCESS_SINGLE
            }
            (Mutation::Set, ExperienceType::Points) => {
                &translations::COMMANDS_EXPERIENCE_SET_POINTS_SUCCESS_SINGLE
            }
            (Mutation::Set, ExperienceType::Levels) => {
                &translations::COMMANDS_EXPERIENCE_SET_LEVELS_SUCCESS_SINGLE
            }
        };
        translation
            .message([amount, TextComponent::plain(player.plain_text_name())])
            .component()
    } else {
        let translation = match (mutation, experience_type) {
            (Mutation::Add, ExperienceType::Points) => {
                &translations::COMMANDS_EXPERIENCE_ADD_POINTS_SUCCESS_MULTIPLE
            }
            (Mutation::Add, ExperienceType::Levels) => {
                &translations::COMMANDS_EXPERIENCE_ADD_LEVELS_SUCCESS_MULTIPLE
            }
            (Mutation::Set, ExperienceType::Points) => {
                &translations::COMMANDS_EXPERIENCE_SET_POINTS_SUCCESS_MULTIPLE
            }
            (Mutation::Set, ExperienceType::Levels) => {
                &translations::COMMANDS_EXPERIENCE_SET_LEVELS_SUCCESS_MULTIPLE
            }
        };
        translation
            .message([amount, TextComponent::from(players.len().to_string())])
            .component()
    };
    context.source().send_success(&message, true);
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "Command executors use a shared fallible callback signature."
)]
fn clear_source(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    if let Some(player) = context.source().player() {
        player.experience.lock().clear();
    }
    Ok(1)
}

fn clear_targets(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    let players = context.players("target")?;
    for player in &players {
        player.experience.lock().clear();
    }
    player_count_result(&players)
}

fn required_amount(
    context: &SteelCommandContext<CommandSource>,
) -> Result<i32, CommandSyntaxError> {
    context.integer("amount").ok_or_else(|| {
        CommandSyntaxError::dynamic("Parsed experience amount is missing from the command context")
    })
}

fn player_count_result(players: &[Arc<Player>]) -> Result<i32, CommandSyntaxError> {
    i32::try_from(players.len()).map_err(|_| {
        CommandSyntaxError::dynamic("Target player count exceeds the command result range")
    })
}

#[derive(Clone, Copy)]
enum ExperienceType {
    Points,
    Levels,
}

#[derive(Clone, Copy)]
enum Mutation {
    Add,
    Set,
}

#[cfg(test)]
mod tests {
    use steel_registry::test_support::init_test_registry;

    use super::super::create_dispatcher;
    use super::*;
    use crate::command::{
        brigadier::{CommandDispatcher, NodeId},
        execution::SteelArgumentType,
    };

    type Dispatcher = CommandDispatcher<CommandSource, SteelCommandRuntime>;

    fn child(dispatcher: &Dispatcher, parent: NodeId, name: &str) -> NodeId {
        let Some(children) = dispatcher.children(parent) else {
            panic!("parent node should exist");
        };
        let Some(child) = children.iter().copied().find(|child| {
            dispatcher
                .node(*child)
                .is_some_and(|node| node.name() == name)
        }) else {
            panic!("child `{name}` should exist");
        };
        child
    }

    #[test]
    #[expect(
        clippy::redundant_closure_for_method_calls,
        reason = "the private CommandNode type cannot be named from this module"
    )]
    #[expect(
        clippy::too_many_lines,
        reason = "one table-shaped test keeps both command aliases on the same graph contract"
    )]
    fn experience_and_xp_roots_share_the_expected_graph() {
        init_test_registry();
        let Ok(dispatcher) = create_dispatcher() else {
            panic!("built-in commands should register");
        };

        for root_name in ["experience", "xp"] {
            let root = child(&dispatcher, dispatcher.root(), root_name);
            assert!(
                dispatcher
                    .node(root)
                    .is_some_and(|node| node.is_restricted())
            );

            let add = child(&dispatcher, root, "add");
            let add_target = child(&dispatcher, add, "target");
            assert_eq!(
                dispatcher
                    .node(add_target)
                    .and_then(|node| node.argument_type()),
                Some(&SteelArgumentType::players())
            );
            let add_amount = child(&dispatcher, add_target, "amount");
            assert_eq!(
                dispatcher
                    .node(add_amount)
                    .and_then(|node| node.argument_type()),
                Some(&SteelArgumentType::from(ArgumentType::integer(
                    i32::MIN,
                    i32::MAX
                )))
            );
            assert!(
                dispatcher
                    .node(add_amount)
                    .is_some_and(|node| node.is_executable())
            );
            for suffix in ["points", "levels"] {
                let suffix = child(&dispatcher, add_amount, suffix);
                assert!(
                    dispatcher
                        .node(suffix)
                        .is_some_and(|node| node.is_executable())
                );
            }

            let set = child(&dispatcher, root, "set");
            let set_target = child(&dispatcher, set, "target");
            assert_eq!(
                dispatcher
                    .node(set_target)
                    .and_then(|node| node.argument_type()),
                Some(&SteelArgumentType::players())
            );
            let set_amount = child(&dispatcher, set_target, "amount");
            assert_eq!(
                dispatcher
                    .node(set_amount)
                    .and_then(|node| node.argument_type()),
                Some(&SteelArgumentType::from(ArgumentType::integer(0, i32::MAX)))
            );
            assert!(
                dispatcher
                    .node(set_amount)
                    .is_some_and(|node| node.is_executable())
            );
            for suffix in ["points", "levels"] {
                let suffix = child(&dispatcher, set_amount, suffix);
                assert!(
                    dispatcher
                        .node(suffix)
                        .is_some_and(|node| node.is_executable())
                );
            }

            let query = child(&dispatcher, root, "query");
            let query_target = child(&dispatcher, query, "target");
            assert_eq!(
                dispatcher
                    .node(query_target)
                    .and_then(|node| node.argument_type()),
                Some(&SteelArgumentType::player())
            );
            for suffix in ["points", "levels"] {
                let suffix = child(&dispatcher, query_target, suffix);
                assert!(
                    dispatcher
                        .node(suffix)
                        .is_some_and(|node| node.is_executable())
                );
            }

            let clear = child(&dispatcher, root, "clear");
            assert!(
                dispatcher
                    .node(clear)
                    .is_some_and(|node| node.is_executable())
            );
            let clear_target = child(&dispatcher, clear, "target");
            assert_eq!(
                dispatcher
                    .node(clear_target)
                    .and_then(|node| node.argument_type()),
                Some(&SteelArgumentType::players())
            );
            assert!(
                dispatcher
                    .node(clear_target)
                    .is_some_and(|node| node.is_executable())
            );
        }
    }
}
