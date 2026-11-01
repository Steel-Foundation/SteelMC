//! Closure-based command trees.
//!
//! ```
//! use steel_extension_api::command::{Arg, CommandBuilder, argument};
//! let greet = CommandBuilder::new("example:greet")
//!     .default_access()
//!     .then(argument("target", Arg::player()).executes(|ctx| {
//!         let target = ctx.player_arg("target")?;
//!         let _ = ctx.server().send_message(target, "Hello!".into());
//!         Ok(1)
//!     }));
//! ```

use super::{
    ArgumentKind, CommandNodeSpec, CommandSpec, NodeKind, ROOT_PARENT, SenderKind,
    api::SuggestionRequest,
};
use crate::{
    ApiResult, BlockPos, BlockState, Command, Commands, Hooks, PlayerId, Position, Server, World,
    callbacks::Table,
};
use stabby::{string::String as ApiString, vec::Vec as ApiVec};

type Executor = dyn Fn(&Invocation<'_, '_>) -> Result<i32, String> + Send + Sync;
type Suggester = dyn Fn(&SuggestionContext<'_, '_>) -> Vec<String> + Send + Sync;

/// Argument parser for an [`argument`] node.
#[derive(Debug, Clone, Copy)]
pub struct Arg {
    kind: ArgumentKind,
    min: f64,
    max: f64,
}

impl Arg {
    const fn of(kind: ArgumentKind) -> Self {
        Self {
            kind,
            min: 0.0,
            max: 0.0,
        }
    }
    /// `true` or `false`.
    #[must_use]
    pub const fn boolean() -> Self {
        Self::of(ArgumentKind::Boolean)
    }
    /// 32-bit integer in `min..=max`.
    #[must_use]
    pub const fn integer(min: i32, max: i32) -> Self {
        Self {
            kind: ArgumentKind::Integer,
            min: min as f64,
            max: max as f64,
        }
    }
    /// 64-bit integer in `min..=max` (bounds beyond ±2^53 lose precision).
    #[must_use]
    #[expect(
        clippy::cast_precision_loss,
        reason = "bounds travel as f64 and are rounded back on the host"
    )]
    pub const fn long(min: i64, max: i64) -> Self {
        Self {
            kind: ArgumentKind::Long,
            min: min as f64,
            max: max as f64,
        }
    }
    /// 64-bit float in `min..=max`.
    #[must_use]
    pub const fn double(min: f64, max: f64) -> Self {
        Self {
            kind: ArgumentKind::Double,
            min,
            max,
        }
    }
    /// One unquoted word.
    #[must_use]
    pub const fn word() -> Self {
        Self::of(ArgumentKind::Word)
    }
    /// A quoted or unquoted phrase.
    #[must_use]
    pub const fn string() -> Self {
        Self::of(ArgumentKind::String)
    }
    /// The rest of the input.
    #[must_use]
    pub const fn greedy() -> Self {
        Self::of(ArgumentKind::Greedy)
    }
    /// One player selector.
    #[must_use]
    pub const fn player() -> Self {
        Self::of(ArgumentKind::Player)
    }
    /// A player selector with one or more results.
    #[must_use]
    pub const fn players() -> Self {
        Self::of(ArgumentKind::Players)
    }
    /// Block coordinates (`~`, `^` supported).
    #[must_use]
    pub const fn block_pos() -> Self {
        Self::of(ArgumentKind::BlockPos)
    }
    /// A block state.
    #[must_use]
    pub const fn block_state() -> Self {
        Self::of(ArgumentKind::BlockState)
    }
    /// An item.
    #[must_use]
    pub const fn item() -> Self {
        Self::of(ArgumentKind::Item)
    }
    /// A loaded world.
    #[must_use]
    pub const fn world() -> Self {
        Self::of(ArgumentKind::World)
    }
}

/// A literal or argument node with its children.
pub struct Node {
    kind: NodeKind,
    name: String,
    argument: Arg,
    permission: String,
    executor: Option<Box<Executor>>,
    suggester: Option<Box<Suggester>>,
    children: Vec<Node>,
}

