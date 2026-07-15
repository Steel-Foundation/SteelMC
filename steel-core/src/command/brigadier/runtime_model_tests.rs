use std::sync::Arc;

use super::{
    ArgumentSuggestionContext, CommandArgumentParser, CommandDispatcher, CommandNodeBuilder,
    CommandRuntime, CommandSyntaxError, ContextChainStage, StringReader, SuggestionsBuilder,
};

#[derive(Clone, Debug, PartialEq, Eq)]
enum OpaqueArgument {
    SourceWord,
    PreviousWord { argument: &'static str },
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum OpaqueArgumentValue {
    SourceWord(Box<str>),
}

impl CommandArgumentParser<String> for OpaqueArgument {
    type Value = OpaqueArgumentValue;

    fn parse(
        &self,
        reader: &mut StringReader<'_>,
        source: &String,
    ) -> Result<Self::Value, CommandSyntaxError> {
        match self {
            Self::SourceWord | Self::PreviousWord { .. } => {
                let word = reader.read_unquoted_string();
                if word.is_empty() {
                    return Err(CommandSyntaxError::dynamic("expected a source word"));
                }
                Ok(OpaqueArgumentValue::SourceWord(
                    format!("{source}:{word}").into_boxed_str(),
                ))
            }
        }
    }

    fn list_suggestions(
        &self,
        context: &ArgumentSuggestionContext<'_, String, Self::Value>,
        builder: &mut SuggestionsBuilder<'_>,
    ) {
        let suggestion = match self {
            Self::SourceWord => context.source().as_str(),
            Self::PreviousWord { argument } => {
                let Some(OpaqueArgumentValue::SourceWord(value)) = context.argument(argument)
                else {
                    return;
                };
                value
            }
        };
        if suggestion
            .to_lowercase()
            .starts_with(builder.remaining_lowercase())
        {
            builder.suggest(suggestion);
        }
    }
}

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
    type Argument = OpaqueArgument;
    type ArgumentValue = OpaqueArgumentValue;
    type Executor = OpaqueExecutor;
    type Modifier = OpaqueModifier;
}

#[test]
fn parsing_uses_the_runtime_argument_representation() {
    let mut dispatcher = CommandDispatcher::<String, OpaqueRuntime>::new();
    let Ok(_) = dispatcher.register(
        CommandNodeBuilder::literal("run").then(
            CommandNodeBuilder::argument("prefix", OpaqueArgument::SourceWord).then(
                CommandNodeBuilder::argument(
                    "value",
                    OpaqueArgument::PreviousWord { argument: "prefix" },
                )
                .executes_with_executor(Arc::new(OpaqueExecutor::Terminal)),
            ),
        ),
    ) else {
        panic!("argument registration should succeed");
    };

    let parse = dispatcher.parse("run first input", "source".to_owned());

    assert_eq!(
        parse.context().argument("value"),
        Some(&OpaqueArgumentValue::SourceWord("source:input".into()))
    );
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("runtime argument command should form a context chain");
    };
    assert_eq!(
        chain.top_context().argument("prefix"),
        Some(&OpaqueArgumentValue::SourceWord("source:first".into()))
    );

    let suggestion_parse = dispatcher.parse("run first so", "source".to_owned());
    let Ok(suggestions) = dispatcher.completion_suggestions(&suggestion_parse) else {
        panic!("runtime argument suggestions should build");
    };
    assert_eq!(suggestions.list().len(), 1);
    assert_eq!(suggestions.list()[0].text(), "source:first");
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
