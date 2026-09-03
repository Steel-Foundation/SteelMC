//! Server-side explosions.
//!
//! Mirrors vanilla `ServerExplosion`, `ServerLevel.explode`, and
//! `Level.ExplosionInteraction`. Only the server-authoritative half exists: the client
//! renders particles, sound, shake, and its own knockback from the `CExplode` packet,
//! so `Level.addParticle`/`playLocalSound` calls in vanilla's client path have no
//! server counterpart here.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use glam::DVec3;
use steel_protocol::packets::game::CExplode;
use steel_protocol::utils::ConnectionProtocol;
use steel_protocol::packet_traits::EncodedPacket;

use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::item_stack::ItemStack;
use steel_registry::particle_type::ParticleData;
use steel_registry::sound_events;
use steel_registry::vanilla_game_rules::{
    BLOCK_EXPLOSION_DROP_DECAY, MOB_EXPLOSION_DROP_DECAY, MOB_GRIEFING, TNT_EXPLOSION_DROP_DECAY,
};
use steel_registry::{
    vanilla_attributes, vanilla_blocks, vanilla_damage_types, vanilla_game_events,
    vanilla_particle_types,
};
use steel_utils::{BlockPos, BlockStateId, WorldAabb, types::UpdateFlags};

use crate::behavior::blocks::FireBlock;
use crate::behavior::{BLOCK_BEHAVIORS, BlockLootContext, FLUID_BEHAVIORS};
use crate::entity::damage::DamageSource;
use crate::entity::entities::ItemEntity;
use crate::player::connection::NetworkConnection as _;
use crate::entity::{Entity, LivingEntity, SharedEntity};
use crate::world::level_reader::LevelReader as _;
use crate::world::{ClipBlockShape, ClipFluid, World};

/// Vanilla `Explosion.BlockInteraction`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplosionBlockInteraction {
    /// Blocks are left untouched.
    Keep,
    /// Blocks are destroyed; every broken block drops its full loot.
    Destroy,
    /// Blocks are destroyed; loot tables see `EXPLOSION_RADIUS` and may drop nothing.
    DestroyWithDecay,
    /// Blocks are not removed, only triggered (used by wind charges).
    TriggerBlock,
}

impl ExplosionBlockInteraction {
    /// Vanilla `Explosion.BlockInteraction.shouldAffectBlocklikeEntities`.
    #[must_use]
    pub const fn should_affect_blocklike_entities(self) -> bool {
        matches!(self, Self::Destroy | Self::DestroyWithDecay)
    }

    /// Vanilla `ServerExplosion.interactsWithBlocks`.
    #[must_use]
    pub const fn interacts_with_blocks(self) -> bool {
        !matches!(self, Self::Keep)
    }
}

/// Vanilla `Level.ExplosionInteraction`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplosionInteraction {
    /// Never destroys blocks.
    None,
    /// Block-triggered explosion, honoring `blockExplosionDropDecay`.
    Block,
    /// Mob-triggered explosion, additionally honoring `mobGriefing`.
    Mob,
    /// TNT explosion, honoring `tntExplosionDropDecay`.
    Tnt,
    /// Only triggers blocks.
    Trigger,
}

/// Read-only view of the in-progress explosion handed to `Entity::ignore_explosion`.
///
/// This is the subset of vanilla's `Explosion` interface that entity types consult.
pub struct ExplosionView<'a> {
    should_affect_blocklike_entities: bool,
    direct_source: Option<&'a dyn Entity>,
    indirect_source: Option<&'a dyn LivingEntity>,
}

impl<'a> ExplosionView<'a> {
    /// Creates the view for an explosion with the given block interaction and sources.
    #[must_use]
    pub const fn new(
        should_affect_blocklike_entities: bool,
        direct_source: Option<&'a dyn Entity>,
        indirect_source: Option<&'a dyn LivingEntity>,
    ) -> Self {
        Self {
            should_affect_blocklike_entities,
            direct_source,
            indirect_source,
        }
    }

