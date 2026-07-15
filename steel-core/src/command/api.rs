//! Public startup command-registration API.

use std::{error::Error, fmt, sync::Arc};

use glam::DVec3;
use steel_protocol::packets::game::{
    ArgumentType as ProtocolArgumentType, SuggestionType as ProtocolSuggestionType,
};
use steel_utils::{DowncastType, Identifier};
use text_components::TextComponent;

use super::{
    brigadier::{
        ArgumentType, CommandNodeBuilder, CommandRequirement, CommandSyntaxError,
        CommandSyntaxErrorKind, ReaderCursor, StringReader, SuggestionsBuilder,
    },
    execution::{
        CommandArgumentSource, CommandPermissionSource, CommandResultSuspension,
        CommandResultSuspensionPoll, CommandSource as InternalCommandSource,
        CommandSuspensionOrder, SteelArgumentParser, SteelArgumentSuggestionContext,
        SteelArgumentType, SteelCommandContext, SteelCommandRuntime,
    },
    registration::{
        CommandDispatcherBuilder, CommandRegistration as InternalCommandRegistration,
        CommandRegistrationError as InternalCommandRegistrationError,
    },
};
use crate::{
    entity::SharedEntity,
    permission::{PermissionExpr, PermissionState},
    player::Player,
    server::Server,
    world::World,
};

/// A command declaration collected before the server constructs its dispatcher atomically.
pub struct CommandRegistration {
    inner: InternalCommandRegistration<InternalCommandSource>,
}

impl CommandRegistration {
    /// Declares a command whose literal root must match the path of `id`.
    pub fn new(id: Identifier, factory: impl FnOnce() -> CommandNode + Send + 'static) -> Self {
        Self {
            inner: InternalCommandRegistration::new(id, move |_| factory().inner),
        }
    }

    /// Adds an unqualified alias owned by this command.
    #[must_use]
    pub fn alias(mut self, alias: impl Into<Box<str>>) -> Self {
        self.inner = self.inner.alias(alias);
        self
    }

    /// Allows an unset root permission while still respecting an explicit deny.
    #[must_use]
    pub fn default_access(mut self) -> Self {
        self.inner = self.inner.default_access();
        self
    }

    /// Replaces the permission expression derived from the command ID.
    #[must_use]
    pub fn permission(mut self, permission: PermissionExpr) -> Self {
        self.inner = self.inner.permission(permission);
        self
    }

    /// Allows a literal path through a permission derived from the command ID.
    #[must_use]
    pub fn subcommand_permission<I, T>(mut self, path: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<Box<str>>,
    {
        self.inner = self.inner.subcommand_permission(path);
        self
    }
}

/// Additional command declarations supplied before server startup.
pub struct CommandRegistry {
    inner: CommandDispatcherBuilder<InternalCommandSource>,
}

impl CommandRegistry {
    /// Creates an empty extension registry. Built-in commands are added separately by the server.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: CommandDispatcherBuilder::new(),
        }
    }

    /// Declares a non-command permission for discovery and autocomplete.
    pub fn declare_permission(
        &mut self,
        permission: impl Into<String>,
    ) -> Result<&mut Self, CommandRegistrationError> {
        self.inner
            .declare_permission(permission)
            .map_err(CommandRegistrationError::from)?;
        Ok(self)
    }

    /// Adds one command declaration to this startup registry.
    pub fn register(
        &mut self,
        registration: CommandRegistration,
    ) -> Result<&mut Self, CommandRegistrationError> {
        self.inner
            .register(registration.inner)
            .map_err(CommandRegistrationError::from)?;
        Ok(self)
    }

    pub(crate) fn into_inner(self) -> CommandDispatcherBuilder<InternalCommandSource> {
        self.inner
    }
}

impl Default for CommandRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// A command registration failed validation or collided by stable owner ID.
#[derive(Debug)]
pub struct CommandRegistrationError {
    inner: InternalCommandRegistrationError,
}

impl From<InternalCommandRegistrationError> for CommandRegistrationError {
    fn from(inner: InternalCommandRegistrationError) -> Self {
        Self { inner }
    }
}

impl fmt::Display for CommandRegistrationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.inner.fmt(formatter)
    }
}

impl Error for CommandRegistrationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.inner)
    }
}

