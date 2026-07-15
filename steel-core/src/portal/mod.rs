//! World portal system for nether/end portals and future portal types.
//!
//! Vanilla commonly calls loaded worlds "dimensions". Steel uses "world" for
//! loaded runtime worlds and reserves "dimension type" for the vanilla registry
//! entry that defines world rules.

use crate::entity::{Entity, PendingWorldChangeToken};
use crate::world::World;
use glam::DVec3;
use smallvec::SmallVec;
use std::sync::Arc;
use steel_protocol::packets::game::RelativeMovement;
use steel_registry::game_rules::GameRuleRef;
use steel_registry::vanilla_game_rules::{
    PLAYERS_NETHER_PORTAL_CREATIVE_DELAY, PLAYERS_NETHER_PORTAL_DEFAULT_DELAY,
};
use steel_utils::BlockPos;

pub(crate) mod end_gateway;
pub(crate) mod end_portal;
pub(crate) mod nether_portal;
pub mod portal_shape;

/// Vanilla portal behavior kind tracked by an entity while it is inside a portal.
///
/// Java stores a reference to the `Portal` block behavior object. Steel keeps a
/// compact explicit kind here so entity state does not depend on block behavior
/// object identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortalKind {
    /// Vanilla nether portal block.
    Nether,
    /// Vanilla end portal block.
    End,
    /// Vanilla end gateway block.
    EndGateway,
}

impl PortalKind {
    /// Returns vanilla `Portal.getPortalTransitionTime`.
    #[must_use]
    pub fn transition_time(self, world: &World, entity: &dyn Entity) -> i32 {
        let player_invulnerable = entity
            .as_player()
            .map(|player| player.abilities.lock().invulnerable);
        self.transition_time_for_player_state(world, player_invulnerable)
    }

    /// Returns vanilla `Portal.getPortalTransitionTime` from object-safe entity state.
    #[must_use]
    pub fn transition_time_for_player_state(
        self,
        world: &World,
        player_invulnerable: Option<bool>,
    ) -> i32 {
        match self {
            Self::Nether => nether_portal_transition_time(world, player_invulnerable),
            Self::End | Self::EndGateway => 0,
        }
    }
}

fn nether_portal_transition_time(world: &World, player_invulnerable: Option<bool>) -> i32 {
    let Some(player_invulnerable) = player_invulnerable else {
        return 0;
    };

    let rule = nether_portal_transition_rule(player_invulnerable);
    let delay = portal_transition_game_rule(world, rule);
    clamped_portal_transition_time(delay)
}

fn nether_portal_transition_rule(player_invulnerable: bool) -> GameRuleRef<i32> {
    if player_invulnerable {
        &PLAYERS_NETHER_PORTAL_CREATIVE_DELAY
    } else {
        &PLAYERS_NETHER_PORTAL_DEFAULT_DELAY
    }
}

fn clamped_portal_transition_time(delay: i32) -> i32 {
    delay.max(0)
}

fn portal_transition_game_rule(world: &World, rule: GameRuleRef<i32>) -> i32 {
    world.get_game_rule(rule)
}

/// Result of advancing an entity's active portal process for one server tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortalProcessResult {
    /// The entity has not reached the portal transition threshold.
    Waiting,
    /// The portal transition threshold was reached this tick.
    Ready,
}

/// Per-entity portal timer state.
///
/// Mirrors vanilla `PortalProcessor`: the active portal kind, the entry block
/// position, the accumulated portal time, and whether the entity touched the
/// portal during the current tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortalProcessor {
    portal: PortalKind,
    entry_position: BlockPos,
    portal_time: i32,
    inside_portal_this_tick: bool,
}

impl PortalProcessor {
    /// Creates a portal process for a freshly entered portal.
    #[must_use]
    pub const fn new(portal: PortalKind, entry_position: BlockPos) -> Self {
        Self {
            portal,
            entry_position,
            portal_time: 0,
            inside_portal_this_tick: true,
        }
    }

    /// Returns the tracked portal kind.
    #[must_use]
    pub const fn portal(self) -> PortalKind {
        self.portal
    }

    /// Returns the portal block position the entity entered from.
    #[must_use]
    pub const fn entry_position(self) -> BlockPos {
        self.entry_position
    }