    /// Whether block-like entities (items, armor stands, decorations) may be affected.
    #[must_use]
    pub const fn should_affect_blocklike_entities(&self) -> bool {
        self.should_affect_blocklike_entities
    }

    /// Vanilla `Explosion.getDirectSourceEntity`.
    #[must_use]
    pub const fn direct_source(&self) -> Option<&'a dyn Entity> {
        self.direct_source
    }

    /// Vanilla `Explosion.getIndirectSourceEntity`.
    #[must_use]
    pub const fn indirect_source(&self) -> Option<&'a dyn LivingEntity> {
        self.indirect_source
    }
}

/// The ray-cast step length used by vanilla's block damage sweep.
const RAY_STEP: f64 = 0.3;
/// Vanilla casts rays from the surface of a 16x16x16 cube.
const RAY_STEPS: i32 = 16;
/// Vanilla's per-step power loss, `0.22500001`.
const RAY_STEP_POWER_LOSS: f32 = 0.225_000_01;
/// Vanilla's resistance offset and scale, `(resistance + 0.3) * 0.3`.
const RESISTANCE_OFFSET: f32 = 0.3;
const RESISTANCE_SCALE: f32 = 0.3;
/// Vanilla `ServerExplosion.MAX_DROPS_PER_COMBINED_STACK`.
const MAX_DROPS_PER_COMBINED_STACK: i32 = 16;
/// Vanilla `ServerExplosion.LARGE_EXPLOSION_RADIUS`.
const LARGE_EXPLOSION_RADIUS: f32 = 2.0;
/// Vanilla's `distanceToSqr(center) < 4096.0` recipient check for the explode packet.
const EXPLODE_PACKET_RANGE_SQ: f64 = 4096.0;

/// One collected drop stack, mirroring vanilla `ServerExplosion.StackCollector`.
struct StackCollector {
    pos: BlockPos,
    stack: ItemStack,
}

impl StackCollector {
    /// Vanilla `ServerExplosion.addOrAppendStack`.
    fn add_or_append(collectors: &mut Vec<Self>, mut stack: ItemStack, pos: BlockPos) {
        for collector in collectors.iter_mut() {
            if !ItemEntity::are_mergeable(&collector.stack, &stack) {
                continue;
            }
            let space = (collector.stack.max_stack_size().min(MAX_DROPS_PER_COMBINED_STACK)
                - collector.stack.count())
            .min(stack.count());
            if space > 0 {
                collector.stack.grow(space);
                stack.shrink(space);
            }
            if stack.is_empty() {
                return;
            }
        }

        collectors.push(Self { pos, stack });
    }
}

/// A server-authoritative explosion, mirroring vanilla `ServerExplosion`.
struct Explosion<'a> {
    world: &'a Arc<World>,
    center: DVec3,
    radius: f32,
    fire: bool,
    block_interaction: ExplosionBlockInteraction,
    direct_source: Option<&'a dyn Entity>,
    /// Vanilla `Explosion.getIndirectSourceEntity`, resolved once because a projectile
    /// owner is only reachable as an owned handle.
    indirect_source: Option<SharedEntity>,
    damage_source: DamageSource,
    hit_players: HashMap<i32, DVec3>,
}

impl<'a> Explosion<'a> {
    fn new(
        world: &'a Arc<World>,
        direct_source: Option<&'a dyn Entity>,
        center: DVec3,
        radius: f32,
        fire: bool,
        block_interaction: ExplosionBlockInteraction,
    ) -> Self {
        let indirect_source = direct_source.and_then(|source| Self::indirect_source_of(world, source));
        let damage_type = if direct_source.is_some() && indirect_source.is_some() {
            &vanilla_damage_types::PLAYER_EXPLOSION
        } else {
            &vanilla_damage_types::EXPLOSION
        };

        let mut damage_source =
            DamageSource::environment(damage_type).with_source_position(center);
        if let Some(source) = direct_source {
            damage_source = damage_source.with_direct_entity(source.id());
        }
        if let Some(indirect) = &indirect_source {
            damage_source = damage_source.with_causing_entity(indirect.id());
        }

        Self {
            world,
            center,
            radius,
            fire,
            block_interaction,
            direct_source,
            indirect_source,
            damage_source,
            hit_players: HashMap::new(),
        }
    }

