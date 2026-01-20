//! This module contains the command building structs.
pub mod execute;
pub mod gamemode;
pub mod gamerule;
pub mod seed;
pub mod stop;
pub mod tick;
pub mod weather;

use std::marker::PhantomData;
use std::ops::Not;
use std::sync::Arc;

use steel_protocol::packets::game::{
    CommandNode, CommandNodeInfo, SuggestionEntry, SuggestionType,
};

use crate::command::arguments::{CommandArgument, SuggestionContext};
use crate::command::context::CommandContext;
use crate::command::error::CommandError;
use crate::server::Server;

/// Result of a suggestion query, containing the suggestions and where to apply them.
#[derive(Debug, Clone)]
pub struct SuggestionResult {
    /// The suggestions to show.
    pub suggestions: Vec<SuggestionEntry>,
    /// Start position in the input string (after the slash).
    pub start: i32,
    /// Length of text to replace.
    pub length: i32,
}

/// A trait that defines the behavior of a type safe command executor.
pub trait CommandExecutor<S> {
    /// Executes the command with the given type safe arguments.
    fn execute(
        &self,
        parsed: S,
        context: &mut CommandContext,
        server: &Arc<Server>,
    ) -> Result<(), CommandError>;
}

/// The builder struct that holds command handler data and executor.
pub struct CommandHandlerBuilder {
    names: &'static [&'static str],
    description: &'static str,
    permission: &'static str,
}

/// The struct that holds command handler data and executor.
pub struct CommandHandler<E> {
    names: &'static [&'static str],
    description: &'static str,
    permission: &'static str,
    executor: E,
}

/// Defines a command handler that can be dynamically dispatched.
pub trait CommandHandlerDyn {
    /// Returns the names of the command.
    fn names(&self) -> &'static [&'static str];

    /// Returns the description of the command.
    fn description(&self) -> &'static str;

    /// Returns the permission of the command.
    fn permission(&self) -> &'static str;

    /// Handles the execution of a command sent by a player.
    fn execute(
        &self,
        command_args: &[&str],
        context: &mut CommandContext,
        server: &Arc<Server>,
    ) -> Result<(), CommandError>;

    /// Generates the usage information for the command.
    fn usage(&self, buffer: &mut Vec<CommandNode>, root_children: &mut Vec<i32>);

    /// Returns suggestions for the given arguments.
    /// `args` is the argument list (command name already removed).
    /// `args_start_pos` is the byte position where args start in the original input (after command name + space).
    fn suggest(
        &self,
        _args: &[&str],
        _args_start_pos: usize,
        _context: &mut CommandContext,
    ) -> Option<SuggestionResult> {
        None
    }
}

impl CommandHandlerBuilder {
    /// Creates a new command handler builder.
    #[must_use]
    pub fn new(
        names: &'static [&'static str],
        description: &'static str,
        permission: &'static str,
    ) -> CommandHandlerBuilder {
        CommandHandlerBuilder {
            names,
            description,
            permission,
        }
    }

    /// Chains a command executor to this command handler.
    #[must_use]
    pub fn then<E>(self, executor: E) -> CommandHandler<E>
    where
        E: CommandParserExecutor<()>,
    {
        CommandHandler {
            names: self.names,
            description: self.description,
            permission: self.permission,
            executor,
        }
    }

    /// Executes the command executor if the command was ran without arguments.
    pub fn executes<E>(self, executor: E) -> CommandHandler<CommandParserLeafExecutor<(), E>>
    where
        E: CommandExecutor<()>,
    {
        CommandHandler {
            names: self.names,
            description: self.description,
            permission: self.permission,
            executor: CommandParserLeafExecutor {
                executor,
                _source: PhantomData,
            },
        }
    }
}

impl<E1> CommandHandler<E1> {
    /// Chains a command executor that parses arguments.
    #[must_use]
    pub fn then<E2>(self, executor: E2) -> CommandHandler<CommandParserSplitExecutor<(), E1, E2>>
    where
        E2: CommandParserExecutor<()>,
    {
        CommandHandler {
            names: self.names,
            description: self.description,
            permission: self.permission,
            executor: CommandParserSplitExecutor {
                first_executor: self.executor,
                second_executor: executor,
                _source: PhantomData,
            },
        }
    }

