//! Rules an explosion consults about what it may break and how hard it hits.
//!
//! Mirrors vanilla's `ExplosionDamageCalculator` class hierarchy, which Rust expresses
//! as a trait whose provided methods are the base class's bodies.

use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::fluid::FluidState;
use steel_utils::{BlockPos, BlockStateId};

use super::Explosion;
use crate::behavior::fluid::FLUID_BEHAVIORS;
use crate::entity::Entity;

/// The resistance vanilla reports for a block an explosion must never break.
const INDESTRUCTIBLE_RESISTANCE: f32 = 3_600_000.0;

/// Decides what an explosion breaks, whom it hurts, and how hard.
///
/// Mirrors vanilla `ExplosionDamageCalculator`; the provided bodies here are that
/// class's, so an implementor overrides only what its vanilla subclass overrides.
pub trait ExplosionDamageCalculator: Send + Sync {
    /// How much power a block absorbs, or `None` when there is nothing to absorb it.
    ///
    /// Mirrors vanilla `getBlockExplosionResistance`.
    fn block_explosion_resistance(
        &self,
        _explosion: &Explosion,
        _pos: BlockPos,
        state: BlockStateId,
        fluid: FluidState,
    ) -> Option<f32> {
        if state.is_air() && fluid.is_empty() {
            return None;
        }
        Some(block_and_fluid_resistance(state, fluid))
    }

    /// Mirrors vanilla `shouldBlockExplode`.
    fn should_block_explode(
        &self,
        _explosion: &Explosion,
        _pos: BlockPos,
        _state: BlockStateId,
        _power: f32,
    ) -> bool {
        true
    }

    /// Mirrors vanilla `shouldDamageEntity`.
    fn should_damage_entity(&self, _explosion: &Explosion, _entity: &dyn Entity) -> bool {
        true
    }

    /// Mirrors vanilla `getKnockbackMultiplier`.
    fn knockback_multiplier(&self, _entity: &dyn Entity) -> f32 {
        1.0
    }

    /// Mirrors vanilla `getEntityDamageAmount`.
    ///
    /// Damage falls off with distance and with how much of the entity the blast could
    /// actually see, then squares up again close in, which is why a direct hit is so
    /// much worse than one a block away.
    fn entity_damage_amount(
        &self,
        explosion: &Explosion,
        entity: &dyn Entity,
        exposure: f32,
    ) -> f32 {
        let double_radius = f64::from(explosion.radius() * 2.0);
        let distance = entity.distance_to_sqr(explosion.center()).sqrt() / double_radius;
        let power = (1.0 - distance) * f64::from(exposure);
        (power.mul_add(power, power) / 2.0 * 7.0 * double_radius + 1.0) as f32
    }
}

/// The larger of a block's and its fluid's resistance. Mirrors vanilla's `Math.max`.
fn block_and_fluid_resistance(state: BlockStateId, fluid: FluidState) -> f32 {
    let block = state.get_block().config.explosion_resistance;
    let fluid = FLUID_BEHAVIORS
        .get_behavior(fluid.fluid_id)
        .explosion_resistance();
    block.max(fluid)
}

/// Vanilla's plain `ExplosionDamageCalculator`, used when nothing caused the blast.
pub struct DefaultExplosionDamageCalculator;

impl ExplosionDamageCalculator for DefaultExplosionDamageCalculator {}

/// Mirrors vanilla `SimpleExplosionDamageCalculator`.
///
/// Lets a caller turn block breaking or entity damage off wholesale, override the
/// knockback, or name the only blocks that resist, which is how the respawn anchor and
/// wither describe their blasts.
pub struct SimpleExplosionDamageCalculator {
    explodes_blocks: bool,
    damages_entities: bool,
    knockback_multiplier: Option<f32>,
    /// When set, *only* these blocks resist, and they resist absolutely.
    immune_blocks: Option<fn(BlockStateId) -> bool>,
}

impl SimpleExplosionDamageCalculator {
    /// Builds a calculator from the four knobs vanilla exposes.
    #[must_use]
    pub const fn new(
        explodes_blocks: bool,
        damages_entities: bool,
        knockback_multiplier: Option<f32>,
        immune_blocks: Option<fn(BlockStateId) -> bool>,
    ) -> Self {
        Self {
            explodes_blocks,
            damages_entities,
            knockback_multiplier,
            immune_blocks,
        }
    }
}

impl ExplosionDamageCalculator for SimpleExplosionDamageCalculator {
    fn block_explosion_resistance(
        &self,
        explosion: &Explosion,
        pos: BlockPos,
        state: BlockStateId,
        fluid: FluidState,
    ) -> Option<f32> {
        let Some(is_immune) = self.immune_blocks else {
            return DefaultExplosionDamageCalculator
                .block_explosion_resistance(explosion, pos, state, fluid);
        };

        // Vanilla inverts the usual rule here: with an immune set, everything outside it
        // offers no resistance at all, however tough it normally is.
        is_immune(state).then_some(INDESTRUCTIBLE_RESISTANCE)
    }

    fn should_block_explode(
        &self,
        _explosion: &Explosion,
        _pos: BlockPos,
        _state: BlockStateId,
        _power: f32,
    ) -> bool {
        self.explodes_blocks
    }

    fn should_damage_entity(&self, _explosion: &Explosion, _entity: &dyn Entity) -> bool {
        self.damages_entities
    }

    fn knockback_multiplier(&self, entity: &dyn Entity) -> f32 {
        let creative_flying = entity
            .as_player()
            .is_some_and(|player| player.abilities.lock().flying);
        if creative_flying {
            return 0.0;
        }
        self.knockback_multiplier
            .unwrap_or_else(|| DefaultExplosionDamageCalculator.knockback_multiplier(entity))
    }
}

/// Mirrors vanilla `EntityBasedExplosionDamageCalculator`.
///
/// Hands the two block questions to the entity that caused the blast. No Steel entity
/// overrides those hooks yet; vanilla's are the wither skull and the TNT minecart,
/// neither of which exists here, but the dispatch is what makes adding one a
/// three-line override rather than a rework.
pub struct EntityBasedExplosionDamageCalculator {
    source_id: i32,
}

impl EntityBasedExplosionDamageCalculator {
    /// Builds a calculator that defers block questions to entity `source_id`.
    #[must_use]
    pub const fn new(source_id: i32) -> Self {
        Self { source_id }
    }
}

impl ExplosionDamageCalculator for EntityBasedExplosionDamageCalculator {
    fn block_explosion_resistance(
        &self,
        explosion: &Explosion,
        pos: BlockPos,
        state: BlockStateId,
        fluid: FluidState,
    ) -> Option<f32> {
        let resistance = DefaultExplosionDamageCalculator
            .block_explosion_resistance(explosion, pos, state, fluid)?;
        let source = explosion.world().get_entity_by_id(self.source_id)?;
        Some(source.block_explosion_resistance(explosion, pos, state, fluid, resistance))
    }

    fn should_block_explode(
        &self,
        explosion: &Explosion,
        pos: BlockPos,
        state: BlockStateId,
        power: f32,
    ) -> bool {
        explosion
            .world()
            .get_entity_by_id(self.source_id)
            .is_none_or(|source| source.should_block_explode(explosion, pos, state, power))
    }
}
