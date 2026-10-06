use super::super::{
    Arc, BlockPos, ChunkPos, ChunkRequest, ChunkRequestHandle, ChunkRequestState, ChunkStatus,
    ChunkStorage, ChunkTicketKind, DVec3, DomainResidenceToken, EntityBase, NetworkConnection,
    PendingWorldChangeToken, PersistentEntity, PersistentRootVehicle, Player, PlayerSpawnSearch,
    PlayerSpawnSearchPoll, PreparedSpawn, RelativeMovement, RemovalReason, RespawnData, Server,
    SharedEntity, TeleportPostTransition, TeleportTransition, Uuid, World, change_entity_world,
    end_gateway, end_portal, is_allowed_to_enter_portal, nether_portal, vanilla_entities,
};
use super::{JobPoll, ServerJob, ServerJobContext};
use crate::entity::LivingEntity as _;

pub(in crate::server) struct RootVehicleRestoreJob {
    player: Arc<Player>,
    world: Arc<World>,
    request: ChunkRequestHandle,
    attach: [u8; 16],
    root_uuid: [u8; 16],
    residence_token: DomainResidenceToken,
}

impl RootVehicleRestoreJob {
    pub(in crate::server) fn new(
        player: Arc<Player>,
        world: Arc<World>,
        root_vehicle: &PersistentRootVehicle,
        residence_token: DomainResidenceToken,
    ) -> Option<Self> {
        let root_chunk = persistent_entity_chunk(&root_vehicle.entity)?;
        let request = world.chunk_map.request_chunk(
            root_chunk,
            ChunkStatus::StructureStarts,
            ChunkTicketKind::PlayerSpawn,
        );
        Some(Self {
            player,
            world,
            request,
            attach: root_vehicle.attach,
            root_uuid: root_vehicle.entity.uuid,
            residence_token,
        })
    }
}

impl ServerJob for RootVehicleRestoreJob {
    fn poll(&mut self, _context: &mut ServerJobContext) -> JobPoll {
        if self.player.connection.closed()
            || !self
                .player
                .is_domain_residence_current(self.residence_token)
            || !self.world.contains_player(&self.player)
        {
            return JobPoll::Finished;
        }

        match self.request.poll() {
            ChunkRequestState::Pending { .. } => JobPoll::Pending,
            ChunkRequestState::Cancelled => JobPoll::Finished,
            ChunkRequestState::Ready => {
                let Some(_ready) = self.request.ready_chunks() else {
                    return JobPoll::Pending;
                };
                if let Some(root_vehicle) = self.player.take_matching_pending_root_vehicle(
                    self.residence_token,
                    &self.world,
                    self.attach,
                    self.root_uuid,
                ) {
                    restore_root_vehicle_for_player(&self.player, &self.world, root_vehicle);
                }
                JobPoll::Finished
            }
        }
    }

    fn cancel(&mut self) {
        self.request.cancel();
    }
}

pub(in crate::server) fn clear_pending_world_change(
    entity: &SharedEntity,
    pending_token: PendingWorldChangeToken,
) {
    if entity.finish_pending_world_change(pending_token)
        && let Some(player) = entity.as_player()
    {
        player.retry_deferred_death_respawn();
    }
}

fn finish_pending_world_change_after_transition(
    entity: &SharedEntity,
    pending_token: PendingWorldChangeToken,
    changed_entity: Option<SharedEntity>,
) {
    match changed_entity {
        Some(changed_entity) if Arc::ptr_eq(entity, &changed_entity) => {
            clear_pending_world_change(&changed_entity, pending_token);
        }
        Some(_) => {}
        None => {
            clear_pending_world_change(entity, pending_token);
        }
    }
}

fn finish_portal_world_change(
    entity: &SharedEntity,
    pending_token: PendingWorldChangeToken,
    changed_entity: Option<SharedEntity>,
) -> JobPoll {
    finish_pending_world_change_after_transition(entity, pending_token, changed_entity);
    JobPoll::Finished
}

