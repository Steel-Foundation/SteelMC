//! Explosions.
//!
//! Mirrors vanilla `ServerExplosion` and the `Explosion` interface it implements. The
//! two are merged: vanilla splits them only so the client can share the interface, and
//! Steel is server-only.
//!
//! An [`Explosion`] is a short-lived value: built, [`Explosion::explode`]d, and
//! dropped inside one call, so it holds its world and source outright rather than by
//! id, and its bookkeeping needs no locks.

mod damage_calculator;
#[cfg(test)]
mod tests;

pub use damage_calculator::{
    DefaultExplosionDamageCalculator, EntityBasedExplosionDamageCalculator,
    ExplosionDamageCalculator, SimpleExplosionDamageCalculator,
};

use std::sync::Arc;

use glam::DVec3;
use rand::seq::SliceRandom as _;
use rustc_hash::{FxHashMap, FxHashSet};
use steel_math::lerp;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::game_rules::GameRule;
use steel_registry::item_stack::ItemStack;
use steel_registry::particle_type::{ExplosionParticleInfo, ParticleData};
use steel_registry::sound_event::SoundEventHolder;
use steel_registry::vanilla_entity_type_tags::EntityTypeTag;
use steel_registry::vanilla_game_rules::{
    BLOCK_EXPLOSION_DROP_DECAY, MOB_EXPLOSION_DROP_DECAY, MOB_GRIEFING, TNT_EXPLOSION_DROP_DECAY,
};
use steel_registry::{REGISTRY, TaggedRegistryExt as _, sound_events, vanilla_particle_types};
use steel_registry::{vanilla_attributes, vanilla_damage_types, vanilla_entities};
use steel_utils::BlockPos;
use steel_utils::geometry::WorldAabb;
use steel_utils::random::weighted::Weighted;
use steel_utils::types::{GameType, UpdateFlags};

use steel_protocol::packets::game::CExplode;

use crate::behavior::BLOCK_BEHAVIORS;
use crate::behavior::blocks::FireBlock;
use crate::entity::damage::DamageSource;
use crate::entity::entities::ItemEntity;
use crate::entity::{Entity, SharedEntity};
use crate::world::World;
use crate::world::raycast::{ClipBlockShape, ClipFluid};

/// Rays are cast from the faces of a 16x16x16 grid, so the blast is sampled evenly in
/// every direction rather than as a sphere of blocks.
const RAY_GRID_SIZE: i32 = 16;
/// How far each ray advances per step.
const RAY_STEP: f32 = 0.3;
/// Power each ray loses per step, on top of what the blocks it passes absorb.
const RAY_STEP_DECAY: f32 = 0.225_000_01;
/// A ray starts somewhere in `radius * [0.7, 1.3)`, which is what makes craters ragged.
const RAY_POWER_JITTER: f32 = 0.6;
const RAY_POWER_FLOOR: f32 = 0.7;
/// One in this many destroyed positions catches fire, when the blast makes fire.
const FIRE_CHANCE: i32 = 3;
/// Vanilla caps an exploded drop stack well below its normal maximum.
const MAX_DROPS_PER_COMBINED_STACK: i32 = 16;
/// A blast smaller than this does not bother looking for entities.
const MINIMUM_DAMAGING_RADIUS: f32 = 1.0e-5;

/// What an explosion does to the blocks it reaches. Mirrors vanilla
/// `Explosion.BlockInteraction`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockInteraction {
    /// Leave every block standing.
    Keep,
    /// Break blocks and drop everything they would normally drop.
    Destroy,
    /// Break blocks, but let the explosion-decay loot function eat most of the drops.
    DestroyWithDecay,
    /// Break nothing, but let blocks react, as a wind charge flips a lever.
    TriggerBlock,
}

impl BlockInteraction {
    /// Whether block-shaped entities, such as boats and item frames, are affected.
    ///
    /// Mirrors the flag vanilla stores on each enum constant.
    #[must_use]
    pub const fn affects_blocklike_entities(self) -> bool {
        matches!(self, Self::Destroy | Self::DestroyWithDecay)
    }

    /// Whether this interaction touches blocks at all. Mirrors `interactsWithBlocks`.
    #[must_use]
    pub const fn interacts_with_blocks(self) -> bool {
        !matches!(self, Self::Keep)
    }
}