    /// Executes the command executor if the command was ran without arguments.
    pub fn executes<E2>(self, executor: E2) -> CommandHandler<CommandParserLeafExecutor<(), E2>>
    where
        E2: CommandExecutor<()>,
    {
        CommandHandler {
            names: self.names,
            description: self.description,
            permission: self.permission,
            executor: CommandParserLeafExecutor {
                executor,
                _source: PhantomData,
            },
        }
    }
}

impl<E> CommandHandlerDyn for CommandHandler<E>
where
    E: CommandParserExecutor<()>,
{
    /// Returns the names of the command.
    fn names(&self) -> &'static [&'static str] {
        self.names
    }

    /// Returns the description of the command.
    fn description(&self) -> &'static str {
        self.description
    }

    /// Returns the permission of the command.
    fn permission(&self) -> &'static str {
        self.permission
    }

    /// Executes the command with the given unparsed arguments.
    fn execute(
        &self,
        command_args: &[&str],
        context: &mut CommandContext,
        server: &Arc<Server>,
    ) -> Result<(), CommandError> {
        match self
            .executor
            .execute(command_args, (), context, server, self)
        {
            Some(result) => result,
            None => Err(CommandError::CommandFailed(Box::new(
                "Invalid Syntax.".into(),
            ))),
        }
    }

    fn usage(&self, buffer: &mut Vec<CommandNode>, root_children: &mut Vec<i32>) {
        let node_index = buffer.len();
        let node = CommandNode::new_root(); // Reserve spot in buffer before calling children
        buffer.push(node);
        root_children.push(node_index as i32);

        buffer[node_index] = CommandNode::new_literal(
            self.executor.usage(buffer, node_index as i32),
            self.names()[0],
        );

        for name in self.names().iter().skip(1) {
            root_children.push(buffer.len() as i32);
            buffer.push(CommandNode::new_literal(
                CommandNodeInfo::new_redirect(node_index as i32),
                *name,
            ));
        }
    }

    fn suggest(
        &self,
        args: &[&str],
        args_start_pos: usize,
        context: &mut CommandContext,
    ) -> Option<SuggestionResult> {
        let mut suggestion_ctx = SuggestionContext::new();
        self.executor
            .suggest(args, args_start_pos, context, &mut suggestion_ctx)
    }
}

/// A trait that defines the behavior of a type safe command executor.
pub trait CommandParserExecutor<S> {
    /// Executes the command with the given unparsed and parsed arguments.
    fn execute(
        &self,
        args: &[&str],
        parsed: S,
        context: &mut CommandContext,
        server: &Arc<Server>,
        handler: &dyn CommandHandlerDyn,
    ) -> Option<Result<(), CommandError>>;

    /// Generates usage information for the command.
    fn usage(&self, buffer: &mut Vec<CommandNode>, node_index: i32) -> CommandNodeInfo;

    /// Returns suggestions for the current argument position.
    /// `args` is the remaining unparsed arguments.
    /// `current_pos` is the byte position in the original input where args start.
    /// `suggestion_ctx` contains previously parsed argument values for context-dependent suggestions.
    /// Returns `Some(SuggestionResult)` if suggestions are available.
    fn suggest(
        &self,
        args: &[&str],
        current_pos: usize,
        context: &mut CommandContext,
        suggestion_ctx: &mut SuggestionContext,
    ) -> Option<SuggestionResult>;
}

/// Tree node that executes a command with the given parsed arguments.
pub struct CommandParserLeafExecutor<S, E> {
    executor: E,
    _source: PhantomData<S>,
}

