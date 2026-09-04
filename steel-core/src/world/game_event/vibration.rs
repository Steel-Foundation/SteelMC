//! Vanilla `VibrationSystem` — delayed, occluded game-event delivery.

use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::NbtCompound as NbtCompoundView;
use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::game_events::GameEventRef;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_game_event_tags::GameEventsTag;
use steel_registry::{REGISTRY, RegistryExt};
use steel_utils::locks::SyncMutex;
use steel_utils::{BlockPos, ChunkPos, Direction, Identifier, UuidExt};
use uuid::Uuid;

use super::{GameEventContext, GameEventDeliveryMode, GameEventListener};
use crate::entity::Entity;
use crate::world::{RaytraceAction, World};

const OCCLUSION_NUDGE: f64 = 1.0e-5;

/// Vanilla `VibrationSystem.Data`.
#[derive(Clone, Debug, Default)]
pub struct VibrationData {
    current: Option<VibrationInfo>,
    travel_time_in_ticks: i32,
    selector: VibrationSelector,
}

impl VibrationData {
    /// Creates empty vibration storage.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns whether a vibration is currently travelling toward the listener.
    #[must_use]
    pub fn has_incoming_vibration(&self) -> bool {
        self.current.is_some()
    }

    fn save(&self) -> NbtCompound {
        let mut nbt = NbtCompound::new();
        if let Some(current) = &self.current {
            nbt.insert("event", current.save());
        }
        nbt.insert("selector", self.selector.save());
        nbt.insert("event_delay", self.travel_time_in_ticks);
        nbt
    }

    fn load(nbt: &NbtCompoundView<'_, '_>) -> Self {
        Self {
            current: nbt.compound("event").and_then(|event| VibrationInfo::load(&event)),
            travel_time_in_ticks: nbt.int("event_delay").unwrap_or(0).max(0),
            selector: nbt
                .compound("selector")
                .map(|selector| VibrationSelector::load(&selector))
                .unwrap_or_default(),
        }
    }
}

/// One candidate or in-flight vibration.
#[derive(Clone, Debug)]
pub struct VibrationInfo {
    event: GameEventRef,
    distance: f32,
    pos: DVec3,
    source: Option<Uuid>,
    projectile_owner: Option<Uuid>,
}

impl VibrationInfo {
    fn new(
        event: GameEventRef,
        distance: f32,
        pos: DVec3,
        source: Option<&dyn Entity>,
    ) -> Self {
        Self {
            event,
            distance,
            pos,
            source: source.map(Entity::uuid),
            projectile_owner: source.and_then(Entity::projectile_owner_uuid),
        }
    }

    /// Returns the game event that produced this vibration.
    #[must_use]
    pub const fn event(&self) -> GameEventRef {
        self.event
    }

    /// Returns the source position of this vibration.
    #[must_use]
    pub const fn pos(&self) -> DVec3 {
        self.pos
    }

    /// Returns the travel distance in blocks.
    #[must_use]
    pub const fn distance(&self) -> f32 {
        self.distance
    }

    fn source_entity<'a>(&self, world: &'a World) -> Option<crate::entity::SharedEntity> {
        self.source
            .as_ref()
            .and_then(|uuid| world.get_entity_by_uuid(uuid))
    }

    fn projectile_owner_entity(&self, world: &World) -> Option<crate::entity::SharedEntity> {
        if let Some(source) = self.source_entity(world)
            && let Some(owner) = source.projectile_owner()
        {
            return Some(owner);
        }
        self.projectile_owner
            .as_ref()
            .and_then(|uuid| world.get_entity_by_uuid(uuid))
    }

    fn save(&self) -> NbtCompound {
        let mut nbt = NbtCompound::new();
        nbt.insert("game_event", self.event.key.to_string());
        nbt.insert("distance", self.distance);
        nbt.insert(
            "pos",
            NbtTag::List(NbtList::Double(vec![self.pos.x, self.pos.y, self.pos.z])),
        );
        if let Some(source) = self.source {
            nbt.insert("source", NbtTag::IntArray(source.to_int_array().to_vec()));
        }
        if let Some(owner) = self.projectile_owner {
            nbt.insert(
                "projectile_owner",
                NbtTag::IntArray(owner.to_int_array().to_vec()),
            );
        }
        nbt
    }

    fn load(nbt: &NbtCompoundView<'_, '_>) -> Option<Self> {
        let event_name = nbt.string("game_event")?;
        let ident: Identifier = event_name.to_str().parse().ok()?;
        let event = REGISTRY.game_events.by_key(&ident)?;
        let distance = nbt.float("distance").unwrap_or(0.0).max(0.0);
        let pos = load_vec3(nbt, "pos")?;
        Some(Self {
            event,
            distance,
            pos,
            source: nbt
                .int_array("source")
                .and_then(|arr| Uuid::from_int_array(&arr)),
            projectile_owner: nbt
                .int_array("projectile_owner")
                .and_then(|arr| Uuid::from_int_array(&arr)),
        })
    }
}

