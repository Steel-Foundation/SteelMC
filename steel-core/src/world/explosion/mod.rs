//! Server-side explosion calculation and application.

use std::sync::Arc;

use glam::DVec3;
use steel_protocol::packets::game::{
    CExplode, ExplosionParticleInfo, ExplosionParticlePalette, WeightedExplosionParticleInfo,
};
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::fluid::FluidState;
use steel_registry::game_rules::GameRule;
use steel_registry::particle_type::ParticleData;
use steel_registry::sound_event::SoundEventHolder;
use steel_registry::vanilla_game_rules::{
    BLOCK_EXPLOSION_DROP_DECAY, MOB_EXPLOSION_DROP_DECAY, MOB_GRIEFING, TNT_EXPLOSION_DROP_DECAY,
};
use steel_registry::{sound_events, vanilla_particle_types};
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::FLUID_BEHAVIORS;
use crate::entity::Entity;
use crate::entity::damage::DamageSource;
use crate::world::World;

use server::ServerExplosion;

mod server;

const MAX_PACKET_DISTANCE_SQUARED: f64 = 64.0 * 64.0;

/// High-level source category used to resolve Vanilla explosion gamerules.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExplosionInteraction {
    /// Damage entities without changing blocks.
    None,
    /// A block-created explosion governed by `block_explosion_drop_decay`.
    Block,
    /// A mob-created explosion governed by mob griefing and mob drop decay.
    Mob,
    /// A TNT explosion governed by `tnt_explosion_drop_decay`.
    Tnt,
    /// Trigger eligible blocks without destroying them.
    ///
    /// TODO: Ring bells once Steel has its Bell block and block-entity foundation.
    Trigger,
}

/// How an explosion interacts with blocks after its affected positions are calculated.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlockInteraction {
    /// Preserve blocks.
    Keep,
    /// Destroy affected blocks without explosion decay.
    Destroy,
    /// Destroy affected blocks with the explosion radius in their loot context.
    DestroyWithDecay,
    /// Invoke trigger-aware block hooks without normal destruction.
    TriggerBlock,
}

impl BlockInteraction {
    /// Returns Vanilla's default block-like-entity interaction flag.
    #[must_use]
    pub const fn should_affect_blocklike_entities(self) -> bool {
        matches!(self, Self::Destroy | Self::DestroyWithDecay)
    }
}

/// Read-only explosion context exposed to blocks, entities, and calculators.
pub trait Explosion {
    /// Returns the world executing this explosion.
    fn world(&self) -> &Arc<World>;
    /// Returns the damage source carried by this explosion.
    fn damage_source(&self) -> &DamageSource;
    /// Returns the resolved block interaction policy.
    fn block_interaction(&self) -> BlockInteraction;
    /// Returns the living entity credited for this explosion, if any.
    fn indirect_source_entity(&self) -> Option<&dyn Entity>;
    /// Returns the entity directly creating this explosion, if any.
    fn direct_source_entity(&self) -> Option<&dyn Entity>;
    /// Returns the explosion radius.
    fn radius(&self) -> f32;
    /// Returns the exact explosion center.
    fn center(&self) -> DVec3;
    /// Returns whether trigger-aware block behavior may run.
    fn can_trigger_blocks(&self) -> bool;
    /// Returns whether block-like entities may be affected.
    fn should_affect_blocklike_entities(&self) -> bool;
}

/// Customizable Vanilla explosion resistance, damage, and knockback policy.
pub trait ExplosionDamageCalculator: Send + Sync {
    /// Returns the effective resistance at an affected block position.
    fn block_explosion_resistance(
        &self,
        _explosion: &dyn Explosion,
        _world: &World,
        _pos: BlockPos,
        state: BlockStateId,
        fluid: FluidState,
    ) -> Option<f32> {
        default_block_explosion_resistance(state, fluid)
    }

    /// Returns whether the current ray may affect this block position.
    fn should_block_explode(
        &self,
        _explosion: &dyn Explosion,
        _world: &World,
        _pos: BlockPos,
        _state: BlockStateId,
        _power: f32,
    ) -> bool {
        true
    }

    /// Returns whether the explosion damages this entity.
    fn should_damage_entity(&self, _explosion: &dyn Explosion, _entity: &dyn Entity) -> bool {
        true
    }

    /// Returns the multiplier applied to this entity's explosion knockback.
    fn knockback_multiplier(&self, _entity: &dyn Entity) -> f32 {
        1.0
    }