impl<S, E> CommandParserExecutor<S> for CommandParserLeafExecutor<S, E>
where
    E: CommandExecutor<S>,
{
    fn execute(
        &self,
        args: &[&str],
        parsed: S,
        context: &mut CommandContext,
        server: &Arc<Server>,
        _: &dyn CommandHandlerDyn,
    ) -> Option<Result<(), CommandError>> {
        args.is_empty()
            .then(|| self.executor.execute(parsed, context, server))
    }

    fn usage(&self, _buffer: &mut Vec<CommandNode>, _: i32) -> CommandNodeInfo {
        CommandNodeInfo::new_executable()
    }

    fn suggest(
        &self,
        _args: &[&str],
        _current_pos: usize,
        _context: &mut CommandContext,
        _suggestion_ctx: &mut SuggestionContext,
    ) -> Option<SuggestionResult> {
        // Leaf executor has no more arguments to suggest
        None
    }
}

/// Tree node that passes execution to the second executor if the first one fails.
/// This allows for branching command syntax where multiple alternatives can be tried.
pub struct CommandParserSplitExecutor<S, E1, E2> {
    first_executor: E1,
    second_executor: E2,
    _source: PhantomData<S>,
}

impl<S, E1, E2> CommandParserExecutor<S> for CommandParserSplitExecutor<S, E1, E2>
where
    S: Clone,
    E1: CommandParserExecutor<S>,
    E2: CommandParserExecutor<S>,
{
    fn execute(
        &self,
        args: &[&str],
        parsed: S,
        context: &mut CommandContext,
        server: &Arc<Server>,
        handler: &dyn CommandHandlerDyn,
    ) -> Option<Result<(), CommandError>> {
        let result = self
            .first_executor
            .execute(args, parsed.clone(), context, server, handler);
        if result.is_some() {
            return result;
        }

        self.second_executor
            .execute(args, parsed, context, server, handler)
    }

    fn usage(&self, buffer: &mut Vec<CommandNode>, node_index: i32) -> CommandNodeInfo {
        self.first_executor
            .usage(buffer, node_index)
            .chain(self.second_executor.usage(buffer, node_index))
    }

    fn suggest(
        &self,
        args: &[&str],
        current_pos: usize,
        context: &mut CommandContext,
        suggestion_ctx: &mut SuggestionContext,
    ) -> Option<SuggestionResult> {
        // Get suggestions from both executors and combine them
        let first =
            self.first_executor
                .suggest(args, current_pos, context, &mut suggestion_ctx.clone());
        let second = self
            .second_executor
            .suggest(args, current_pos, context, suggestion_ctx);

        match (first, second) {
            (Some(mut first_result), Some(second_result)) => {
                // Combine suggestions from both branches if they have the same position
                if first_result.start == second_result.start
                    && first_result.length == second_result.length
                {
                    first_result.suggestions.extend(second_result.suggestions);
                }
                Some(first_result)
            }
            (Some(result), None) | (None, Some(result)) => Some(result),
            (None, None) => None,
        }
    }
}

/// Tree node that redirects to another node after executing.
/// This allows commands to chain into other commands or recursively into themselves.
pub struct CommandParserRedirectExecutor<S, E> {
    to: CommandRedirectTarget,
    executor: E,
    _source: PhantomData<S>,
}

/// Creates a new command redirect builder.
pub fn redirect<S, E>(
    to: CommandRedirectTarget,
    executor: E,
) -> CommandParserRedirectExecutor<S, E> {
    CommandParserRedirectExecutor {
        to,
        executor,
        _source: PhantomData,
    }
}

/// Target for redirecting command execution after a subcommand executes.
pub enum CommandRedirectTarget {
    /// Redirects to the current `CommandHandler`, allowing any branch of the current command to be executed.
    /// Used for commands that chain into themselves (e.g., `/execute anchored feet execute rotated ~ ~ run ...`).
    Current,
    /// Redirects to the `CommandDispatcher`, allowing any registered command to be executed.
    /// Used for commands that can execute arbitrary other commands (e.g., `/execute run <any command>`).
    All,
}

