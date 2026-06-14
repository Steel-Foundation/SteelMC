//! Shared movement synchronization state for tracked entities.

use glam::DVec3;
use steel_protocol::packets::game::{
    CEntityPositionSync, CMoveEntityPos, CMoveEntityPosRot, CMoveEntityRot, CRotateHead,
    CSetEntityMotion, PackedEntityDelta, calc_delta, to_angle_byte,
};

/// Squared position delta needed before vanilla considers a movement worth syncing.
pub const POSITION_SYNC_THRESHOLD: f64 = 7.629_394_5e-6;
/// Squared velocity delta needed before vanilla sends an entity motion packet.
pub const VELOCITY_SYNC_THRESHOLD: f64 = 1.0e-7;
/// Vanilla `ServerEntity.FORCED_POS_UPDATE_PERIOD`.
pub const FORCED_POS_UPDATE_PERIOD: i32 = 60;
/// Vanilla `ServerEntity.FORCED_TELEPORT_PERIOD`.
pub const FORCED_TELEPORT_PERIOD: i32 = 400;

/// Packed body rotation used by entity movement packets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackedEntityRotation {
    yaw: i8,
    pitch: i8,
}

impl PackedEntityRotation {
    /// Packs yaw and pitch using vanilla's angle-byte representation.
    #[must_use]
    pub fn from_degrees(rotation: (f32, f32)) -> Self {
        Self {
            yaw: to_angle_byte(rotation.0),
            pitch: to_angle_byte(rotation.1),
        }
    }

    /// Returns packed yaw.
    #[must_use]
    pub const fn yaw(self) -> i8 {
        self.yaw
    }

    /// Returns packed pitch.
    #[must_use]
    pub const fn pitch(self) -> i8 {
        self.pitch
    }
}

/// Last packed rotation values known to tracking clients.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntityRotationSyncState {
    last_body_rotation: PackedEntityRotation,
    last_head_yaw: i8,
}

impl EntityRotationSyncState {
    /// Creates rotation sync state for values already known to tracking clients.
    #[must_use]
    pub fn new(body_rotation: (f32, f32), head_yaw: f32) -> Self {
        Self {
            last_body_rotation: PackedEntityRotation::from_degrees(body_rotation),
            last_head_yaw: to_angle_byte(head_yaw),
        }
    }

    /// Records a body rotation packet if the packed yaw or pitch changed.
    pub fn record_body_rotation(&mut self, rotation: (f32, f32)) -> Option<PackedEntityRotation> {
        let packed = PackedEntityRotation::from_degrees(rotation);
        if packed == self.last_body_rotation {
            return None;
        }

        self.last_body_rotation = packed;
        Some(packed)
    }

    /// Returns whether packed body yaw or pitch changed since the last sync.
    #[must_use]
    pub fn body_rotation_changed(self, rotation: (f32, f32)) -> bool {
        PackedEntityRotation::from_degrees(rotation) != self.last_body_rotation
    }

    /// Marks a body rotation as sent because a full position sync includes it.
    pub fn mark_body_rotation_sent(&mut self, rotation: (f32, f32)) {
        self.last_body_rotation = PackedEntityRotation::from_degrees(rotation);
    }

    /// Records a head-rotation packet if the packed yaw changed.
    pub fn record_head_yaw(&mut self, head_yaw: f32) -> Option<i8> {
        let packed = to_angle_byte(head_yaw);
        if packed == self.last_head_yaw {
            return None;
        }

        self.last_head_yaw = packed;
        Some(packed)
    }
}

/// Encoded position sync selected for an entity movement update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityPositionSyncDecision {
    /// Delta-encoded movement update.
    Delta {
        /// Packed X delta.
        dx: PackedEntityDelta,
        /// Packed Y delta.
        dy: PackedEntityDelta,
        /// Packed Z delta.
        dz: PackedEntityDelta,
    },
    /// Full absolute position sync.
    Full,
}

/// Position sync packet for entities that do not include rotation in delta updates.
#[derive(Clone, Debug)]
pub enum EntityPositionSyncPacket {
    /// Delta-encoded position update.
    Delta(CMoveEntityPos),
    /// Full absolute position sync.
    Full(CEntityPositionSync),
}

/// Position sync packet for entities that include rotation in delta updates.
#[derive(Clone, Debug)]
pub enum EntityPositionRotSyncPacket {
    /// Delta-encoded position and rotation update.
    Delta(CMoveEntityPosRot),
    /// Full absolute position sync.
    Full(CEntityPositionSync),
}

/// Concrete movement sync packet ready for broadcast.
#[derive(Clone, Debug)]
pub enum EntityMovementSyncPacket {
    /// Delta-encoded position update.
    Position(CMoveEntityPos),
    /// Delta-encoded position and body rotation update.
    PositionRotation(CMoveEntityPosRot),
    /// Body rotation-only update.
    Rotation(CMoveEntityRot),
    /// Head yaw update.
    HeadRotation(CRotateHead),
    /// Full absolute position sync.
    PositionSync(CEntityPositionSync),
    /// Velocity sync.
    Velocity(CSetEntityMotion),
}

impl From<EntityPositionSyncPacket> for EntityMovementSyncPacket {
    fn from(packet: EntityPositionSyncPacket) -> Self {
        match packet {
            EntityPositionSyncPacket::Delta(packet) => Self::Position(packet),
            EntityPositionSyncPacket::Full(packet) => Self::PositionSync(packet),
        }
    }
}

impl From<EntityPositionRotSyncPacket> for EntityMovementSyncPacket {
    fn from(packet: EntityPositionRotSyncPacket) -> Self {
        match packet {
            EntityPositionRotSyncPacket::Delta(packet) => Self::PositionRotation(packet),
            EntityPositionRotSyncPacket::Full(packet) => Self::PositionSync(packet),
        }
    }
}