fn load_vec3(nbt: &NbtCompoundView<'_, '_>, key: &str) -> Option<DVec3> {
    let list = nbt.list(key)?;
    let doubles = list.doubles()?;
    if doubles.len() != 3 {
        return None;
    }
    Some(DVec3::new(doubles[0], doubles[1], doubles[2]))
}

/// Vanilla `VibrationSelector` — keeps the closest same-tick candidate.
#[derive(Clone, Debug, Default)]
struct VibrationSelector {
    candidate: Option<VibrationInfo>,
    tick: i64,
}

impl VibrationSelector {
    fn add_candidate(&mut self, info: VibrationInfo, tick_time: i64) {
        if self.should_replace(&info, tick_time) {
            self.candidate = Some(info);
            self.tick = tick_time;
        }
    }

    fn should_replace(&self, new: &VibrationInfo, tick_time: i64) -> bool {
        let Some(previous) = &self.candidate else {
            return true;
        };
        if tick_time != self.tick {
            return false;
        }
        if new.distance < previous.distance {
            return true;
        }
        if new.distance > previous.distance {
            return false;
        }
        game_event_frequency(new.event) > game_event_frequency(previous.event)
    }

    fn chosen_candidate(&self, time: i64) -> Option<VibrationInfo> {
        if self.tick < time {
            self.candidate.clone()
        } else {
            None
        }
    }

    fn start_over(&mut self) {
        self.candidate = None;
        self.tick = -1;
    }

    fn save(&self) -> NbtCompound {
        let mut nbt = NbtCompound::new();
        if let Some(candidate) = &self.candidate {
            nbt.insert("event", candidate.save());
        }
        nbt.insert("tick", self.tick);
        nbt
    }

    fn load(nbt: &NbtCompoundView<'_, '_>) -> Self {
        Self {
            candidate: nbt.compound("event").and_then(|event| VibrationInfo::load(&event)),
            tick: nbt.long("tick").unwrap_or(-1),
        }
    }
}

/// Vanilla `VibrationSystem.User`.
pub trait VibrationUser: Send + Sync {
    /// Listener radius in blocks.
    fn listener_radius(&self) -> i32;

    /// Current listener position.
    fn listener_pos(&self) -> Option<DVec3>;

    /// Tag of events this user can hear.
    fn listenable_events(&self) -> Identifier {
        GameEventsTag::VIBRATIONS
    }

    /// Extra per-user filter after generic vibration validation.
    fn can_receive_vibration(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        event: GameEventRef,
        context: &GameEventContext<'_>,
    ) -> bool;

    /// Called when a scheduled vibration arrives.
    fn on_receive_vibration(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        event: GameEventRef,
        source: Option<&dyn Entity>,
        projectile_owner: Option<&dyn Entity>,
        receiving_distance: f32,
    );

    /// Whether sneaking past this listener awards the avoid-vibration criterion.
    fn can_trigger_avoid_vibration(&self) -> bool {
        false
    }

    /// Sensors wait until neighboring chunks are ticking before receiving.
    fn requires_adjacent_chunks_to_be_ticking(&self) -> bool {
        false
    }

    /// Vanilla travel delay: `floor(distance)` ticks.
    fn calculate_travel_time_in_ticks(&self, distance: f32) -> i32 {
        distance.floor() as i32
    }

    /// Called when stored vibration data mutates.
    fn on_data_changed(&self) {}
}

/// Shared vibration listener used by sensors, shriekers, and the Warden.
pub struct VibrationListener {
    user: Arc<dyn VibrationUser>,
    data: Arc<SyncMutex<VibrationData>>,
}

impl VibrationListener {
    /// Creates a listener bound to `user` and shared `data`.
    #[must_use]
    pub fn new(user: Arc<dyn VibrationUser>, data: Arc<SyncMutex<VibrationData>>) -> Self {
        Self { user, data }
    }