pub(in crate::server) fn portal_entity_still_valid(
    entity: &SharedEntity,
    source_world: &Arc<World>,
    pending_token: PendingWorldChangeToken,
) -> bool {
    !entity.is_removed()
        && entity.can_use_portal(false)
        && entity.is_world_change_token_pending(pending_token)
        && entity
            .level()
            .is_some_and(|world| Arc::ptr_eq(&world, source_world))
        && source_world.contains_live_or_unloading_entity(entity)
        && !entity
            .as_player()
            .is_some_and(|player| player.connection.closed())
}

fn poll_portal_chunks_until_ready(
    request: &mut ChunkRequestHandle,
    entity: &SharedEntity,
    pending_token: PendingWorldChangeToken,
) -> Option<JobPoll> {
    match request.poll() {
        ChunkRequestState::Pending { .. } => Some(JobPoll::Pending),
        ChunkRequestState::Cancelled => {
            clear_pending_world_change(entity, pending_token);
            Some(JobPoll::Finished)
        }
        ChunkRequestState::Ready => {
            if request.ready_chunks().is_some() {
                None
            } else {
                Some(JobPoll::Pending)
            }
        }
    }
}

const WORLD_SPAWN_SEARCH_READY_CANDIDATE_BUDGET: usize = 8;

pub(in crate::server) struct WorldSpawnTeleportJob {
    entity: SharedEntity,
    source_world: Arc<World>,
    target_world: Arc<World>,
    pending_token: PendingWorldChangeToken,
    rotation: (f32, f32),
    phase: WorldSpawnTeleportPhase,
}

enum WorldSpawnTeleportPhase {
    Searching(PlayerSpawnSearch),
    Loading {
        spawn: PreparedSpawn,
        request: ChunkRequestHandle,
    },
}

impl WorldSpawnTeleportJob {
    pub(in crate::server) fn new(
        entity: SharedEntity,
        source_world: Arc<World>,
        target_world: Arc<World>,
        pending_token: PendingWorldChangeToken,
    ) -> Result<Self, String> {
        if entity.as_player().is_none() {
            return Err("world spawn selection does not belong to a player".to_owned());
        }
        let (spawn_suggestion, rotation) = {
            let level_data = target_world.level_data.read();
            let data = level_data.data();
            (data.spawn_pos(), (data.spawn.angle, 0.0))
        };
        let search = PlayerSpawnSearch::new(
            &target_world,
            spawn_suggestion,
            target_world.default_gamemode,
        )?;
        Ok(Self {
            entity,
            source_world,
            target_world,
            pending_token,
            rotation,
            phase: WorldSpawnTeleportPhase::Searching(search),
        })
    }

    fn still_valid(&self, server: &Server) -> bool {
        let Some(player) = self.entity.as_player() else {
            return false;
        };
        !self.entity.is_removed()
            && !player.connection.closed()
            && player.get_health() > 0.0
            && !player.has_won_game()
            && self
                .entity
                .is_world_change_token_pending(self.pending_token)
            && self
                .entity
                .level()
                .is_some_and(|world| Arc::ptr_eq(&world, &self.source_world))
            && self.source_world.contains_player(player)
            && self.source_world.domain() == self.target_world.domain()
            && server
                .worlds
                .get(&self.target_world.key)
                .is_some_and(|registered| Arc::ptr_eq(registered, &self.target_world))
    }

    fn finish_pending(&self) {
        clear_pending_world_change(&self.entity, self.pending_token);
    }
}