/// Creates a literal node.
#[must_use]
pub fn literal(name: impl Into<String>) -> Node {
    Node {
        kind: NodeKind::Literal,
        name: name.into(),
        argument: Arg::of(ArgumentKind::None),
        permission: String::new(),
        executor: None,
        suggester: None,
        children: Vec::new(),
    }
}

/// Creates an argument node.
#[must_use]
pub fn argument(name: impl Into<String>, argument: Arg) -> Node {
    Node {
        kind: NodeKind::Argument,
        argument,
        ..literal(name)
    }
}

impl Node {
    /// Adds a child.
    #[must_use]
    pub fn then(mut self, child: Self) -> Self {
        self.children.push(child);
        self
    }

    /// Runs `executor` when the input ends at this node. Errors become red feedback.
    #[must_use]
    pub fn executes(
        mut self,
        executor: impl Fn(&Invocation<'_, '_>) -> Result<i32, String> + Send + Sync + 'static,
    ) -> Self {
        self.executor = Some(Box::new(executor));
        self
    }

    /// Requires players to hold `permission` to see and use this node.
    #[must_use]
    pub fn permission(mut self, permission: impl Into<String>) -> Self {
        self.permission = permission.into();
        self
    }

    /// Completes this argument with `suggester`; results are filtered by the typed prefix.
    #[must_use]
    pub fn suggests(
        mut self,
        suggester: impl Fn(&SuggestionContext<'_, '_>) -> Vec<String> + Send + Sync + 'static,
    ) -> Self {
        self.suggester = Some(Box::new(suggester));
        self
    }
}

/// A command declaration built from closures.
pub struct CommandBuilder {
    key: String,
    permission: String,
    aliases: Vec<String>,
    default_access: bool,
    root: Node,
}

impl CommandBuilder {
    /// Starts a command whose root literal is the path of `key` (`namespace:path`).
    #[must_use]
    pub fn new(key: impl Into<String>) -> Self {
        let key = key.into();
        let path = key.split_once(':').map_or(key.as_str(), |(_, path)| path);
        let root = literal(path);
        Self {
            key,
            permission: String::new(),
            aliases: Vec::new(),
            default_access: false,
            root,
        }
    }

    /// Adds another root literal.
    #[must_use]
    pub fn alias(mut self, alias: impl Into<String>) -> Self {
        self.aliases.push(alias.into());
        self
    }

    /// Uses `permission` for the root instead of `<namespace>.command.<path>`.
    #[must_use]
    pub fn permission(mut self, permission: impl Into<String>) -> Self {
        self.permission = permission.into();
        self
    }

    /// Lets everyone use the command unless its permission is explicitly denied.
    #[must_use]
    pub const fn default_access(mut self) -> Self {
        self.default_access = true;
        self
    }

    /// Runs `executor` for the bare command.
    #[must_use]
    pub fn executes(
        mut self,
        executor: impl Fn(&Invocation<'_, '_>) -> Result<i32, String> + Send + Sync + 'static,
    ) -> Self {
        self.root = self.root.executes(executor);
        self
    }

    /// Adds a child of the root literal.
    #[must_use]
    pub fn then(mut self, child: Node) -> Self {
        self.root = self.root.then(child);
        self
    }
}

struct Handlers {
    executors: Vec<Option<Box<Executor>>>,
    suggesters: Vec<Option<Box<Suggester>>>,
}

static HANDLERS: Table<Handlers> = Table::new();

fn flatten(node: Node, parent: u32, specs: &mut Vec<CommandNodeSpec>, handlers: &mut Handlers) {
    let index = u32::try_from(specs.len()).unwrap_or(u32::MAX);
    specs.push(CommandNodeSpec {
        parent,
        kind: node.kind,
        name: node.name.as_str().into(),
        argument: node.argument.kind,
        min: node.argument.min,
        max: node.argument.max,
        executes: node.executor.is_some(),
        permission: node.permission.as_str().into(),
        suggests: node.suggester.is_some(),
    });
    handlers.executors.push(node.executor);
    handlers.suggesters.push(node.suggester);
    for child in node.children {
        flatten(child, index, specs, handlers);
    }
}

