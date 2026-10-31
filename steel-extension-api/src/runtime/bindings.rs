#[stabby::host_api]
/// Checked command and runtime bindings.
pub mod api {
    #[cfg(feature = "host")]
    use crate::host::runtime as host;
    use crate::runtime::{
        BotInput, GameMode, Operation, OperationStatus, PlayerId, PlayerInfo, Position,
        WorldSettings,
    };
    use crate::{ApiResult, BlockPos, BlockState, Hooks};
    #[cfg(feature = "host")]
    use stabby::string::String as ApiString;
    use stabby::{slice::Slice, str::Str, vec::Vec as ApiVec};
    #[cfg(feature = "host")]
    use std::sync::Arc;
    #[cfg(feature = "host")]
    use steel_core::entity::Entity;
    #[cfg(feature = "host")]
    use steel_core::{
        command::{CommandContext, CommandRegistry},
        server::Server as CoreServer,
    };
    #[cfg(feature = "host")]
    use text_components::TextComponent;
    /// Server access on its tick thread, never retained across a callback.
    #[opaque]
    pub struct Server<'a>(&'a Arc<CoreServer>);
    /// Live command registry before its dispatcher is frozen.
    #[opaque]
    pub struct Commands<'a>(&'a mut CommandRegistry);
    /// Current command execution context, including player ownership and permissions.
    ///
    /// ```compile_fail
    /// use steel_extension_api::{Command, Server};
    /// fn retain(command: Command<'_>) -> Server<'static> { command.server() }
    /// ```
    #[opaque]
    pub struct Command<'a>(&'a CommandContext<'a>);
    impl<'a> Hooks<'a> {
        /// Subscribes in pre-init; edits the actual command registry before freezing.
        pub fn register_commands(
            &mut self,
            callback: extern "C" fn(Commands<'_>) -> ApiResult<()>,
        ) -> ApiResult<()> {
            host::subscribe_commands(self.host_mut(), callback)
                .map_err(ApiString::from)
                .into()
        }
        /// Runs before world workers, once per normal server tick.
        pub fn register_server_tick(&mut self, callback: extern "C" fn(Server<'_>, u64)) {
            self.host_mut()
                .hooks
                .register_server_tick(Box::new(host::Tick(callback)));
        }
    }
    impl<'a> Commands<'a> {
        /// Registers a public player command with literal subcommands. Explicit permission
        /// denials still apply. `key` supplies the namespace and root, such as `example:spleef`.
        pub fn register_player_command(
            &mut self,
            key: Str<'_>,
            actions: Slice<'_, Str<'_>>,
            callback: extern "C" fn(Command<'_>, Str<'_>) -> ApiResult<i32>,
        ) -> ApiResult<()> {
            host::register_command(self.host_mut(), key.as_str(), actions.as_slice(), callback)
                .map_err(ApiString::from)
                .into()
        }
    }
    impl<'a> Command<'a> {
        /// Server owning this execution.
        #[must_use]
        pub fn server(&self) -> Server<'a> {
            Server::from_host(self.host().source().server())
        }
        /// Current execution player, or an error for console/entity-only sources.
        #[must_use]
        pub fn player(&self) -> ApiResult<PlayerId> {
            self.host()
                .source()
                .player()
                .map(|p| PlayerId {
                    bytes: *p.gameprofile.id().as_bytes(),
                    entity_id: p.id(),
                })
                .ok_or_else(|| ApiString::from("this command requires a player"))
                .into()
        }
        /// Sends plain success feedback to the command sender.
        pub fn reply(&self, message: Str<'_>) {
            self.host()
                .source()
                .send_success(&TextComponent::plain(message.as_str().to_owned()), false);
        }
    }
    impl<'a> Server<'a> {
        /// Starts preparing a world using a registered generator and RAM-only storage.
        /// `primary` creates a new isolated domain; later worlds join that domain's clock.
        pub fn create_temporary_world(
            &self,
            key: Str<'_>,
            generator: Str<'_>,
            settings: WorldSettings,
            primary: bool,
        ) -> ApiResult<Operation> {
            host::create_world(
                self.host(),
                key.as_str(),
                generator.as_str(),
                settings,
                primary,
            )
            .map_err(ApiString::from)
            .into()
        }
        /// Reads an operation, consuming its result when complete.
        pub fn operation_status(&self, operation: Operation) -> ApiResult<OperationStatus> {
            host::operation_status(self.host(), operation)
                .map_err(ApiString::from)
                .into()
        }
        /// Rejects new entries and retires the domain after everyone has left.
        pub fn destroy_temporary_domain(&self, domain: Str<'_>) -> ApiResult<Operation> {
            self.host()
                .destroy_temporary_domain(domain.as_str())
                .map(|id| Operation { id: id.0 })
                .map_err(ApiString::from)
                .into()
        }
        /// Whether a domain uses temporary player-state routing.
        #[must_use]
        pub fn domain_is_temporary(&self, domain: Str<'_>) -> bool {
            self.host()
                .player_data_storage
                .is_temporary_domain(domain.as_str())
        }
        /// Takes a snapshot of connected players actually resident in their world.
        #[must_use]
        pub fn players(&self) -> ApiVec<PlayerInfo> {
            host::players(self.host())
        }
        /// Whether the same player incarnation remains connected, including during transfers.
        #[must_use]
        pub fn is_connected(&self, player: PlayerId) -> bool {
            host::is_connected(self.host(), player)
        }
        /// Whether a connected actor has finished relocation and client loading.
        #[must_use]
        pub fn player_ready(&self, player: PlayerId) -> bool {
            host::player_ready(self.host(), player)
        }
        /// Switches domains through the normal save/restore pipeline.
        pub fn switch_domain(&self, player: PlayerId, domain: Str<'_>) -> ApiResult<()> {
            host::switch_domain(self.host(), player, domain.as_str())
                .map_err(ApiString::from)
                .into()
        }
        /// Selects a loaded world through the normal relocation pipeline.
        pub fn select_world(&self, player: PlayerId, key: Str<'_>) -> ApiResult<()> {
            host::select_world(self.host(), player, key.as_str())
                .map_err(ApiString::from)
                .into()
        }
        /// Teleports within the current world. The destination chunk must already be loaded.
        pub fn teleport(&self, player: PlayerId, position: Position) -> ApiResult<()> {
            host::teleport(self.host(), player, position)
                .map_err(ApiString::from)
                .into()
        }
        /// Changes a resident player's game mode.
        pub fn set_game_mode(&self, player: PlayerId, mode: GameMode) -> ApiResult<()> {
            host::set_mode(self.host(), player, mode)
                .map_err(ApiString::from)
                .into()
        }
        /// Clears the player's current domain inventory.
        pub fn clear_inventory(&self, player: PlayerId) -> ApiResult<()> {
            host::clear_inventory(self.host(), player)
                .map_err(ApiString::from)
                .into()
        }
        /// Equips one registered item in the selected hand.
        pub fn equip_item(&self, player: PlayerId, key: Str<'_>) -> ApiResult<()> {
            host::equip_item(self.host(), player, key.as_str())
                .map_err(ApiString::from)
                .into()
        }
        /// Sends plain system chat to a player.
        pub fn message(&self, player: PlayerId, message: Str<'_>) -> ApiResult<()> {
            host::message(self.host(), player, message.as_str())
                .map_err(ApiString::from)
                .into()
        }
        /// Creates a visible server-controlled player in a prepared temporary world.
        pub fn spawn_bot(
            &self,
            key: Str<'_>,
            name: Str<'_>,
            position: Position,
        ) -> ApiResult<PlayerId> {
            host::spawn_bot(self.host(), key.as_str(), name.as_str(), position)
                .map_err(ApiString::from)
                .into()
        }
        /// Removes a server-controlled player through the normal disconnect path.
        pub fn despawn_bot(&self, player: PlayerId) -> ApiResult<()> {
            host::despawn_bot(self.host(), player)
                .map_err(ApiString::from)
                .into()
        }
        /// Supplies intent for a bot's next physics and digging tick.
        pub fn control_bot(&self, player: PlayerId, input: BotInput) -> ApiResult<()> {
            host::control_bot(self.host(), player, input)
                .map_err(ApiString::from)
                .into()
        }
        /// Reads a block in a loaded world without requesting generation.
        pub fn block(&self, key: Str<'_>, pos: BlockPos) -> ApiResult<BlockState> {
            host::block(self.host(), key.as_str(), pos)
                .map_err(ApiString::from)
                .into()
        }
    }
}