impl From<CMoveEntityRot> for EntityMovementSyncPacket {
    fn from(packet: CMoveEntityRot) -> Self {
        Self::Rotation(packet)
    }
}

impl From<CRotateHead> for EntityMovementSyncPacket {
    fn from(packet: CRotateHead) -> Self {
        Self::HeadRotation(packet)
    }
}

impl From<CSetEntityMotion> for EntityMovementSyncPacket {
    fn from(packet: CSetEntityMotion) -> Self {
        Self::Velocity(packet)
    }
}

/// At most two movement packets are emitted for one tracked movement update:
/// one body movement/rotation packet and one head-rotation packet.
#[derive(Clone, Debug, Default)]
pub struct EntityMovementSyncPackets {
    primary: Option<EntityMovementSyncPacket>,
    head_rotation: Option<EntityMovementSyncPacket>,
}

impl EntityMovementSyncPackets {
    /// Creates a bundle from body movement and head-rotation packets.
    fn new(primary: Option<EntityMovementSyncPacket>, head_rotation: Option<CRotateHead>) -> Self {
        Self {
            primary,
            head_rotation: head_rotation.map(EntityMovementSyncPacket::from),
        }
    }

    /// Returns whether no movement sync packets were selected.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.primary.is_none() && self.head_rotation.is_none()
    }

    /// Visits selected packets in vanilla send order.
    pub fn for_each(self, mut send: impl FnMut(EntityMovementSyncPacket)) {
        if let Some(packet) = self.primary {
            send(packet);
        }
        if let Some(packet) = self.head_rotation {
            send(packet);
        }
    }
}

/// Runtime values needed to build a vanilla position sync packet.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntityPositionSyncSnapshot {
    entity_id: i32,
    position: DVec3,
    velocity: DVec3,
    rotation: (f32, f32),
    on_ground: bool,
}

impl EntityPositionSyncSnapshot {
    /// Creates a packet snapshot for the current entity state.
    #[must_use]
    pub const fn new(
        entity_id: i32,
        position: DVec3,
        velocity: DVec3,
        rotation: (f32, f32),
        on_ground: bool,
    ) -> Self {
        Self {
            entity_id,
            position,
            velocity,
            rotation,
            on_ground,
        }
    }

    const fn full_sync_packet(self) -> CEntityPositionSync {
        CEntityPositionSync {
            entity_id: self.entity_id,
            x: self.position.x,
            y: self.position.y,
            z: self.position.z,
            velocity_x: self.velocity.x,
            velocity_y: self.velocity.y,
            velocity_z: self.velocity.z,
            yaw: self.rotation.0,
            pitch: self.rotation.1,
            on_ground: self.on_ground,
        }
    }
}

impl EntityPositionSyncDecision {
    /// Builds the protocol packet for a position-only entity update.
    #[must_use]
    pub const fn into_position_packet(
        self,
        snapshot: EntityPositionSyncSnapshot,
    ) -> EntityPositionSyncPacket {
        match self {
            Self::Delta { dx, dy, dz } => EntityPositionSyncPacket::Delta(CMoveEntityPos {
                entity_id: snapshot.entity_id,
                dx,
                dy,
                dz,
                on_ground: snapshot.on_ground,
            }),
            Self::Full => EntityPositionSyncPacket::Full(snapshot.full_sync_packet()),
        }
    }

    /// Builds the protocol packet for a position-and-rotation entity update.
    #[must_use]
    pub fn into_position_rot_packet(
        self,
        snapshot: EntityPositionSyncSnapshot,
    ) -> EntityPositionRotSyncPacket {
        match self {
            Self::Delta { dx, dy, dz } => EntityPositionRotSyncPacket::Delta(CMoveEntityPosRot {
                entity_id: snapshot.entity_id,
                dx,
                dy,
                dz,
                y_rot: to_angle_byte(snapshot.rotation.0),
                x_rot: to_angle_byte(snapshot.rotation.1),
                on_ground: snapshot.on_ground,
            }),
            Self::Full => EntityPositionRotSyncPacket::Full(snapshot.full_sync_packet()),
        }
    }
}

/// Per-entity position sync state shared by player and entity tracking.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntityPositionSyncState {
    last_sent_position: DVec3,
    last_sent_on_ground: bool,
    sync_delay: i32,
}

impl EntityPositionSyncState {
    /// Creates sync state at the position/on-ground state already known to clients.
    #[must_use]
    pub const fn new(position: DVec3, on_ground: bool) -> Self {
        Self {
            last_sent_position: position,
            last_sent_on_ground: on_ground,
            sync_delay: 0,
        }
    }

    /// Returns the last absolute position used as the client's delta base.
    #[must_use]
    pub const fn last_sent_position(self) -> DVec3 {
        self.last_sent_position
    }

    /// Returns the last on-ground state sent to tracking clients.
    #[must_use]
    pub const fn last_sent_on_ground(self) -> bool {
        self.last_sent_on_ground
    }

    /// Returns the current delay since the last full position sync.
    #[must_use]
    pub const fn sync_delay(self) -> i32 {
        self.sync_delay
    }

    /// Increments the full-sync delay and returns the previous value.
    pub const fn advance_sync_delay(&mut self) -> i32 {
        let delay = self.sync_delay;
        self.sync_delay += 1;
        delay
    }

    /// Returns whether `current_position` moved far enough to sync.
    #[must_use]
    pub fn position_changed(self, current_position: DVec3) -> bool {
        let diff = current_position - self.last_sent_position;
        diff.length_squared() >= POSITION_SYNC_THRESHOLD
    }