impl<S, E> CommandParserExecutor<S> for CommandParserRedirectExecutor<S, E>
where
    E: CommandExecutor<S>,
{
    fn execute(
        &self,
        args: &[&str],
        parsed: S,
        context: &mut CommandContext,
        server: &Arc<Server>,
        handler: &dyn CommandHandlerDyn,
    ) -> Option<Result<(), CommandError>> {
        if let Err(err) = self.executor.execute(parsed, context, server) {
            return Some(Err(err));
        }

        args.is_empty().not().then(|| match self.to {
            CommandRedirectTarget::Current => handler.execute(args, context, server),
            CommandRedirectTarget::All => {
                server
                    .command_dispatcher
                    .read()
                    .execute(args[0], &args[1..], context, server)
            }
        })
    }

    fn usage(&self, _buffer: &mut Vec<CommandNode>, node_index: i32) -> CommandNodeInfo {
        CommandNodeInfo::new_redirect(match self.to {
            CommandRedirectTarget::Current => node_index,
            CommandRedirectTarget::All => 0,
        })
    }

    fn suggest(
        &self,
        _args: &[&str],
        _current_pos: usize,
        _context: &mut CommandContext,
        _suggestion_ctx: &mut SuggestionContext,
    ) -> Option<SuggestionResult> {
        // Redirect executors don't provide suggestions themselves
        // The redirected command would handle suggestions
        None
    }
}

/// A builder struct for creating command literal argument executors.
/// Literals match exact string values (e.g., "clear", "rain", "thunder" in `/weather <clear|rain|thunder>`).
pub struct CommandParserLiteralBuilder<S> {
    expected: &'static str,
    _source: PhantomData<S>,
}

/// Creates a new literal command argument builder.
#[must_use]
pub fn literal<S>(expected: &'static str) -> CommandParserLiteralBuilder<S> {
    CommandParserLiteralBuilder {
        expected,
        _source: PhantomData,
    }
}

impl<S> CommandParserLiteralBuilder<S> {
    /// Executes the command argument executor after the argument is parsed.
    pub fn then<E>(self, executor: E) -> CommandParserLiteralExecutor<S, E>
    where
        E: CommandParserExecutor<S>,
    {
        CommandParserLiteralExecutor {
            expected: self.expected,
            executor,
            _source: PhantomData,
        }
    }

    /// Executes the command executor after the argument is parsed.
    pub fn executes<E>(
        self,
        executor: E,
    ) -> CommandParserLiteralExecutor<S, CommandParserLeafExecutor<S, E>>
    where
        E: CommandExecutor<S>,
    {
        CommandParserLiteralExecutor {
            expected: self.expected,
            executor: CommandParserLeafExecutor {
                executor,
                _source: PhantomData,
            },
            _source: PhantomData,
        }
    }
}

/// Tree node that parses a single literal string and provides execution to the next executor.
/// The literal must match exactly (case-sensitive).
pub struct CommandParserLiteralExecutor<S, E> {
    expected: &'static str,
    executor: E,
    _source: PhantomData<S>,
}

impl<S, E1> CommandParserLiteralExecutor<S, E1> {
    /// Executes the command argument executor after the argument is parsed.
    pub fn then<E2>(
        self,
        executor: E2,
    ) -> CommandParserLiteralExecutor<S, CommandParserSplitExecutor<S, E1, E2>>
    where
        E2: CommandParserExecutor<S>,
    {
        CommandParserLiteralExecutor {
            expected: self.expected,
            executor: CommandParserSplitExecutor {
                first_executor: self.executor,
                second_executor: executor,
                _source: PhantomData,
            },
            _source: PhantomData,
        }
    }

    /// Executes the command executor after the argument is parsed.
    pub fn executes<E2>(
        self,
        executor: E2,
    ) -> CommandParserLiteralExecutor<S, SplitLeafExecutor<S, E1, E2>>
    where
        E2: CommandExecutor<S>,
    {
        CommandParserLiteralExecutor {
            expected: self.expected,
            executor: CommandParserSplitExecutor {
                first_executor: self.executor,
                second_executor: CommandParserLeafExecutor {
                    executor,
                    _source: PhantomData,
                },
                _source: PhantomData,
            },
            _source: PhantomData,
        }
    }
}

