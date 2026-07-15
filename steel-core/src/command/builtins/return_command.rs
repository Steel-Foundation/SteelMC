//! Vanilla command-frame returns.

use std::sync::Arc;

use steel_utils::Identifier;

use super::super::{
    brigadier::{ArgumentType, CommandNodeBuilder, NodeId},
    execution::{
        ChainModifiers, CommandSource, CustomCommandExecutor, CustomModifierExecutor,
        ExecutionCommandSource, ExecutionControl, SteelCommandRuntime, SteelContextChain, argument,
        literal,
    },
    registration::CommandRegistration,
};

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("return"), command)
}

fn command(dispatcher_root: NodeId) -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("return")
        .then(
            argument("value", ArgumentType::integer(i32::MIN, i32::MAX))
                .executes_custom(ReturnValue),
        )
        .then(literal("fail").executes_custom(ReturnFail))
        .then(literal("run").redirects_custom(dispatcher_root, ReturnRun, false))
}

struct ReturnValue;

impl CustomCommandExecutor<CommandSource> for ReturnValue {
    fn run(
        &self,
        source: Arc<CommandSource>,
        chain: &SteelContextChain<CommandSource>,
        _modifiers: ChainModifiers,
        control: &mut ExecutionControl<'_, CommandSource>,
    ) {
        let Some(value) = chain.top_context().integer("value") else {
            unreachable!("the return value executor always follows its integer argument")
        };
        source.callback().on_result(true, value);
        control.return_success(value);
    }
}

struct ReturnFail;

impl CustomCommandExecutor<CommandSource> for ReturnFail {
    fn run(
        &self,
        source: Arc<CommandSource>,
        _chain: &SteelContextChain<CommandSource>,
        _modifiers: ChainModifiers,
        control: &mut ExecutionControl<'_, CommandSource>,
    ) {
        source.callback().on_result(false, 0);
        control.return_failure();
    }
}

struct ReturnRun;

impl CustomModifierExecutor<CommandSource> for ReturnRun {
    fn apply(
        &self,
        original_source: Arc<CommandSource>,
        sources: Vec<Arc<CommandSource>>,
        chain: &SteelContextChain<CommandSource>,
        modifiers: ChainModifiers,
        control: &mut ExecutionControl<'_, CommandSource>,
    ) {
        if sources.is_empty() {
            if modifiers.is_return() {
                control.queue_fallthrough();
            }
            return;
        }

        let Some(next_stage) = chain.next_stage() else {
            unreachable!("return run redirects to a following command-root stage")
        };
        control.discard_frame();
        control.queue_contexts(
            next_stage,
            original_source,
            sources,
            modifiers.with_return(),
        );
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::test_support::init_test_registry;

    use super::super::create_dispatcher;
    use crate::command::{brigadier::ArgumentType, execution::SteelArgumentType};

    #[test]
    fn return_graph_matches_vanillas_three_forms() {
        init_test_registry();
        let Ok(dispatcher) = create_dispatcher() else {
            panic!("built-in dispatcher should build");
        };
        let Some(root) = dispatcher.children(dispatcher.root()).and_then(|children| {
            children.iter().copied().find(|child| {
                dispatcher
                    .node(*child)
                    .is_some_and(|node| node.name() == "return")
            })
        }) else {
            panic!("return root should exist");
        };
        let Some(children) = dispatcher.children(root) else {
            panic!("return root should have children");
        };
        assert_eq!(children.len(), 3);

        let value = children.iter().copied().find(|child| {
            dispatcher
                .node(*child)
                .is_some_and(|node| node.name() == "value")
        });
        let Some(value) = value.and_then(|value| dispatcher.node(value)) else {
            panic!("return value argument should exist");
        };
        assert_eq!(
            value.argument_type(),
            Some(&SteelArgumentType::from(ArgumentType::integer(
                i32::MIN,
                i32::MAX,
            )))
        );
        assert!(value.is_executable());

        let fail = children.iter().copied().find(|child| {
            dispatcher
                .node(*child)
                .is_some_and(|node| node.name() == "fail")
        });
        let Some(fail) = fail.and_then(|fail| dispatcher.node(fail)) else {
            panic!("return fail should exist");
        };
        assert!(fail.is_executable());

        let run = children.iter().copied().find(|child| {
            dispatcher
                .node(*child)
                .is_some_and(|node| node.name() == "run")
        });
        let Some(run) = run.and_then(|run| dispatcher.node(run)) else {
            panic!("return run should exist");
        };
        assert_eq!(run.redirect(), Some(dispatcher.root()));
        assert!(!run.is_forked_redirect());
        assert!(run.has_redirect_modifier());
        assert!(!run.is_executable());
    }
}