/// One explosion, from its center outwards. Mirrors vanilla `ServerExplosion`.
pub struct Explosion {
    world: Arc<World>,
    center: DVec3,
    radius: f32,
    /// Whether the blast leaves fires behind.
    fire: bool,
    block_interaction: BlockInteraction,
    source: Option<SharedEntity>,
    /// Whoever is ultimately to blame, which is not always what physically exploded.
    indirect_source: Option<SharedEntity>,
    damage_source: DamageSource,
    damage_calculator: Box<dyn ExplosionDamageCalculator>,
    /// Knockback owed to each player, keyed by entity id.
    ///
    /// Players are told their own knockback in the explosion packet rather than having
    /// it applied server-side, so the client can react without waiting for a velocity
    /// update.
    hit_players: FxHashMap<i32, DVec3>,
}

impl Explosion {
    /// Builds an explosion, filling in vanilla's defaults for anything not given.
    #[must_use]
    #[expect(
        clippy::too_many_arguments,
        reason = "mirrors vanilla ServerExplosion's constructor; callers use World::explode"
    )]
    pub fn new(
        world: &Arc<World>,
        source: Option<SharedEntity>,
        damage_source: Option<DamageSource>,
        damage_calculator: Option<Box<dyn ExplosionDamageCalculator>>,
        center: DVec3,
        radius: f32,
        fire: bool,
        block_interaction: BlockInteraction,
    ) -> Self {
        let indirect_source = source.as_ref().and_then(indirect_source_entity);
        let damage_source = damage_source.unwrap_or_else(|| {
            default_damage_source(source.as_ref(), indirect_source.as_ref(), center)
        });
        let damage_calculator = damage_calculator.unwrap_or_else(|| match source.as_ref() {
            Some(source) => Box::new(EntityBasedExplosionDamageCalculator::new(source.id())),
            None => Box::new(DefaultExplosionDamageCalculator),
        });

        Self {
            world: Arc::clone(world),
            center,
            radius,
            fire,
            block_interaction,
            source,
            indirect_source,
            damage_source,
            damage_calculator,
            hit_players: FxHashMap::default(),
        }
    }

    /// The world the blast happened in.
    #[must_use]
    pub const fn world(&self) -> &Arc<World> {
        &self.world
    }

    /// Where the blast originated.
    #[must_use]
    pub const fn center(&self) -> DVec3 {
        self.center
    }

    /// How powerful the blast is; its reach is twice this.
    #[must_use]
    pub const fn radius(&self) -> f32 {
        self.radius
    }

    /// What the blast does to the blocks it reaches.
    #[must_use]
    pub const fn block_interaction(&self) -> BlockInteraction {
        self.block_interaction
    }

    /// The entity that set the blast off, if any. Mirrors `getDirectSourceEntity`.
    #[must_use]
    pub const fn direct_source_entity(&self) -> Option<&SharedEntity> {
        self.source.as_ref()
    }

    /// The damage every entity caught in the blast takes.
    #[must_use]
    pub const fn damage_source(&self) -> &DamageSource {
        &self.damage_source
    }

    /// Whoever is ultimately to blame for the blast, the player who lit the fuse
    /// rather than the fuse. Mirrors vanilla `getIndirectSourceEntity`.
    #[must_use]
    pub const fn indirect_source_entity(&self) -> Option<&SharedEntity> {
        self.indirect_source.as_ref()
    }

    /// Whether the blast traces back to a player.
    #[must_use]
    pub fn is_caused_by_player(&self) -> bool {
        self.indirect_source
            .as_ref()
            .is_some_and(|entity| entity.as_player().is_some())
    }

    /// Whether blocks may react to the blast without being broken by it.
    ///
    /// Mirrors vanilla `canTriggerBlocks`, which is what flips a lever caught in a
    /// wind charge. A breeze's charge is additionally gated on `mobGriefing`.
    #[must_use]
    pub fn can_trigger_blocks(&self) -> bool {
        if self.block_interaction != BlockInteraction::TriggerBlock {
            return false;
        }
        if self
            .source
            .as_ref()
            .is_none_or(|source| source.entity_type() != &vanilla_entities::BREEZE_WIND_CHARGE)
        {
            return true;
        }
        self.world.get_game_rule(&MOB_GRIEFING)
    }

    /// The knockback each player is owed, keyed by entity id.
    #[must_use]
    pub const fn hit_players(&self) -> &FxHashMap<i32, DVec3> {
        &self.hit_players
    }

    /// Whether the client should use the small explosion effect. Mirrors `isSmall`.
    #[must_use]
    pub fn is_small(&self) -> bool {
        self.radius < 2.0 || !self.block_interaction.interacts_with_blocks()
    }

    /// How much of `entity` the blast can see, from 0.0 to 1.0.
    ///
    /// Mirrors vanilla `getSeenPercent`. Rays are cast from a grid spread over the
    /// entity's box towards the center; a ray that reaches without hitting a block
    /// counts as exposure, which is why standing behind cover halves the damage.
    #[must_use]
    pub fn seen_percent(world: &World, center: DVec3, entity: &dyn Entity) -> f32 {
        let box_ = entity.bounding_box();
        let x_step = 1.0 / ((box_.max_x() - box_.min_x()) * 2.0 + 1.0);
        let y_step = 1.0 / ((box_.max_y() - box_.min_y()) * 2.0 + 1.0);
        let z_step = 1.0 / ((box_.max_z() - box_.min_z()) * 2.0 + 1.0);
        if x_step < 0.0 || y_step < 0.0 || z_step < 0.0 {
            return 0.0;
        }

        // Centers the sample grid inside the box, so a wide entity is not sampled
        // lopsidedly when its width does not divide evenly.
        let x_offset = (1.0 - (1.0 / x_step).floor() * x_step) / 2.0;
        let z_offset = (1.0 - (1.0 / z_step).floor() * z_step) / 2.0;

        let mut hits = 0_u32;
        let mut samples = 0_u32;
        let mut x_fraction = 0.0;
        while x_fraction <= 1.0 {
            let mut y_fraction = 0.0;
            while y_fraction <= 1.0 {
                let mut z_fraction = 0.0;
                while z_fraction <= 1.0 {
                    let from = DVec3::new(
                        lerp(x_fraction, box_.min_x(), box_.max_x()) + x_offset,
                        lerp(y_fraction, box_.min_y(), box_.max_y()),
                        lerp(z_fraction, box_.min_z(), box_.max_z()) + z_offset,
                    );
                    if world
                        .clip(from, center, ClipBlockShape::Collider, ClipFluid::None)
                        .is_miss()
                    {
                        hits += 1;
                    }
                    samples += 1;
                    z_fraction += z_step;
                }
                y_fraction += y_step;
            }
            x_fraction += x_step;
        }

        hits as f32 / samples as f32
    }

    /// Finds every block the blast reaches. Mirrors `calculateExplodedPositions`.
    fn calculate_exploded_positions(&self) -> Vec<BlockPos> {
        let mut reached = FxHashSet::default();

        for grid_x in 0..RAY_GRID_SIZE {
            for grid_y in 0..RAY_GRID_SIZE {
                for grid_z in 0..RAY_GRID_SIZE {
                    // Only the shell of the grid, so every ray leaves the center in a
                    // distinct direction instead of retracing its neighbors.
                    if !is_grid_shell(grid_x, grid_y, grid_z) {
                        continue;
                    }
                    self.cast_ray(grid_x, grid_y, grid_z, &mut reached);
                }
            }
        }

        reached.into_iter().collect()
    }

    /// Walks one ray out from the center, recording what it breaks.
    fn cast_ray(&self, grid_x: i32, grid_y: i32, grid_z: i32, reached: &mut FxHashSet<BlockPos>) {
        let direction = DVec3::new(
            grid_axis_direction(grid_x),
            grid_axis_direction(grid_y),
            grid_axis_direction(grid_z),
        )
        .normalize_or_zero()
            * f64::from(RAY_STEP);

        let mut power = self.radius * (RAY_POWER_FLOOR + rand::random::<f32>() * RAY_POWER_JITTER);
        let mut position = self.center;

        while power > 0.0 {
            let pos = BlockPos::containing(position.x, position.y, position.z);
            if !self.world.is_in_world_bounds(pos) {
                return;
            }

            let state = self.world.get_block_state(pos);
            let fluid = state.get_fluid_state();
            if let Some(resistance) = self
                .damage_calculator
                .block_explosion_resistance(self, pos, state, fluid)
            {
                power -= (resistance + RAY_STEP) * RAY_STEP;
            }

            if power > 0.0
                && self
                    .damage_calculator
                    .should_block_explode(self, pos, state, power)
            {
                reached.insert(pos);
            }

            position += direction;
            power -= RAY_STEP_DECAY;
        }
    }

    /// Damages and shoves everything in range. Mirrors `hurtEntities`.
    fn hurt_entities(&mut self) {
        if self.radius < MINIMUM_DAMAGING_RADIUS {
            return;
        }

        let double_radius = f64::from(self.radius * 2.0);
        let reach = DVec3::splat(double_radius + 1.0);
        let min = (self.center - reach).floor();
        let max = (self.center + reach).floor();
        let query = WorldAabb::new(min.x, min.y, min.z, max.x, max.y, max.z);

        for entity in self.world.get_entities_in_aabb(&query) {
            if entity.ignore_explosion(self) {
                continue;
            }

            let distance = entity.distance_to_sqr(self.center).sqrt() / double_radius;
            if distance > 1.0 {
                continue;
            }
            self.hurt_entity(entity.as_ref(), distance);
        }
    }

    /// Applies one entity's share of the blast.
    fn hurt_entity(&mut self, entity: &dyn Entity, distance: f64) {
        // Vanilla singles out primed TNT, measuring from its feet rather than its eyes.
        let origin = if entity.entity_type() == &vanilla_entities::TNT {
            entity.position()
        } else {
            DVec3::new(entity.position().x, entity.get_eye_y(), entity.position().z)
        };
        let direction = (origin - self.center).normalize_or_zero();

        let should_damage = self.damage_calculator.should_damage_entity(self, entity);
        let knockback_multiplier = self.damage_calculator.knockback_multiplier(entity);
        // Exposure costs a raycast per sample, so it is skipped when neither the damage
        // nor the shove would use it.
        let exposure = if should_damage || knockback_multiplier != 0.0 {
            Self::seen_percent(&self.world, self.center, entity)
        } else {
            0.0
        };

        if should_damage {
            let damage = self
                .damage_calculator
                .entity_damage_amount(self, entity, exposure);
            entity.hurt(&self.world, &self.damage_source, damage);
        }

        // Vanilla reads the attribute only from living entities; everything else takes
        // the full shove.
        let knockback_resistance = entity
            .as_living_entity()
            .and_then(|living| {
                living
                    .attributes()
                    .lock()
                    .get_value(vanilla_attributes::EXPLOSION_KNOCKBACK_RESISTANCE)
            })
            .unwrap_or(0.0);
        let power = (1.0 - distance)
            * f64::from(exposure)
            * f64::from(knockback_multiplier)
            * (1.0 - knockback_resistance);
        let knockback = direction * power;
        entity.push_impulse(knockback);

        // A blast can take ownership of a projectile it deflects, so an arrow batted
        // back by a wind charge is credited to whoever fired the charge.
        if let Some(projectile) = entity.as_projectile()
            && REGISTRY.entity_types.is_in_tag(
                entity.entity_type(),
                &EntityTypeTag::REDIRECTABLE_PROJECTILE,
            )
        {
            let owner = self
                .damage_source
                .causing_entity_id
                .and_then(|id| self.world.get_entity_by_id(id));
            projectile.set_owner_entity(owner.as_ref());
            return;
        }

        // A player is told their own knockback in the packet instead, so the client can
        // start moving without waiting for a velocity update.
        if let Some(player) = entity.as_player()
            && !player.is_spectator()
            && (player.game_mode() != GameType::Creative || !player.abilities.lock().flying)
        {
            self.hit_players.insert(entity.id(), knockback);
        }
    }

    /// Hands every reached block to its behavior, then spawns the merged drops.
    ///
    /// Mirrors vanilla `interactWithBlocks`. The positions are shuffled first so a
    /// crater's item stacks are not all attributed to its lowest corner.
    fn interact_with_blocks(&self, reached: &[BlockPos]) {
        let mut shuffled = reached.to_vec();
        shuffled.shuffle(&mut rand::rng());

        let mut collectors: Vec<StackCollector> = Vec::new();
        for &pos in &shuffled {
            let state = self.world.get_block_state(pos);
            BLOCK_BEHAVIORS
                .get_behavior(state.get_block())
                .on_explosion_hit(state, &self.world, pos, self, &mut |stack, pos| {
                    collect_drop(&mut collectors, stack, pos);
                });
        }

        for collector in collectors {
            self.world.pop_resource(collector.pos, collector.stack);
        }
    }

    /// Scatters fire across the crater. Mirrors vanilla `createFire`.
    fn create_fire(&self, reached: &[BlockPos]) {
        for &pos in reached {
            if rand::random_range(0..FIRE_CHANCE) != 0 {
                continue;
            }
            if !self.world.get_block_state(pos).is_air()
                || !self.world.get_block_state(pos.below()).is_solid_render()
            {
                continue;
            }
            self.world.set_block(
                pos,
                FireBlock::get_state(self.world.as_ref(), pos),
                UpdateFlags::UPDATE_ALL,
            );
        }
    }

    /// Runs the explosion, returning how many blocks it destroyed.
    ///
    /// Mirrors vanilla `explode`, including its ordering: the block list is computed
    /// before anything is damaged, so entities are hurt against the terrain as it stood
    /// when the blast went off.
    pub fn explode(&mut self) -> usize {
        let reached = self.calculate_exploded_positions();
        self.hurt_entities();

        if self.block_interaction.interacts_with_blocks() {
            self.interact_with_blocks(&reached);
        }
        if self.fire {
            self.create_fire(&reached);
        }

        reached.len()
    }
}