/// One literal or argument node in a server command declaration.
pub struct CommandNode {
    inner: CommandNodeBuilder<InternalCommandSource, SteelCommandRuntime>,
}

impl CommandNode {
    /// Creates a literal node.
    #[must_use]
    pub fn literal(name: impl Into<Box<str>>) -> Self {
        Self {
            inner: CommandNodeBuilder::literal(name),
        }
    }

    /// Creates a typed argument node.
    #[must_use]
    pub fn argument(name: impl Into<Box<str>>, argument: CommandArgument) -> Self {
        Self {
            inner: CommandNodeBuilder::argument(name, argument.inner),
        }
    }

    /// Adds a child while preserving declaration order.
    #[must_use]
    pub fn then(mut self, child: Self) -> Self {
        self.inner = self.inner.then(child.inner);
        self
    }

    /// Attaches a synchronous terminal executor.
    #[must_use]
    pub fn executes(
        mut self,
        executor: impl for<'context> Fn(&CommandContext<'context>) -> Result<i32, CommandError>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        self.inner = self.inner.executes(move |context| {
            executor(&CommandContext { inner: context }).map_err(CommandError::into_inner)
        });
        self
    }

    /// Attaches a terminal executor whose result is produced across server ticks.
    #[must_use]
    pub fn executes_suspended<T>(
        mut self,
        executor: impl for<'context> Fn(&CommandContext<'context>) -> Result<T, CommandError>
        + Send
        + Sync
        + 'static,
    ) -> Self
    where
        T: SuspendedCommand,
    {
        self.inner = self.inner.executes_suspended(move |context| {
            executor(&CommandContext { inner: context })
                .map(ExternalCommandSuspension)
                .map_err(CommandError::into_inner)
        });
        self
    }

    /// Adds a non-permission source requirement used for tree visibility and execution.
    #[must_use]
    pub fn requires(
        mut self,
        requirement: impl for<'source> Fn(CommandSource<'source>) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.inner = self
            .inner
            .requires(CommandRequirement::contextual(move |source| {
                requirement(CommandSource { inner: source })
            }));
        self
    }
}

/// Creates a literal command node.
#[must_use]
pub fn literal(name: impl Into<Box<str>>) -> CommandNode {
    CommandNode::literal(name)
}

/// Creates a typed argument command node.
#[must_use]
pub fn argument(name: impl Into<Box<str>>, argument: CommandArgument) -> CommandNode {
    CommandNode::argument(name, argument)
}

/// A parsed command invocation exposed to an extension executor.
#[derive(Clone, Copy)]
pub struct CommandContext<'context> {
    inner: &'context SteelCommandContext<InternalCommandSource>,
}

impl<'context> CommandContext<'context> {
    /// Returns the execution source.
    #[must_use]
    pub fn source(self) -> CommandSource<'context> {
        CommandSource {
            inner: self.inner.source(),
        }
    }

    /// Returns a parsed custom value by its deterministic concrete type key.
    #[must_use]
    pub fn value<T: DowncastType>(self, name: &str) -> Option<&'context T> {
        self.inner.argument(name)?.downcast_ref::<T>()
    }

    #[must_use]
    /// Returns a parsed boolean, or `None` when the named argument has another type.
    pub fn boolean(self, name: &str) -> Option<bool> {
        self.inner.boolean(name)
    }

    #[must_use]
    /// Returns a parsed 32-bit integer.
    pub fn integer(self, name: &str) -> Option<i32> {
        self.inner.integer(name)
    }

    #[must_use]
    /// Returns a parsed 64-bit integer.
    pub fn long(self, name: &str) -> Option<i64> {
        self.inner.long(name)
    }

    #[must_use]
    /// Returns a parsed 32-bit floating-point value.
    pub fn float(self, name: &str) -> Option<f32> {
        self.inner.float(name)
    }

    #[must_use]
    /// Returns a parsed 64-bit floating-point value.
    pub fn double(self, name: &str) -> Option<f64> {
        self.inner.double(name)
    }

    #[must_use]
    /// Returns a parsed word, phrase, or greedy string.
    pub fn string(self, name: &str) -> Option<&'context str> {
        self.inner.string(name)
    }

    /// Returns a parsed configured domain name.
    #[must_use]
    pub fn domain(self, name: &str) -> Option<&'context str> {
        self.inner.domain(name)
    }

    /// Resolves a parsed loaded-world argument against the current source domain.
    pub fn world(self, name: &str) -> Result<Arc<World>, CommandError> {
        let Some(world) = self.inner.world_argument(name) else {
            return Err(CommandError::from(format!(
                "missing parsed world argument '{name}'"
            )));
        };
        world
            .resolve(self.inner.source())
            .map_err(CommandError::from)
    }

    /// Resolves a player selector and requires at least one result.
    pub fn players(self, name: &str) -> Result<Vec<Arc<Player>>, CommandError> {
        self.inner.players(name).map_err(CommandError::from)
    }

    /// Resolves a single player selector.
    pub fn player(self, name: &str) -> Result<Arc<Player>, CommandError> {
        self.inner.player(name).map_err(CommandError::from)
    }

    /// Resolves an entity selector and requires at least one result.
    pub fn entities(self, name: &str) -> Result<Vec<SharedEntity>, CommandError> {
        self.inner.entities(name).map_err(CommandError::from)
    }

    /// Resolves a single entity selector.
    pub fn entity(self, name: &str) -> Result<SharedEntity, CommandError> {
        self.inner.entity(name).map_err(CommandError::from)
    }
}