    /// Encodes the delta from the last sent position to `current_position`.
    ///
    /// Returns `None` when any component overflows the protocol delta range and
    /// the caller must send a full position sync instead.
    #[must_use]
    pub fn packed_delta(
        self,
        current_position: DVec3,
    ) -> Option<(PackedEntityDelta, PackedEntityDelta, PackedEntityDelta)> {
        let dx = calc_delta(current_position.x, self.last_sent_position.x)?;
        let dy = calc_delta(current_position.y, self.last_sent_position.y)?;
        let dz = calc_delta(current_position.z, self.last_sent_position.z)?;
        Some((dx, dy, dz))
    }

    /// Marks a delta movement packet as sent.
    pub const fn mark_delta_sent(&mut self, position: DVec3, on_ground: bool) {
        self.last_sent_position = position;
        self.last_sent_on_ground = on_ground;
    }

    /// Marks a full position sync packet as sent and resets the full-sync delay.
    pub const fn mark_full_sent(&mut self, position: DVec3, on_ground: bool) {
        self.last_sent_position = position;
        self.last_sent_on_ground = on_ground;
        self.sync_delay = 0;
    }

    /// Resets the delta base when vanilla updates `VecDeltaCodec` without a packet.
    pub const fn reset_base_without_packet(&mut self, position: DVec3, on_ground: bool) {
        self.last_sent_position = position;
        self.last_sent_on_ground = on_ground;
    }

    /// Selects and records the next movement sync form.
    ///
    /// Callers decide whether a sync is needed and whether vanilla forces a full
    /// sync for their tracking mode. This method owns the shared protocol delta
    /// overflow fallback and updates the sync base consistently.
    pub fn record_movement_sync(
        &mut self,
        position: DVec3,
        on_ground: bool,
        force_full: bool,
    ) -> EntityPositionSyncDecision {
        if !force_full && let Some((dx, dy, dz)) = self.packed_delta(position) {
            self.mark_delta_sent(position, on_ground);
            return EntityPositionSyncDecision::Delta { dx, dy, dz };
        }

        self.mark_full_sent(position, on_ground);
        EntityPositionSyncDecision::Full
    }
}

/// Per-entity velocity sync state for tracked entities.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntityVelocitySyncState {
    last_sent_velocity: DVec3,
}

impl EntityVelocitySyncState {
    /// Creates sync state at the velocity already known to clients.
    #[must_use]
    pub const fn new(velocity: DVec3) -> Self {
        Self {
            last_sent_velocity: velocity,
        }
    }

    /// Returns the last velocity sent to tracking clients.
    #[must_use]
    pub const fn last_sent_velocity(self) -> DVec3 {
        self.last_sent_velocity
    }

    /// Selects and records a velocity packet if vanilla requires one.
    pub fn record_velocity_sync(
        &mut self,
        entity_id: i32,
        current_velocity: DVec3,
    ) -> Option<CSetEntityMotion> {
        let diff = current_velocity - self.last_sent_velocity;
        let diff_sq = diff.length_squared();
        let became_stationary = diff_sq > 0.0 && current_velocity == DVec3::ZERO;
        if diff_sq <= VELOCITY_SYNC_THRESHOLD && !became_stationary {
            return None;
        }

        self.last_sent_velocity = current_velocity;
        Some(CSetEntityMotion::new(
            entity_id,
            current_velocity.x,
            current_velocity.y,
            current_velocity.z,
        ))
    }
}

/// Per-entity movement sync state for tracked position and rotation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntityMovementSyncState {
    position: EntityPositionSyncState,
    rotation: EntityRotationSyncState,
}

/// Runtime values accepted by tracked entity movement sync.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntityMovementSyncUpdate {
    /// Entity network id.
    pub entity_id: i32,
    /// Whether this update includes position.
    pub has_position: bool,
    /// Whether this update includes body/head rotation.
    pub has_rotation: bool,
    /// Current entity position, or the previous position for rotation-only updates.
    pub position: DVec3,
    /// Current entity velocity.
    pub velocity: DVec3,
    /// Current body yaw and pitch in degrees.
    pub body_rotation: (f32, f32),
    /// Current head yaw in degrees.
    pub head_yaw: f32,
    /// Current on-ground flag.
    pub on_ground: bool,
}

/// Per-tracked-entity movement state owned by the entity tracker.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ServerEntityMovementSyncState {
    position: EntityPositionSyncState,
    rotation: EntityRotationSyncState,
    velocity: EntityVelocitySyncState,
    update_interval: i32,
    track_delta: bool,
    tick_count: i32,
    teleport_delay: i32,
    was_riding: bool,
    was_on_ground: bool,
}

/// Runtime values accepted by vanilla `ServerEntity.sendChanges` movement sync.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ServerEntityMovementSyncUpdate {
    /// Entity network id.
    pub entity_id: i32,
    /// Whether the entity is currently riding another entity.
    pub is_passenger: bool,
    /// Current entity tracking position.
    pub position: DVec3,
    /// Current entity velocity.
    pub velocity: DVec3,
    /// Current body yaw and pitch in degrees.
    pub body_rotation: (f32, f32),
    /// Current head yaw in degrees.
    pub head_yaw: f32,
    /// Current on-ground flag.
    pub on_ground: bool,
    /// Vanilla `Entity.needsSync`.
    pub needs_velocity_sync: bool,
    /// Whether synced entity data is dirty this tick.
    pub has_dirty_entity_data: bool,
    /// Vanilla living fall-flying velocity sync exception.
    pub force_velocity_sync: bool,
}