    /// Calculates Vanilla explosion damage for one entity and exposure value.
    #[expect(
        clippy::manual_midpoint,
        reason = "preserve Vanilla's exact floating-point expression"
    )]
    fn entity_damage_amount(
        &self,
        explosion: &dyn Explosion,
        entity: &dyn Entity,
        exposure: f32,
    ) -> f32 {
        let double_radius = explosion.radius() * 2.0;
        let distance = entity.position().distance(explosion.center()) / f64::from(double_radius);
        let impact = (1.0 - distance) * f64::from(exposure);
        (((impact * impact + impact) / 2.0) * 7.0 * f64::from(double_radius) + 1.0) as f32
    }
}

/// State-only view available to explicitly immutable explosion block calculators.
///
/// Keeping this surface narrow prevents worker tasks from reaching mutable world,
/// entity, block-entity, or general [`crate::world::LevelReader`] behavior.
pub(crate) trait ExplosionBlockReader: Sync {
    /// Returns `None` when `pos` is outside a bounded reader's stable view.
    fn block_state(&self, pos: BlockPos) -> Option<BlockStateId>;
}

impl ExplosionBlockReader for World {
    #[inline]
    fn block_state(&self, pos: BlockPos) -> Option<BlockStateId> {
        Some(self.get_block_state(pos))
    }
}

/// Opt-in capability for calculators whose block-ray decisions are pure.
///
/// This trait is crate-private so public custom calculators continue through the
/// sequential compatibility lane unless Steel can prove their implementation is
/// immutable. Implementations may only inspect block states through
/// [`ExplosionBlockReader`].
pub(crate) trait ImmutableExplosionBlockCalculator: Send + Sync {
    /// Returns the greatest per-axis block offset (Chebyshev radius) this calculator reads from
    /// each ray position. Implementations opting in must never increase ray power through a
    /// returned resistance.
    ///
    /// Opting in permits one bounded, stable world view for the ray phase. Every reader access
    /// made by [`Self::explosion_resistance`] and [`Self::should_explode`] must remain within this
    /// distance. Returning `None` retains the live-world compatibility path.
    fn bounded_block_read_radius(&self) -> Option<u32> {
        None
    }

    /// Whether repeated resistance queries for an unchanged position may reuse one result.
    ///
    /// A bounded reader guarantees stable block states during ray traversal. An implementation
    /// must still opt in because the compatibility contract permits calculators to observe how
    /// often this method runs. Live-world fallback traversal does not cache reads or resistance.
    fn can_cache_explosion_resistance(&self) -> bool {
        false
    }

    /// Returns the effective resistance at a ray position.
    fn explosion_resistance(
        &self,
        _reader: &dyn ExplosionBlockReader,
        _pos: BlockPos,
        state: BlockStateId,
        fluid: FluidState,
    ) -> Option<f32> {
        default_block_explosion_resistance(state, fluid)
    }

    fn should_explode(
        &self,
        _reader: &dyn ExplosionBlockReader,
        _pos: BlockPos,
        _state: BlockStateId,
        _power: f32,
    ) -> bool {
        true
    }
}

impl ImmutableExplosionBlockCalculator for DefaultExplosionDamageCalculator {
    fn bounded_block_read_radius(&self) -> Option<u32> {
        Some(0)
    }

    fn can_cache_explosion_resistance(&self) -> bool {
        true
    }
}

fn default_block_explosion_resistance(state: BlockStateId, fluid: FluidState) -> Option<f32> {
    if state.is_air() && fluid.is_empty() {
        return None;
    }

    let block_resistance = state.get_block().config.explosion_resistance;
    let fluid_resistance = FLUID_BEHAVIORS
        .get_behavior(fluid.fluid_id)
        .explosion_resistance();
    Some(block_resistance.max(fluid_resistance))
}

/// Vanilla's source-less default explosion calculator.
#[derive(Debug, Default)]
pub struct DefaultExplosionDamageCalculator;

impl ExplosionDamageCalculator for DefaultExplosionDamageCalculator {}

/// Vanilla's calculator that delegates resistance and block decisions to the source entity.
pub struct EntityBasedExplosionDamageCalculator<'a> {
    source: &'a dyn Entity,
}

impl<'a> EntityBasedExplosionDamageCalculator<'a> {
    /// Creates a calculator that delegates block decisions to `source`.
    #[must_use]
    pub const fn new(source: &'a dyn Entity) -> Self {
        Self { source }
    }
}