/// Read-only execution source and feedback operations available to extension commands.
#[derive(Clone, Copy)]
pub struct CommandSource<'source> {
    inner: &'source InternalCommandSource,
}

impl<'source> CommandSource<'source> {
    /// Returns the current execution player, if any.
    #[must_use]
    pub const fn player(self) -> Option<&'source Arc<Player>> {
        self.inner.player()
    }

    /// Returns the current execution entity, if any.
    #[must_use]
    pub const fn entity(self) -> Option<&'source SharedEntity> {
        self.inner.entity()
    }

    /// Returns the current execution world.
    #[must_use]
    pub const fn world(self) -> &'source Arc<World> {
        self.inner.world()
    }

    /// Returns the owning server.
    #[must_use]
    pub const fn server(self) -> &'source Arc<Server> {
        self.inner.server()
    }

    /// Returns the current execution position.
    #[must_use]
    pub const fn position(self) -> DVec3 {
        self.inner.position()
    }

    /// Returns the current execution yaw and pitch.
    #[must_use]
    pub const fn rotation(self) -> (f32, f32) {
        self.inner.rotation()
    }

    /// Resolves a permission against the authorization snapshot captured at command start.
    #[must_use]
    pub fn permission_state(self, permission: &PermissionExpr) -> Option<PermissionState> {
        CommandPermissionSource::permission_state(self.inner, permission)
    }

    /// Sends success feedback and optionally applies vanilla administrative broadcasting.
    pub fn send_success(self, message: &TextComponent, broadcast_to_admins: bool) {
        self.inner.send_success(message, broadcast_to_admins);
    }

    /// Sends red failure feedback to the original sender.
    pub fn send_failure(self, message: TextComponent) {
        self.inner.send_failure(message);
    }
}

/// A command parsing or execution error with vanilla-style feedback.
#[derive(Debug)]
pub struct CommandError {
    inner: CommandSyntaxError,
}

impl CommandError {
    /// Creates an execution error from a rich feedback component.
    #[must_use]
    pub fn new(message: impl Into<TextComponent>) -> Self {
        Self {
            inner: CommandSyntaxError::dynamic(message),
        }
    }

    fn into_inner(self) -> CommandSyntaxError {
        self.inner
    }
}

impl From<CommandSyntaxError> for CommandError {
    fn from(inner: CommandSyntaxError) -> Self {
        Self { inner }
    }
}

impl From<String> for CommandError {
    fn from(message: String) -> Self {
        Self::new(TextComponent::plain(message))
    }
}

impl From<&str> for CommandError {
    fn from(message: &str) -> Self {
        Self::new(TextComponent::plain(message.to_owned()))
    }
}

impl From<TextComponent> for CommandError {
    fn from(message: TextComponent) -> Self {
        Self::new(message)
    }
}

impl fmt::Display for CommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.inner.fmt(formatter)
    }
}

impl Error for CommandError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.inner)
    }
}

/// An argument parser accepted by a public command node.
pub struct CommandArgument {
    inner: SteelArgumentType,
}