/// Movement packets and side effects selected by one `ServerEntity.sendChanges` pass.
#[derive(Clone, Debug, Default)]
pub struct ServerEntityMovementSyncResult {
    packets: Vec<EntityMovementSyncPacket>,
    clear_velocity_sync: bool,
}

impl ServerEntityMovementSyncResult {
    /// Returns whether vanilla `Entity.needsSync` should be cleared after processing.
    #[must_use]
    pub const fn should_clear_velocity_sync(&self) -> bool {
        self.clear_velocity_sync
    }

    /// Visits selected packets in vanilla send order.
    pub fn for_each_packet(self, send: impl FnMut(EntityMovementSyncPacket)) {
        self.packets.into_iter().for_each(send);
    }
}

impl ServerEntityMovementSyncState {
    /// Creates movement sync state for a newly tracked entity.
    #[must_use]
    pub fn new(
        position: DVec3,
        velocity: DVec3,
        on_ground: bool,
        body_rotation: (f32, f32),
        head_yaw: f32,
        update_interval: i32,
        track_delta: bool,
    ) -> Self {
        Self {
            position: EntityPositionSyncState::new(position, on_ground),
            rotation: EntityRotationSyncState::new(body_rotation, head_yaw),
            velocity: EntityVelocitySyncState::new(velocity),
            update_interval,
            track_delta,
            tick_count: 0,
            teleport_delay: 0,
            was_riding: false,
            was_on_ground: on_ground,
        }
    }

    /// Selects packets for a vanilla `ServerEntity.sendChanges` movement pass.
    #[must_use]
    pub fn record_send_changes(
        &mut self,
        update: ServerEntityMovementSyncUpdate,
    ) -> ServerEntityMovementSyncResult {
        let mut result = ServerEntityMovementSyncResult::default();
        let should_process =
            self.should_process(update.needs_velocity_sync, update.has_dirty_entity_data);
        if should_process {
            result.clear_velocity_sync = update.needs_velocity_sync;
            let should_send_rotation = self.rotation.body_rotation_changed(update.body_rotation);

            if update.is_passenger {
                self.record_passenger_update(update, should_send_rotation, &mut result);
            } else {
                self.record_non_passenger_update(update, should_send_rotation, &mut result);
            }

            if let Some(head_y_rot) = self.rotation.record_head_yaw(update.head_yaw) {
                result
                    .packets
                    .push(EntityMovementSyncPacket::from(CRotateHead {
                        entity_id: update.entity_id,
                        head_y_rot,
                    }));
            }
        }

        self.tick_count = self.tick_count.wrapping_add(1);
        result
    }

    const fn should_process(self, needs_velocity_sync: bool, has_dirty_entity_data: bool) -> bool {
        needs_velocity_sync
            || has_dirty_entity_data
            || (self.update_interval > 0 && self.tick_count % self.update_interval == 0)
    }

    fn record_passenger_update(
        &mut self,
        update: ServerEntityMovementSyncUpdate,
        should_send_rotation: bool,
        result: &mut ServerEntityMovementSyncResult,
    ) {
        if should_send_rotation
            && let Some(body_rotation) = self.rotation.record_body_rotation(update.body_rotation)
        {
            result
                .packets
                .push(EntityMovementSyncPacket::from(CMoveEntityRot {
                    entity_id: update.entity_id,
                    y_rot: body_rotation.yaw(),
                    x_rot: body_rotation.pitch(),
                    on_ground: update.on_ground,
                }));
        }

        self.position
            .reset_base_without_packet(update.position, update.on_ground);
        self.was_riding = true;
    }

    fn record_non_passenger_update(
        &mut self,
        update: ServerEntityMovementSyncUpdate,
        should_send_rotation: bool,
        result: &mut ServerEntityMovementSyncResult,
    ) {
        self.teleport_delay = self.teleport_delay.wrapping_add(1);

        let position_changed = self.position.position_changed(update.position);
        let should_send_position =
            position_changed || self.tick_count % FORCED_POS_UPDATE_PERIOD == 0;
        let delta_too_big = self.position.packed_delta(update.position).is_none();
        let force_full = delta_too_big
            || self.teleport_delay > FORCED_TELEPORT_PERIOD
            || self.was_riding
            || self.was_on_ground != update.on_ground;

        if (update.needs_velocity_sync || self.track_delta || update.force_velocity_sync)
            && let Some(packet) = self
                .velocity
                .record_velocity_sync(update.entity_id, update.velocity)
        {
            result.packets.push(EntityMovementSyncPacket::from(packet));
        }

        if force_full {
            self.was_on_ground = update.on_ground;
            self.teleport_delay = 0;
            let decision =
                self.position
                    .record_movement_sync(update.position, update.on_ground, true);
            self.rotation.mark_body_rotation_sent(update.body_rotation);
            result.packets.push(EntityMovementSyncPacket::from(
                decision.into_position_rot_packet(EntityPositionSyncSnapshot::new(
                    update.entity_id,
                    update.position,
                    update.velocity,
                    update.body_rotation,
                    update.on_ground,
                )),
            ));
        } else if should_send_position && should_send_rotation {
            let decision =
                self.position
                    .record_movement_sync(update.position, update.on_ground, false);
            self.rotation.mark_body_rotation_sent(update.body_rotation);
            result.packets.push(EntityMovementSyncPacket::from(
                decision.into_position_rot_packet(EntityPositionSyncSnapshot::new(
                    update.entity_id,
                    update.position,
                    update.velocity,
                    update.body_rotation,
                    update.on_ground,
                )),
            ));
        } else if should_send_position {
            let decision =
                self.position
                    .record_movement_sync(update.position, update.on_ground, false);
            result.packets.push(EntityMovementSyncPacket::from(
                decision.into_position_packet(EntityPositionSyncSnapshot::new(
                    update.entity_id,
                    update.position,
                    update.velocity,
                    update.body_rotation,
                    update.on_ground,
                )),
            ));
        } else if should_send_rotation
            && let Some(body_rotation) = self.rotation.record_body_rotation(update.body_rotation)
        {
            result
                .packets
                .push(EntityMovementSyncPacket::from(CMoveEntityRot {
                    entity_id: update.entity_id,
                    y_rot: body_rotation.yaw(),
                    x_rot: body_rotation.pitch(),
                    on_ground: update.on_ground,
                }));
        }

        self.was_riding = false;
    }
}