#[stabby::export(callback)]
fn execute_trampoline(command: Command<'_>, node: u32, token: u64) -> ApiResult<i32> {
    let executor = HANDLERS.get(token).and_then(|handlers| {
        usize::try_from(node)
            .ok()
            .and_then(|node| handlers.executors.get(node))
            .and_then(Option::as_deref)
    });
    let Some(executor) = executor else {
        return Err(ApiString::from("command handler is missing")).into();
    };
    executor(&Invocation { command: &command })
        .map_err(ApiString::from)
        .into()
}

#[stabby::export(callback)]
fn suggest_trampoline(request: SuggestionRequest<'_>, node: u32, token: u64) -> ApiVec<ApiString> {
    let suggester = HANDLERS.get(token).and_then(|handlers| {
        usize::try_from(node)
            .ok()
            .and_then(|node| handlers.suggesters.get(node))
            .and_then(Option::as_deref)
    });
    let Some(suggester) = suggester else {
        return ApiVec::new();
    };
    suggester(&SuggestionContext { request: &request })
        .into_iter()
        .map(ApiString::from)
        .collect()
}

impl Commands<'_> {
    /// Registers a closure-based command.
    pub fn register(&mut self, command: CommandBuilder) -> Result<(), String> {
        let mut specs = Vec::new();
        let mut handlers = Handlers {
            executors: Vec::new(),
            suggesters: Vec::new(),
        };
        flatten(command.root, ROOT_PARENT, &mut specs, &mut handlers);
        let token = HANDLERS.insert(Box::new(handlers));
        let spec = CommandSpec {
            key: command.key.as_str().into(),
            permission: command.permission.as_str().into(),
            aliases: command
                .aliases
                .iter()
                .map(|alias| ApiString::from(alias.as_str()))
                .collect(),
            default_access: command.default_access,
            nodes: specs.into_iter().collect(),
        };
        self.register_tree(spec, execute_trampoline, suggest_trampoline, token)
            .match_owned(Ok, |error| Err(error.to_string()))
    }
}

