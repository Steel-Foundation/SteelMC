use super::world_tick_workers::{WorldTickWorkerError, WorldTickWorkers};
use super::{
    Arc, CCommandSuggestions, CHUNK_SENDING_TPS, COMMAND_DATA_AUTOSAVE_INTERVAL,
    COMMAND_REQUESTS_PER_TICK, COMMAND_RESUMPTIONS_PER_TICK, CancellationToken, ChunkPos,
    ChunkSender, CommandExecutionContext, CommandExecutionOwner, CommandRequest,
    CommandResultCallback, CommandSender, CommandSource, Duration, EncodedChunk,
    ExecutionCommandSource, ExecutionStop, GameTickTaskGuard, Instant, JoinSet, NetworkConnection,
    PendingCommandExecutionQueue, Player, SEND_PLAYER_INFO_INTERVAL, SLOW_CHUNK_TICK_THRESHOLD,
    Server, StringReader, SuggestionError, Suggestions, TAB_LIST_UPDATE_INTERVAL, TabListTickStats,
    ThreadPool, World, command_suggestions_packet, configured_packet_workers, sleep,
    spawn_blocking,
};

impl Server {
    /// Runs gameplay packets, game ticks, and chunk sending. Game-tick boundaries
    /// fork background chunk-scheduling epochs through each world's task tracker.
    pub async fn run(self: Arc<Self>, cancel_token: CancellationToken) {
        self.packet_processor.open_after_tick();
        let packet_worker_count = configured_packet_workers(self.config.packet_workers);
        let mut packet_handles = Vec::with_capacity(packet_worker_count);
        for worker_id in 0..packet_worker_count {
            let s = self.clone();
            let t = cancel_token.clone();
            packet_handles.push(tokio::spawn(async move {
                if let Err(error) = spawn_blocking(move || s.packet_processor.run(&s)).await {
                    log::error!("Gameplay packet worker {worker_id} failed: {error}");
                    t.cancel();
                }
            }));
        }
        let packet_supervisor_cancel = cancel_token.clone();
        let packet_workers = async move {
            for handle in packet_handles {
                if let Err(error) = handle.await {
                    log::error!("Gameplay packet supervisor failed: {error}");
                    packet_supervisor_cancel.cancel();
                }
            }
        };
        let game_handle = {
            let s = self.clone();
            let t = cancel_token.clone();
            let task_guard = GameTickTaskGuard::new(self.clone(), cancel_token.clone());
            tokio::spawn(async move {
                let _task_guard = task_guard;
                s.run_game_tick(t).await;
            })
        };
        let chunk_send_handle = {
            let s = self.clone();
            let t = cancel_token.clone();
            tokio::spawn(async move { s.run_chunk_sending_tick(t).await })
        };
        let ((), game_result, chunk_send_result) =
            tokio::join!(packet_workers, game_handle, chunk_send_handle);
        for (task, result) in [
            ("Game tick", game_result),
            ("Chunk sending tick", chunk_send_result),
        ] {
            if let Err(error) = result {
                log::error!("{task} task failed: {error}");
            }
        }
    }