impl EntityMovementSyncState {
    /// Creates movement sync state for values already known to tracking clients.
    #[must_use]
    pub fn new(position: DVec3, on_ground: bool, body_rotation: (f32, f32), head_yaw: f32) -> Self {
        Self {
            position: EntityPositionSyncState::new(position, on_ground),
            rotation: EntityRotationSyncState::new(body_rotation, head_yaw),
        }
    }

    /// Returns the last absolute position used as the client's delta base.
    #[must_use]
    pub const fn last_sent_position(self) -> DVec3 {
        self.position.last_sent_position()
    }

    /// Selects and records a position sync that forces full packets after a delay.
    ///
    /// Vanilla player movement uses this form: delta packets are sent while the
    /// packed delta base is fresh, then a full position sync refreshes that base.
    pub fn record_position_sync_with_full_delay(
        &mut self,
        position: DVec3,
        on_ground: bool,
        full_sync_delay: i32,
    ) -> EntityPositionSyncDecision {
        let delay = self.position.advance_sync_delay();
        let on_ground_changed = self.position.last_sent_on_ground() != on_ground;
        let force_full = delay > full_sync_delay || on_ground_changed;
        self.position
            .record_movement_sync(position, on_ground, force_full)
    }

    /// Selects and records packets for a tracked movement update.
    pub fn record_update_with_full_delay(
        &mut self,
        update: EntityMovementSyncUpdate,
        full_sync_delay: i32,
    ) -> EntityMovementSyncPackets {
        let head_rotation = if update.has_rotation {
            self.record_head_yaw(update.head_yaw)
                .map(|head_y_rot| CRotateHead {
                    entity_id: update.entity_id,
                    head_y_rot,
                })
        } else {
            None
        };

        let primary = if update.has_position {
            let decision = self.record_position_sync_with_full_delay(
                update.position,
                update.on_ground,
                full_sync_delay,
            );
            let position_includes_rotation = matches!(decision, EntityPositionSyncDecision::Full);
            let body_rotation = if position_includes_rotation {
                self.mark_body_rotation_sent(update.body_rotation);
                None
            } else if update.has_rotation {
                self.record_body_rotation(update.body_rotation)
            } else {
                None
            };
            let snapshot = EntityPositionSyncSnapshot::new(
                update.entity_id,
                update.position,
                update.velocity,
                update.body_rotation,
                update.on_ground,
            );

            if position_includes_rotation || body_rotation.is_some() {
                Some(EntityMovementSyncPacket::from(
                    decision.into_position_rot_packet(snapshot),
                ))
            } else {
                Some(EntityMovementSyncPacket::from(
                    decision.into_position_packet(snapshot),
                ))
            }
        } else if update.has_rotation {
            self.record_body_rotation(update.body_rotation)
                .map(|body_rotation| {
                    EntityMovementSyncPacket::from(CMoveEntityRot {
                        entity_id: update.entity_id,
                        y_rot: body_rotation.yaw(),
                        x_rot: body_rotation.pitch(),
                        on_ground: update.on_ground,
                    })
                })
        } else {
            None
        };

        EntityMovementSyncPackets::new(primary, head_rotation)
    }

    /// Records a body rotation packet when the packed yaw or pitch changed.
    pub fn record_body_rotation(&mut self, rotation: (f32, f32)) -> Option<PackedEntityRotation> {
        self.rotation.record_body_rotation(rotation)
    }

    /// Marks body rotation as sent because a full position sync includes it.
    pub fn mark_body_rotation_sent(&mut self, rotation: (f32, f32)) {
        self.rotation.mark_body_rotation_sent(rotation);
    }

    /// Records a head-rotation packet when the packed yaw changed.
    pub fn record_head_yaw(&mut self, head_yaw: f32) -> Option<i8> {
        self.rotation.record_head_yaw(head_yaw)
    }
}

#[cfg(test)]
mod tests {
    use glam::DVec3;
    use steel_protocol::packets::game::{calc_delta, to_angle_byte};

    use super::{
        EntityMovementSyncPacket, EntityMovementSyncState, EntityMovementSyncUpdate,
        EntityPositionRotSyncPacket, EntityPositionSyncDecision, EntityPositionSyncPacket,
        EntityPositionSyncSnapshot, EntityPositionSyncState, EntityRotationSyncState,
        EntityVelocitySyncState, PackedEntityRotation, ServerEntityMovementSyncState,
        ServerEntityMovementSyncUpdate,
    };

    #[test]
    fn movement_sync_records_delta_when_packed_delta_fits() {
        let mut state = EntityPositionSyncState::new(DVec3::ZERO, false);
        state.advance_sync_delay();

        let position = DVec3::new(0.25, -0.125, 0.5);
        let decision = state.record_movement_sync(position, true, false);

        assert_eq!(
            decision,
            EntityPositionSyncDecision::Delta {
                dx: calc_delta(position.x, 0.0).expect("delta should fit"),
                dy: calc_delta(position.y, 0.0).expect("delta should fit"),
                dz: calc_delta(position.z, 0.0).expect("delta should fit"),
            }
        );
        assert_eq!(state.last_sent_position(), position);
        assert!(state.last_sent_on_ground());
        assert_eq!(state.sync_delay(), 1);
    }