/// One position's worth of drops, merged with everything compatible near it.
struct StackCollector {
    pos: BlockPos,
    stack: ItemStack,
}

/// Folds `stack` into an existing collector where it fits, else starts a new one.
///
/// Mirrors vanilla `addOrAppendStack`. Merging is what stops a large crater spawning
/// one item entity per block broken.
fn collect_drop(collectors: &mut Vec<StackCollector>, mut stack: ItemStack, pos: BlockPos) {
    for collector in collectors.iter_mut() {
        collector.try_merge(&mut stack);
        if stack.is_empty() {
            return;
        }
    }

    collectors.push(StackCollector { pos, stack });
}

impl StackCollector {
    /// Moves as much of `incoming` into this stack as vanilla's cap allows.
    fn try_merge(&mut self, incoming: &mut ItemStack) {
        if !ItemEntity::are_mergeable(&self.stack, incoming) {
            return;
        }

        // Vanilla gates on the full stack size but transfers at most 16, so exploded
        // drops arrive in small stacks even for items that stack to 64.
        let capacity = self
            .stack
            .max_stack_size()
            .min(MAX_DROPS_PER_COMBINED_STACK)
            - self.stack.count();
        let moved = capacity.min(incoming.count());
        if moved <= 0 {
            return;
        }

        self.stack = self.stack.copy_with_count(self.stack.count() + moved);
        incoming.shrink(moved);
    }
}