    /// Returns the accumulated portal time.
    #[must_use]
    pub const fn portal_time(self) -> i32 {
        self.portal_time
    }

    /// Returns whether the entity touched this portal during the current tick.
    #[must_use]
    pub const fn is_inside_portal_this_tick(self) -> bool {
        self.inside_portal_this_tick
    }

    /// Returns true if this process tracks the same portal behavior.
    #[must_use]
    pub fn is_same_portal(self, portal: PortalKind) -> bool {
        self.portal == portal
    }

    /// Marks this process as touched by the entity for the current tick.
    pub const fn set_as_inside_portal(&mut self, entry_position: BlockPos) {
        if !self.inside_portal_this_tick {
            self.entry_position = entry_position;
            self.inside_portal_this_tick = true;
        }
    }

    /// Advances vanilla portal timing for one server tick.
    pub fn process_portal_teleportation(
        &mut self,
        allowed_to_teleport: bool,
        transition_time: i32,
    ) -> PortalProcessResult {
        if !self.inside_portal_this_tick {
            self.decay_tick();
            return PortalProcessResult::Waiting;
        }

        self.inside_portal_this_tick = false;
        if !allowed_to_teleport {
            return PortalProcessResult::Waiting;
        }

        let ready = self.portal_time >= transition_time;
        self.portal_time += 1;
        if ready {
            PortalProcessResult::Ready
        } else {
            PortalProcessResult::Waiting
        }
    }

    fn decay_tick(&mut self) {
        self.portal_time = self.portal_time.saturating_sub(4).max(0);
    }

    /// Returns true when vanilla would clear the active portal process.
    #[must_use]
    pub const fn has_expired(self) -> bool {
        self.portal_time <= 0
    }
}

/// Describes a teleport transition to another loaded world.
///
/// Vanilla names loaded worlds "dimensions" in packets and saves. Steel uses
/// "world" for runtime loaded world instances, reserving "dimension type" for
/// the vanilla registry entry that defines height, skylight, ceiling, etc.
#[derive(Clone)]
pub struct TeleportTransition {
    /// The target world to teleport into.
    pub target_world: Arc<World>,
    /// The position in the target world.
    pub position: DVec3,
    /// The rotation (yaw, pitch) values, interpreted by `relatives`.
    pub rotation: (f32, f32),
    /// The velocity component carried by this transition, interpreted by `relatives`.
    pub velocity: DVec3,
    /// Vanilla relative movement flags carried through to clientbound player position packets.
    pub relatives: RelativeMovement,
    /// Portal cooldown in ticks (prevents immediate re-entry).
    pub portal_cooldown: i32,
    /// Whether this transition is being applied recursively to a passenger.
    pub as_passenger: bool,
    /// Side effects vanilla runs after the entity has reached the target world.
    pub post_transition: TeleportPostTransition,
}

/// Vanilla post-teleport side effects, composed in transition order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeleportPostTransition {
    actions: SmallVec<[TeleportPostAction; 2]>,
}

impl TeleportPostTransition {
    /// No post-transition work.
    #[must_use]
    pub fn do_nothing() -> Self {
        Self {
            actions: SmallVec::new(),
        }
    }

    /// Plays vanilla's portal travel level event for players.
    #[must_use]
    pub fn play_portal_sound() -> Self {
        Self::single(TeleportPostAction::PlayPortalSound)
    }

    /// Places a portal chunk ticket after the transition.
    #[must_use]
    pub fn place_portal_ticket(target: PortalTicketTarget) -> Self {
        Self::single(TeleportPostAction::PlacePortalTicket(target))
    }

    /// Appends another post-transition action sequence.
    #[must_use]
    pub fn then(mut self, next: Self) -> Self {
        self.actions.extend(next.actions);
        self
    }

    /// Returns post-transition actions in vanilla execution order.
    #[must_use]
    pub fn actions(&self) -> &[TeleportPostAction] {
        self.actions.as_slice()
    }

    fn single(action: TeleportPostAction) -> Self {
        let mut actions = SmallVec::new();
        actions.push(action);
        Self { actions }
    }
}

impl Default for TeleportPostTransition {
    fn default() -> Self {
        Self::do_nothing()
    }
}