    #[test]
    fn movement_sync_records_full_when_forced() {
        let mut state = EntityPositionSyncState::new(DVec3::ZERO, false);
        state.advance_sync_delay();

        let decision = state.record_movement_sync(DVec3::new(0.25, 0.0, 0.0), true, true);

        assert_eq!(decision, EntityPositionSyncDecision::Full);
        assert_eq!(state.last_sent_position(), DVec3::new(0.25, 0.0, 0.0));
        assert!(state.last_sent_on_ground());
        assert_eq!(state.sync_delay(), 0);
    }

    #[test]
    fn movement_sync_records_full_when_delta_overflows() {
        let mut state = EntityPositionSyncState::new(DVec3::ZERO, false);

        let decision = state.record_movement_sync(DVec3::new(10.0, 0.0, 0.0), false, false);

        assert_eq!(decision, EntityPositionSyncDecision::Full);
        assert_eq!(state.last_sent_position(), DVec3::new(10.0, 0.0, 0.0));
    }

    #[test]
    fn rotation_sync_records_body_rotation_only_when_packed_angle_changes() {
        let mut state = EntityRotationSyncState::new((0.0, 0.0), 0.0);

        assert_eq!(state.record_body_rotation((0.5, 0.5)), None);
        assert_eq!(
            state.record_body_rotation((2.0, 0.0)),
            Some(PackedEntityRotation {
                yaw: to_angle_byte(2.0),
                pitch: to_angle_byte(0.0),
            })
        );
        assert_eq!(state.record_body_rotation((2.5, 0.0)), None);
        assert_eq!(
            state.record_body_rotation((2.5, 2.0)),
            Some(PackedEntityRotation {
                yaw: to_angle_byte(2.5),
                pitch: to_angle_byte(2.0),
            })
        );
    }

    #[test]
    fn rotation_sync_records_head_rotation_only_when_packed_angle_changes() {
        let mut state = EntityRotationSyncState::new((0.0, 0.0), 0.0);

        assert_eq!(state.record_head_yaw(0.5), None);
        assert_eq!(state.record_head_yaw(2.0), Some(to_angle_byte(2.0)));
        assert_eq!(state.record_head_yaw(2.5), None);
    }

    #[test]
    fn velocity_sync_records_packet_when_delta_exceeds_threshold() {
        let mut state = EntityVelocitySyncState::new(DVec3::ZERO);

        let packet = state
            .record_velocity_sync(12, DVec3::new(0.001, 0.0, 0.0))
            .expect("velocity should sync");

        assert_eq!(packet.entity_id, 12);
        assert_eq!(packet.velocity_x.to_bits(), 0.001_f64.to_bits());
        assert_eq!(packet.velocity_y.to_bits(), 0.0_f64.to_bits());
        assert_eq!(packet.velocity_z.to_bits(), 0.0_f64.to_bits());
        assert_eq!(state.last_sent_velocity(), DVec3::new(0.001, 0.0, 0.0));
    }

    #[test]
    fn velocity_sync_skips_sub_threshold_non_zero_delta() {
        let mut state = EntityVelocitySyncState::new(DVec3::ZERO);

        assert!(
            state
                .record_velocity_sync(12, DVec3::new(0.000_1, 0.0, 0.0))
                .is_none()
        );
        assert_eq!(state.last_sent_velocity(), DVec3::ZERO);
    }

    #[test]
    fn velocity_sync_records_packet_when_entity_becomes_stationary() {
        let mut state = EntityVelocitySyncState::new(DVec3::new(0.000_1, 0.0, 0.0));

        let packet = state
            .record_velocity_sync(12, DVec3::ZERO)
            .expect("stationary transition should sync");

        assert_eq!(packet.entity_id, 12);
        assert_eq!(packet.velocity_x.to_bits(), 0.0_f64.to_bits());
        assert_eq!(packet.velocity_y.to_bits(), 0.0_f64.to_bits());
        assert_eq!(packet.velocity_z.to_bits(), 0.0_f64.to_bits());
        assert_eq!(state.last_sent_velocity(), DVec3::ZERO);
    }

    #[test]
    fn movement_sync_state_tracks_position_and_rotation_together() {
        let mut state = EntityMovementSyncState::new(DVec3::ZERO, false, (0.0, 0.0), 0.0);

        let decision =
            state.record_position_sync_with_full_delay(DVec3::new(0.25, 0.0, 0.0), true, 400);
        assert_eq!(decision, EntityPositionSyncDecision::Full);
        assert_eq!(state.last_sent_position(), DVec3::new(0.25, 0.0, 0.0));

        assert_eq!(state.record_body_rotation((0.5, 0.5)), None);
        assert_eq!(
            state.record_body_rotation((2.0, 0.0)),
            Some(PackedEntityRotation {
                yaw: to_angle_byte(2.0),
                pitch: to_angle_byte(0.0),
            })
        );
        state.mark_body_rotation_sent((90.0, 45.0));
        assert_eq!(state.record_body_rotation((90.0, 45.0)), None);
        assert_eq!(state.record_head_yaw(2.0), Some(to_angle_byte(2.0)));
    }

