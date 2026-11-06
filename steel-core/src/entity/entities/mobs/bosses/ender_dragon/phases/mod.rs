//! The Ender Dragon's phase state machine. Instead of vanilla's reflective, lazily cached
//! instances, [`EnderDragonPhaseManager`] owns all eleven as fields.
//!
//! `doClientTick` is not ported: the client runs its own copy of this machine from the
//! synced phase id, so keeping that id correct is what matters client-side.

mod death;
mod holding_pattern;
mod hover;
mod navigation;

pub use death::DragonDeathPhase;
pub use holding_pattern::DragonHoldingPatternPhase;
pub use hover::DragonHoverPhase;

use std::sync::atomic::{AtomicI32, Ordering};

use glam::DVec3;

use super::EnderDragonEntity;
use crate::entity::Entity as _;
use crate::entity::damage::DamageSource;
use crate::world::World;
use std::sync::Arc;

/// Which behavior the dragon is currently running.
///
/// The explicit discriminants are vanilla's registration order and the `DATA_PHASE` wire
/// value, so reordering them is a protocol break. Third-party phases would be an additive
/// `Custom(u16)` variant plus a manager lookup; nothing outside this module matches on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum EnderDragonPhase {
    /// Circling the arena, the dragon's default flying behavior.
    HoldingPattern = 0,
    /// Closing on a player to breathe a fireball at them.
    StrafePlayer = 1,
    /// Flying to the node opposite a player, on the way to perching.
    LandingApproach = 2,
    /// Descending onto the exit portal.
    Landing = 3,
    /// Leaving the portal to resume circling.
    Takeoff = 4,
    /// Perched and breathing fire.
    SittingFlaming = 5,
    /// Perched and watching for a player to come close.
    SittingScanning = 6,
    /// Perched and roaring before it breathes.
    SittingAttacking = 7,
    /// Diving at a player's last known position.
    ChargingPlayer = 8,
    /// Flying to the portal to die.
    Dying = 9,
    /// Holding position. The phase a dragon starts in.
    Hovering = 10,
}

impl EnderDragonPhase {
    /// Every phase, in wire-id order.
    pub const ALL: [Self; 11] = [
        Self::HoldingPattern,
        Self::StrafePlayer,
        Self::LandingApproach,
        Self::Landing,
        Self::Takeoff,
        Self::SittingFlaming,
        Self::SittingScanning,
        Self::SittingAttacking,
        Self::ChargingPlayer,
        Self::Dying,
        Self::Hovering,
    ];

    /// Returns the id sent in `DATA_PHASE`.
    #[must_use]
    pub const fn id(self) -> i32 {
        self as i32
    }

    /// Returns the phase for a wire id, falling back to the holding pattern when out of
    /// range (as vanilla `getById` does).
    #[must_use]
    pub fn by_id(id: i32) -> Self {
        usize::try_from(id)
            .ok()
            .and_then(|index| Self::ALL.get(index).copied())
            .unwrap_or(Self::HoldingPattern)
    }
}

/// One behavior the dragon can run: vanilla's `DragonPhaseInstance` with the
/// `AbstractDragonPhaseInstance` defaults.
///
/// Every method takes `&self` because a phase may switch phase from its own tick,
/// re-entering the manager and calling [`Self::end`] on itself. Phase-local state lives
/// behind its own lock, and **no phase may hold that lock across a `set_phase` call**.
pub trait DragonPhaseInstance: Send + Sync {
    /// Which phase this is.
    fn phase(&self) -> EnderDragonPhase;

    /// Whether the dragon is perched; also drives the faster wing beat and lowered head.
    fn is_sitting(&self) -> bool {
        false
    }

    /// Runs one server tick of this phase.
    fn do_server_tick(&self, _dragon: &EnderDragonEntity, _world: &Arc<World>) {}

    /// Called when the dragon enters this phase.
    fn begin(&self, _dragon: &EnderDragonEntity) {}

    /// Called when the dragon leaves this phase.
    fn end(&self, _dragon: &EnderDragonEntity) {}

    /// How fast the dragon flies in this phase.
    fn fly_speed(&self) -> f32 {
        0.6
    }

    /// How fast the dragon turns toward its fly target.
    fn turn_speed(&self, dragon: &EnderDragonEntity) -> f32 {
        let velocity = dragon.velocity();
        let rot_speed = velocity.x.hypot(velocity.z) as f32 + 1.0;
        0.7 / rot_speed.min(40.0) / rot_speed
    }

    /// Where the dragon is steering, or `None` to coast.
    fn fly_target_location(&self) -> Option<DVec3> {
        None
    }

    /// Lets a phase scale or veto incoming damage.
    fn on_hurt(&self, _source: &DamageSource, damage: f32) -> f32 {
        damage
    }
}

