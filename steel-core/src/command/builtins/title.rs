//! Vanilla title display command.

use std::sync::Arc;

use steel_utils::{Identifier, translations};
use text_components::TextComponent;

use super::super::{
    brigadier::{CommandNodeBuilder, CommandSyntaxError},
    execution::{
        CommandSource, CommandTextResolver, SteelArgumentType, SteelCommandContext,
        SteelCommandRuntime, argument, literal,
    },
    registration::CommandRegistration,
};
use crate::{entity::Entity as _, player::Player};

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("title"), |_| command())
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("title").then(
        argument("targets", SteelArgumentType::players())
            .then(literal("clear").executes(clear_titles))
            .then(literal("reset").executes(reset_titles))
            .then(
                literal("title").then(
                    argument("title", SteelArgumentType::component())
                        .executes(|context| show_title(context, TitleKind::Title)),
                ),
            )
            .then(
                literal("subtitle").then(
                    argument("title", SteelArgumentType::component())
                        .executes(|context| show_title(context, TitleKind::Subtitle)),
                ),
            )
            .then(
                literal("actionbar").then(
                    argument("title", SteelArgumentType::component())
                        .executes(|context| show_title(context, TitleKind::Actionbar)),
                ),
            )
            .then(
                literal("times").then(
                    argument("fadeIn", SteelArgumentType::time(0)).then(
                        argument("stay", SteelArgumentType::time(0)).then(
                            argument("fadeOut", SteelArgumentType::time(0)).executes(set_times),
                        ),
                    ),
                ),
            ),
    )
}

fn clear_titles(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    let targets = context.players("targets")?;
    let result = target_count(&targets)?;
    for target in &targets {
        target.clear_titles();
    }
    send_success(context, &targets, TitleOperation::Clear);
    Ok(result)
}

fn reset_titles(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    let targets = context.players("targets")?;
    let result = target_count(&targets)?;
    for target in &targets {
        target.reset_titles();
    }
    send_success(context, &targets, TitleOperation::Reset);
    Ok(result)
}

fn show_title(
    context: &SteelCommandContext<CommandSource>,
    kind: TitleKind,
) -> Result<i32, CommandSyntaxError> {
    let targets = context.players("targets")?;
    let result = target_count(&targets)?;
    let title = context.text_component("title")?;

    for target in &targets {
        let title = title.try_resolve(&CommandTextResolver::with_entity_override(
            context.source(),
            target.as_ref(),
        ))?;
        match kind {
            TitleKind::Title => target.send_title(title),
            TitleKind::Subtitle => target.send_subtitle(title),
            TitleKind::Actionbar => target.send_action_bar(title),
        }
    }

    send_success(context, &targets, TitleOperation::Show(kind));
    Ok(result)
}

fn set_times(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    let targets = context.players("targets")?;
    let result = target_count(&targets)?;
    let fade_in = context.time("fadeIn")?;
    let stay = context.time("stay")?;
    let fade_out = context.time("fadeOut")?;

    for target in &targets {
        target.send_title_times(fade_in, stay, fade_out);
    }
    send_success(context, &targets, TitleOperation::Times);
    Ok(result)
}

fn target_count(targets: &[Arc<Player>]) -> Result<i32, CommandSyntaxError> {
    i32::try_from(targets.len()).map_err(|_| {
        CommandSyntaxError::dynamic("Target player count exceeds the command result range")
    })
}

fn send_success(
    context: &SteelCommandContext<CommandSource>,
    targets: &[Arc<Player>],
    operation: TitleOperation,
) {
    let message = operation.success_message(targets);
    context.source().send_success(&message, true);
}

#[derive(Clone, Copy)]
enum TitleKind {
    Title,
    Subtitle,
    Actionbar,
}

#[derive(Clone, Copy)]
enum TitleOperation {
    Clear,
    Reset,
    Show(TitleKind),
    Times,
}