/// A single post-teleport side effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TeleportPostAction {
    /// Send vanilla portal travel level event 1032 to the player.
    PlayPortalSound,
    /// Add a portal chunk ticket.
    PlacePortalTicket(PortalTicketTarget),
}

/// Position used for vanilla portal ticket placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortalTicketTarget {
    /// Use the entity's final block position after teleporting.
    Destination,
    /// Use a specific portal block position.
    Block(BlockPos),
}

impl TeleportTransition {
    /// Returns this transition with a new target position.
    #[must_use]
    pub fn with_position(&self, position: DVec3) -> Self {
        Self {
            target_world: self.target_world.clone(),
            position,
            rotation: self.rotation,
            velocity: self.velocity,
            relatives: self.relatives,
            portal_cooldown: self.portal_cooldown,
            as_passenger: self.as_passenger,
            post_transition: self.post_transition.clone(),
        }
    }

    /// Marks this transition as the recursive passenger variant.
    #[must_use]
    pub fn transition_as_passenger(&self) -> Self {
        Self {
            target_world: self.target_world.clone(),
            position: self.position,
            rotation: self.rotation,
            velocity: self.velocity,
            relatives: self.relatives,
            portal_cooldown: self.portal_cooldown,
            as_passenger: true,
            post_transition: self.post_transition.clone(),
        }
    }

    /// Resolves this transition's position against the entity's current position.
    #[must_use]
    pub fn resolved_position(&self, current_position: DVec3) -> DVec3 {
        resolve_position(self.position, self.relatives, current_position)
    }

    /// Resolves this transition's yaw and pitch against the entity's current rotation.
    #[must_use]
    pub fn resolved_rotation(&self, current_rotation: (f32, f32)) -> (f32, f32) {
        resolve_rotation(self.rotation, self.relatives, current_rotation)
    }

    /// Resolves this transition's velocity against the entity's current motion and rotation.
    #[must_use]
    pub fn resolved_velocity(
        &self,
        current_velocity: DVec3,
        current_rotation: (f32, f32),
        resolved_rotation: (f32, f32),
    ) -> DVec3 {
        resolve_velocity(
            self.velocity,
            self.relatives,
            current_velocity,
            current_rotation,
            resolved_rotation,
        )
    }
}

fn resolve_position(
    position: DVec3,
    relatives: RelativeMovement,
    current_position: DVec3,
) -> DVec3 {
    DVec3::new(
        if relatives.is_x_relative() {
            current_position.x + position.x
        } else {
            position.x
        },
        if relatives.is_y_relative() {
            current_position.y + position.y
        } else {
            position.y
        },
        if relatives.is_z_relative() {
            current_position.z + position.z
        } else {
            position.z
        },
    )
}

fn resolve_rotation(
    rotation: (f32, f32),
    relatives: RelativeMovement,
    current_rotation: (f32, f32),
) -> (f32, f32) {
    let yaw = if relatives.is_y_rot_relative() {
        current_rotation.0 + rotation.0
    } else {
        rotation.0
    };
    let pitch = if relatives.is_x_rot_relative() {
        current_rotation.1 + rotation.1
    } else {
        rotation.1
    };
    (yaw, clamp_pitch(pitch))
}

const fn clamp_pitch(pitch: f32) -> f32 {
    pitch.clamp(-90.0, 90.0)
}

fn resolve_velocity(
    velocity: DVec3,
    relatives: RelativeMovement,
    current_velocity: DVec3,
    current_rotation: (f32, f32),
    resolved_rotation: (f32, f32),
) -> DVec3 {
    let current_velocity = if relatives.rotates_delta() {
        let diff_yaw = current_rotation.0 - resolved_rotation.0;
        let diff_pitch = current_rotation.1 - resolved_rotation.1;
        rotate_y(
            rotate_x(current_velocity, diff_pitch.to_radians()),
            diff_yaw.to_radians(),
        )
    } else {
        current_velocity
    };

    DVec3::new(
        if relatives.is_delta_x_relative() {
            current_velocity.x + velocity.x
        } else {
            velocity.x
        },
        if relatives.is_delta_y_relative() {
            current_velocity.y + velocity.y
        } else {
            velocity.y
        },
        if relatives.is_delta_z_relative() {
            current_velocity.z + velocity.z
        } else {
            velocity.z
        },
    )
}