impl CommandArgument {
    /// Creates a lowercase boolean argument.
    #[must_use]
    pub fn boolean() -> Self {
        Self::primitive(ArgumentType::bool())
    }

    /// Creates a bounded signed 32-bit integer argument.
    #[must_use]
    pub fn integer(minimum: i32, maximum: i32) -> Self {
        Self::primitive(ArgumentType::integer(minimum, maximum))
    }

    /// Creates a bounded signed 64-bit integer argument.
    #[must_use]
    pub fn long(minimum: i64, maximum: i64) -> Self {
        Self::primitive(ArgumentType::long(minimum, maximum))
    }

    /// Creates a bounded 32-bit floating-point argument.
    #[must_use]
    pub fn float(minimum: f32, maximum: f32) -> Self {
        Self::primitive(ArgumentType::float(minimum, maximum))
    }

    /// Creates a bounded 64-bit floating-point argument.
    #[must_use]
    pub fn double(minimum: f64, maximum: f64) -> Self {
        Self::primitive(ArgumentType::double(minimum, maximum))
    }

    /// Creates a single unquoted word argument.
    #[must_use]
    pub fn word() -> Self {
        Self::primitive(ArgumentType::word())
    }

    /// Creates a quoted or unquoted phrase argument.
    #[must_use]
    pub fn string() -> Self {
        Self::primitive(ArgumentType::string())
    }

    /// Creates an argument that consumes the remaining command input.
    #[must_use]
    pub fn greedy_string() -> Self {
        Self::primitive(ArgumentType::greedy_string())
    }

    /// Creates a single-entity selector argument.
    #[must_use]
    pub fn entity() -> Self {
        Self {
            inner: SteelArgumentType::entity(),
        }
    }

    /// Creates a multiple-entity selector argument.
    #[must_use]
    pub fn entities() -> Self {
        Self {
            inner: SteelArgumentType::entities(),
        }
    }

    /// Creates a single-player selector argument.
    #[must_use]
    pub fn player() -> Self {
        Self {
            inner: SteelArgumentType::player(),
        }
    }

    /// Creates a multiple-player selector argument.
    #[must_use]
    pub fn players() -> Self {
        Self {
            inner: SteelArgumentType::players(),
        }
    }

    /// Creates a configured Steel domain argument.
    #[must_use]
    pub fn domain() -> Self {
        Self {
            inner: SteelArgumentType::domain(),
        }
    }

    /// Creates a loaded-world argument.
    #[must_use]
    pub fn world() -> Self {
        Self {
            inner: SteelArgumentType::world(),
        }
    }

    /// Erases a keyed extension parser without using `Any` or `TypeId`.
    #[must_use]
    pub fn custom<P>(parser: P) -> Self
    where
        P: CommandArgumentParser,
    {
        Self {
            inner: SteelArgumentType::new(parser),
        }
    }

    fn primitive(argument: ArgumentType) -> Self {
        Self {
            inner: SteelArgumentType::from(argument),
        }
    }
}

/// Typed, deterministically keyed parser contract for extension arguments.
pub trait CommandArgumentParser:
    DowncastType + fmt::Debug + PartialEq + Send + Sync + 'static
{
    /// Concrete keyed value retained in the parsed command context.
    type Value: DowncastType + fmt::Debug + Send + Sync + 'static;

    /// Parses one value from the reader's current cursor.
    fn parse(
        &self,
        reader: &mut CommandReader<'_, '_>,
        source: CommandParserSource<'_>,
    ) -> Result<Self::Value, CommandError>;

    /// Adds context-aware completions for a partially entered value.
    fn list_suggestions(
        &self,
        _context: CommandSuggestionContext<'_>,
        _suggestions: &mut CommandSuggestions<'_, '_>,
    ) {
    }

    /// Returns the vanilla command-tree parser and optional server suggestion provider.
    fn protocol_argument(&self) -> (ProtocolArgumentType, Option<ProtocolSuggestionType>);
}

