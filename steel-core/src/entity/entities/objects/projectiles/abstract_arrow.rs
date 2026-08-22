//! Vanilla `AbstractArrow` base (`net.minecraft.world.entity.projectile.arrow.AbstractArrow`).
//!
//! Shared flight model for `Arrow`, `SpectralArrow` and `Trident`: stick into
//! blocks (`IN_GROUND`), gravity/drag, entity-hit damage, despawn and pickup.
//! Mirrored as a Rust trait so future `SpectralArrowEntity` / trident entities
//! can reuse the same base without duplicating `ArrowEntity` constants/logic.

use crate::entity::Projectile;

/// Vanilla `AbstractArrow.Pickup`. Ordinals match the vanilla enum for NBT.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pickup {
    /// Vanilla `DISALLOWED` — nobody can pick the arrow up.
    #[default]
    Disallowed = 0,
    /// Vanilla `ALLOWED` — players can pick the arrow up.
    Allowed = 1,
    /// Vanilla `CREATIVE_ONLY` — only players with infinite materials.
    CreativeOnly = 2,
}

impl From<i8> for Pickup {
    fn from(value: i8) -> Self {
        match value {
            1 => Self::Allowed,
            2 => Self::CreativeOnly,
            _ => Self::Disallowed,
        }
    }
}

/// Shared `AbstractArrow` behaviour (vanilla `AbstractArrow`).
///
/// Concrete arrows (`ArrowEntity`, future `SpectralArrowEntity`, tridents)
/// implement this trait and keep their own `EntityBase` / `ProjectileBase` /
/// synced-data storage. The trait only abstracts the common interface so the
/// constants and helpers are not duplicated per concrete type.
pub trait AbstractArrow: Projectile {
    /// Vanilla `AbstractArrow.ARROW_BASE_DAMAGE`.
    const BASE_DAMAGE: f64 = 2.0;
    /// Vanilla `AbstractArrow.SHAKE_TIME`.
    const SHAKE_TIME: i32 = 7;
    /// Vanilla `AbstractArrow.WATER_INERTIA`.
    const WATER_INERTIA: f64 = 0.6;
    /// Vanilla `AbstractArrow.INERTIA` (air drag).
    const INERTIA: f64 = 0.99;
    /// Vanilla `AbstractArrow.tickDespawn`: 1200 ticks (~60s) before discard.
    const DESPAWN_LIFE: i32 = 1200;
    /// Vanilla `AbstractArrow.getDefaultGravity`.
    const GRAVITY: f64 = 0.05;
    /// Vanilla `AbstractArrow.startFalling`: per-axis random velocity multiplier
    /// upper bound applied when a stuck arrow pops free.
    const START_FALLING_JITTER_SCALE: f32 = 0.2;
    /// Vanilla `AbstractArrow.onHitBlock`: distance backed off along the impact
    /// sign direction before the arrow sticks into the block.
    const HIT_BLOCK_BACKOFF: f64 = 0.05;

    /// Returns whether the arrow is stuck in ground (`IN_GROUND` synced).
    fn is_in_ground(&self) -> bool;
    /// Sets the `IN_GROUND` synced flag.
    fn set_in_ground(&self, value: bool);

    /// Returns whether the arrow is a crit arrow (flag bit 0).
    fn is_crit_arrow(&self) -> bool;
    /// Sets the crit-arrow synced flag.
    fn set_crit_arrow(&self, value: bool);

    /// Returns the current pickup rule.
    fn pickup(&self) -> Pickup;
    /// Sets the pickup rule.
    fn set_pickup(&self, pickup: Pickup);
}