impl TitleOperation {
    fn success_message(self, targets: &[Arc<Player>]) -> TextComponent {
        if let [target] = targets {
            let name = target.display_name();
            return match self {
                Self::Clear => translations::COMMANDS_TITLE_CLEARED_SINGLE
                    .message([name])
                    .component(),
                Self::Reset => translations::COMMANDS_TITLE_RESET_SINGLE
                    .message([name])
                    .component(),
                Self::Show(TitleKind::Title) => translations::COMMANDS_TITLE_SHOW_TITLE_SINGLE
                    .message([name])
                    .component(),
                Self::Show(TitleKind::Subtitle) => {
                    translations::COMMANDS_TITLE_SHOW_SUBTITLE_SINGLE
                        .message([name])
                        .component()
                }
                Self::Show(TitleKind::Actionbar) => {
                    translations::COMMANDS_TITLE_SHOW_ACTIONBAR_SINGLE
                        .message([name])
                        .component()
                }
                Self::Times => translations::COMMANDS_TITLE_TIMES_SINGLE
                    .message([name])
                    .component(),
            };
        }

        let count = TextComponent::plain(targets.len().to_string());
        match self {
            Self::Clear => translations::COMMANDS_TITLE_CLEARED_MULTIPLE
                .message([count])
                .component(),
            Self::Reset => translations::COMMANDS_TITLE_RESET_MULTIPLE
                .message([count])
                .component(),
            Self::Show(TitleKind::Title) => translations::COMMANDS_TITLE_SHOW_TITLE_MULTIPLE
                .message([count])
                .component(),
            Self::Show(TitleKind::Subtitle) => translations::COMMANDS_TITLE_SHOW_SUBTITLE_MULTIPLE
                .message([count])
                .component(),
            Self::Show(TitleKind::Actionbar) => {
                translations::COMMANDS_TITLE_SHOW_ACTIONBAR_MULTIPLE
                    .message([count])
                    .component()
            }
            Self::Times => translations::COMMANDS_TITLE_TIMES_MULTIPLE
                .message([count])
                .component(),
        }
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::init_vanilla_registry;

    use super::super::create_dispatcher;
    use crate::command::brigadier::{CommandDispatcher, NodeId};
    use crate::command::execution::{CommandSource, SteelArgumentType, SteelCommandRuntime};

    fn child(
        dispatcher: &CommandDispatcher<CommandSource, SteelCommandRuntime>,
        parent: NodeId,
        name: &str,
    ) -> NodeId {
        let Some(child) = dispatcher.children(parent).and_then(|children| {
            children.iter().copied().find(|child| {
                dispatcher
                    .node(*child)
                    .is_some_and(|node| node.name() == name)
            })
        }) else {
            panic!("expected {name:?} command child");
        };
        child
    }

    #[test]
    fn title_graph_matches_vanilla_shape() {
        init_vanilla_registry();
        let Ok(dispatcher) = create_dispatcher() else {
            panic!("built-in commands should register");
        };
        let title = child(&dispatcher, dispatcher.root(), "title");
        let Some(title_node) = dispatcher.node(title) else {
            panic!("title root should exist");
        };
        assert!(title_node.is_restricted());
        assert!(!title_node.is_executable());

        let targets = dispatcher
            .children(title)
            .and_then(|children| children.first())
            .copied();
        let Some(targets) = targets else {
            panic!("title targets should exist");
        };
        assert_eq!(
            dispatcher
                .node(targets)
                .and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::players())
        );

        let Some(branches) = dispatcher.children(targets) else {
            panic!("title branches should exist");
        };
        let branch_names = branches
            .iter()
            .map(|branch| {
                let Some(node) = dispatcher.node(*branch) else {
                    panic!("title branch should exist");
                };
                node.name()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            branch_names,
            ["clear", "reset", "title", "subtitle", "actionbar", "times"]
        );

        for branch in ["clear", "reset"] {
            let branch = child(&dispatcher, targets, branch);
            let Some(branch_node) = dispatcher.node(branch) else {
                panic!("title branch should exist");
            };
            assert!(branch_node.is_executable());
            assert!(dispatcher.children(branch).is_some_and(<[_]>::is_empty));
        }

        for branch in ["title", "subtitle", "actionbar"] {
            let branch = child(&dispatcher, targets, branch);
            let title_argument = child(&dispatcher, branch, "title");
            let Some(title_argument_node) = dispatcher.node(title_argument) else {
                panic!("title component argument should exist");
            };
            assert_eq!(
                title_argument_node.argument_type(),
                Some(&SteelArgumentType::component())
            );
            assert!(title_argument_node.is_executable());
        }

        let times = child(&dispatcher, targets, "times");
        let fade_in = child(&dispatcher, times, "fadeIn");
        assert_eq!(
            dispatcher
                .node(fade_in)
                .and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::time(0))
        );
        let stay = child(&dispatcher, fade_in, "stay");
        assert_eq!(
            dispatcher.node(stay).and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::time(0))
        );
        let fade_out = child(&dispatcher, stay, "fadeOut");
        let Some(fade_out_node) = dispatcher.node(fade_out) else {
            panic!("fadeOut argument should exist");
        };
        assert_eq!(
            fade_out_node.argument_type(),
            Some(&SteelArgumentType::time(0))
        );
        assert!(fade_out_node.is_executable());
    }
}