impl ServerJob for WorldSpawnTeleportJob {
    fn poll(&mut self, context: &mut ServerJobContext) -> JobPoll {
        let Some(server) = context.server() else {
            self.finish_pending();
            return JobPoll::Finished;
        };
        if !self.still_valid(&server) {
            self.finish_pending();
            return JobPoll::Finished;
        }

        loop {
            match &mut self.phase {
                WorldSpawnTeleportPhase::Searching(search) => {
                    match search.poll_with_ready_candidate_budget(
                        &self.target_world,
                        WORLD_SPAWN_SEARCH_READY_CANDIDATE_BUDGET,
                    ) {
                        PlayerSpawnSearchPoll::Pending => return JobPoll::Pending,
                        PlayerSpawnSearchPoll::Cancelled => {
                            self.finish_pending();
                            return JobPoll::Finished;
                        }
                        PlayerSpawnSearchPoll::Ready(position) => {
                            let spawn = PreparedSpawn {
                                position,
                                rotation: self.rotation,
                            };
                            let request = self
                                .target_world
                                .request_player_spawn_chunks(spawn.position);
                            self.phase = WorldSpawnTeleportPhase::Loading { spawn, request };
                        }
                    }
                }
                WorldSpawnTeleportPhase::Loading { spawn, request } => match request.poll() {
                    ChunkRequestState::Pending { .. } => return JobPoll::Pending,
                    ChunkRequestState::Cancelled => {
                        self.finish_pending();
                        return JobPoll::Finished;
                    }
                    ChunkRequestState::Ready => {
                        if request.ready_chunks().is_none() {
                            return JobPoll::Pending;
                        }
                        let transition = TeleportTransition {
                            target_world: Arc::clone(&self.target_world),
                            position: spawn.position,
                            rotation: spawn.rotation,
                            velocity: DVec3::ZERO,
                            relatives: RelativeMovement::NONE,
                            portal_cooldown: 0,
                            as_passenger: false,
                            post_transition: TeleportPostTransition::do_nothing(),
                        };
                        let changed_entity =
                            change_entity_world(Arc::clone(&self.entity), &transition);
                        return finish_portal_world_change(
                            &self.entity,
                            self.pending_token,
                            changed_entity,
                        );
                    }
                },
            }
        }
    }

    fn cancel(&mut self) {
        self.finish_pending();
        if let WorldSpawnTeleportPhase::Loading { request, .. } = &mut self.phase {
            request.cancel();
        }
    }
}

pub(in crate::server) struct NetherPortalTeleportJob {
    entity: SharedEntity,
    source_world: Arc<World>,
    target_world: Arc<World>,
    portal_pos: BlockPos,
    approximate_exit_pos: BlockPos,
    to_nether: bool,
    pending_token: PendingWorldChangeToken,
    request: ChunkRequestHandle,
}

impl NetherPortalTeleportJob {
    pub(in crate::server) fn new(
        entity: SharedEntity,
        source_world: Arc<World>,
        target_world: Arc<World>,
        portal_pos: BlockPos,
        approximate_exit_pos: BlockPos,
        to_nether: bool,
        pending_token: PendingWorldChangeToken,
    ) -> Self {
        let request = target_world.chunk_map.request_square(
            nether_portal::prewarm_center(approximate_exit_pos),
            nether_portal::prewarm_chunk_radius(to_nether),
            ChunkStatus::Full,
            ChunkTicketKind::Portal,
        );
        Self {
            entity,
            source_world,
            target_world,
            portal_pos,
            approximate_exit_pos,
            to_nether,
            pending_token,
            request,
        }
    }

    fn still_valid(&self) -> bool {
        portal_entity_still_valid(&self.entity, &self.source_world, self.pending_token)
    }

    fn clear_pending(&self) {
        clear_pending_world_change(&self.entity, self.pending_token);
    }

    fn finish_transition(&self, changed_entity: Option<SharedEntity>) {
        finish_pending_world_change_after_transition(
            &self.entity,
            self.pending_token,
            changed_entity,
        );
    }
}