    /// Vanilla `Explosion.getIndirectSourceEntity`.
    ///
    /// A projectile owner is only reachable as an owned handle, so the resolved entity is
    /// returned as a shared handle rather than a borrow.
    fn indirect_source_of(world: &World, direct_source: &dyn Entity) -> Option<SharedEntity> {
        if direct_source.as_living_entity().is_some() {
            return world.get_entity_by_id(direct_source.id());
        }
        direct_source.as_projectile()?.get_owner()
    }

    /// Builds the view handed to `Entity::ignore_explosion`.
    fn view(&self) -> ExplosionView<'_> {
        ExplosionView::new(
            self.block_interaction.should_affect_blocklike_entities(),
            self.direct_source,
            self.indirect_source
                .as_deref()
                .and_then(Entity::as_living_entity),
        )
    }

    /// Vanilla `ServerExplosion.isSmall`.
    fn is_small(&self) -> bool {
        self.radius < LARGE_EXPLOSION_RADIUS || !self.block_interaction.interacts_with_blocks()
    }

    /// Runs vanilla `ServerExplosion.explode` and returns the number of blocks hit.
    fn explode(&mut self) -> usize {
        self.world.game_event_at(
            &vanilla_game_events::EXPLODE,
            self.center,
            &crate::world::game_event::GameEventContext::new(self.direct_source, None),
        );

        let exploded_positions = self.calculate_exploded_positions();
        let block_count = exploded_positions.len();
        self.hurt_entities();

        if self.block_interaction.interacts_with_blocks() {
            self.interact_with_blocks(&exploded_positions);
        }
        if self.fire {
            self.create_fire(&exploded_positions);
        }

        block_count
    }
}