fn rotate_x(vec: DVec3, radians: f32) -> DVec3 {
    let cos = f64::from(radians.cos());
    let sin = f64::from(radians.sin());
    DVec3::new(vec.x, vec.y * cos + vec.z * sin, vec.z * cos - vec.y * sin)
}

fn rotate_y(vec: DVec3, radians: f32) -> DVec3 {
    let cos = f64::from(radians.cos());
    let sin = f64::from(radians.sin());
    DVec3::new(vec.x * cos + vec.z * sin, vec.y, vec.z * cos - vec.x * sin)
}

/// A queued request to move an entity between loaded worlds.
///
/// Vanilla calls these world changes "dimension changes". Steel keeps the
/// runtime API named after loaded worlds to avoid confusing worlds with vanilla
/// dimension types.
pub enum WorldChangeRequest {
    /// Pre-computed transition (players after chunk pre-warming).
    Computed(TeleportTransition),
    /// Command-driven world change to the target world's spawn.
    WorldSpawn {
        /// The target world to teleport into.
        target_world: Arc<World>,
    },
    /// Portal position — server computes portal-specific destination after chunk pre-warming.
    Portal {
        /// The portal behavior that produced this request.
        portal: PortalKind,
        /// The world the entity is currently in.
        source_world: Arc<World>,
        /// The portal block position.
        portal_pos: BlockPos,
        /// Runtime token proving this request still owns the entity's pending transition.
        pending_token: PendingWorldChangeToken,
    },
}

#[cfg(test)]
mod tests {
    use glam::DVec3;
    use steel_protocol::packets::game::RelativeMovement;
    use steel_registry::vanilla_game_rules::{
        PLAYERS_NETHER_PORTAL_CREATIVE_DELAY, PLAYERS_NETHER_PORTAL_DEFAULT_DELAY,
    };
    use steel_utils::BlockPos;

    use super::{
        PortalKind, PortalProcessResult, PortalProcessor, PortalTicketTarget, TeleportPostAction,
        TeleportPostTransition, clamped_portal_transition_time, nether_portal_transition_rule,
        resolve_position, resolve_rotation, resolve_velocity,
    };

    #[test]
    fn portal_processor_reaches_transition_after_vanilla_threshold() {
        let mut processor = PortalProcessor::new(PortalKind::Nether, BlockPos::new(1, 64, 1));

        assert_eq!(
            processor.process_portal_teleportation(true, 2),
            PortalProcessResult::Waiting
        );
        processor.set_as_inside_portal(BlockPos::new(1, 64, 1));
        assert_eq!(
            processor.process_portal_teleportation(true, 2),
            PortalProcessResult::Waiting
        );
        processor.set_as_inside_portal(BlockPos::new(1, 64, 1));
        assert_eq!(
            processor.process_portal_teleportation(true, 2),
            PortalProcessResult::Ready
        );
        assert_eq!(processor.portal_time(), 3);
    }

    #[test]
    fn portal_processor_does_not_increment_when_teleport_is_disallowed() {
        let mut processor = PortalProcessor::new(PortalKind::End, BlockPos::new(0, 80, 0));

        assert_eq!(
            processor.process_portal_teleportation(false, 0),
            PortalProcessResult::Waiting
        );

        assert_eq!(processor.portal_time(), 0);
        assert!(!processor.is_inside_portal_this_tick());
    }

    #[test]
    fn portal_processor_decays_when_entity_leaves_portal() {
        let mut processor = PortalProcessor::new(PortalKind::EndGateway, BlockPos::new(3, 70, 4));
        for _ in 0..5 {
            processor.set_as_inside_portal(BlockPos::new(3, 70, 4));
            processor.process_portal_teleportation(true, 20);
        }

        assert_eq!(processor.portal_time(), 5);
        processor.process_portal_teleportation(true, 20);
        assert_eq!(processor.portal_time(), 1);
        processor.process_portal_teleportation(true, 20);
        assert_eq!(processor.portal_time(), 0);
        assert!(processor.has_expired());
    }