impl ServerJob for NetherPortalTeleportJob {
    fn poll(&mut self, context: &mut ServerJobContext) -> JobPoll {
        if !self.still_valid() {
            self.clear_pending();
            return JobPoll::Finished;
        }

        if let Some(job_poll) =
            poll_portal_chunks_until_ready(&mut self.request, &self.entity, self.pending_token)
        {
            return job_poll;
        }

        let Some(server) = context.server() else {
            self.clear_pending();
            return JobPoll::Finished;
        };
        if !is_allowed_to_enter_portal(&self.source_world, &self.target_world)
            || !server.can_teleport_between_worlds(
                self.entity.as_ref(),
                &self.source_world,
                &self.target_world,
            )
        {
            self.clear_pending();
            return JobPoll::Finished;
        }
        let Some(transition) = nether_portal::calculate_transition(
            &self.source_world,
            &self.target_world,
            self.entity.as_ref(),
            self.portal_pos,
            self.approximate_exit_pos,
            self.to_nether,
        ) else {
            self.clear_pending();
            return JobPoll::Finished;
        };
        let changed_entity = change_entity_world(Arc::clone(&self.entity), &transition);
        self.finish_transition(changed_entity);
        JobPoll::Finished
    }

    fn cancel(&mut self) {
        self.clear_pending();
        self.request.cancel();
    }
}

const END_PORTAL_RESPAWN_SEARCH_READY_CANDIDATE_BUDGET: usize = 8;

struct EndPortalRespawnSpawn {
    position: DVec3,
    rotation: (f32, f32),
}

pub(in crate::server) struct EndPortalTeleportJob {
    entity: SharedEntity,
    source_world: Arc<World>,
    pending_token: PendingWorldChangeToken,
    phase: EndPortalTeleportPhase,
}

enum EndPortalTeleportPhase {
    EntryToEnd {
        target_world: Arc<World>,
        request: ChunkRequestHandle,
    },
    ReturningEntity {
        target_world: Arc<World>,
        respawn_data: RespawnData,
        request: ChunkRequestHandle,
    },
    SearchingPlayerRespawn {
        target_world: Arc<World>,
        respawn_data: RespawnData,
        search: PlayerSpawnSearch,
    },
    LoadingPlayerRespawn {
        target_world: Arc<World>,
        spawn: EndPortalRespawnSpawn,
        request: ChunkRequestHandle,
    },
}

impl EndPortalTeleportJob {
    pub(in crate::server) fn entry_to_end(
        entity: SharedEntity,
        source_world: Arc<World>,
        target_world: Arc<World>,
        pending_token: PendingWorldChangeToken,
    ) -> Self {
        let request = target_world.chunk_map.request_square(
            end_portal::end_platform_prewarm_center(),
            end_portal::end_platform_prewarm_chunk_radius(),
            ChunkStatus::Full,
            ChunkTicketKind::Portal,
        );
        Self {
            entity,
            source_world,
            pending_token,
            phase: EndPortalTeleportPhase::EntryToEnd {
                target_world,
                request,
            },
        }
    }

    pub(in crate::server) fn returning_entity(
        entity: SharedEntity,
        source_world: Arc<World>,
        target_world: Arc<World>,
        respawn_data: RespawnData,
        pending_token: PendingWorldChangeToken,
    ) -> Self {
        let request = target_world.chunk_map.request_chunk(
            end_portal::prewarm_center(respawn_data.pos()),
            ChunkStatus::Full,
            ChunkTicketKind::Portal,
        );
        Self {
            entity,
            source_world,
            pending_token,
            phase: EndPortalTeleportPhase::ReturningEntity {
                target_world,
                respawn_data,
                request,
            },
        }
    }

    pub(in crate::server) fn returning_player(
        entity: SharedEntity,
        source_world: Arc<World>,
        target_world: Arc<World>,
        respawn_data: RespawnData,
        pending_token: PendingWorldChangeToken,
    ) -> Result<Self, String> {
        let search = PlayerSpawnSearch::new(
            &target_world,
            respawn_data.pos(),
            target_world.default_gamemode,
        )?;
        Ok(Self {
            entity,
            source_world,
            pending_token,
            phase: EndPortalTeleportPhase::SearchingPlayerRespawn {
                target_world,
                respawn_data,
                search,
            },
        })
    }