impl<S, E> CommandParserExecutor<S> for CommandParserLiteralExecutor<S, E>
where
    E: CommandParserExecutor<S>,
{
    fn execute(
        &self,
        args: &[&str],
        parsed: S,
        context: &mut CommandContext,
        server: &Arc<Server>,
        handler: &dyn CommandHandlerDyn,
    ) -> Option<Result<(), CommandError>> {
        if *args.first()? == self.expected {
            self.executor
                .execute(&args[1..], parsed, context, server, handler)
        } else {
            None
        }
    }

    fn usage(&self, buffer: &mut Vec<CommandNode>, node_index: i32) -> CommandNodeInfo {
        let node = CommandNode::new_literal(self.executor.usage(buffer, node_index), self.expected);
        let result = vec![buffer.len() as i32];
        buffer.push(node);

        CommandNodeInfo::new(result)
    }

    fn suggest(
        &self,
        args: &[&str],
        current_pos: usize,
        context: &mut CommandContext,
        suggestion_ctx: &mut SuggestionContext,
    ) -> Option<SuggestionResult> {
        let first = args.first()?;

        // If we're typing this literal (partial match or complete match with more args)
        if self.expected.starts_with(*first) && args.len() == 1 {
            // Suggest this literal if it's a partial match
            if *first != self.expected {
                return Some(SuggestionResult {
                    suggestions: vec![SuggestionEntry::new(self.expected)],
                    start: current_pos as i32,
                    length: first.len() as i32,
                });
            }
        }

        // If the literal matches exactly, continue to next argument
        if *first == self.expected {
            let next_pos = current_pos + first.len() + 1; // +1 for space
            self.executor
                .suggest(&args[1..], next_pos, context, suggestion_ctx)
        } else {
            None
        }
    }
}

/// A builder struct for creating typed command argument executors.
/// Arguments are parsed values (e.g., integers, coordinates, entities) defined by `CommandArgument` implementations.
pub struct CommandParserArgumentBuilder<S, A> {
    name: &'static str,
    argument: Box<dyn CommandArgument<Output = A>>,
    _source: PhantomData<S>,
}

/// Creates a new command argument builder.
pub fn argument<S, A>(
    name: &'static str,
    argument: impl CommandArgument<Output = A> + 'static,
) -> CommandParserArgumentBuilder<S, A> {
    CommandParserArgumentBuilder {
        name,
        argument: Box::new(argument),
        _source: PhantomData,
    }
}

impl<S, A> CommandParserArgumentBuilder<S, A> {
    /// Executes the command argument executor after the argument is parsed.
    pub fn then<E>(self, executor: E) -> CommandParserArgumentExecutor<S, A, E>
    where
        E: CommandParserExecutor<(S, A)>,
    {
        CommandParserArgumentExecutor {
            name: self.name,
            argument: self.argument,
            executor,
            _source: PhantomData,
        }
    }

    /// Executes the command executor after the argument is parsed.
    pub fn executes<E>(
        self,
        executor: E,
    ) -> CommandParserArgumentExecutor<S, A, CommandParserLeafExecutor<(S, A), E>>
    where
        E: CommandExecutor<(S, A)>,
    {
        CommandParserArgumentExecutor {
            name: self.name,
            argument: self.argument,
            executor: CommandParserLeafExecutor {
                executor,
                _source: PhantomData,
            },
            _source: PhantomData,
        }
    }
}