impl Explosion<'_> {
    /// Vanilla `ServerExplosion.calculateExplodedPositions`.
    ///
    /// Casts 16x16x16 rays from the surface of a unit cube around the center, walking each
    /// ray in 0.3-block steps and spending power on every block's explosion resistance.
    fn calculate_exploded_positions(&self) -> Vec<BlockPos> {
        let mut exploded = HashSet::new();
        let last_step = RAY_STEPS - 1;

        for xx in 0..RAY_STEPS {
            for yy in 0..RAY_STEPS {
                for zz in 0..RAY_STEPS {
                    if xx != 0 && xx != last_step && yy != 0 && yy != last_step
                        && zz != 0 && zz != last_step
                    {
                        continue;
                    }

                    let mut xd = xx as f64 / last_step as f64 * 2.0 - 1.0;
                    let mut yd = yy as f64 / last_step as f64 * 2.0 - 1.0;
                    let mut zd = zz as f64 / last_step as f64 * 2.0 - 1.0;
                    let length = (xd * xd + yd * yd + zd * zd).sqrt();
                    xd /= length;
                    yd /= length;
                    zd /= length;

                    let mut remaining_power = self.radius * (0.7 + rand::random::<f32>() * 0.6);
                    let (mut xp, mut yp, mut zp) = (self.center.x, self.center.y, self.center.z);

                    while remaining_power > 0.0 {
                        let pos = BlockPos::new(
                            xp.floor() as i32,
                            yp.floor() as i32,
                            zp.floor() as i32,
                        );
                        if !self.world.is_in_valid_bounds(pos) {
                            break;
                        }

                        let state = self.world.get_block_state(pos);
                        let fluid = state.get_fluid_state();
                        if let Some(resistance) = self.block_explosion_resistance(state, fluid) {
                            remaining_power -= (resistance + RESISTANCE_OFFSET) * RESISTANCE_SCALE;
                        }
                        if remaining_power > 0.0 {
                            exploded.insert(pos);
                        }

                        xp += xd * RAY_STEP;
                        yp += yd * RAY_STEP;
                        zp += zd * RAY_STEP;
                        remaining_power -= RAY_STEP_POWER_LOSS;
                    }
                }
            }
        }

        exploded.into_iter().collect()
    }

    /// Vanilla `ExplosionDamageCalculator.getBlockExplosionResistance`.
    ///
    /// `None` mirrors vanilla's `Optional.empty()` for air with no fluid, which costs the
    /// ray no power at all.
    fn block_explosion_resistance(
        &self,
        state: BlockStateId,
        fluid: steel_registry::fluid::FluidState,
    ) -> Option<f32> {
        if state.is_air() && fluid.is_empty() {
            return None;
        }

        let fluid_resistance = FLUID_BEHAVIORS
            .get_behavior(fluid.fluid_id)
            .explosion_resistance();
        Some(state.get_block().config.explosion_resistance.max(fluid_resistance))
    }

    /// Vanilla `ServerExplosion.getSeenPercent`: the fraction of the entity's bounding box
    /// sample points that have an unobstructed line to the explosion center.
    fn seen_percent(&self, entity: &dyn Entity) -> f32 {
        let bb = entity.bounding_box();
        let min = bb.min_corner();
        let max = bb.max_corner();

        let xs = 1.0 / ((max.x - min.x) * 2.0 + 1.0);
        let ys = 1.0 / ((max.y - min.y) * 2.0 + 1.0);
        let zs = 1.0 / ((max.z - min.z) * 2.0 + 1.0);
        if xs < 0.0 || ys < 0.0 || zs < 0.0 {
            return 0.0;
        }
        let x_offset = (1.0 - (1.0 / xs).floor() * xs) / 2.0;
        let z_offset = (1.0 - (1.0 / zs).floor() * zs) / 2.0;

        let mut hits = 0_u32;
        let mut total = 0_u32;
        let mut xx = 0.0_f64;
        while xx <= 1.0 {
            let mut yy = 0.0_f64;
            while yy <= 1.0 {
                let mut zz = 0.0_f64;
                while zz <= 1.0 {
                    let from = DVec3::new(
                        lerp(xx, min.x, max.x) + x_offset,
                        lerp(yy, min.y, max.y),
                        lerp(zz, min.z, max.z) + z_offset,
                    );
                    if self
                        .world
                        .clip(from, self.center, ClipBlockShape::Collider, ClipFluid::None)
                        .is_miss()
                    {
                        hits += 1;
                    }
                    total += 1;
                    zz += zs;
                }
                yy += ys;
            }
            xx += xs;
        }

        hits as f32 / total as f32
    }

    /// Vanilla `ExplosionDamageCalculator.getEntityDamageAmount`.
    fn entity_damage_amount(&self, entity: &dyn Entity, exposure: f32) -> f32 {
        let double_radius = f64::from(self.radius) * 2.0;
        let distance = entity.position().distance(self.center) / double_radius;
        let power = (1.0 - distance) as f32 * exposure;
        (power * power + power) / 2.0 * 7.0 * double_radius as f32 + 1.0
    }
}