    /// Vanilla `forceScheduleVibration` used by sculk-sensor `stepOn`.
    pub fn force_schedule_vibration(
        &self,
        world: &Arc<World>,
        event: GameEventRef,
        context: &GameEventContext<'_>,
        origin: DVec3,
    ) {
        let Some(destination) = self.user.listener_pos() else {
            return;
        };
        self.schedule(world, event, context, origin, destination);
    }

    fn schedule(
        &self,
        world: &Arc<World>,
        event: GameEventRef,
        context: &GameEventContext<'_>,
        origin: DVec3,
        destination: DVec3,
    ) {
        let distance = origin.distance(destination) as f32;
        self.data.lock().selector.add_candidate(
            VibrationInfo::new(event, distance, origin, context.source_entity()),
            world.game_time(),
        );
        self.user.on_data_changed();
    }
}

impl GameEventListener for VibrationListener {
    fn listener_pos(&self) -> Option<DVec3> {
        self.user.listener_pos()
    }

    fn listener_radius(&self) -> i32 {
        self.user.listener_radius()
    }

    fn delivery_mode(&self) -> GameEventDeliveryMode {
        GameEventDeliveryMode::ByDistance
    }

    fn handle_game_event(
        &self,
        world: &Arc<World>,
        event: GameEventRef,
        context: &GameEventContext<'_>,
        source_pos: DVec3,
    ) -> bool {
        if self.data.lock().current.is_some() {
            return false;
        }
        if !is_valid_vibration(self.user.as_ref(), event, context) {
            return false;
        }
        let Some(destination) = self.user.listener_pos() else {
            return false;
        };
        if !self.user.can_receive_vibration(
            world,
            BlockPos::from(source_pos),
            event,
            context,
        ) {
            return false;
        }
        if is_vibration_occluded(world, source_pos, destination) {
            return false;
        }
        self.schedule(world, event, context, source_pos, destination);
        true
    }
}

/// Ticks one vibration system, matching vanilla `VibrationSystem.Ticker`.
pub fn tick_vibration(world: &Arc<World>, data: &Arc<SyncMutex<VibrationData>>, user: &dyn VibrationUser) {
    let mut state = data.lock();
    if state.current.is_none()
        && let Some(chosen) = state.selector.chosen_candidate(world.game_time())
    {
        state.current = Some(chosen.clone());
        state.travel_time_in_ticks = user.calculate_travel_time_in_ticks(chosen.distance);
        state.selector.start_over();
        drop(state);
        user.on_data_changed();
        return tick_vibration(world, data, user);
    }

    let Some(current) = state.current.clone() else {
        return;
    };
    let mut changed = state.travel_time_in_ticks > 0;
    state.travel_time_in_ticks = (state.travel_time_in_ticks - 1).max(0);
    if state.travel_time_in_ticks <= 0 {
        changed = receive_vibration(world, &mut state, user, &current);
    }
    drop(state);
    if changed {
        user.on_data_changed();
    }
}

fn receive_vibration(
    world: &Arc<World>,
    data: &mut VibrationData,
    user: &dyn VibrationUser,
    current: &VibrationInfo,
) -> bool {
    let origin = BlockPos::from(current.pos);
    let destination = user
        .listener_pos()
        .map(BlockPos::from)
        .unwrap_or(origin);
    if user.requires_adjacent_chunks_to_be_ticking() && !adjacent_chunks_ticking(world, destination)
    {
        return false;
    }

    let source = current.source_entity(world);
    let projectile_owner = current.projectile_owner_entity(world);
    user.on_receive_vibration(
        world,
        origin,
        current.event,
        source.as_deref(),
        projectile_owner.as_deref(),
        distance_between_in_blocks(origin, destination),
    );
    data.current = None;
    true
}

fn adjacent_chunks_ticking(world: &World, listener_pos: BlockPos) -> bool {
    let chunk = ChunkPos::from_block_pos(listener_pos);
    for dx in -1..=1 {
        for dz in -1..=1 {
            if !world.has_full_chunk(ChunkPos::new(chunk.0.x + dx, chunk.0.y + dz)) {
                return false;
            }
        }
    }
    true
}

/// Vanilla `VibrationSystem.Listener.distanceBetweenInBlocks`.
#[must_use]
pub fn distance_between_in_blocks(origin: BlockPos, dest: BlockPos) -> f32 {
    let dx = f64::from(origin.x() - dest.x());
    let dy = f64::from(origin.y() - dest.y());
    let dz = f64::from(origin.z() - dest.z());
    (dx * dx + dy * dy + dz * dz).sqrt() as f32
}