/// Whether a grid coordinate triple lies on the shell of the sampling cube.
const fn is_grid_shell(x: i32, y: i32, z: i32) -> bool {
    let last = RAY_GRID_SIZE - 1;
    x == 0 || x == last || y == 0 || y == last || z == 0 || z == last
}

/// Maps a grid coordinate to the `[-1, 1]` component of a ray direction.
fn grid_axis_direction(coordinate: i32) -> f64 {
    f64::from((coordinate as f32 / (RAY_GRID_SIZE - 1) as f32).mul_add(2.0, -1.0))
}

/// Mirrors vanilla `Explosion.getDefaultDamageSource`.
///
/// A blast traced back to a player reports as `PLAYER_EXPLOSION`, which is what gives
/// the death message a name in it.
fn default_damage_source(
    source: Option<&SharedEntity>,
    indirect: Option<&SharedEntity>,
    center: DVec3,
) -> DamageSource {
    let damage_type = if indirect.is_some_and(|entity| entity.as_player().is_some()) {
        &vanilla_damage_types::PLAYER_EXPLOSION
    } else {
        &vanilla_damage_types::EXPLOSION
    };

    let mut damage_source = DamageSource::environment(damage_type).with_source_position(center);
    if let Some(source) = source {
        damage_source = damage_source.with_direct_entity(source.id());
    }
    if let Some(indirect) = indirect {
        damage_source = damage_source.with_causing_entity(indirect.id());
    }
    damage_source
}