impl Explosion<'_> {
    /// Vanilla `ServerExplosion.hurtEntities`.
    fn hurt_entities(&mut self) {
        if self.radius < 1.0e-5 {
            return;
        }

        let double_radius = f64::from(self.radius) * 2.0;
        let search_box = WorldAabb::from_min_max(
            DVec3::new(
                self.center.x - double_radius - 1.0,
                self.center.y - double_radius - 1.0,
                self.center.z - double_radius - 1.0,
            ),
            DVec3::new(
                self.center.x + double_radius + 1.0,
                self.center.y + double_radius + 1.0,
                self.center.z + double_radius + 1.0,
            ),
        );

        let source_id = self.direct_source.map(Entity::id);
        let view = self.view();
        for entity in self.world.get_entities_in_aabb_matching(&search_box, |entity| {
            Some(entity.id()) != source_id && !entity.ignore_explosion(&view)
        }) {
            let entity: &dyn Entity = entity.as_ref();
            let distance = entity.position().distance(self.center) / double_radius;
            if distance > 1.0 {
                continue;
            }

            let entity_origin = DVec3::new(
                entity.position().x,
                Entity::get_eye_y(entity),
                entity.position().z,
            );
            let direction = (entity_origin - self.center).normalize_or_zero();
            let exposure = self.seen_percent(entity);

            entity.hurt(self.world, &self.damage_source, self.entity_damage_amount(entity, exposure));

            let knockback_resistance = entity
                .as_living_entity()
                .map_or(0.0, |living| {
                    living
                        .attributes()
                        .lock()
                        .get_value(&vanilla_attributes::EXPLOSION_KNOCKBACK_RESISTANCE)
                        .unwrap_or(0.0)
                });
            let knockback_power =
                (1.0 - distance) * f64::from(exposure) * (1.0 - knockback_resistance);
            let knockback = direction * knockback_power;
            entity.push_impulse(knockback);

            if let Some(player) = entity.as_player() {
                let is_flying_creative = player.has_infinite_materials() && player.is_flying();
                if !player.is_spectator() && !is_flying_creative {
                    self.hit_players.insert(player.id(), knockback);
                }
            }
        }
    }

    /// Vanilla `ServerExplosion.interactWithBlocks` plus `BlockBehaviour.onExplosionHit`.
    fn interact_with_blocks(&self, positions: &[BlockPos]) {
        let mut shuffled = positions.to_vec();
        shuffle(&mut shuffled);

        let mut collectors = Vec::new();
        // Vanilla's explosion loot params always carry an empty tool.
        let empty_tool = ItemStack::empty();
        for &pos in &shuffled {
            let state = self.world.get_block_state(pos);
            if state.is_air() {
                continue;
            }

            let mut context = BlockLootContext::new(self.world, pos)
                .with_entity(self.direct_source)
                .with_tool(&empty_tool);
            if self.block_interaction == ExplosionBlockInteraction::DestroyWithDecay {
                context = context.with_explosion_radius(self.radius);
            }

            BLOCK_BEHAVIORS
                .get_behavior(state.get_block())
                .spawn_after_break(state, self.world, pos, &ItemStack::empty(), false);
            for stack in context.get_drops(state) {
                if !stack.is_empty() {
                    StackCollector::add_or_append(&mut collectors, stack, pos);
                }
            }

            // Vanilla uses flags 3 here: UPDATE_NEIGHBORS | UPDATE_CLIENTS.
            self.world.set_block(
                pos,
                vanilla_blocks::AIR.default_state(),
                UpdateFlags::UPDATE_ALL,
            );
        }

        for collector in collectors {
            self.world.pop_resource(collector.pos, collector.stack);
        }
    }

    /// Vanilla `ServerExplosion.createFire`.
    fn create_fire(&self, positions: &[BlockPos]) {
        for &pos in positions {
            if rand::random_range(0..3) != 0 {
                continue;
            }
            let state = self.world.get_block_state(pos);
            let below = self.world.get_block_state(pos.below());
            if state.is_air() && below.is_solid_render() {
                let fire = FireBlock::get_state(self.world.as_ref(), pos);
                self.world.set_block(pos, fire, UpdateFlags::UPDATE_ALL);
            }
        }
    }
}

