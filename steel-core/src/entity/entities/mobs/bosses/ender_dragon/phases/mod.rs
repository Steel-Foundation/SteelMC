//! The Ender Dragon's phase state machine.
//!
//! Mirrors vanilla's `phases` package. Vanilla builds phase instances reflectively
//! from a `Class` and caches them lazily; Rust cannot, so [`EnderDragonPhaseManager`]
//! owns all eleven as fields and hands out `&dyn DragonPhaseInstance`.
//!
//! `doClientTick` is not ported. A vanilla client runs its own copy of this machine
//! driven by the synced phase id, which makes keeping that id correct the important
//! client-facing invariant rather than any server-side animation work.

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

/// Which behaviour the dragon is currently running.
///
/// The discriminants are the vanilla registration order and are what `DATA_PHASE`
/// carries on the wire, so reordering them is a protocol break. They are written
/// explicitly rather than left implicit for exactly that reason.
///
/// Third-party phases would be added as a `Custom(u16)` variant plus a lookup on the
/// manager; nothing outside this module matches on the enum, so that stays additive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum EnderDragonPhase {
    /// Circling the arena, the dragon's default flying behaviour.
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

    /// Returns the id sent in `DATA_PHASE`. Mirrors vanilla `getId`.
    #[must_use]
    pub const fn id(self) -> i32 {
        self as i32
    }

    /// Returns the phase for a wire id.
    ///
    /// Mirrors vanilla `getById`, which falls back to the holding pattern rather
    /// than failing on an out-of-range id.
    #[must_use]
    pub fn by_id(id: i32) -> Self {
        usize::try_from(id)
            .ok()
            .and_then(|index| Self::ALL.get(index).copied())
            .unwrap_or(Self::HoldingPattern)
    }
}

/// One behaviour the dragon can run.
///
/// Merges vanilla's `DragonPhaseInstance` interface with the defaults its
/// `AbstractDragonPhaseInstance` supplies, which Rust expresses as trait defaults.
///
/// Every method takes `&self`: a phase may switch the dragon out of itself from
/// inside its own tick, which re-enters the manager and calls [`Self::end`] on the
/// phase still executing. Phase-local state therefore lives behind its own lock, and
/// **no phase may hold that lock across a `set_phase` call**.
pub trait DragonPhaseInstance: Send + Sync {
    /// Which phase this is. Mirrors vanilla `getPhase`.
    fn phase(&self) -> EnderDragonPhase;

    /// Whether the dragon is perched. Mirrors vanilla `isSitting`.
    ///
    /// Drives the faster wing beat and the lowered head, not just the landing logic.
    fn is_sitting(&self) -> bool {
        false
    }

    /// Runs one server tick of this phase. Mirrors vanilla `doServerTick`.
    fn do_server_tick(&self, _dragon: &EnderDragonEntity, _world: &Arc<World>) {}

    /// Called when the dragon enters this phase. Mirrors vanilla `begin`.
    fn begin(&self, _dragon: &EnderDragonEntity) {}

    /// Called when the dragon leaves this phase. Mirrors vanilla `end`.
    fn end(&self, _dragon: &EnderDragonEntity) {}

    /// Mirrors vanilla `getFlySpeed`.
    fn fly_speed(&self) -> f32 {
        0.6
    }

    /// Mirrors vanilla `getTurnSpeed`.
    fn turn_speed(&self, dragon: &EnderDragonEntity) -> f32 {
        let velocity = dragon.velocity();
        let rot_speed = velocity.x.hypot(velocity.z) as f32 + 1.0;
        0.7 / rot_speed.min(40.0) / rot_speed
    }

    /// Where the dragon is steering, or `None` to stop steering.
    ///
    /// Mirrors vanilla `getFlyTargetLocation`. A phase that has not been implemented
    /// yet returns `None` here, which leaves the dragon coasting rather than failing.
    fn fly_target_location(&self) -> Option<DVec3> {
        None
    }

    /// Lets a phase scale or veto incoming damage. Mirrors vanilla `onHurt`.
    fn on_hurt(&self, _source: &DamageSource, damage: f32) -> f32 {
        damage
    }
}

/// Owns the dragon's phases and which one is active.
///
/// Mirrors vanilla `EnderDragonPhaseManager`.
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
    /// The active phase.
    ///
    /// Only the id is shared state; the instances are immutable fields. Holding this
    /// across a phase call would deadlock, since a phase can re-enter `set_phase`.
    current: AtomicI32,
}

impl EnderDragonPhaseManager {
    /// Creates a manager starting in the hovering phase.
    ///
    /// Vanilla's constructor calls `setPhase(HOVERING)`, which would need the dragon
    /// this manager is being built for. The initial phase is set directly instead:
    /// `DragonHoverPhase::begin` only clears an already-empty target, and the synced
    /// data already defaults to this phase's id.
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

    /// Returns the active phase. Mirrors vanilla `getCurrentPhase().getPhase()`.
    #[must_use]
    pub fn current_phase(&self) -> EnderDragonPhase {
        EnderDragonPhase::by_id(self.current.load(Ordering::Relaxed))
    }

    /// Returns the active phase's behaviour. Mirrors vanilla `getCurrentPhase`.
    #[must_use]
    pub fn current(&self) -> &dyn DragonPhaseInstance {
        self.instance(self.current_phase())
    }

    /// Returns a specific phase's behaviour. Mirrors vanilla `getPhase`.
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

    /// Switches the dragon to `target`. Mirrors vanilla `setPhase`.
    ///
    /// The active id is swapped *before* `end`/`begin` run, so a phase that switches
    /// from inside its own tick never observes the manager mid-transition. Nothing in
    /// vanilla's `end` or `begin` reads the current phase, so the reordering is not
    /// observable.
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

/// A phase that has not been ported yet.
///
/// Returning no fly target leaves the dragon coasting instead of steering, so
/// entering one of these degrades to a stationary dragon rather than a panic or a
/// missing match arm. Each is replaced by its real behaviour as it lands.
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