impl<S, A, E> CommandParserExecutor<S> for CommandParserArgumentExecutor<S, A, E>
where
    E: CommandParserExecutor<(S, A)>,
{
    fn execute(
        &self,
        args: &[&str],
        parsed: S,
        context: &mut CommandContext,
        server: &Arc<Server>,
        handler: &dyn CommandHandlerDyn,
    ) -> Option<Result<(), CommandError>> {
        let (args, arg) = self.argument.parse(args, context)?;
        self.executor
            .execute(args, (parsed, arg), context, server, handler)
    }

    fn usage(&self, buffer: &mut Vec<CommandNode>, node_index: i32) -> CommandNodeInfo {
        let node = CommandNode::new_argument(
            self.executor.usage(buffer, node_index),
            self.name,
            self.argument.usage(),
        );
        let result = vec![buffer.len() as i32];
        buffer.push(node);

        CommandNodeInfo::new(result)
    }

    fn suggest(
        &self,
        args: &[&str],
        current_pos: usize,
        context: &mut CommandContext,
        suggestion_ctx: &mut SuggestionContext,
    ) -> Option<SuggestionResult> {
        // Check if this argument uses AskServer suggestions
        let (_, suggestion_type) = self.argument.usage();
        let uses_ask_server = matches!(suggestion_type, Some(SuggestionType::AskServer));

        // Try to parse the current argument
        if let Some((remaining, _)) = self.argument.parse(args, context) {
            // Argument parsed successfully - store parsed value in context for downstream args
            if let Some(parsed_value) = self.argument.parsed_value(args, context) {
                suggestion_ctx.set(self.name, parsed_value);
            }

            // Continue to next argument
            if remaining.is_empty() {
                // No more args
                return None;
            }
            // Calculate position after this argument
            let consumed_len: usize = args
                .iter()
                .take(args.len() - remaining.len())
                .map(|s| s.len() + 1) // +1 for space
                .sum();
            let next_pos = current_pos + consumed_len;
            return self
                .executor
                .suggest(remaining, next_pos, context, suggestion_ctx);
        }

        // Argument didn't parse - if we use AskServer, provide suggestions
        if uses_ask_server {
            let prefix = args.first().copied().unwrap_or("");
            let suggestions = self.argument.suggest(prefix, suggestion_ctx);

            if !suggestions.is_empty() {
                return Some(SuggestionResult {
                    suggestions,
                    start: current_pos as i32,
                    length: prefix.len() as i32,
                });
            }
        }

        None
    }
}

/// Tree node that parses a typed argument and provides the parsed value to the next executor.
/// The argument type `A` is determined by the `CommandArgument` implementation.
pub struct CommandParserArgumentExecutor<S, A, E> {
    name: &'static str,
    argument: Box<dyn CommandArgument<Output = A>>,
    executor: E,
    _source: PhantomData<S>,
}

impl<S, A, E1> CommandParserArgumentExecutor<S, A, E1> {
    /// Executes the command argument executor after the argument is parsed.
    pub fn then<E2>(
        self,
        executor: E2,
    ) -> CommandParserArgumentExecutor<S, A, CommandParserSplitExecutor<(S, A), E1, E2>>
    where
        E2: CommandParserExecutor<(S, A)>,
    {
        CommandParserArgumentExecutor {
            name: self.name,
            argument: self.argument,
            executor: CommandParserSplitExecutor {
                first_executor: self.executor,
                second_executor: executor,
                _source: PhantomData,
            },
            _source: PhantomData,
        }
    }

    /// Executes the command executor after the argument is parsed.
    pub fn executes<E2>(
        self,
        executor: E2,
    ) -> CommandParserArgumentExecutor<S, A, SplitLeafExecutor<(S, A), E1, E2>>
    where
        E2: CommandExecutor<(S, A)>,
    {
        CommandParserArgumentExecutor {
            name: self.name,
            argument: self.argument,
            executor: CommandParserSplitExecutor {
                first_executor: self.executor,
                second_executor: CommandParserLeafExecutor {
                    executor,
                    _source: PhantomData,
                },
                _source: PhantomData,
            },
            _source: PhantomData,
        }
    }
}

type SplitLeafExecutor<S, E1, E2> =
    CommandParserSplitExecutor<S, E1, CommandParserLeafExecutor<S, E2>>;

// ============================================================================
// Dynamic command building support
// ============================================================================