/// Vanilla `Util.shuffle`, a Fisher-Yates shuffle over the world's gameplay random source.
fn shuffle<T>(values: &mut [T]) {
    for i in (1..values.len()).rev() {
        let j = rand::random_range(0..=i);
        values.swap(i, j);
    }
}

/// Vanilla `Mth.lerp` for scalar components.
fn lerp(delta: f64, from: f64, to: f64) -> f64 {
    from + delta * (to - from)
}

impl World {
    /// Runs a vanilla-shaped server explosion and notifies nearby clients.
    ///
    /// Mirrors `ServerLevel.explode(Entity, DamageSource, ExplosionDamageCalculator, double,
    /// double, double, float, boolean, ExplosionInteraction)` for the default damage
    /// calculator, particles, and sound that `Level.explode` supplies.
    ///
    /// Returns the number of block positions the explosion destroyed.
    pub fn explode(
        self: &Arc<Self>,
        source: Option<&dyn Entity>,
        center: DVec3,
        radius: f32,
        fire: bool,
        interaction: ExplosionInteraction,
    ) -> usize {
        let block_interaction = self.block_interaction_for(interaction);

        let mut explosion = Explosion::new(self, source, center, radius, fire, block_interaction);
        let block_count = explosion.explode();
        let is_small = explosion.is_small();

        let particle = if is_small {
            ParticleData::simple(&vanilla_particle_types::EXPLOSION)
        } else {
            ParticleData::simple(&vanilla_particle_types::EXPLOSION_EMITTER)
        };

        let mut base = CExplode::new(
            center,
            radius,
            i32::try_from(block_count).unwrap_or(i32::MAX),
            particle,
            &sound_events::ENTITY_GENERIC_EXPLODE,
        );
        let Ok(base_encoded) =
            EncodedPacket::from_bare(base.clone(), self.compression, ConnectionProtocol::Play)
        else {
            log::warn!("Failed to encode explosion packet");
            return block_count;
        };

        self.players.iter_players(|_, player| {
            if player.position().distance_squared(center) >= EXPLODE_PACKET_RANGE_SQ {
                return true;
            }
            let packet = match explosion.hit_players.get(&player.id()) {
                Some(knockback) => {
                    base.player_knockback = Some(*knockback);
                    EncodedPacket::from_bare(
                        base.clone(),
                        self.compression,
                        ConnectionProtocol::Play,
                    )
                }
                None => Ok(base_encoded.clone()),
            };
            match packet {
                Ok(encoded) => player.connection.send_encoded(encoded),
                Err(error) => log::warn!("Failed to encode explosion packet: {error}"),
            }
            true
        });

        block_count
    }

    /// Vanilla `ServerLevel.explode`'s `ExplosionInteraction` -> `BlockInteraction` switch.
    fn block_interaction_for(&self, interaction: ExplosionInteraction) -> ExplosionBlockInteraction {
        match interaction {
            ExplosionInteraction::None => ExplosionBlockInteraction::Keep,
            ExplosionInteraction::Block => {
                self.destroy_type(&BLOCK_EXPLOSION_DROP_DECAY)
            }
            ExplosionInteraction::Mob => {
                if self.get_game_rule(&MOB_GRIEFING) {
                    self.destroy_type(&MOB_EXPLOSION_DROP_DECAY)
                } else {
                    ExplosionBlockInteraction::Keep
                }
            }
            ExplosionInteraction::Tnt => {
                self.destroy_type(&TNT_EXPLOSION_DROP_DECAY)
            }
            ExplosionInteraction::Trigger => ExplosionBlockInteraction::TriggerBlock,
        }
    }

    /// Vanilla `ServerLevel.getDestroyType`.
    fn destroy_type(&self, decay_rule: &steel_registry::game_rules::GameRule<bool>) -> ExplosionBlockInteraction {
        if self.get_game_rule(decay_rule) {
            ExplosionBlockInteraction::DestroyWithDecay
        } else {
            ExplosionBlockInteraction::Destroy
        }
    }
}
