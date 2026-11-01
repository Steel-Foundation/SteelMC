//! Typed command trees with arguments, permissions and suggestions.
//!
//! Most extensions use [`CommandBuilder`] from [`Hooks::register_command_trees`]; the native form
//! is a flat [`CommandSpec`] registered with [`Commands::register_tree`].
//!
//! Every command's root permission derives from its key: `essentials:home` requires
//! `essentials.command.home` unless the command uses default access. Node permissions are
//! additional dotted keys checked for players; the console always passes them.
//!
//! [`Hooks::register_command_trees`]: crate::Hooks::register_command_trees
//! [`Commands::register_tree`]: crate::Commands::register_tree

mod builder;

pub use builder::{Arg, CommandBuilder, Invocation, Node, SuggestionContext, argument, literal};

use stabby::{string::String as ApiString, vec::Vec as ApiVec};

/// Parent index meaning "no parent" (the root node).
pub const ROOT_PARENT: u32 = u32::MAX;

/// Whether a node matches a fixed word or parses an argument.
#[stabby::stabby]
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    /// A fixed word.
    Literal,
    /// A typed argument.
    Argument,
}

/// Argument parser of an argument node.
#[stabby::stabby]
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgumentKind {
    /// Unused for literals.
    None,
    /// `true` or `false`.
    Boolean,
    /// Bounded 32-bit integer.
    Integer,
    /// Bounded 64-bit integer.
    Long,
    /// Bounded 64-bit float.
    Double,
    /// One unquoted word.
    Word,
    /// A quoted or unquoted phrase.
    String,
    /// The rest of the input.
    Greedy,
    /// One player selector.
    Player,
    /// A player selector with one or more results.
    Players,
    /// Absolute, relative or local block coordinates.
    BlockPos,
    /// A block state such as `minecraft:oak_log[axis=x]`.
    BlockState,
    /// An item stack such as `minecraft:diamond`.
    Item,
    /// A loaded world.
    World,
}

/// One node of a [`CommandSpec`].
#[stabby::stabby]
#[derive(Debug, Clone)]
pub struct CommandNodeSpec {
    /// Literal word or argument name.
    pub name: ApiString,
    /// Dotted permission players need to see and use this node; empty for none.
    pub permission: ApiString,
    /// Lower bound for numeric arguments.
    pub min: f64,
    /// Upper bound for numeric arguments.
    pub max: f64,
    /// Index of the parent node, or [`ROOT_PARENT`] for the root.
    pub parent: u32,
    /// Literal or argument.
    pub kind: NodeKind,
    /// Argument parser; `None` for literals.
    pub argument: ArgumentKind,
    /// Whether input ending at this node runs the command callback.
    pub executes: bool,
    /// Whether argument completions come from the suggestion callback.
    pub suggests: bool,
}

/// A complete command declaration.
#[stabby::stabby]
#[derive(Debug, Clone)]
pub struct CommandSpec {
    /// Namespaced key whose path is the root literal, such as `essentials:home`.
    pub key: ApiString,
    /// Root permission replacing the derived `<namespace>.command.<path>`; empty to derive.
    /// Needed when the path is not a valid permission segment, such as `WorldEdit`'s `/set`.
    pub permission: ApiString,
    /// Additional root literals.
    pub aliases: ApiVec<ApiString>,
    /// Whether the root is usable unless its permission is explicitly denied.
    pub default_access: bool,
    /// Nodes; the first is the root literal and parents precede their children.
    pub nodes: ApiVec<CommandNodeSpec>,
}

/// Who runs a command.
#[stabby::stabby]
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SenderKind {
    /// A connected player.
    Player,
    /// The server console.
    Console,
    /// A command block, entity or other source.
    Other,
}

#[stabby::host_api]
/// Command registration and invocation bindings.
pub mod api {
    use super::{CommandSpec, SenderKind};
    use crate::{
        ApiResult, BlockPos, BlockState, Command, Commands, PlayerId, Position, World,
        players::Uuid,
    };
    use stabby::{
        option::Option as StableOption, str::Str, string::String as ApiString, vec::Vec as ApiVec,
    };

    #[cfg(feature = "host")]
    use crate::host::commands::{self as host, SuggestionData};

    /// Context for completing an argument.
    #[opaque]
    pub struct SuggestionRequest<'a>(&'a SuggestionData<'a>);