/// Mirrors vanilla `Explosion.getIndirectSourceEntity`.
///
/// Walks past the thing that physically exploded to whoever is to blame for it: the
/// player who lit the TNT, or the shooter behind a projectile.
fn indirect_source_entity(source: &SharedEntity) -> Option<SharedEntity> {
    if let Some(projectile) = source.as_projectile() {
        return projectile
            .get_owner()
            .filter(|owner| owner.as_living_entity().is_some());
    }

    // TODO: Credit whoever lit it once `PrimedTnt` exists; vanilla checks that first.
    source.as_living_entity().map(|_| Arc::clone(source))
}

/// How a caller wants an explosion to treat blocks, before the game rules weigh in.
///
/// Mirrors vanilla `Level.ExplosionInteraction`. This is the knob a caller turns;
/// [`BlockInteraction`] is what it resolves to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplosionInteraction {
    /// Never touch blocks.
    None,
    /// A blast from a block, such as a bed in the wrong dimension.
    Block,
    /// A blast from a mob, which `mobGriefing` can switch off entirely.
    Mob,
    /// A blast from TNT.
    Tnt,
    /// A blast that only nudges blocks into reacting, such as a wind charge.
    Trigger,
}

impl ExplosionInteraction {
    /// Applies the game rules that decide what this blast may actually do.
    ///
    /// Mirrors the `switch` at the top of vanilla `ServerLevel.explode`. Each
    /// destroying variant reads its own drop-decay rule, and a mob's blast is
    /// suppressed wholesale when `mobGriefing` is off.
    fn resolve(self, world: &World) -> BlockInteraction {
        match self {
            Self::None => BlockInteraction::Keep,
            Self::Block => destroy_type(world, &BLOCK_EXPLOSION_DROP_DECAY),
            Self::Mob => {
                if world.get_game_rule(&MOB_GRIEFING) {
                    destroy_type(world, &MOB_EXPLOSION_DROP_DECAY)
                } else {
                    BlockInteraction::Keep
                }
            }
            Self::Tnt => destroy_type(world, &TNT_EXPLOSION_DROP_DECAY),
            Self::Trigger => BlockInteraction::TriggerBlock,
        }
    }
}