impl ExplosionDamageCalculator for EntityBasedExplosionDamageCalculator<'_> {
    fn block_explosion_resistance(
        &self,
        explosion: &dyn Explosion,
        world: &World,
        pos: BlockPos,
        state: BlockStateId,
        fluid: FluidState,
    ) -> Option<f32> {
        DefaultExplosionDamageCalculator
            .block_explosion_resistance(explosion, world, pos, state, fluid)
            .map(|resistance| {
                self.source
                    .block_explosion_resistance(explosion, world, pos, state, fluid, resistance)
            })
    }

    fn should_block_explode(
        &self,
        explosion: &dyn Explosion,
        world: &World,
        pos: BlockPos,
        state: BlockStateId,
        power: f32,
    ) -> bool {
        self.source
            .should_block_explode(explosion, world, pos, state, power)
    }
}

enum SelectedDamageCalculator<'a> {
    Default,
    Entity(&'a dyn Entity),
    Custom(&'a dyn ExplosionDamageCalculator),
}

impl ExplosionDamageCalculator for SelectedDamageCalculator<'_> {
    fn block_explosion_resistance(
        &self,
        explosion: &dyn Explosion,
        world: &World,
        pos: BlockPos,
        state: BlockStateId,
        fluid: FluidState,
    ) -> Option<f32> {
        match self {
            Self::Default => DefaultExplosionDamageCalculator
                .block_explosion_resistance(explosion, world, pos, state, fluid),
            Self::Entity(source) => EntityBasedExplosionDamageCalculator::new(*source)
                .block_explosion_resistance(explosion, world, pos, state, fluid),
            Self::Custom(calculator) => {
                calculator.block_explosion_resistance(explosion, world, pos, state, fluid)
            }
        }
    }

    fn should_block_explode(
        &self,
        explosion: &dyn Explosion,
        world: &World,
        pos: BlockPos,
        state: BlockStateId,
        power: f32,
    ) -> bool {
        match self {
            Self::Default => DefaultExplosionDamageCalculator
                .should_block_explode(explosion, world, pos, state, power),
            Self::Entity(source) => EntityBasedExplosionDamageCalculator::new(*source)
                .should_block_explode(explosion, world, pos, state, power),
            Self::Custom(calculator) => {
                calculator.should_block_explode(explosion, world, pos, state, power)
            }
        }
    }

    fn should_damage_entity(&self, explosion: &dyn Explosion, entity: &dyn Entity) -> bool {
        match self {
            Self::Custom(calculator) => calculator.should_damage_entity(explosion, entity),
            Self::Default | Self::Entity(_) => {
                DefaultExplosionDamageCalculator.should_damage_entity(explosion, entity)
            }
        }
    }

    fn knockback_multiplier(&self, entity: &dyn Entity) -> f32 {
        match self {
            Self::Custom(calculator) => calculator.knockback_multiplier(entity),
            Self::Default | Self::Entity(_) => {
                DefaultExplosionDamageCalculator.knockback_multiplier(entity)
            }
        }
    }

    fn entity_damage_amount(
        &self,
        explosion: &dyn Explosion,
        entity: &dyn Entity,
        exposure: f32,
    ) -> f32 {
        match self {
            Self::Custom(calculator) => {
                calculator.entity_damage_amount(explosion, entity, exposure)
            }
            Self::Default | Self::Entity(_) => {
                DefaultExplosionDamageCalculator.entity_damage_amount(explosion, entity, exposure)
            }
        }
    }
}

/// Full visual and gameplay parameters for one server explosion.
pub struct ExplosionOptions<'a> {
    /// Entity directly creating the explosion.
    pub source: Option<&'a dyn Entity>,
    /// Optional non-default damage source.
    pub damage_source: Option<DamageSource>,
    /// Optional non-default damage calculator.
    pub damage_calculator: Option<&'a dyn ExplosionDamageCalculator>,
    /// Proven-immutable block-ray calculator for Steel-owned explosion sources.
    pub(crate) immutable_block_calculator: Option<&'a dyn ImmutableExplosionBlockCalculator>,
    /// Exact explosion center.
    pub center: DVec3,
    /// Explosion radius.
    pub radius: f32,
    /// Whether the explosion can create fire.
    pub fire: bool,
    /// High-level block interaction category.
    pub interaction: ExplosionInteraction,
    /// Particle used for small or non-block-interacting explosions.
    pub small_explosion_particle: ParticleData,
    /// Particle used for large block-interacting explosions.
    pub large_explosion_particle: ParticleData,
    /// Weighted client-side block debris particle palette.
    pub block_particles: ExplosionParticlePalette,
    /// Client-side explosion sound.
    pub explosion_sound: SoundEventHolder,
}

