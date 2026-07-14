use steel_registry::{
    REGISTRY,
    game_rules::{GameRuleRef, GameRuleType, GameRuleValue},
};
use steel_utils::{Identifier, translations};
use text_components::TextComponent;

use super::super::{
    brigadier::{ArgumentType, CommandNodeBuilder, CommandSyntaxError},
    execution::{CommandSource, SteelCommandContext, SteelCommandRuntime, argument, literal},
    registration::CommandRegistration,
};

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("gamerule"), |_| command())
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    let mut command = literal("gamerule");
    for (_, rule) in REGISTRY.game_rules.iter() {
        // Vanilla's short identifier only omits the `minecraft` namespace.
        if rule.key.namespace == Identifier::VANILLA_NAMESPACE {
            command = command.then(rule_literal(rule.key.path.to_string(), rule));
        }
        command = command.then(rule_literal(rule.key.to_string(), rule));
    }
    command
}

fn rule_literal(
    name: String,
    rule: GameRuleRef,
) -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    match rule.value_type {
        GameRuleType::Bool => literal(name)
            .executes(move |context| query_rule(context, rule))
            .then(
                argument("value", ArgumentType::bool())
                    .executes(move |context| set_bool_rule(context, rule)),
            ),
        GameRuleType::Int => {
            let minimum = rule.min_value.unwrap_or(i32::MIN);
            let maximum = rule.max_value.unwrap_or(i32::MAX);
            literal(name)
                .executes(move |context| query_rule(context, rule))
                .then(
                    argument("value", ArgumentType::integer(minimum, maximum))
                        .executes(move |context| set_int_rule(context, rule)),
                )
        }
    }
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "Command executors use a shared fallible callback signature."
)]
fn query_rule(
    context: &SteelCommandContext<CommandSource>,
    rule: GameRuleRef,
) -> Result<i32, CommandSyntaxError> {
    let value = context.source().world().get_game_rule(rule);
    let message = translations::COMMANDS_GAMERULE_QUERY
        .message([
            TextComponent::from(rule_display_name(rule)),
            TextComponent::from(value.to_string()),
        ])
        .component();
    context.source().send_success(&message, false);
    Ok(game_rule_result(value))
}

fn set_bool_rule(
    context: &SteelCommandContext<CommandSource>,
    rule: GameRuleRef,
) -> Result<i32, CommandSyntaxError> {
    let Some(value) = context.boolean("value") else {
        return Err(missing_rule_value(rule));
    };
    set_rule(context, rule, GameRuleValue::Bool(value))
}

fn set_int_rule(
    context: &SteelCommandContext<CommandSource>,
    rule: GameRuleRef,
) -> Result<i32, CommandSyntaxError> {
    let Some(value) = context.integer("value") else {
        return Err(missing_rule_value(rule));
    };
    set_rule(context, rule, GameRuleValue::Int(value))
}

fn set_rule(
    context: &SteelCommandContext<CommandSource>,
    rule: GameRuleRef,
    value: GameRuleValue,
) -> Result<i32, CommandSyntaxError> {
    if !context.source().world().set_game_rule(rule, value) {
        return Err(CommandSyntaxError::dynamic(format!(
            "Parsed value does not match game rule {}",
            rule.key
        )));
    }

    let message = translations::COMMANDS_GAMERULE_SET
        .message([
            TextComponent::from(rule_display_name(rule)),
            TextComponent::from(value.to_string()),
        ])
        .component();
    context.source().send_success(&message, true);
    Ok(game_rule_result(value))
}

fn missing_rule_value(rule: GameRuleRef) -> CommandSyntaxError {
    CommandSyntaxError::dynamic(format!(
        "Parsed value for game rule {} is missing from the command context",
        rule.key
    ))
}

fn rule_display_name(rule: GameRuleRef) -> String {
    if rule.key.namespace == Identifier::VANILLA_NAMESPACE {
        rule.key.path.to_string()
    } else {
        rule.key.to_string()
    }
}

fn game_rule_result(value: GameRuleValue) -> i32 {
    match value {
        GameRuleValue::Bool(value) => i32::from(value),
        GameRuleValue::Int(value) => value,
    }
}

#[cfg(test)]
mod tests {
    use super::super::create_dispatcher;
    use crate::command::{
        brigadier::{ArgumentType, CommandDispatcher, NodeId},
        execution::{CommandSource, SteelArgumentType, SteelCommandRuntime},
    };
    use steel_registry::test_support::init_test_registry;

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
    fn vanilla_rules_have_short_and_qualified_literals() {
        init_test_registry();
        let Ok(dispatcher) = create_dispatcher() else {
            panic!("built-in commands should register");
        };
        let gamerule = child(&dispatcher, dispatcher.root(), "gamerule");
        let short = child(&dispatcher, gamerule, "keep_inventory");
        let qualified = child(&dispatcher, gamerule, "minecraft:keep_inventory");

        for rule in [short, qualified] {
            let Some(rule_node) = dispatcher.node(rule) else {
                panic!("gamerule literal should exist");
            };
            assert!(rule_node.is_executable());
            let value = child(&dispatcher, rule, "value");
            assert_eq!(
                dispatcher.node(value).and_then(|node| node.argument_type()),
                Some(&SteelArgumentType::from(ArgumentType::bool()))
            );
        }
    }

    #[test]
    fn integer_rule_bounds_are_retained_in_the_graph() {
        init_test_registry();
        let Ok(dispatcher) = create_dispatcher() else {
            panic!("built-in commands should register");
        };
        let gamerule = child(&dispatcher, dispatcher.root(), "gamerule");
        let rule = child(&dispatcher, gamerule, "max_command_forks");
        let value = child(&dispatcher, rule, "value");

        assert_eq!(
            dispatcher.node(value).and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::from(ArgumentType::integer(0, i32::MAX)))
        );
    }
}