    #[test]
    fn movement_sync_update_emits_position_rotation_before_head_rotation() {
        let mut state = EntityMovementSyncState::new(DVec3::ZERO, false, (0.0, 0.0), 0.0);
        let position = DVec3::new(0.25, 0.0, 0.0);
        let update = EntityMovementSyncUpdate {
            entity_id: 12,
            has_position: true,
            has_rotation: true,
            position,
            velocity: DVec3::new(1.0, 2.0, 3.0),
            body_rotation: (2.0, 0.0),
            head_yaw: 2.0,
            on_ground: false,
        };

        let packets = state.record_update_with_full_delay(update, 400);
        let mut emitted = Vec::new();
        packets.for_each(|packet| emitted.push(packet));

        assert_eq!(emitted.len(), 2);
        let EntityMovementSyncPacket::PositionRotation(packet) = &emitted[0] else {
            panic!("expected position-rotation packet");
        };
        assert_eq!(packet.entity_id, 12);
        assert_eq!(
            packet.dx,
            calc_delta(position.x, 0.0).expect("delta should fit")
        );
        assert_eq!(packet.y_rot, to_angle_byte(2.0));
        assert_eq!(packet.x_rot, to_angle_byte(0.0));

        let EntityMovementSyncPacket::HeadRotation(packet) = &emitted[1] else {
            panic!("expected head-rotation packet");
        };
        assert_eq!(packet.entity_id, 12);
        assert_eq!(packet.head_y_rot, to_angle_byte(2.0));
    }

    #[test]
    fn movement_sync_update_full_position_marks_body_and_head_rotation_sent() {
        let mut state = EntityMovementSyncState::new(DVec3::ZERO, false, (0.0, 0.0), 0.0);
        let full_update = EntityMovementSyncUpdate {
            entity_id: 12,
            has_position: true,
            has_rotation: true,
            position: DVec3::new(0.25, 0.0, 0.0),
            velocity: DVec3::ZERO,
            body_rotation: (90.0, 45.0),
            head_yaw: 90.0,
            on_ground: true,
        };

        let packets = state.record_update_with_full_delay(full_update, 400);
        let mut emitted = Vec::new();
        packets.for_each(|packet| emitted.push(packet));

        assert_eq!(emitted.len(), 2);
        assert!(matches!(
            emitted[0],
            EntityMovementSyncPacket::PositionSync(_)
        ));
        assert!(matches!(
            emitted[1],
            EntityMovementSyncPacket::HeadRotation(_)
        ));

        let rotation_only_update = EntityMovementSyncUpdate {
            has_position: false,
            position: full_update.position,
            ..full_update
        };
        let packets = state.record_update_with_full_delay(rotation_only_update, 400);

        assert!(packets.is_empty());
    }

    fn collect_server_packets(
        state: &mut ServerEntityMovementSyncState,
        update: ServerEntityMovementSyncUpdate,
    ) -> Vec<EntityMovementSyncPacket> {
        let result = state.record_send_changes(update);
        let mut packets = Vec::new();
        result.for_each_packet(|packet| packets.push(packet));
        packets
    }

    fn server_update(position: DVec3, velocity: DVec3) -> ServerEntityMovementSyncUpdate {
        ServerEntityMovementSyncUpdate {
            entity_id: 12,
            is_passenger: false,
            position,
            velocity,
            body_rotation: (0.0, 0.0),
            head_yaw: 0.0,
            on_ground: false,
            needs_velocity_sync: false,
            has_dirty_entity_data: false,
            force_velocity_sync: false,
        }
    }

    #[test]
    fn server_entity_sync_sends_track_delta_velocity_before_position() {
        let mut state = ServerEntityMovementSyncState::new(
            DVec3::ZERO,
            DVec3::ZERO,
            false,
            (0.0, 0.0),
            0.0,
            20,
            true,
        );
        let packets = collect_server_packets(
            &mut state,
            server_update(DVec3::new(0.25, 0.0, 0.0), DVec3::new(0.001, 0.0, 0.0)),
        );

        assert_eq!(packets.len(), 2);
        assert!(matches!(packets[0], EntityMovementSyncPacket::Velocity(_)));
        assert!(matches!(packets[1], EntityMovementSyncPacket::Position(_)));
    }

    #[test]
    fn server_entity_sync_skips_velocity_for_non_track_delta_without_needs_sync() {
        let mut state = ServerEntityMovementSyncState::new(
            DVec3::ZERO,
            DVec3::ZERO,
            false,
            (0.0, 0.0),
            0.0,
            2,
            false,
        );
        let packets = collect_server_packets(
            &mut state,
            server_update(DVec3::new(0.25, 0.0, 0.0), DVec3::new(0.001, 0.0, 0.0)),
        );

        assert_eq!(packets.len(), 1);
        assert!(matches!(packets[0], EntityMovementSyncPacket::Position(_)));
    }

    #[test]
    fn server_entity_sync_processes_and_clears_explicit_needs_sync() {
        let mut state = ServerEntityMovementSyncState::new(
            DVec3::ZERO,
            DVec3::ZERO,
            false,
            (0.0, 0.0),
            0.0,
            20,
            false,
        );
        let mut update = server_update(DVec3::new(0.25, 0.0, 0.0), DVec3::new(0.001, 0.0, 0.0));
        update.needs_velocity_sync = true;

        let result = state.record_send_changes(update);
        assert!(result.should_clear_velocity_sync());
        let mut packets = Vec::new();
        result.for_each_packet(|packet| packets.push(packet));

        assert_eq!(packets.len(), 2);
        assert!(matches!(packets[0], EntityMovementSyncPacket::Velocity(_)));
        assert!(matches!(packets[1], EntityMovementSyncPacket::Position(_)));
    }