impl ExplosionOptions<'_> {
    /// Creates an explosion with Vanilla's standard visual effects.
    #[must_use]
    pub fn new(center: DVec3, radius: f32, interaction: ExplosionInteraction) -> Self {
        Self {
            source: None,
            damage_source: None,
            damage_calculator: None,
            immutable_block_calculator: None,
            center,
            radius,
            fire: false,
            interaction,
            small_explosion_particle: ParticleData::simple(&vanilla_particle_types::EXPLOSION),
            large_explosion_particle: ParticleData::simple(
                &vanilla_particle_types::EXPLOSION_EMITTER,
            ),
            block_particles: default_block_particles(),
            explosion_sound: SoundEventHolder::registry(&sound_events::ENTITY_GENERIC_EXPLODE),
        }
    }
}

/// Observable server result of running one explosion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExplosionOutcome {
    /// Number of unique positions reached by the explosion rays.
    pub affected_block_count: usize,
}

impl World {
    /// Runs one explosion atomically on the game tick and sends its client effects.
    pub fn explode(self: &Arc<Self>, options: ExplosionOptions<'_>) -> ExplosionOutcome {
        let block_interaction = self.resolve_block_interaction(options.interaction);
        let mut explosion = ServerExplosion::new(
            self,
            options.source,
            options.damage_source,
            options.damage_calculator,
            options.immutable_block_calculator,
            options.center,
            options.radius,
            options.fire,
            block_interaction,
        );
        let affected_block_count = explosion.explode();
        let explosion_particle = if explosion.is_small() {
            options.small_explosion_particle
        } else {
            options.large_explosion_particle
        };
        #[expect(
            clippy::manual_unwrap_or,
            reason = "make the protocol saturation behavior explicit"
        )]
        let packet_block_count = match i32::try_from(affected_block_count) {
            Ok(count) => count,
            Err(_) => i32::MAX,
        };

        self.players.iter_players(|_, player| {
            if player.position().distance_squared(options.center) < MAX_PACKET_DISTANCE_SQUARED {
                player.send_packet(CExplode::new(
                    options.center,
                    options.radius,
                    packet_block_count,
                    explosion.hit_players.get(&player.id()).copied(),
                    explosion_particle.clone(),
                    options.explosion_sound.clone(),
                    options.block_particles.clone(),
                ));
            }
            true
        });

        ExplosionOutcome {
            affected_block_count,
        }
    }

    fn resolve_block_interaction(&self, interaction: ExplosionInteraction) -> BlockInteraction {
        match interaction {
            ExplosionInteraction::None => BlockInteraction::Keep,
            ExplosionInteraction::Block => self.destroy_interaction(&BLOCK_EXPLOSION_DROP_DECAY),
            ExplosionInteraction::Mob => {
                if self.get_game_rule(&MOB_GRIEFING) {
                    self.destroy_interaction(&MOB_EXPLOSION_DROP_DECAY)
                } else {
                    BlockInteraction::Keep
                }
            }
            ExplosionInteraction::Tnt => self.destroy_interaction(&TNT_EXPLOSION_DROP_DECAY),
            ExplosionInteraction::Trigger => BlockInteraction::TriggerBlock,
        }
    }

    fn destroy_interaction(&self, decay_rule: &GameRule<bool>) -> BlockInteraction {
        if self.get_game_rule(decay_rule) {
            BlockInteraction::DestroyWithDecay
        } else {
            BlockInteraction::Destroy
        }
    }
}

fn default_block_particles() -> ExplosionParticlePalette {
    let entries = [
        ExplosionParticleInfo {
            particle: ParticleData::simple(&vanilla_particle_types::POOF),
            scaling: 0.5,
            speed: 1.0,
        },
        ExplosionParticleInfo {
            particle: ParticleData::simple(&vanilla_particle_types::SMOKE),
            scaling: 1.0,
            speed: 1.0,
        },
    ]
    .into_iter()
    .map(|particle| {
        let Ok(entry) = WeightedExplosionParticleInfo::try_new(particle, 1) else {
            panic!("Vanilla explosion particle weight must be valid");
        };
        entry
    })
    .collect();
    let Ok(palette) = ExplosionParticlePalette::try_new(entries) else {
        panic!("Vanilla explosion particle palette must be valid");
    };
    palette
}