/// Mirrors vanilla `ServerLevel.getDestroyType`.
fn destroy_type(world: &World, rule: &GameRule<bool>) -> BlockInteraction {
    if world.get_game_rule(rule) {
        BlockInteraction::DestroyWithDecay
    } else {
        BlockInteraction::Destroy
    }
}

/// The debris a blast paints, which no vanilla caller varies.
fn default_block_particles() -> Vec<Weighted<ExplosionParticleInfo>> {
    vec![
        Weighted::unit(ExplosionParticleInfo::new(
            ParticleData::simple(&vanilla_particle_types::POOF),
            0.5,
            ExplosionParticleInfo::DEFAULT_FACTOR,
        )),
        Weighted::unit(ExplosionParticleInfo::new(
            ParticleData::simple(&vanilla_particle_types::SMOKE),
            ExplosionParticleInfo::DEFAULT_FACTOR,
            ExplosionParticleInfo::DEFAULT_FACTOR,
        )),
    ]
}

impl World {
    /// Sets off an explosion and tells nearby clients about it.
    ///
    /// Mirrors vanilla `ServerLevel.explode`. Vanilla's widest overload also takes two
    /// particle types, a debris list and a sound, but every vanilla caller passes the
    /// defaults, so those are constants here rather than parameters nothing varies.
    #[expect(
        clippy::too_many_arguments,
        reason = "mirrors the vanilla ServerLevel.explode overload every caller uses"
    )]
    pub fn explode(
        self: &Arc<Self>,
        source: Option<SharedEntity>,
        damage_source: Option<DamageSource>,
        damage_calculator: Option<Box<dyn ExplosionDamageCalculator>>,
        center: DVec3,
        radius: f32,
        fire: bool,
        interaction: ExplosionInteraction,
    ) {
        let mut explosion = Explosion::new(
            self,
            source,
            damage_source,
            damage_calculator,
            center,
            radius,
            fire,
            interaction.resolve(self),
        );
        let block_count = explosion.explode();
        let particle = if explosion.is_small() {
            &vanilla_particle_types::EXPLOSION
        } else {
            &vanilla_particle_types::EXPLOSION_EMITTER
        };

        // Built per player rather than broadcast: each recipient is told only its own
        // knockback, so the client can react without waiting for a velocity update.
        self.players.iter_players(|_, player| {
            if !Self::recipient_within_64_blocks_of(player.position(), center) {
                return true;
            }
            player.send_packet(CExplode {
                center,
                radius,
                block_count: i32::try_from(block_count).unwrap_or(i32::MAX),
                player_knockback: explosion.hit_players().get(&player.id()).copied(),
                explosion_particle: ParticleData::simple(particle),
                explosion_sound: SoundEventHolder::registry(&sound_events::ENTITY_GENERIC_EXPLODE),
                block_particles: default_block_particles(),
            });
            true
        });
    }
}