    fn still_valid(&self) -> bool {
        portal_entity_still_valid(&self.entity, &self.source_world, self.pending_token)
    }

    fn clear_pending(&self) {
        clear_pending_world_change(&self.entity, self.pending_token);
    }
}

impl ServerJob for EndPortalTeleportJob {
    fn poll(&mut self, _context: &mut ServerJobContext) -> JobPoll {
        if !self.still_valid() {
            self.clear_pending();
            return JobPoll::Finished;
        }

        let entity = Arc::clone(&self.entity);
        let pending_token = self.pending_token;
        loop {
            match &mut self.phase {
                EndPortalTeleportPhase::EntryToEnd {
                    target_world,
                    request,
                } => {
                    if let Some(job_poll) =
                        poll_portal_chunks_until_ready(request, &entity, pending_token)
                    {
                        return job_poll;
                    }
                    let Some(transition) =
                        end_portal::calculate_entry_transition(target_world, entity.as_ref())
                    else {
                        clear_pending_world_change(&entity, pending_token);
                        return JobPoll::Finished;
                    };
                    let changed_entity = change_entity_world(Arc::clone(&entity), &transition);
                    return finish_portal_world_change(&entity, pending_token, changed_entity);
                }
                EndPortalTeleportPhase::ReturningEntity {
                    target_world,
                    respawn_data,
                    request,
                } => {
                    if let Some(job_poll) =
                        poll_portal_chunks_until_ready(request, &entity, pending_token)
                    {
                        return job_poll;
                    }
                    let transition = end_portal::calculate_entity_return_transition(
                        target_world,
                        entity.as_ref(),
                        respawn_data,
                    );
                    let changed_entity = change_entity_world(Arc::clone(&entity), &transition);
                    return finish_portal_world_change(&entity, pending_token, changed_entity);
                }
                EndPortalTeleportPhase::SearchingPlayerRespawn {
                    target_world,
                    respawn_data,
                    search,
                } => match search.poll_with_ready_candidate_budget(
                    target_world,
                    END_PORTAL_RESPAWN_SEARCH_READY_CANDIDATE_BUDGET,
                ) {
                    PlayerSpawnSearchPoll::Pending => return JobPoll::Pending,
                    PlayerSpawnSearchPoll::Cancelled => {
                        clear_pending_world_change(&entity, pending_token);
                        return JobPoll::Finished;
                    }
                    PlayerSpawnSearchPoll::Ready(position) => {
                        let spawn = EndPortalRespawnSpawn {
                            position,
                            rotation: (respawn_data.yaw, respawn_data.pitch),
                        };
                        let request = target_world.request_player_spawn_chunks(position);
                        self.phase = EndPortalTeleportPhase::LoadingPlayerRespawn {
                            target_world: target_world.clone(),
                            spawn,
                            request,
                        };
                    }
                },
                EndPortalTeleportPhase::LoadingPlayerRespawn {
                    target_world,
                    spawn,
                    request,
                } => {
                    if let Some(job_poll) =
                        poll_portal_chunks_until_ready(request, &entity, pending_token)
                    {
                        return job_poll;
                    }
                    let transition = end_portal::calculate_player_return_transition(
                        target_world,
                        entity.as_ref(),
                        spawn.position,
                        spawn.rotation,
                    );
                    let changed_entity = change_entity_world(Arc::clone(&entity), &transition);
                    return finish_portal_world_change(&entity, pending_token, changed_entity);
                }
            }
        }
    }

    fn cancel(&mut self) {
        self.clear_pending();
        match &mut self.phase {
            EndPortalTeleportPhase::EntryToEnd { request, .. }
            | EndPortalTeleportPhase::ReturningEntity { request, .. }
            | EndPortalTeleportPhase::LoadingPlayerRespawn { request, .. } => request.cancel(),
            EndPortalTeleportPhase::SearchingPlayerRespawn { .. } => {}
        }
    }
}