    /// The main game tick loop (20 TPS, governed by tick rate manager).
    #[expect(
        clippy::too_many_lines,
        reason = "the ordered tick phases and their shutdown joins remain easier to audit together"
    )]
    async fn run_game_tick(self: Arc<Self>, cancel_token: CancellationToken) {
        let world_tick_workers = match WorldTickWorkers::spawn(self.worlds.values()) {
            Ok(workers) => workers,
            Err(error) => {
                log::error!("Failed to start world tick workers: {error}");
                cancel_token.cancel();
                return;
            }
        };
        let mut next_tick_time = Instant::now();
        let mut next_command_data_autosave = Instant::now() + COMMAND_DATA_AUTOSAVE_INTERVAL;
        let mut player_info_ticks = 0_u64;
        let mut pending_command_executions = PendingCommandExecutionQueue::<CommandSource>::new();
        let mut player_disconnect_saves = JoinSet::new();
        let mut command_data_autosaves = JoinSet::new();

        loop {
            if cancel_token.is_cancelled() {
                break;
            }

            let (nanoseconds_per_tick, should_sprint_this_tick) = {
                let mut tick_manager = self.tick_rate_manager.write();
                let nanoseconds_per_tick = tick_manager.nanoseconds_per_tick;
                let (should_sprint, sprint_report) = tick_manager.check_should_sprint_this_tick();
                drop(tick_manager);

                if let Some(report) = sprint_report {
                    self.broadcast_sprint_report(&report);
                    self.broadcast_ticking_state();
                }

                (nanoseconds_per_tick, should_sprint)
            };

            if should_sprint_this_tick {
                next_tick_time = Instant::now();
            } else {
                let now = Instant::now();
                if now < next_tick_time {
                    tokio::select! {
                        () = cancel_token.cancelled() => break,
                        () = sleep(next_tick_time - now) => {}
                    }
                }
                next_tick_time += Duration::from_nanos(nanoseconds_per_tick);
            }

            if cancel_token.is_cancelled() {
                break;
            }

            let tick_start = Instant::now();
            self.packet_processor.close_for_tick().await;
            self.advance_chunk_scheduling();
            self.start_player_disconnect_saves(&mut player_disconnect_saves);

            let (tick_count, runs_normally) = {
                let mut tick_manager = self.tick_rate_manager.write();
                tick_manager.tick();
                let runs_normally = tick_manager.runs_normally();
                tick_manager.increment_tick_count();
                (tick_manager.tick_count, runs_normally)
            };

            self.tick_pending_command_executions(&mut pending_command_executions);
            self.tick_command_requests(&mut pending_command_executions);
            if let Err(error) = self
                .tick_worlds_game(&world_tick_workers, tick_count, runs_normally)
                .await
            {
                log::error!("World game tick failed: {error}");
                cancel_token.cancel();
                break;
            }
            player_info_ticks += 1;
            if player_info_ticks > SEND_PLAYER_INFO_INTERVAL {
                let _span = tracing::trace_span!("broadcast_latency").entered();
                self.broadcast_player_latency_updates();
                player_info_ticks = 0;
            }
            self.tick_jobs(tick_count, runs_normally);
            self.process_player_joins();

            {
                let server = self.clone();
                let _ =
                    spawn_blocking(move || server.process_world_changes(tick_count, runs_normally))
                        .await;
            }

            self.process_domain_switches();

            self.tick_command_data_autosave(
                &mut next_command_data_autosave,
                &mut command_data_autosaves,
            );

            let tab_list_tick_stats = self.record_tick_and_capture_tab_stats(
                tick_count,
                tick_start.elapsed().as_nanos() as u64,
            );

            if let Some(tick_stats) = tab_list_tick_stats {
                self.broadcast_tab_list(tick_stats);
            }

            if should_sprint_this_tick {
                let mut tick_manager = self.tick_rate_manager.write();
                tick_manager.end_tick_work();
            }

            self.packet_processor.open_after_tick();
            if should_sprint_this_tick || Instant::now() >= next_tick_time {
                self.packet_processor.wait_for_overload_progress().await;
            }
        }

        self.jobs.cancel_all();
        pending_command_executions.cancel_all();
        self.command_requests.clear();
        self.packet_processor.stop();
        self.start_player_disconnect_saves(&mut player_disconnect_saves);
        self.pending_player_disconnects.clear();
        while let Some(result) = player_disconnect_saves.join_next().await {
            if let Err(error) = result {
                log::error!("Player disconnect save task failed during shutdown: {error}");
            }
        }
        while let Some(result) = command_data_autosaves.join_next().await {
            if let Err(error) = result {
                log::error!("Command data autosave task failed during shutdown: {error}");
            }
        }
    }

    fn record_tick_and_capture_tab_stats(
        &self,
        tick_count: u64,
        tick_duration_nanos: u64,
    ) -> Option<TabListTickStats> {
        let mut tick_manager = self.tick_rate_manager.write();
        tick_manager.record_tick_time(tick_duration_nanos);
        tick_count
            .is_multiple_of(TAB_LIST_UPDATE_INTERVAL)
            .then(|| TabListTickStats::capture(&tick_manager))
    }

    async fn autosave_command_data(&self) {
        tracing::debug!("Command data autosave started");
        let results = self.save_command_data().await;
        match results.scoreboards {
            Ok(saved) => tracing::debug!(saved, "Domain scoreboard autosave completed"),
            Err(error) => tracing::error!(%error, "Domain scoreboard autosave failed"),
        }
        match results.storage {
            Ok(saved) => tracing::debug!(saved, "Domain command-storage autosave completed"),
            Err(error) => tracing::error!(%error, "Domain command-storage autosave failed"),
        }
    }

    fn tick_command_data_autosave(
        self: &Arc<Self>,
        next_autosave: &mut Instant,
        saves: &mut JoinSet<()>,
    ) {
        while let Some(result) = saves.try_join_next() {
            if let Err(error) = result {
                log::error!("Command data autosave task failed: {error}");
            }
        }
        if Instant::now() < *next_autosave {
            return;
        }
        if saves.is_empty() {
            let server = Arc::clone(self);
            saves.spawn(async move {
                server.autosave_command_data().await;
            });
        } else {
            tracing::warn!("Skipping command data autosave while the previous save runs");
        }
        *next_autosave = Instant::now() + COMMAND_DATA_AUTOSAVE_INTERVAL;
    }

    fn tick_pending_command_executions(
        &self,
        pending: &mut PendingCommandExecutionQueue<CommandSource>,
    ) {
        let stats = pending.tick(COMMAND_RESUMPTIONS_PER_TICK, |owner| owner.is_current(self));
        if stats.polled == COMMAND_RESUMPTIONS_PER_TICK && stats.pending > 0 {
            tracing::debug!(
                polled = stats.polled,
                finished = stats.finished,
                pending = stats.pending,
                "Command resumption tick reached per-tick processing limit"
            );
        }
    }

    fn tick_command_requests(
        self: &Arc<Self>,
        pending: &mut PendingCommandExecutionQueue<CommandSource>,
    ) {
        let mut handled = 0;
        for _ in 0..COMMAND_REQUESTS_PER_TICK {
            let Some(request) = self
                .command_requests
                .pop_front_runnable(|owner| !pending.blocks(owner.key()))
            else {
                break;
            };
            handled += 1;

            match request {
                CommandRequest::Execute { owner, command } => {
                    if !owner.is_current(self) {
                        continue;
                    }
                    self.execute_command_request(pending, owner, &command);
                }
                CommandRequest::Suggestions {
                    owner,
                    transaction_id,
                    input,
                } => {
                    if !owner.is_current(self) {
                        continue;
                    }
                    let Some(player) = owner.sender().get_player() else {
                        tracing::error!("command suggestion request has a non-player owner");
                        continue;
                    };
                    self.send_command_suggestions(player, transaction_id, &input);
                }
            }
        }

        if handled == COMMAND_REQUESTS_PER_TICK {
            tracing::debug!(handled, "Command request tick reached its processing limit");
        }
    }

    fn execute_command_request(
        self: &Arc<Self>,
        pending: &mut PendingCommandExecutionQueue<CommandSource>,
        owner: CommandExecutionOwner,
        command: &str,
    ) {
        let source = CommandSource::new(owner.sender().clone(), Arc::clone(self));
        let command = command.strip_prefix('/').unwrap_or(command);
        let chain = {
            let dispatcher = self.command_dispatcher.read();
            let parse = dispatcher.parse(command, source.clone());
            dispatcher.context_chain(parse)
        };
        let chain = match chain {
            Ok(chain) => chain,
            Err(error) => {
                source.handle_error(&error, false);
                return;
            }
        };

        let mut execution = CommandExecutionContext::for_source(&source);
        execution.queue_initial_command(chain, source, CommandResultCallback::empty());
        if execution.run() == ExecutionStop::Suspended && !pending.push_suspended(owner, execution)
        {
            tracing::error!("suspended command execution could not be retained");
        }
    }

    fn send_command_suggestions(
        self: &Arc<Self>,
        player: &Arc<Player>,
        transaction_id: i32,
        input: &str,
    ) {
        let suggestions =
            self.build_command_suggestions(CommandSender::Player(Arc::clone(player)), input);
        match suggestions {
            Ok(suggestions) => {
                player.send_packet(command_suggestions_packet(transaction_id, &suggestions));
            }
            Err(error) => {
                tracing::warn!(%error, "failed to build command suggestions");
                player.send_packet(CCommandSuggestions::new(transaction_id, 0, 0, Vec::new()));
            }
        }
    }

    pub(super) fn build_command_suggestions(
        self: &Arc<Self>,
        sender: CommandSender,
        input: &str,
    ) -> Result<Suggestions, SuggestionError> {
        let source = CommandSource::new(sender, Arc::clone(self));
        let mut reader = StringReader::new(input);
        if reader.peek() == Some('/') {
            reader.skip();
        }
        let dispatcher = self.command_dispatcher.read();
        let parse = dispatcher.parse_reader(reader, source);
        dispatcher.completion_suggestions(&parse)
    }

    /// Chunk sending tick loop — encodes and sends chunks to players independently.
    async fn run_chunk_sending_tick(self: Arc<Self>, cancel_token: CancellationToken) {
        let nanos_per_tick = 1_000_000_000 / CHUNK_SENDING_TPS;
        let mut next_tick_time = Instant::now();

        loop {
            if cancel_token.is_cancelled() {
                break;
            }

            let now = Instant::now();
            if now < next_tick_time {
                tokio::select! {
                    () = cancel_token.cancelled() => break,
                    () = sleep(next_tick_time - now) => {}
                }
            }
            next_tick_time += Duration::from_nanos(nanos_per_tick);

            if cancel_token.is_cancelled() {
                break;
            }

            let server = self.clone();
            let _ = spawn_blocking(move || {
                server.tick_chunk_sending();
            })
            .await;
        }
    }

    /// Executes one chunk sending tick across all worlds and players.
    ///
    /// A per-world per-tick encode cache is used so overlapping view areas
    /// don't re-encode the same chunk within a single tick.
    fn tick_chunk_sending(&self) {
        let tick_start = Instant::now();
        for world in self.worlds.values() {
            let mut encode_cache = rustc_hash::FxHashMap::default();
            world.players.iter_players(|_uuid, player| {
                Self::send_chunks_for_player(
                    player,
                    world,
                    &mut encode_cache,
                    self.chunk_encoding_pool.as_ref(),
                );
                true
            });
        }

        let elapsed = tick_start.elapsed();
        if elapsed >= SLOW_CHUNK_TICK_THRESHOLD {
            tracing::warn!(?elapsed, "Chunk sending tick slow");
        }
    }

    /// Three-phase chunk send for a single player: prepare (lock briefly),
    /// encode (no lock), commit (lock briefly + generation check).
    fn send_chunks_for_player(
        player: &Arc<Player>,
        world: &Arc<World>,
        encode_cache: &mut rustc_hash::FxHashMap<ChunkPos, EncodedChunk>,
        encoding_pool: &ThreadPool,
    ) {
        let chunk_pos = *player.last_chunk_pos.lock();
        let connection = &player.connection;

        // Phase 1: prepare (brief lock)
        let prepared = {
            let mut sender = player.chunk_sender.lock();
            sender.prepare_batch(world, chunk_pos, &player.chunk_send_epoch)
        };

        let Some(batch) = prepared else {
            return;
        };

        // Phase 2: encode (no lock held — uses per-tick local cache)
        let compression = connection.compression();
        let encoded = ChunkSender::encode_batch(&batch, encode_cache, compression, encoding_pool);

        // Phase 3: commit while holding the tracking view that world detachment invalidates.
        // This makes membership validation, packet commit, and tracker refresh one side of
        // the same synchronization boundary.
        let tracking_view = player.last_tracking_view.lock();
        let Some(view) = *tracking_view else {
            return;
        };
        if !world.contains_player(player) {
            return;
        }
        let sent_chunks = {
            let mut sender = player.chunk_sender.lock();
            sender.commit_batch(&batch, encoded, connection, &player.chunk_send_epoch)
        };

        if sent_chunks.is_empty() {
            return;
        }

        let sent_chunks = player.chunk_sender.lock().sent_chunks_snapshot();
        world
            .entity_tracker()
            .update_player(player, &view, |chunk| sent_chunks.contains(&chunk));
    }

    /// Commits ready chunk lifecycle epochs and forks the next background work.
    fn advance_chunk_scheduling(&self) {
        for (i, world) in self.worlds.values().enumerate() {
            let timings = world.chunk_map.advance_scheduling();

            let background_elapsed = timings.ticket_updates
                + timings.schedule_generation
                + timings.run_generation
                + timings.process_unloads;
            let boundary_elapsed = timings.block_entity_unloads
                + timings.readiness_demotions
                + timings.lifecycle_commit
                + timings.readiness_reconcile
                + timings.ticking_snapshot_rebuild;
            let work_elapsed = background_elapsed + boundary_elapsed;

            if work_elapsed >= SLOW_CHUNK_TICK_THRESHOLD {
                tracing::warn!(
                    world = i,
                    work_elapsed = ?work_elapsed,
                    background_elapsed = ?background_elapsed,
                    boundary_elapsed = ?boundary_elapsed,
                    ticket_updates = ?timings.ticket_updates,
                    block_entity_unloads = ?timings.block_entity_unloads,
                    readiness_demotions = ?timings.readiness_demotions,
                    lifecycle_commit = ?timings.lifecycle_commit,
                    readiness_reconcile = ?timings.readiness_reconcile,
                    post_process_generation = ?timings.post_process_generation,
                    post_process_chunk_count = timings.post_process_chunk_count,
                    post_process_position_count = timings.post_process_position_count,
                    readiness_candidate_count = timings.readiness_candidate_count,
                    ticking_snapshot_rebuild = ?timings.ticking_snapshot_rebuild,
                    rebuilt_ticking_chunk_count = timings.rebuilt_ticking_chunk_count,
                    lookup_cache_holder_hits = timings.lookup_cache.holder_hits,
                    lookup_cache_missing_hits = timings.lookup_cache.missing_hits,
                    lookup_cache_scc_lookups = timings.lookup_cache.scc_lookups,
                    lookup_cache_foreign_map_bypasses = timings.lookup_cache.foreign_map_bypasses,
                    lookup_cache_evictions = timings.lookup_cache.evictions,
                    schedule_generation = ?timings.schedule_generation,
                    scheduled_count = timings.scheduled_count,
                    run_generation = ?timings.run_generation,
                    process_unloads = ?timings.process_unloads,
                    "Chunk scheduling epoch slow"
                );
            }
        }
    }

    #[tracing::instrument(level = "trace", skip(self, workers), name = "tick_worlds")]
    async fn tick_worlds_game(
        &self,
        workers: &WorldTickWorkers,
        tick_count: u64,
        runs_normally: bool,
    ) -> Result<(), WorldTickWorkerError> {
        let all_timings = workers.tick_all(tick_count, runs_normally).await?;
        for (i, timings) in all_timings.iter().enumerate() {
            if timings.elapsed.as_millis() < 50 {
                continue;
            }
            let cm = &timings.chunk_map;
            tracing::warn!(
                world = i,
                elapsed = ?timings.elapsed,
                tick_count,
                entity_tick = ?timings.entity_tick,
                broadcast_changes = ?cm.broadcast_changes,
                collect_tickable = ?cm.collect_tickable,
                tick_chunks = ?cm.tick_chunks,
                tick_block_entities = ?cm.tick_block_entities,
                tickable_count = cm.tickable_count,
                total_chunks = cm.total_chunks,
                lookup_cache_holder_hits = cm.lookup_cache.holder_hits,
                lookup_cache_missing_hits = cm.lookup_cache.missing_hits,
                lookup_cache_scc_lookups = cm.lookup_cache.scc_lookups,
                lookup_cache_foreign_map_bypasses = cm.lookup_cache.foreign_map_bypasses,
                lookup_cache_evictions = cm.lookup_cache.evictions,
                "Game tick slow"
            );
        }
        Ok(())
    }

    pub(super) fn tick_jobs(self: &Arc<Self>, tick_count: u64, runs_normally: bool) {
        let stats = self
            .jobs
            .tick(Arc::downgrade(self), tick_count, runs_normally);
        if stats.polled > 0 && stats.pending > 0 && tick_count.is_multiple_of(100) {
            tracing::debug!(
                polled = stats.polled,
                finished = stats.finished,
                pending = stats.pending,
                "Server jobs pending"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use rustc_hash::FxHashMap;
    use uuid::Uuid;

    use super::Server;
    use crate::{
        player::ResetReason,
        test_support::{TestPlayerBuilder, fresh_test_world, insert_ready_full_chunk},
    };
    use steel_utils::ChunkPos;

    #[test]
    fn chunk_send_commit_rechecks_live_world_membership() {
        let world = fresh_test_world("chunk_send_membership_revalidation");
        let center = ChunkPos::new(0, 0);
        insert_ready_full_chunk(&world, center);
        let player =
            TestPlayerBuilder::new(Arc::clone(&world), Uuid::from_u128(1), "ChunkTester", 1)
                .build();
        assert!(world.add_player(Arc::clone(&player), ResetReason::InitialJoin));
        assert!(world.players.remove_player_sync(&player).is_some());

        let encoding_pool = rayon::ThreadPoolBuilder::new().num_threads(1).build();
        let Ok(encoding_pool) = encoding_pool else {
            panic!("test chunk encoding pool should initialize");
        };
        let mut encode_cache = FxHashMap::default();
        Server::send_chunks_for_player(&player, &world, &mut encode_cache, &encoding_pool);

        let sender = player.chunk_sender.lock();
        assert!(sender.pending_chunks.contains(&center));
        assert!(!sender.is_chunk_sent(center));
        assert_eq!(sender.unacknowledged_batch_count_for_test(), 0);
        drop(sender);

        assert!(world.players.insert(Arc::clone(&player)));
        world.remove_player_for_world_change(&player);
    }
}