    #[test]
    fn server_entity_sync_processes_dirty_data_gate_between_intervals() {
        let mut state = ServerEntityMovementSyncState::new(
            DVec3::ZERO,
            DVec3::ZERO,
            false,
            (0.0, 0.0),
            0.0,
            20,
            false,
        );
        let first_packets =
            collect_server_packets(&mut state, server_update(DVec3::ZERO, DVec3::ZERO));
        assert_eq!(first_packets.len(), 1);

        let mut update = server_update(DVec3::ZERO, DVec3::ZERO);
        update.body_rotation = (2.0, 0.0);
        update.has_dirty_entity_data = true;

        let packets = collect_server_packets(&mut state, update);

        assert_eq!(packets.len(), 1);
        assert!(matches!(packets[0], EntityMovementSyncPacket::Rotation(_)));
    }

    #[test]
    fn server_entity_sync_forces_full_position_when_on_ground_changes() {
        let mut state = ServerEntityMovementSyncState::new(
            DVec3::ZERO,
            DVec3::ZERO,
            false,
            (0.0, 0.0),
            0.0,
            20,
            false,
        );
        let mut update = server_update(DVec3::new(0.25, 0.0, 0.0), DVec3::ZERO);
        update.on_ground = true;

        let packets = collect_server_packets(&mut state, update);

        assert_eq!(packets.len(), 1);
        assert!(matches!(
            packets[0],
            EntityMovementSyncPacket::PositionSync(_)
        ));
    }

    #[test]
    fn server_entity_sync_rotates_passenger_without_position_packet() {
        let mut state = ServerEntityMovementSyncState::new(
            DVec3::ZERO,
            DVec3::ZERO,
            false,
            (0.0, 0.0),
            0.0,
            1,
            true,
        );
        let mut update = server_update(DVec3::new(0.25, 0.0, 0.0), DVec3::new(0.001, 0.0, 0.0));
        update.is_passenger = true;
        update.body_rotation = (2.0, 0.0);

        let packets = collect_server_packets(&mut state, update);

        assert_eq!(packets.len(), 1);
        assert!(matches!(packets[0], EntityMovementSyncPacket::Rotation(_)));
    }

    #[test]
    fn sync_decision_builds_position_delta_packet() {
        let position = DVec3::new(0.25, 0.0, -0.5);
        let decision = EntityPositionSyncDecision::Delta {
            dx: calc_delta(position.x, 0.0).expect("delta should fit"),
            dy: calc_delta(position.y, 0.0).expect("delta should fit"),
            dz: calc_delta(position.z, 0.0).expect("delta should fit"),
        };

        let packet = decision.into_position_packet(EntityPositionSyncSnapshot::new(
            12,
            position,
            DVec3::new(1.0, 2.0, 3.0),
            (90.0, 45.0),
            true,
        ));

        let EntityPositionSyncPacket::Delta(packet) = packet else {
            panic!("expected delta packet");
        };
        assert_eq!(packet.entity_id, 12);
        assert_eq!(
            packet.dx,
            calc_delta(position.x, 0.0).expect("delta should fit")
        );
        assert_eq!(
            packet.dy,
            calc_delta(position.y, 0.0).expect("delta should fit")
        );
        assert_eq!(
            packet.dz,
            calc_delta(position.z, 0.0).expect("delta should fit")
        );
        assert!(packet.on_ground);
    }

    #[test]
    fn sync_decision_builds_position_rotation_delta_packet() {
        let position = DVec3::new(0.25, 0.0, -0.5);
        let decision = EntityPositionSyncDecision::Delta {
            dx: calc_delta(position.x, 0.0).expect("delta should fit"),
            dy: calc_delta(position.y, 0.0).expect("delta should fit"),
            dz: calc_delta(position.z, 0.0).expect("delta should fit"),
        };

        let packet = decision.into_position_rot_packet(EntityPositionSyncSnapshot::new(
            12,
            position,
            DVec3::new(1.0, 2.0, 3.0),
            (90.0, 45.0),
            true,
        ));

        let EntityPositionRotSyncPacket::Delta(packet) = packet else {
            panic!("expected position-rotation delta packet");
        };
        assert_eq!(packet.entity_id, 12);
        assert_eq!(
            packet.dx,
            calc_delta(position.x, 0.0).expect("delta should fit")
        );
        assert_eq!(
            packet.dy,
            calc_delta(position.y, 0.0).expect("delta should fit")
        );
        assert_eq!(
            packet.dz,
            calc_delta(position.z, 0.0).expect("delta should fit")
        );
        assert_eq!(packet.y_rot, to_angle_byte(90.0));
        assert_eq!(packet.x_rot, to_angle_byte(45.0));
        assert!(packet.on_ground);
    }

    #[test]
    fn sync_decision_builds_full_position_sync_packet() {
        let snapshot = EntityPositionSyncSnapshot::new(
            12,
            DVec3::new(10.0, 20.0, 30.0),
            DVec3::new(1.0, 2.0, 3.0),
            (90.0, 45.0),
            true,
        );

        let packet = EntityPositionSyncDecision::Full.into_position_packet(snapshot);

        let EntityPositionSyncPacket::Full(packet) = packet else {
            panic!("expected full packet");
        };
        assert_eq!(packet.entity_id, 12);
        assert_eq!(packet.x.to_bits(), 10.0_f64.to_bits());
        assert_eq!(packet.y.to_bits(), 20.0_f64.to_bits());
        assert_eq!(packet.z.to_bits(), 30.0_f64.to_bits());
        assert_eq!(packet.velocity_x.to_bits(), 1.0_f64.to_bits());
        assert_eq!(packet.velocity_y.to_bits(), 2.0_f64.to_bits());
        assert_eq!(packet.velocity_z.to_bits(), 3.0_f64.to_bits());
        assert_eq!(packet.yaw.to_bits(), 90.0_f32.to_bits());
        assert_eq!(packet.pitch.to_bits(), 45.0_f32.to_bits());
        assert!(packet.on_ground);
    }
}