pub(in crate::server) struct EndGatewayTeleportJob {
    entity: SharedEntity,
    source_world: Arc<World>,
    portal_pos: BlockPos,
    source_is_end: bool,
    pending_token: PendingWorldChangeToken,
    phase: EndGatewayTeleportPhase,
}

enum EndGatewayTeleportPhase {
    LoadingReady { request: ChunkRequestHandle },
    LoadingSearchPath { request: ChunkRequestHandle },
}

impl EndGatewayTeleportJob {
    pub(in crate::server) fn new(
        entity: SharedEntity,
        source_world: Arc<World>,
        portal_pos: BlockPos,
        source_is_end: bool,
        pending_token: PendingWorldChangeToken,
    ) -> Option<Self> {
        let preparation = end_gateway::initial_chunks(&source_world, portal_pos, source_is_end)?;
        let phase = match preparation {
            end_gateway::EndGatewayChunkPreparation::Ready(chunks) => {
                EndGatewayTeleportPhase::LoadingReady {
                    request: request_end_gateway_chunks(&source_world, chunks),
                }
            }
            end_gateway::EndGatewayChunkPreparation::SearchPath(chunks) => {
                EndGatewayTeleportPhase::LoadingSearchPath {
                    request: request_end_gateway_chunks(&source_world, chunks),
                }
            }
        };
        Some(Self {
            entity,
            source_world,
            portal_pos,
            source_is_end,
            pending_token,
            phase,
        })
    }

    fn still_valid(&self) -> bool {
        portal_entity_still_valid(&self.entity, &self.source_world, self.pending_token)
    }

    fn clear_pending(&self) {
        clear_pending_world_change(&self.entity, self.pending_token);
    }
}

impl ServerJob for EndGatewayTeleportJob {
    fn poll(&mut self, _context: &mut ServerJobContext) -> JobPoll {
        if !self.still_valid() {
            self.clear_pending();
            return JobPoll::Finished;
        }

        let entity = Arc::clone(&self.entity);
        let pending_token = self.pending_token;
        let source_world = Arc::clone(&self.source_world);
        let portal_pos = self.portal_pos;
        let source_is_end = self.source_is_end;
        loop {
            match &mut self.phase {
                EndGatewayTeleportPhase::LoadingReady { request } => match request.poll() {
                    ChunkRequestState::Pending { .. } => return JobPoll::Pending,
                    ChunkRequestState::Cancelled => {
                        clear_pending_world_change(&entity, pending_token);
                        return JobPoll::Finished;
                    }
                    ChunkRequestState::Ready => {
                        let Some(_ready) = request.ready_chunks() else {
                            return JobPoll::Pending;
                        };
                        let Some(transition) = end_gateway::calculate_transition(
                            &source_world,
                            entity.as_ref(),
                            portal_pos,
                            source_is_end,
                        ) else {
                            clear_pending_world_change(&entity, pending_token);
                            return JobPoll::Finished;
                        };
                        let changed_entity = change_entity_world(Arc::clone(&entity), &transition);
                        finish_pending_world_change_after_transition(
                            &entity,
                            pending_token,
                            changed_entity,
                        );
                        return JobPoll::Finished;
                    }
                },
                EndGatewayTeleportPhase::LoadingSearchPath { request } => match request.poll() {
                    ChunkRequestState::Pending { .. } => return JobPoll::Pending,
                    ChunkRequestState::Cancelled => {
                        clear_pending_world_change(&entity, pending_token);
                        return JobPoll::Finished;
                    }
                    ChunkRequestState::Ready => {
                        let Some(_ready) = request.ready_chunks() else {
                            return JobPoll::Pending;
                        };
                        let Some(chunks) = end_gateway::final_chunks_after_search(
                            &source_world,
                            portal_pos,
                            source_is_end,
                        ) else {
                            clear_pending_world_change(&entity, pending_token);
                            return JobPoll::Finished;
                        };
                        self.phase = EndGatewayTeleportPhase::LoadingReady {
                            request: request_end_gateway_chunks(&source_world, chunks),
                        };
                    }
                },
            }
        }
    }