    impl<'a> Commands<'a> {
        /// Registers a validated command tree. `execute(command, node, token)` runs for executing
        /// nodes; `suggest(request, node, token)` returns completions for suggesting nodes.
        pub fn register_tree(
            &mut self,
            spec: CommandSpec,
            execute: extern "C" fn(Command<'_>, u32, u64) -> ApiResult<i32>,
            suggest: extern "C" fn(SuggestionRequest<'_>, u32, u64) -> ApiVec<ApiString>,
            token: u64,
        ) -> ApiResult<()> {
            host::register_tree(self.host_mut(), &spec, execute, suggest, token)
                .map_err(ApiString::from)
                .into()
        }
    }

    impl<'a> Command<'a> {
        /// Kind of command sender.
        #[must_use]
        pub fn sender(&self) -> SenderKind {
            host::sender(self.host())
        }

        /// Execution world.
        #[must_use]
        pub fn world(&self) -> World<'a> {
            World::from_host(self.host().source().world())
        }

        /// Execution position.
        #[must_use]
        pub fn position(&self) -> Position {
            crate::host::events::position(self.host().source().position())
        }

        /// Execution yaw and pitch.
        #[must_use]
        pub fn yaw(&self) -> f32 {
            self.host().source().rotation().0
        }

        /// Execution pitch.
        #[must_use]
        pub fn pitch(&self) -> f32 {
            self.host().source().rotation().1
        }

        /// Whether the sender holds `node`; the console always does.
        #[must_use]
        pub fn has_permission(&self, node: Str<'_>, default: bool) -> bool {
            host::has_permission(self.host(), node.as_str(), default)
        }

        /// Parsed boolean argument.
        pub fn boolean(&self, name: Str<'_>) -> ApiResult<bool> {
            (self.host().boolean(name.as_str()))
                .map_err(host::read_error)
                .into()
        }

        /// Parsed integer argument.
        pub fn integer(&self, name: Str<'_>) -> ApiResult<i32> {
            (self.host().integer(name.as_str()))
                .map_err(host::read_error)
                .into()
        }

        /// Parsed long argument.
        pub fn long(&self, name: Str<'_>) -> ApiResult<i64> {
            (self.host().long(name.as_str()))
                .map_err(host::read_error)
                .into()
        }

        /// Parsed double argument.
        pub fn double(&self, name: Str<'_>) -> ApiResult<f64> {
            (self.host().double(name.as_str()))
                .map_err(host::read_error)
                .into()
        }

        /// Parsed word, phrase or greedy argument.
        pub fn string(&self, name: Str<'_>) -> ApiResult<ApiString> {
            (self.host().string(name.as_str()).map(ApiString::from))
                .map_err(host::read_error)
                .into()
        }

        /// Resolved single-player selector.
        pub fn player_arg(&self, name: Str<'_>) -> ApiResult<PlayerId> {
            (self
                .host()
                .player(name.as_str())
                .map(|player| crate::host::events::player_id(&player)))
            .map_err(host::read_error)
            .into()
        }

        /// Resolved multi-player selector.
        pub fn players_arg(&self, name: Str<'_>) -> ApiResult<ApiVec<PlayerId>> {
            (self.host().players(name.as_str()).map(|players| {
                players
                    .iter()
                    .map(|player| crate::host::events::player_id(player))
                    .collect()
            }))
            .map_err(host::read_error)
            .into()
        }

        /// Resolved block position argument.
        pub fn block_pos(&self, name: Str<'_>) -> ApiResult<BlockPos> {
            (self
                .host()
                .block_pos(name.as_str())
                .map(crate::host::events::block_pos))
            .map_err(host::read_error)
            .into()
        }

        /// Parsed block state argument.
        pub fn block_state(&self, name: Str<'_>) -> ApiResult<BlockState> {
            (self
                .host()
                .block_state(name.as_str())
                .map(|state| BlockState { id: state.0 }))
            .map_err(host::read_error)
            .into()
        }

        /// Item key of an item argument.
        pub fn item(&self, name: Str<'_>) -> ApiResult<ApiString> {
            (self
                .host()
                .item_stack(name.as_str())
                .map(|stack| ApiString::from(stack.item().key.to_string())))
            .map_err(host::read_error)
            .into()
        }

        /// Key of a world argument.
        pub fn world_arg(&self, name: Str<'_>) -> ApiResult<ApiString> {
            (self
                .host()
                .world(name.as_str())
                .map(|world| ApiString::from(world.key.to_string())))
            .map_err(host::read_error)
            .into()
        }

        /// Sends success feedback (JSON or plain text).
        pub fn send(&self, text: Str<'_>) {
            self.host()
                .source()
                .send_success(&crate::host::events::parse_text(text.as_str()), false);
        }

        /// Sends red failure feedback (JSON or plain text).
        pub fn send_failure(&self, text: Str<'_>) {
            self.host()
                .source()
                .send_failure(crate::host::events::parse_text(text.as_str()));
        }
    }

    impl<'a> SuggestionRequest<'a> {
        /// Partial input being completed.
        #[must_use]
        pub fn remaining(&self) -> ApiString {
            self.host().remaining.as_str().into()
        }

        /// UUID of the completing player, if any.
        #[must_use]
        pub fn player_uuid(&self) -> StableOption<Uuid> {
            self.host()
                .context
                .source()
                .player_uuid()
                .map(|uuid| Uuid {
                    bytes: *uuid.as_bytes(),
                })
                .into()
        }

        /// Names of players visible to the completing source.
        #[must_use]
        pub fn player_names(&self) -> ApiVec<ApiString> {
            self.host()
                .context
                .source()
                .player_names()
                .into_iter()
                .map(Into::into)
                .collect()
        }
    }
}