/// A boxed command parser executor that allows dynamic command tree construction.
/// This enables building command trees in loops where the concrete type changes each iteration.
pub type BoxedExecutor<S> = Box<dyn CommandParserExecutor<S> + Send + Sync>;

impl<S> CommandParserExecutor<S> for BoxedExecutor<S> {
    fn execute(
        &self,
        args: &[&str],
        parsed: S,
        context: &mut CommandContext,
        server: &Arc<Server>,
        handler: &dyn CommandHandlerDyn,
    ) -> Option<Result<(), CommandError>> {
        (**self).execute(args, parsed, context, server, handler)
    }

    fn usage(&self, buffer: &mut Vec<CommandNode>, node_index: i32) -> CommandNodeInfo {
        (**self).usage(buffer, node_index)
    }

    fn suggest(
        &self,
        args: &[&str],
        current_pos: usize,
        context: &mut CommandContext,
        suggestion_ctx: &mut SuggestionContext,
    ) -> Option<SuggestionResult> {
        (**self).suggest(args, current_pos, context, suggestion_ctx)
    }
}

/// A dynamic command handler that uses boxed executors for runtime-constructed command trees.
pub struct DynCommandHandler {
    names: &'static [&'static str],
    description: &'static str,
    permission: &'static str,
    executors: Vec<BoxedExecutor<()>>,
}

impl DynCommandHandler {
    /// Creates a new dynamic command handler builder.
    #[must_use]
    pub fn new(
        names: &'static [&'static str],
        description: &'static str,
        permission: &'static str,
    ) -> Self {
        Self {
            names,
            description,
            permission,
            executors: Vec::new(),
        }
    }

    /// Adds an executor branch to this command handler.
    #[must_use]
    pub fn then<E>(mut self, executor: E) -> Self
    where
        E: CommandParserExecutor<()> + Send + Sync + 'static,
    {
        self.executors.push(Box::new(executor));
        self
    }
}

impl CommandHandlerDyn for DynCommandHandler {
    fn names(&self) -> &'static [&'static str] {
        self.names
    }

    fn description(&self) -> &'static str {
        self.description
    }

    fn permission(&self) -> &'static str {
        self.permission
    }

    fn execute(
        &self,
        command_args: &[&str],
        context: &mut CommandContext,
        server: &Arc<Server>,
    ) -> Result<(), CommandError> {
        for executor in &self.executors {
            if let Some(result) = executor.execute(command_args, (), context, server, self) {
                return result;
            }
        }
        Err(CommandError::CommandFailed(Box::new(
            "Invalid Syntax.".into(),
        )))
    }

    fn usage(&self, buffer: &mut Vec<CommandNode>, root_children: &mut Vec<i32>) {
        let node_index = buffer.len();
        buffer.push(CommandNode::new_root()); // Reserve spot
        root_children.push(node_index as i32);

        let mut children = CommandNodeInfo::new(vec![]);
        for executor in &self.executors {
            children = children.chain(executor.usage(buffer, node_index as i32));
        }

        buffer[node_index] = CommandNode::new_literal(children, self.names()[0]);

        for name in self.names().iter().skip(1) {
            root_children.push(buffer.len() as i32);
            buffer.push(CommandNode::new_literal(
                CommandNodeInfo::new_redirect(node_index as i32),
                *name,
            ));
        }
    }

    fn suggest(
        &self,
        args: &[&str],
        args_start_pos: usize,
        context: &mut CommandContext,
    ) -> Option<SuggestionResult> {
        let mut combined: Option<SuggestionResult> = None;
        let suggestion_ctx = SuggestionContext::new();

        for executor in &self.executors {
            if let Some(result) =
                executor.suggest(args, args_start_pos, context, &mut suggestion_ctx.clone())
            {
                match &mut combined {
                    Some(existing)
                        if existing.start == result.start && existing.length == result.length =>
                    {
                        existing.suggestions.extend(result.suggestions);
                    }
                    None => combined = Some(result),
                    _ => {}
                }
            }
        }

        combined
    }
}