    fn cancel(&mut self) {
        self.clear_pending();
        match &mut self.phase {
            EndGatewayTeleportPhase::LoadingReady { request }
            | EndGatewayTeleportPhase::LoadingSearchPath { request } => request.cancel(),
        }
    }
}

fn request_end_gateway_chunks(world: &Arc<World>, chunks: Vec<ChunkPos>) -> ChunkRequestHandle {
    world.chunk_map.request_chunks(ChunkRequest {
        status: ChunkStatus::Full,
        positions: chunks,
        ticket_kind: ChunkTicketKind::Portal,
    })
}

fn persistent_entity_chunk(entity: &PersistentEntity) -> Option<ChunkPos> {
    let pos = DVec3::new(entity.pos[0], entity.pos[1], entity.pos[2]);
    if !pos.x.is_finite() || !pos.y.is_finite() || !pos.z.is_finite() {
        tracing::warn!(
            uuid = ?Uuid::from_bytes(entity.uuid),
            "Skipping persisted entity with non-finite position {pos:?}",
        );
        return None;
    }
    Some(ChunkPos::from_entity_pos(pos))
}

fn restore_root_vehicle_for_player(
    player: &Arc<Player>,
    world: &Arc<World>,
    root_vehicle: PersistentRootVehicle,
) {
    let Some(root_chunk) = persistent_entity_chunk(&root_vehicle.entity) else {
        return;
    };
    let level = Arc::downgrade(world);
    let entities =
        ChunkStorage::persistent_to_entity_tree_at_level(&root_vehicle.entity, root_chunk, &level);
    if entities.is_empty() {
        tracing::warn!(
            player = %player.gameprofile.name,
            "Persisted RootVehicle did not recreate any runtime entities",
        );
        return;
    }

    let attach_uuid = Uuid::from_bytes(root_vehicle.attach);
    let Some(attach_entity) = entities
        .iter()
        .find(|entity| entity.uuid() == attach_uuid)
        .cloned()
    else {
        tracing::warn!(
            player = %player.gameprofile.name,
            attach = ?attach_uuid,
            "Discarding persisted RootVehicle because the attach entity is missing",
        );
        discard_restored_entities(&entities);
        return;
    };

    if let Err(error) = world.register_loaded_entity_tree(&entities) {
        tracing::warn!(
            player = %player.gameprofile.name,
            attach = ?attach_uuid,
            root = ?Uuid::from_bytes(root_vehicle.entity.uuid),
            "Discarding persisted RootVehicle because its entity tree could not be registered: {error}",
        );
        discard_restored_entities(&entities);
        return;
    }

    let player_entity: SharedEntity = player.clone();
    EntityBase::restore_passenger_relationship(&attach_entity, &player_entity);
    attach_entity.position_rider(player.as_ref());
    player.send_restored_vehicle_mount_sync(attach_entity.as_ref());

    world.mark_chunk_dirty(root_chunk);
    for entity in &entities {
        world.mark_chunk_dirty(ChunkPos::from_entity_pos(entity.position()));
    }
}

fn discard_restored_entities(entities: &[SharedEntity]) {
    for entity in entities {
        entity.set_removed(RemovalReason::Discarded);
    }
}

/// Re-spawns a single persisted ender pearl in its own world once the target
/// chunk is loaded (vanilla `ServerPlayer.loadAndSpawnEnderPearl`).
pub(in crate::server) struct EnderPearlRestoreJob {
    player: Arc<Player>,
    world: Arc<World>,
    request: ChunkRequestHandle,
    uuid: Uuid,
    residence_token: DomainResidenceToken,
}

