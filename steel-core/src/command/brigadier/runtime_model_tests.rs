use std::sync::Arc;

use super::{CommandDispatcher, CommandNodeBuilder, CommandRuntime, ContextChainStage};

#[derive(Debug, PartialEq, Eq)]
enum OpaqueExecutor {
    Terminal,
}

#[derive(Debug, PartialEq, Eq)]
enum OpaqueModifier {
    Transform,
}

struct OpaqueRuntime;

impl CommandRuntime<String> for OpaqueRuntime {
    type Executor = OpaqueExecutor;
    type Modifier = OpaqueModifier;
}

#[test]
fn parsing_preserves_opaque_runtime_payloads() {
    let mut dispatcher = CommandDispatcher::<String, OpaqueRuntime>::new();
    let Ok(_) = dispatcher.register(
        CommandNodeBuilder::literal("run")
            .executes_with_executor(Arc::new(OpaqueExecutor::Terminal)),
    ) else {
        panic!("terminal registration should succeed");
    };
    let root = dispatcher.root();
    let Ok(_) = dispatcher.register(
        CommandNodeBuilder::literal("alias").redirects_with_modifier(
            root,
            Arc::new(OpaqueModifier::Transform),
            true,
        ),
    ) else {
        panic!("redirect registration should succeed");
    };

    let parse = dispatcher.parse("alias run", "parse source".to_owned());
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("opaque executors should still form a context chain");
    };

    assert_eq!(chain.stage(), ContextChainStage::Modify);
    assert_eq!(
        chain.top_context().modifier(),
        Some(&OpaqueModifier::Transform)
    );
    assert!(chain.top_context().is_forked());
    let Some(executable) = chain.next_stage() else {
        panic!("redirect should have an executable stage");
    };
    assert_eq!(
        executable.top_context().executor(),
        Some(&OpaqueExecutor::Terminal)
    );
}