/// Owns the dragon's phases and which one is active.
pub struct EnderDragonPhaseManager {
    holding_pattern: DragonHoldingPatternPhase,
    strafe_player: PlaceholderPhase,
    landing_approach: PlaceholderPhase,
    landing: PlaceholderPhase,
    takeoff: PlaceholderPhase,
    sitting_flaming: PlaceholderPhase,
    sitting_scanning: PlaceholderPhase,
    sitting_attacking: PlaceholderPhase,
    charging_player: PlaceholderPhase,
    dying: DragonDeathPhase,
    hovering: DragonHoverPhase,
    /// The active phase's id. Atomic rather than locked because a phase re-enters
    /// `set_phase` mid-call; the instances themselves are immutable.
    current: AtomicI32,
}

impl EnderDragonPhaseManager {
    /// Creates a manager starting in the hovering phase, set directly rather than via
    /// vanilla's `setPhase(HOVERING)` (which needs the dragon): its `begin` only clears an
    /// already-empty target, and the synced data already defaults to this id.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            holding_pattern: DragonHoldingPatternPhase::new(),
            strafe_player: PlaceholderPhase::new(EnderDragonPhase::StrafePlayer),
            landing_approach: PlaceholderPhase::new(EnderDragonPhase::LandingApproach),
            landing: PlaceholderPhase::new(EnderDragonPhase::Landing),
            takeoff: PlaceholderPhase::new(EnderDragonPhase::Takeoff),
            sitting_flaming: PlaceholderPhase::new(EnderDragonPhase::SittingFlaming),
            sitting_scanning: PlaceholderPhase::new(EnderDragonPhase::SittingScanning),
            sitting_attacking: PlaceholderPhase::new(EnderDragonPhase::SittingAttacking),
            charging_player: PlaceholderPhase::new(EnderDragonPhase::ChargingPlayer),
            dying: DragonDeathPhase::new(),
            hovering: DragonHoverPhase::new(),
            current: AtomicI32::new(EnderDragonPhase::Hovering.id()),
        }
    }

    /// Returns the active phase.
    #[must_use]
    pub fn current_phase(&self) -> EnderDragonPhase {
        EnderDragonPhase::by_id(self.current.load(Ordering::Relaxed))
    }

    /// Returns the active phase's behavior.
    #[must_use]
    pub fn current(&self) -> &dyn DragonPhaseInstance {
        self.instance(self.current_phase())
    }

    /// Returns a specific phase's behavior.
    #[must_use]
    pub fn instance(&self, phase: EnderDragonPhase) -> &dyn DragonPhaseInstance {
        match phase {
            EnderDragonPhase::HoldingPattern => &self.holding_pattern,
            EnderDragonPhase::StrafePlayer => &self.strafe_player,
            EnderDragonPhase::LandingApproach => &self.landing_approach,
            EnderDragonPhase::Landing => &self.landing,
            EnderDragonPhase::Takeoff => &self.takeoff,
            EnderDragonPhase::SittingFlaming => &self.sitting_flaming,
            EnderDragonPhase::SittingScanning => &self.sitting_scanning,
            EnderDragonPhase::SittingAttacking => &self.sitting_attacking,
            EnderDragonPhase::ChargingPlayer => &self.charging_player,
            EnderDragonPhase::Dying => &self.dying,
            EnderDragonPhase::Hovering => &self.hovering,
        }
    }

    /// Switches the dragon to `target`. The id is swapped *before* `end`/`begin` run, so a
    /// phase switching from its own tick never sees a mid-transition manager; vanilla's
    /// `end`/`begin` never read the current phase, so the reordering is unobservable.
    pub fn set_phase(&self, dragon: &EnderDragonEntity, target: EnderDragonPhase) {
        let previous = EnderDragonPhase::by_id(self.current.swap(target.id(), Ordering::Relaxed));
        if previous == target {
            return;
        }

        self.instance(previous).end(dragon);
        dragon.set_synced_phase(target);
        log::debug!("Dragon is now in phase {target:?}");
        self.instance(target).begin(dragon);
    }
}

impl Default for EnderDragonPhaseManager {
    fn default() -> Self {
        Self::new()
    }
}

/// A phase not ported yet. With no fly target the dragon coasts, so entering one degrades
/// to a stationary dragon rather than a panic.
struct PlaceholderPhase {
    phase: EnderDragonPhase,
}

impl PlaceholderPhase {
    const fn new(phase: EnderDragonPhase) -> Self {
        Self { phase }
    }
}

impl DragonPhaseInstance for PlaceholderPhase {
    fn phase(&self) -> EnderDragonPhase {
        self.phase
    }
}