/// Vanilla `VibrationSystem.getRedstoneStrengthForDistance`.
#[must_use]
pub fn redstone_strength_for_distance(distance: f32, listener_radius: i32) -> i32 {
    if listener_radius <= 0 {
        return 1;
    }
    let power_scale = 15.0 / f64::from(listener_radius);
    (15 - (power_scale * f64::from(distance)).floor() as i32).max(1)
}

/// Vanilla hardcoded `VIBRATION_FREQUENCY_FOR_EVENT`.
#[must_use]
pub fn game_event_frequency(event: GameEventRef) -> i32 {
    match event.key.path.as_ref() {
        "step" | "swim" | "flap" => 1,
        "projectile_land" | "hit_ground" | "splash" | "bounce" => 2,
        "item_interact_finish" | "projectile_shoot" | "instrument_play" => 3,
        "entity_action" | "elytra_glide" | "unequip" => 4,
        "entity_dismount" | "equip" => 5,
        "entity_interact" | "shear" | "entity_mount" => 6,
        "entity_damage" => 7,
        "drink" | "eat" => 8,
        "container_close" | "block_close" | "block_deactivate" | "block_detach" => 9,
        "container_open"
        | "block_open"
        | "block_activate"
        | "block_attach"
        | "prime_fuse"
        | "note_block_play" => 10,
        "block_change" => 11,
        "block_destroy" | "fluid_pickup" => 12,
        "block_place" | "fluid_place" => 13,
        "entity_place" | "lightning_strike" | "teleport" => 14,
        "entity_die" | "explode" => 15,
        path if let Some(n) = path.strip_prefix("resonate_") => n.parse::<i32>().unwrap_or(0),
        _ => 0,
    }
}

fn is_valid_vibration(
    user: &dyn VibrationUser,
    event: GameEventRef,
    context: &GameEventContext<'_>,
) -> bool {
    if !event.has_tag(&user.listenable_events()) {
        return false;
    }
    if let Some(source) = context.source_entity() {
        if source.is_spectator() {
            return false;
        }
        if source.is_stepping_carefully() && event.has_tag(&GameEventsTag::IGNORE_VIBRATIONS_SNEAKING)
        {
            return false;
        }
        if source.dampens_vibrations() {
            return false;
        }
    }
    if let Some(affected) = context.affected_state()
        && affected.get_block().has_tag(&BlockTag::DAMPENS_VIBRATIONS)
    {
        return false;
    }
    true
}

/// Vanilla wool occlusion: a vibration is blocked only if every axis-nudged
/// ray from source to listener hits `occludes_vibration_signals`.
#[must_use]
pub fn is_vibration_occluded(world: &World, origin: DVec3, dest: DVec3) -> bool {
    let from = DVec3::new(origin.x.floor() + 0.5, origin.y.floor() + 0.5, origin.z.floor() + 0.5);
    let to = DVec3::new(dest.x.floor() + 0.5, dest.y.floor() + 0.5, dest.z.floor() + 0.5);
    Direction::ALL.iter().all(|direction| {
        let (ox, oy, oz) = direction.offset();
        let nudged = DVec3::new(
            from.x + f64::from(ox) * OCCLUSION_NUDGE,
            from.y + f64::from(oy) * OCCLUSION_NUDGE,
            from.z + f64::from(oz) * OCCLUSION_NUDGE,
        );
        let (hit, _) = world.raytrace(nudged, to, |pos, world| {
            if world
                .get_block_state(pos)
                .get_block()
                .has_tag(&BlockTag::OCCLUDES_VIBRATION_SIGNALS)
            {
                RaytraceAction::ImmediateHit
            } else {
                RaytraceAction::Pass
            }
        });
        hit.is_some()
    })
}

/// Saves vanilla `listener` vibration data.
pub fn save_vibration_data(data: &VibrationData) -> NbtCompound {
    data.save()
}

/// Loads vanilla `listener` vibration data.
pub fn load_vibration_data(nbt: &NbtCompoundView<'_, '_>) -> VibrationData {
    VibrationData::load(nbt)
}

/// Shared handle stored on vibration-capable block entities and the Warden.
pub type SharedVibrationData = Arc<SyncMutex<VibrationData>>;

/// Weak handle used by dynamic entity listeners.
pub type WeakVibrationUser = Weak<dyn VibrationUser>;