impl<P> SteelArgumentParser for P
where
    P: CommandArgumentParser,
{
    type Value = P::Value;

    fn parse(
        &self,
        reader: &mut StringReader<'_>,
        source: &dyn CommandArgumentSource,
    ) -> Result<Self::Value, CommandSyntaxError> {
        CommandArgumentParser::parse(
            self,
            &mut CommandReader { inner: reader },
            CommandParserSource { inner: source },
        )
        .map_err(CommandError::into_inner)
    }

    fn list_suggestions(
        &self,
        context: &dyn SteelArgumentSuggestionContext,
        builder: &mut SuggestionsBuilder<'_>,
    ) {
        CommandArgumentParser::list_suggestions(
            self,
            CommandSuggestionContext { inner: context },
            &mut CommandSuggestions { inner: builder },
        );
    }

    fn protocol_argument(&self) -> (ProtocolArgumentType, Option<ProtocolSuggestionType>) {
        CommandArgumentParser::protocol_argument(self)
    }
}

/// Cursor checkpoint for a public custom argument reader.
#[derive(Clone, Copy)]
pub struct CommandReaderCursor(ReaderCursor);

/// Cursor-aware input available to custom argument parsers.
pub struct CommandReader<'reader, 'input> {
    inner: &'reader mut StringReader<'input>,
}

impl<'input> CommandReader<'_, 'input> {
    /// Returns the complete command input.
    #[must_use]
    pub const fn input(&self) -> &'input str {
        self.inner.input()
    }

    /// Returns the current UTF-8 byte cursor.
    #[must_use]
    pub const fn cursor(&self) -> usize {
        self.inner.byte_cursor()
    }

    /// Returns the unconsumed input.
    #[must_use]
    pub fn remaining(&self) -> &'input str {
        self.inner.remaining()
    }

    /// Peeks at the next Unicode scalar without consuming it.
    #[must_use]
    pub fn peek(&self) -> Option<char> {
        self.inner.peek()
    }

    /// Consumes and returns the next Unicode scalar.
    pub fn read(&mut self) -> Option<char> {
        self.inner.read()
    }

    /// Consumes Java-compatible command whitespace.
    pub fn skip_whitespace(&mut self) {
        self.inner.skip_whitespace();
    }

    /// Reads one unquoted Brigadier string.
    pub fn read_unquoted_string(&mut self) -> &'input str {
        self.inner.read_unquoted_string()
    }

    /// Reads one quoted or unquoted Brigadier string.
    pub fn read_string(&mut self) -> Result<String, CommandError> {
        self.inner.read_string().map_err(CommandError::from)
    }

    /// Reads a signed 32-bit integer.
    pub fn read_integer(&mut self) -> Result<i32, CommandError> {
        self.inner.read_int().map_err(CommandError::from)
    }

    /// Reads a signed 64-bit integer.
    pub fn read_long(&mut self) -> Result<i64, CommandError> {
        self.inner.read_long().map_err(CommandError::from)
    }

    /// Reads a 32-bit floating-point value.
    pub fn read_float(&mut self) -> Result<f32, CommandError> {
        self.inner.read_float().map_err(CommandError::from)
    }

    /// Reads a 64-bit floating-point value.
    pub fn read_double(&mut self) -> Result<f64, CommandError> {
        self.inner.read_double().map_err(CommandError::from)
    }

    /// Reads a lowercase boolean.
    pub fn read_boolean(&mut self) -> Result<bool, CommandError> {
        self.inner.read_boolean().map_err(CommandError::from)
    }

    /// Consumes one required symbol.
    pub fn expect(&mut self, expected: char) -> Result<(), CommandError> {
        self.inner.expect(expected).map_err(CommandError::from)
    }

    /// Captures the current cursor for speculative parsing.
    #[must_use]
    pub const fn checkpoint(&self) -> CommandReaderCursor {
        CommandReaderCursor(self.inner.checkpoint())
    }

    /// Restores a previously captured cursor.
    pub const fn restore(&mut self, checkpoint: CommandReaderCursor) {
        self.inner.restore(checkpoint.0);
    }

    /// Creates an error carrying the reader's current Brigadier context.
    #[must_use]
    pub fn error(&self, message: impl Into<TextComponent>) -> CommandError {
        CommandError::from(
            self.inner
                .error(CommandSyntaxErrorKind::Dynamic(Box::new(message.into()))),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::CommandReader;
    use crate::command::brigadier::StringReader;

    #[test]
    fn command_reader_cursor_is_a_utf8_byte_offset() {
        let mut inner = StringReader::new("é🦀");
        let mut reader = CommandReader { inner: &mut inner };

        assert_eq!(reader.read(), Some('é'));
        assert_eq!(reader.cursor(), 2);
        assert_eq!(reader.read(), Some('🦀'));
        assert_eq!(reader.cursor(), 6);
        assert_eq!(&reader.input()[..reader.cursor()], "é🦀");
    }
}

/// Read-only source facts available while a custom argument is parsed.
#[derive(Clone, Copy)]
pub struct CommandParserSource<'source> {
    inner: &'source dyn CommandArgumentSource,
}

