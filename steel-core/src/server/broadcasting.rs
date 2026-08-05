use super::{
    Arc, CEntityEvent, CGameEvent, CSystemChat, CTabList, CTickingState, CTickingStep, Color,
    CommandSender, CommandSource, DisplayResolutor, Entity, GameEventType, Modifier, Player,
    Server, SprintReport, TabListTickStats, TextComponent, Uuid, client_permission_event,
    command_tree_packet, translations,
};

impl Server {
    /// Logs and broadcasts a system chat message to online players.
    fn broadcast_system_chat(&self, message: &TextComponent, excluded_player: Option<Uuid>) {
        log::info!("{}", message.to_plain(&DisplayResolutor));
        self.online_players.iter_players(|uuid, player| {
            if Some(*uuid) != excluded_player {
                player.send_packet(CSystemChat::new(message, false, player));
            }
            true
        });
    }

    /// Builds the tab list header/footer with recent and five-second tick statistics.
    pub(super) fn tab_list_components(
        tick_stats: TabListTickStats,
    ) -> (TextComponent, TextComponent) {
        // Color TPS based on value
        let tps_color = if tick_stats.tps >= 19.5 {
            Color::Green
        } else if tick_stats.tps >= 15.0 {
            Color::Yellow
        } else {
            Color::Red
        };

        let mspt_color = |mspt: f32| {
            if mspt <= 50.0 {
                Color::Aqua
            } else {
                Color::Red
            }
        };

        let header = TextComponent::plain("\n").add_children(vec![
            TextComponent::plain("Steel Dev Build").color(Color::Yellow),
            TextComponent::plain("\n"),
        ]);
        let footer = TextComponent::plain("\n").add_children(vec![
            TextComponent::plain("TPS: ").color(Color::Gray),
            TextComponent::plain(format!("{:.1}", tick_stats.tps)).color(tps_color),
            TextComponent::plain(" | ").color(Color::DarkGray),
            TextComponent::plain("MSPT: ").color(Color::Gray),
            TextComponent::plain(format!("{:.2}", tick_stats.recent_mspt))
                .color(mspt_color(tick_stats.recent_mspt)),
            TextComponent::plain(" recent | ").color(Color::Gray),
            TextComponent::plain(format!("{:.2}", tick_stats.average_mspt))
                .color(mspt_color(tick_stats.average_mspt)),
            TextComponent::plain(" avg (5s) | ").color(Color::Gray),
            TextComponent::plain(format!("{:.2}", tick_stats.p95_mspt))
                .color(mspt_color(tick_stats.p95_mspt)),
            TextComponent::plain(" p95").color(Color::Gray),
            TextComponent::plain("\n"),
        ]);

        (header, footer)
    }

    /// Broadcasts the tab list header/footer with current TPS and MSPT statistics.
    pub(super) fn broadcast_tab_list(&self, tick_stats: TabListTickStats) {
        let (header, footer) = Self::tab_list_components(tick_stats);

        self.broadcast_to_online_with(|player| CTabList::new(&header, &footer, player));
    }

    /// Broadcasts a sprint completion report to all players.
    pub(crate) fn broadcast_sprint_report(&self, report: &SprintReport) {
        let message: TextComponent = translations::COMMANDS_TICK_SPRINT_REPORT
            .message([
                TextComponent::from(format!("{}", report.ticks_per_second)),
                TextComponent::from(format!("{:.2}", report.ms_per_tick)),
            ])
            .into();

        self.broadcast_system_chat(&message, None);
    }

    pub(super) fn broadcast_player_join_message(
        &self,
        player: &Player,
        previous_name: Option<&str>,
    ) {
        let display_name = player.display_name();
        // Fallback to the current name when the cache has no prior entry.
        let old_name = previous_name.unwrap_or(player.gameprofile.name.as_str());
        let message: TextComponent = if player.gameprofile.name.eq_ignore_ascii_case(old_name) {
            translations::MULTIPLAYER_PLAYER_JOINED
                .message([display_name])
                .into()
        } else {
            translations::MULTIPLAYER_PLAYER_JOINED_RENAMED
                .message([display_name, TextComponent::plain(old_name.to_owned())])
                .into()
        };
        let message = message.color(Color::Yellow);
        self.broadcast_system_chat(&message, Some(player.gameprofile.id));
    }

    pub(super) fn broadcast_player_leave_message(&self, player: &Player) {
        let message: TextComponent = translations::MULTIPLAYER_PLAYER_LEFT
            .message([player.display_name()])
            .into();
        let message = message.color(Color::Yellow);
        self.broadcast_system_chat(&message, None);
    }

    /// Broadcasts the current tick rate and frozen state to all clients.
    /// This should be called whenever the tick rate or frozen state changes.
    pub fn broadcast_ticking_state(&self) {
        let tick_manager = self.tick_rate_manager.read();
        let packet = CTickingState::new(tick_manager.tick_rate(), tick_manager.is_frozen());
        drop(tick_manager);

        self.broadcast_to_online(packet);
    }

    /// Broadcasts the current step tick count to all clients.
    /// This should be called whenever the step tick count changes.
    pub fn broadcast_ticking_step(&self) {
        let tick_manager = self.tick_rate_manager.read();
        let packet = CTickingStep::new(tick_manager.frozen_ticks_to_run());
        drop(tick_manager);

        self.broadcast_to_online(packet);
    }

    /// Sends the current ticking state and step packets to a joining player.
    /// This should be called when a player joins the server.
    pub fn send_ticking_state_to_player(&self, player: &Player) {
        let tick_manager = self.tick_rate_manager.read();
        let state_packet = CTickingState::new(tick_manager.tick_rate(), tick_manager.is_frozen());
        let step_packet = CTickingStep::new(tick_manager.frozen_ticks_to_run());
        drop(tick_manager);

        player.send_packet(state_packet);
        player.send_packet(step_packet);
    }

    /// Resends client state that is not fully covered by `CRespawn`.
    pub fn resend_player_context(self: &Arc<Self>, player: &Arc<Player>) {
        player.send_difficulty();
        player.send_inventory_to_remote();

        self.resend_player_permission_context(player);

        self.send_ticking_state_to_player(player);

        player.send_packet(CGameEvent {
            event: GameEventType::ChangeGameMode,
            data: player.game_mode().into(),
        });
    }

    /// Resends the command tree and vanilla client permission-level projection.
    pub fn resend_player_permission_context(self: &Arc<Self>, player: &Arc<Player>) {
        let world = player.get_world();
        player.send_packet(CEntityEvent {
            entity_id: player.id(),
            event: client_permission_event(player, &world),
        });

        let server = player.server();
        if !Arc::ptr_eq(&server, self) {
            tracing::error!(
                player = %player.gameprofile.name,
                "cannot project commands from a different server"
            );
            return;
        }
        let Some(shared_player) = self.online_players.get_by_uuid(&player.gameprofile.id) else {
            tracing::error!(
                player = %player.gameprofile.name,
                "cannot project commands for a player outside the online player map"
            );
            return;
        };
        if !Arc::ptr_eq(&shared_player, player) {
            tracing::error!(
                player = %player.gameprofile.name,
                "cannot project commands for a stale player handle"
            );
            return;
        }
        let source = CommandSource::new(CommandSender::Player(shared_player), server);
        let commands = {
            let dispatcher = self.command_dispatcher.read();
            command_tree_packet(&dispatcher, &source)
        };
        match commands {
            Ok(commands) => player.send_packet(commands),
            Err(error) => tracing::error!(
                player = %player.gameprofile.name,
                %error,
                "failed to project the player's command tree"
            ),
        }
    }
}