type Registrar = dyn Fn(&mut Commands<'_>) -> Result<(), String> + Send + Sync;
static REGISTRARS: Table<Registrar> = Table::new();
static REGISTRARS_INSTALLED: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

#[stabby::export(callback)]
fn registration_trampoline(mut commands: Commands<'_>) -> ApiResult<()> {
    let mut token = 0;
    while let Some(registrar) = REGISTRARS.get(token) {
        if let Err(error) = registrar(&mut commands) {
            return Err(ApiString::from(error)).into();
        }
        token += 1;
    }
    Ok(()).into()
}

impl Hooks<'_> {
    /// Registers commands with `registrar` when the command registry is built, before it is
    /// frozen. May be called several times during pre-init.
    pub fn on_commands(
        &mut self,
        registrar: impl Fn(&mut Commands<'_>) -> Result<(), String> + Send + Sync + 'static,
    ) -> Result<(), String> {
        REGISTRARS.insert(Box::new(registrar));
        if REGISTRARS_INSTALLED.swap(true, core::sync::atomic::Ordering::Relaxed) {
            return Ok(());
        }
        self.register_commands(registration_trampoline)
            .match_owned(Ok, |error| Err(error.to_string()))
    }
}

/// Converts an ABI result into a `Result` with an owned error message.
macro_rules! owned {
    ($value:expr) => {
        $value.match_owned(Ok, |error| Err(error.to_string()))
    };
}

/// One command execution, with argument readers returning `Result<_, String>` for `?`.
pub struct Invocation<'c, 'a> {
    command: &'c Command<'a>,
}

impl<'a> Invocation<'_, 'a> {
    /// The underlying native handle.
    #[must_use]
    pub const fn raw(&self) -> &Command<'a> {
        self.command
    }
    /// Kind of sender.
    #[must_use]
    pub fn sender(&self) -> SenderKind {
        self.command.sender()
    }
    /// The sending player, or an error for other senders.
    pub fn player(&self) -> Result<PlayerId, String> {
        owned!(self.command.player())
    }
    /// The server.
    #[must_use]
    pub fn server(&self) -> Server<'a> {
        self.command.server()
    }
    /// Execution world.
    #[must_use]
    pub fn world(&self) -> World<'a> {
        self.command.world()
    }
    /// Execution position.
    #[must_use]
    pub fn position(&self) -> Position {
        self.command.position()
    }
    /// Whether the sender holds `node` (`default` when unset); the console always does.
    #[must_use]
    pub fn has_permission(&self, node: &str, default: bool) -> bool {
        self.command.has_permission(node.into(), default)
    }
    /// Boolean argument.
    pub fn boolean(&self, name: &str) -> Result<bool, String> {
        owned!(self.command.boolean(name.into()))
    }
    /// Integer argument.
    pub fn integer(&self, name: &str) -> Result<i32, String> {
        owned!(self.command.integer(name.into()))
    }
    /// Long argument.
    pub fn long(&self, name: &str) -> Result<i64, String> {
        owned!(self.command.long(name.into()))
    }
    /// Double argument.
    pub fn double(&self, name: &str) -> Result<f64, String> {
        owned!(self.command.double(name.into()))
    }
    /// Word, phrase or greedy argument.
    pub fn string(&self, name: &str) -> Result<String, String> {
        owned!(self.command.string(name.into())).map(|value| value.to_string())
    }
    /// Single-player argument.
    pub fn player_arg(&self, name: &str) -> Result<PlayerId, String> {
        owned!(self.command.player_arg(name.into()))
    }
    /// Multi-player argument.
    pub fn players_arg(&self, name: &str) -> Result<Vec<PlayerId>, String> {
        owned!(self.command.players_arg(name.into()))
            .map(|players| players.iter().copied().collect())
    }
    /// Block position argument.
    pub fn block_pos(&self, name: &str) -> Result<BlockPos, String> {
        owned!(self.command.block_pos(name.into()))
    }
    /// Block state argument.
    pub fn block_state(&self, name: &str) -> Result<BlockState, String> {
        owned!(self.command.block_state(name.into()))
    }
    /// Item argument key.
    pub fn item(&self, name: &str) -> Result<String, String> {
        owned!(self.command.item(name.into())).map(|value| value.to_string())
    }
    /// World argument key.
    pub fn world_arg(&self, name: &str) -> Result<String, String> {
        owned!(self.command.world_arg(name.into())).map(|value| value.to_string())
    }
    /// Sends feedback (JSON or plain text).
    pub fn send(&self, text: &str) {
        self.command.send(text.into());
    }
    /// Sends feedback written with `&` color codes.
    pub fn send_legacy(&self, text: &str) {
        self.command
            .send(crate::text::legacy(text).to_json().as_str().into());
    }
    /// Sends red failure feedback.
    pub fn fail(&self, text: &str) {
        self.command.send_failure(text.into());
    }
}

/// Completion context for [`Node::suggests`].
pub struct SuggestionContext<'r, 'a> {
    request: &'r SuggestionRequest<'a>,
}

impl SuggestionContext<'_, '_> {
    /// Partial input being completed.
    #[must_use]
    pub fn remaining(&self) -> String {
        self.request.remaining().to_string()
    }
    /// UUID of the completing player, if any.
    #[must_use]
    pub fn player_uuid(&self) -> Option<crate::players::Uuid> {
        self.request.player_uuid().into()
    }
    /// Names of players visible to the completing source.
    #[must_use]
    pub fn player_names(&self) -> Vec<String> {
        self.request
            .player_names()
            .iter()
            .map(ToString::to_string)
            .collect()
    }
}