impl CommandParserSource<'_> {
    /// Returns configured domain names.
    #[must_use]
    pub fn domain_names(&self) -> Vec<&str> {
        self.inner.domain_names()
    }

    /// Returns world names visible to the current command source.
    #[must_use]
    pub fn world_names(&self) -> Vec<String> {
        self.inner.command_world_names()
    }

    /// Returns player names visible to selectors in the source domain.
    #[must_use]
    pub fn player_names(&self) -> Vec<String> {
        self.inner.selector_player_names()
    }

    /// Returns configured permission group names.
    #[must_use]
    pub fn permission_group_names(&self) -> Vec<String> {
        self.inner.permission_group_names()
    }
}

/// Prior parsed values and source facts available to custom suggestions.
#[derive(Clone, Copy)]
pub struct CommandSuggestionContext<'context> {
    inner: &'context dyn SteelArgumentSuggestionContext,
}

impl<'context> CommandSuggestionContext<'context> {
    /// Returns source facts for context-aware completion.
    #[must_use]
    pub fn source(&self) -> CommandParserSource<'context> {
        CommandParserSource {
            inner: self.inner.source(),
        }
    }

    /// Returns a previously parsed custom value by deterministic concrete type key.
    #[must_use]
    pub fn value<T: DowncastType>(&self, name: &str) -> Option<&'context T> {
        self.inner.argument(name)?.downcast_ref::<T>()
    }
}

/// Completion builder available to custom argument parsers.
pub struct CommandSuggestions<'builder, 'input> {
    inner: &'builder mut SuggestionsBuilder<'input>,
}

impl CommandSuggestions<'_, '_> {
    /// Returns the partial input being completed.
    #[must_use]
    pub fn remaining(&self) -> &str {
        self.inner.remaining()
    }

    /// Adds a textual completion.
    pub fn suggest(&mut self, text: impl Into<Box<str>>) {
        self.inner.suggest(text);
    }

    /// Adds a textual completion with a rich tooltip.
    pub fn suggest_with_tooltip(&mut self, text: impl Into<Box<str>>, tooltip: TextComponent) {
        self.inner.suggest_with_tooltip(text, tooltip);
    }

    /// Adds an integer completion with Brigadier's numeric ordering.
    pub fn suggest_integer(&mut self, value: i32) {
        self.inner.suggest_integer(value);
    }
}

/// Poll result for a public command whose result is produced across ticks.
pub enum SuspendedCommandPoll {
    /// The command remains suspended.
    Pending,
    /// The command completed with a result or execution error.
    Ready(Result<i32, CommandError>),
}

/// Cross-tick command work owned and cancelled by Steel's command scheduler.
pub trait SuspendedCommand: Send + 'static {
    /// Returns which later top-level commands must wait for this work.
    fn order(&self) -> CommandSuspensionOrder {
        CommandSuspensionOrder::Source
    }

    /// Polls the command once from the server tick.
    fn poll(&mut self) -> SuspendedCommandPoll;

    /// Cancels retained external work when the command or server stops.
    fn cancel(&mut self) {}
}

struct ExternalCommandSuspension<T>(T);

impl<T> CommandResultSuspension for ExternalCommandSuspension<T>
where
    T: SuspendedCommand,
{
    fn order(&self) -> CommandSuspensionOrder {
        self.0.order()
    }

    fn poll(&mut self) -> CommandResultSuspensionPoll {
        match self.0.poll() {
            SuspendedCommandPoll::Pending => CommandResultSuspensionPoll::Pending,
            SuspendedCommandPoll::Ready(result) => {
                CommandResultSuspensionPoll::Ready(result.map_err(CommandError::into_inner))
            }
        }
    }

    fn cancel(&mut self) {
        self.0.cancel();
    }
}