    #[test]
    fn portal_processor_updates_entry_position_only_after_tick_is_consumed() {
        let mut processor = PortalProcessor::new(PortalKind::Nether, BlockPos::new(1, 64, 1));

        processor.set_as_inside_portal(BlockPos::new(2, 64, 2));
        assert_eq!(processor.entry_position(), BlockPos::new(1, 64, 1));

        processor.process_portal_teleportation(true, 80);
        processor.set_as_inside_portal(BlockPos::new(2, 64, 2));
        assert_eq!(processor.entry_position(), BlockPos::new(2, 64, 2));
    }

    #[test]
    fn nether_portal_transition_rule_matches_player_invulnerability() {
        assert_eq!(
            nether_portal_transition_rule(false).key(),
            PLAYERS_NETHER_PORTAL_DEFAULT_DELAY.key()
        );
        assert_eq!(
            nether_portal_transition_rule(true).key(),
            PLAYERS_NETHER_PORTAL_CREATIVE_DELAY.key()
        );
    }

    #[test]
    fn portal_transition_time_is_clamped_non_negative() {
        assert_eq!(clamped_portal_transition_time(-12), 0);
        assert_eq!(clamped_portal_transition_time(0), 0);
        assert_eq!(clamped_portal_transition_time(80), 80);
    }

    #[test]
    fn relative_portal_transition_rotates_velocity_by_yaw_delta() {
        let resolved_rotation =
            resolve_rotation((90.0, 0.0), RelativeMovement::ROTATION, (0.0, 0.0));
        let velocity = resolve_velocity(
            DVec3::ZERO,
            RelativeMovement::DELTA,
            DVec3::new(1.0, 0.0, 0.0),
            (0.0, 0.0),
            resolved_rotation,
        );

        assert_eq!(resolved_rotation, (90.0, 0.0));
        assert!((velocity - DVec3::new(0.0, 0.0, 1.0)).length_squared() < 1.0e-12);
    }

    #[test]
    fn relative_position_transition_resolves_only_flagged_axes() {
        assert_eq!(
            resolve_position(
                DVec3::new(1.0, 2.0, 3.0),
                RelativeMovement::new(RelativeMovement::X | RelativeMovement::Z),
                DVec3::new(10.0, 20.0, 30.0),
            ),
            DVec3::new(11.0, 2.0, 33.0)
        );
    }

    #[test]
    fn pitch_relative_transition_uses_absolute_yaw_and_relative_pitch() {
        assert_eq!(
            resolve_rotation(
                (90.0, 0.0),
                RelativeMovement::new(RelativeMovement::X_ROT),
                (30.0, 15.0),
            ),
            (90.0, 15.0)
        );
    }

    #[test]
    fn resolved_rotation_clamps_pitch_like_vanilla() {
        assert_eq!(
            resolve_rotation((0.0, 30.0), RelativeMovement::ROTATION, (0.0, 80.0),),
            (0.0, 90.0)
        );
        assert_eq!(
            resolve_rotation((0.0, -120.0), RelativeMovement::NONE, (0.0, 0.0)),
            (0.0, -90.0)
        );
    }

    #[test]
    fn absolute_transition_replaces_velocity_and_rotation() {
        let resolved_rotation =
            resolve_rotation((45.0, 10.0), RelativeMovement::NONE, (90.0, 20.0));

        assert_eq!(resolved_rotation, (45.0, 10.0));
        assert_eq!(
            resolve_velocity(
                DVec3::new(0.0, -0.1, 0.0),
                RelativeMovement::NONE,
                DVec3::new(1.0, 2.0, 3.0),
                (90.0, 20.0),
                resolved_rotation
            ),
            DVec3::new(0.0, -0.1, 0.0)
        );
    }

    #[test]
    fn post_transition_composition_preserves_vanilla_order() {
        let transition = TeleportPostTransition::play_portal_sound().then(
            TeleportPostTransition::place_portal_ticket(PortalTicketTarget::Block(BlockPos::new(
                1, 64, 2,
            ))),
        );

        assert_eq!(
            transition.actions(),
            &[
                TeleportPostAction::PlayPortalSound,
                TeleportPostAction::PlacePortalTicket(PortalTicketTarget::Block(BlockPos::new(
                    1, 64, 2,
                ))),
            ]
        );
    }
}