impl EnderPearlRestoreJob {
    pub(in crate::server) fn new(
        player: Arc<Player>,
        world: Arc<World>,
        entity: PersistentEntity,
        residence_token: DomainResidenceToken,
    ) -> Option<Self> {
        let chunk = persistent_entity_chunk(&entity)?;
        let uuid = Uuid::from_bytes(entity.uuid);
        let request = world.chunk_map.request_chunk(
            chunk,
            ChunkStatus::StructureStarts,
            ChunkTicketKind::PlayerSpawn,
        );
        Some(Self {
            player,
            world,
            request,
            uuid,
            residence_token,
        })
    }
}

impl ServerJob for EnderPearlRestoreJob {
    fn poll(&mut self, _context: &mut ServerJobContext) -> JobPoll {
        // The pearl may live in another world in the same domain, so require a
        // live same-domain owner rather than membership in the pearl's exact world.
        if self.player.connection.closed()
            || !self
                .player
                .is_domain_residence_current(self.residence_token)
        {
            return JobPoll::Finished;
        }
        let Some(server) = self.player.server.upgrade() else {
            return JobPoll::Finished;
        };
        if !server.owns_online_player(&self.player) {
            return JobPoll::Finished;
        }
        let Some(player_world) = server.live_world_for_player(&self.player) else {
            // End credits temporarily detaches a connected player without ending
            // their domain residence. Retain the payload and resume after respawn.
            return JobPoll::Pending;
        };
        if player_world.domain() != self.world.domain() {
            tracing::error!(
                player = %self.player.gameprofile.name,
                player_domain = player_world.domain(),
                pearl_domain = self.world.domain(),
                "Discarding a pending ender pearl whose owner changed domains without a new residence"
            );
            self.player
                .discard_pending_ender_pearl(self.residence_token, self.uuid);
            return JobPoll::Finished;
        }

        match self.request.poll() {
            ChunkRequestState::Pending { .. } => JobPoll::Pending,
            ChunkRequestState::Cancelled => JobPoll::Finished,
            ChunkRequestState::Ready => {
                if self.request.ready_chunks().is_none() {
                    return JobPoll::Pending;
                }
                let Some(pearl) = self.player.take_matching_pending_ender_pearl(
                    self.residence_token,
                    &self.world,
                    self.uuid,
                ) else {
                    return JobPoll::Finished;
                };
                restore_ender_pearl_for_player(&self.player, &self.world, &pearl.entity);
                JobPoll::Finished
            }
        }
    }

    fn cancel(&mut self) {
        self.request.cancel();
    }
}

fn restore_ender_pearl_for_player(
    player: &Arc<Player>,
    world: &Arc<World>,
    entity: &PersistentEntity,
) -> bool {
    let Some(chunk) = persistent_entity_chunk(entity) else {
        return false;
    };
    let level = Arc::downgrade(world);
    let entities = ChunkStorage::persistent_to_entity_tree_at_level(entity, chunk, &level);
    let Some(pearl) = entities.first().cloned() else {
        tracing::warn!(
            player = %player.gameprofile.name,
            "Persisted ender pearl did not recreate a runtime entity",
        );
        return false;
    };
    if pearl.entity_type() != &vanilla_entities::ENDER_PEARL {
        tracing::warn!(
            player = %player.gameprofile.name,
            entity_type = ?pearl.entity_type().key,
            "Persisted ender pearl recreated a non-pearl root entity",
        );
        return false;
    }

    let owner: SharedEntity = player.clone();
    for entity in &entities {
        entity.restore_owner_reference(&owner);
    }

    if let Err(error) = world.register_loaded_entity_tree(&entities) {
        tracing::warn!(
            player = %player.gameprofile.name,
            "Discarding persisted ender pearl because it could not be registered: {error}",
        );
        discard_restored_entities(&entities);
        return false;
    }

    player.register_ender_pearl(&pearl);
    world.chunk_map.place_ender_pearl_ticket(chunk);
    world.mark_chunk_dirty(chunk);
    true
}
