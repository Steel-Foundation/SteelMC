//! The Ender Dragon (vanilla `EnderDragon`).
//!
//! A `Mob` with no goals or path navigation: it steers itself from a phase state
//! machine over a 24-node graph, so its `ai_step` replaces the shared one outright.
//!
//! Client-only members (`onFlap`, `doClientTick`, `onSyncedDataUpdated`,
//! `recreateFromPacket`, `growlTime`) are omitted; the client runs its own phase
//! machine off the synced phase id, so that id is what drives the visuals.

mod flight_graph;
mod flight_history;
mod part;
mod phases;
#[cfg(test)]
mod tests;

pub use flight_graph::DragonFlightGraph;
pub use flight_history::{DragonFlightHistory, DragonFlightSample};
pub use part::EnderDragonPart;
pub use phases::{DragonPhaseInstance, EnderDragonPhase, EnderDragonPhaseManager};

use std::array;
use std::f32::consts::TAU;
use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::entity_behavior;
use steel_math::{trig, wrap_degrees};
use steel_protocol::packets::game::SoundSource;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::entity_type::{EntityDimensions, EntityTypeRef};
use steel_registry::item_stack::ItemStack;
use steel_registry::sound_event::SoundEventRef;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_entity_data::EnderDragonEntityData;
use steel_registry::vanilla_game_rules::{MOB_DROPS, MOB_GRIEFING};
use steel_registry::{
    level_events, sound_events, vanilla_damage_type_tags, vanilla_damage_types, vanilla_game_events,
};
use steel_utils::geometry::WorldAabb;
use steel_utils::locks::SyncMutex;
use steel_utils::{BlockPos, Downcast as _, DowncastType, DowncastTypeKey};

use crate::enchantment_helper::{self, EnchantmentPostAttackContext};
use crate::entity::ai::node::Node;
use crate::entity::ai::path::Path;
use crate::entity::damage::DamageSource;
use crate::entity::entities::{EndCrystalEntity, ExperienceOrbEntity};
use crate::entity::{
    Entity, EntityBase, EntityBaseLoad, EntityPose, EntitySyncedData, LivingEntity,
    LivingEntityBase, Mob, MobBase, MobEffectInstance, PartEntity, RemovalReason, SharedEntity,
    entity_selector, part_entity_id, sync_dirty_mob_effects,
};
use crate::physics::{MoveResult, MoverType};
use crate::world::World;

const SUB_ENTITY_COUNT: usize = 8;
/// Indices into `sub_entities`, in vanilla construction order: head, neck, body,
/// tail x3, wing x2.
const HEAD: usize = 0;
const NECK: usize = 1;
const BODY: usize = 2;
const TAIL_START: usize = 3;
const TAIL_COUNT: usize = 3;
const WING1: usize = 6;
const WING2: usize = 7;

/// Damage below this is dropped rather than applied.
const MINIMUM_EFFECTIVE_DAMAGE: f32 = 0.01;
/// Share of max health that dislodges a perched dragon.
const SITTING_ALLOWED_DAMAGE_FRACTION: f32 = 0.25;
/// Wing beat while perched, where the flight-speed formula does not apply.
const SITTING_FLAP_RATE: f32 = 0.1;
/// Wings held mid-beat while the dragon has no AI.
const NO_AI_FLAP_TIME: f32 = 0.5;
/// Forward thrust per tick, scaled by how well the dragon is already aimed.
const FORWARD_THRUST: f32 = 0.06;
/// Turn rate ceiling per tick, in degrees.
const MAX_TURN_DEGREES: f32 = 50.0;
/// Below this the dragon is close enough on an axis not to bother turning.
const STEERING_EPSILON: f64 = 1.0e-5;
/// Movement scale while clipping terrain.
const IN_WALL_MOVE_SCALE: f64 = 0.8;
const VERTICAL_DRAG: f64 = 0.91;

/// Wing damage, dealt on top of the knockback.
const WING_DAMAGE: f32 = 5.0;
/// Head and neck contact damage.
const BITE_DAMAGE: f32 = 10.0;
/// Floor on the squared knockback distance, so an entity right under the body is
/// not flung.
const MINIMUM_KNOCKBACK_DISTANCE_SQR: f64 = 0.1;
const KNOCKBACK_STRENGTH: f64 = 4.0;
const KNOCKBACK_LIFT: f64 = 0.2;
/// A target hurt within this many ticks is only shoved, not hit again.
const REPEAT_ATTACK_GRACE_TICKS: i32 = 2;

const DRAGON_DEATH_DURATION: i32 = 200;
/// After this death tick, experience trickles out every `DEATH_XP_TRICKLE_INTERVAL`.
const DEATH_XP_TRICKLE_START: i32 = 150;
const DEATH_XP_TRICKLE_INTERVAL: i32 = 5;
const DEATH_XP_TRICKLE_SHARE: f32 = 0.08;
/// Share of the prize paid out when the death animation ends.
const DEATH_XP_FINAL_SHARE: f32 = 0.2;
/// Vanilla awards 12000 instead on a fight's first kill.
// TODO: Read the fight's `hasPreviouslyKilledDragon` once `EnderDragonFight` exists.
const DEATH_EXPERIENCE: i32 = 500;
/// `f32` because vanilla widens a float literal here (`0.100000001...`, not `0.1`).
const DEATH_DRIFT_PER_TICK: f32 = 0.1;

const CRYSTAL_SEARCH_RADIUS: f64 = 32.0;
/// The crystal heals the dragon on ticks divisible by this.
const CRYSTAL_HEAL_INTERVAL: i32 = 10;
const CRYSTAL_HEAL_AMOUNT: f32 = 1.0;
/// One tick in this many rescans for a nearer crystal.
const CRYSTAL_RESCAN_CHANCE: i32 = 10;

const DRAGON_PHASE_KEY: &str = "DragonPhase";
const DRAGON_DEATH_TIME_KEY: &str = "DragonDeathTime";
const SITTING_DAMAGE_RECEIVED_KEY: &str = "sitting_damage_received";

/// Where the exit portal sits for an arena centered on `origin` (vanilla
/// `EndPodiumFeature.getLocation`; an identity since `END_PODIUM_LOCATION` is zero).
// TODO: Move this onto `EndPodiumFeature` once runtime feature placement exists.
#[must_use]
pub const fn end_podium_location(origin: BlockPos) -> BlockPos {
    origin
}

/// Vanilla `Mth.floor(total * share)`, where the multiplication happens in `f32`.
fn share_of(total: i32, share: f32) -> i32 {
    (total as f32 * share).floor() as i32
}

/// Vanilla `rotWrap`, which wraps in `f64` before narrowing. Narrowing first is safe
/// here: the inputs are yaw differences, which both widths wrap to the same value.
fn rot_wrap(degrees: f64) -> f32 {
    wrap_degrees(degrees as f32)
}

/// The Ender Dragon.
#[entity_behavior(class = "EnderDragon", parts = 8)]
pub struct EnderDragonEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    living_base: LivingEntityBase,
    mob_base: MobBase,
    entity_data: SyncMutex<EnderDragonEntityData>,
    sub_entities: Vec<Arc<dyn PartEntity>>,
    dragon_death_time: SyncMutex<i32>,
    sitting_damage_received: SyncMutex<f32>,
    flight_history: SyncMutex<DragonFlightHistory>,
    phase_manager: EnderDragonPhaseManager,
    /// Not just cosmetic: `is_flapping` reads it to emit the FLAP game event.
    flap_time: SyncMutex<f32>,
    o_flap_time: SyncMutex<f32>,
    /// Vanilla `yRotA`, the accumulated turn rate.
    y_rot_a: SyncMutex<f32>,
    in_wall: SyncMutex<bool>,
    /// Set by the fight, not persisted (vanilla saves only phase, death time and
    /// sitting damage).
    fight_origin: SyncMutex<BlockPos>,
    /// Vanilla `nodes`/`nodeAdjacency`, built on first use because it samples terrain.
    flight_graph: SyncMutex<Option<DragonFlightGraph>>,
    /// Held by id so a destroyed crystal stops resolving (vanilla nulls it on
    /// `isRemoved`) without an `Arc` cycle keeping it alive.
    nearest_crystal: SyncMutex<Option<i32>>,
}

// SAFETY: The owner-scoped type key uniquely identifies EnderDragonEntity.
unsafe impl DowncastType for EnderDragonEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/ender_dragon");
}

impl EnderDragonEntity {
    /// Creates a dragon; `id` must start an [`crate::entity::EntityIdBlock`] wide
    /// enough for the eight parts, which the entity registry guarantees.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::new_with_base(
            EntityBase::new(id, position, entity_type.dimensions, Weak::clone(&world)),
            entity_type,
            world,
        )
    }

    /// Loads a saved dragon.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        let world = Weak::clone(&load.world);
        Self::new_with_base(
            EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
            world,
        )
    }

    fn new_with_base(base: EntityBase, entity_type: EntityTypeRef, world: Weak<World>) -> Self {
        let living_base = LivingEntityBase::new(entity_type);
        let mob_base = MobBase::new();
        let mut entity_data = EnderDragonEntityData::new();
        // Seeds health from MAX_HEALTH; otherwise it would start at the synced 1.0.
        living_base.initialize_synced_data(&mut entity_data);

        // The dragon passes through terrain and handles walls in `check_walls`.
        base.set_no_physics(true);

        Self {
            sub_entities: Self::create_sub_entities(entity_type, &base, &world),
            base,
            entity_type,
            living_base,
            mob_base,
            entity_data: SyncMutex::new(entity_data),
            dragon_death_time: SyncMutex::new(0),
            sitting_damage_received: SyncMutex::new(0.0),
            flight_history: SyncMutex::new(DragonFlightHistory::new()),
            phase_manager: EnderDragonPhaseManager::new(),
            flap_time: SyncMutex::new(0.0),
            o_flap_time: SyncMutex::new(0.0),
            y_rot_a: SyncMutex::new(0.0),
            in_wall: SyncMutex::new(false),
            fight_origin: SyncMutex::new(BlockPos::ZERO),
            flight_graph: SyncMutex::new(None),
            nearest_crystal: SyncMutex::new(None),
        }
    }

    fn create_sub_entities(
        entity_type: EntityTypeRef,
        parent: &EntityBase,
        world: &Weak<World>,
    ) -> Vec<Arc<dyn PartEntity>> {
        const PARTS: [(&str, f32, f32); SUB_ENTITY_COUNT] = [
            ("head", 1.0, 1.0),
            ("neck", 3.0, 3.0),
            ("body", 5.0, 3.0),
            ("tail", 2.0, 2.0),
            ("tail", 2.0, 2.0),
            ("tail", 2.0, 2.0),
            ("wing", 4.0, 2.0),
            ("wing", 4.0, 2.0),
        ];

        PARTS
            .iter()
            .enumerate()
            .map(|(index, &(name, width, height))| {
                let part: Arc<dyn PartEntity> = Arc::new(EnderDragonPart::new(
                    entity_type,
                    part_entity_id(parent.id(), index as u32),
                    name,
                    width,
                    height,
                    parent.position(),
                    Weak::clone(world),
                ));
                part
            })
            .collect()
    }

    /// Returns the dragon's hitboxes.
    #[must_use]
    pub fn sub_entities(&self) -> &[Arc<dyn PartEntity>] {
        &self.sub_entities
    }

    /// Returns the head, the only full-damage hitbox.
    #[must_use]
    pub fn head(&self) -> &Arc<dyn PartEntity> {
        &self.sub_entities[HEAD]
    }

    /// Returns vanilla `dragonDeathTime`.
    #[must_use]
    pub fn dragon_death_time(&self) -> i32 {
        *self.dragon_death_time.lock()
    }

    /// Returns the dragon's phase machine.
    #[must_use]
    pub const fn phase_manager(&self) -> &EnderDragonPhaseManager {
        &self.phase_manager
    }

    pub(super) fn set_synced_phase(&self, phase: EnderDragonPhase) {
        self.entity_data
            .lock()
            .ender_dragon_mut()
            .phase
            .set(phase.id());
    }

    /// Returns the phase id currently published to clients.
    #[must_use]
    pub fn synced_phase(&self) -> i32 {
        *self.entity_data.lock().ender_dragon().phase.get()
    }

    /// End crystals still alive; `None` means no fight, which one vanilla call site
    /// treats differently from zero crystals.
    // TODO: Read this from `EnderDragonFight` once that exists.
    #[expect(
        clippy::unused_self,
        reason = "reads the dragon's fight once EnderDragonFight lands"
    )]
    #[must_use]
    pub const fn alive_crystals(&self) -> Option<i32> {
        None
    }

    /// Returns the arena center this dragon belongs to.
    #[must_use]
    pub fn fight_origin(&self) -> BlockPos {
        *self.fight_origin.lock()
    }

    /// Sets the arena center.
    pub fn set_fight_origin(&self, origin: BlockPos) {
        *self.fight_origin.lock() = origin;
    }

    /// Returns the flight-graph node nearest the dragon, building the graph on first
    /// use; `None` while the arena's chunks are still cold.
    pub fn find_closest_node(&self, world: &World) -> Option<usize> {
        let position = self.position();
        let crystals = self.alive_crystals();
        self.with_flight_graph(world, |graph| graph.closest_node(position, crystals))
    }

    /// Paths between two flight-graph nodes.
    pub fn find_path(
        &self,
        world: &World,
        start: usize,
        end: usize,
        final_node: Option<Node>,
    ) -> Option<Path> {
        let crystals = self.alive_crystals();
        self.with_flight_graph(world, |graph| {
            graph.find_path(start, end, final_node, crystals)
        })?
    }

    /// Runs `action` under the graph lock (so it must touch only the graph), building
    /// the graph on first use.
    ///
    /// Lock order is phase state -> flight graph -> chunk map. In practice the build's
    /// world reads never run under a phase lock: `pick_new_path` warms the graph first.
    fn with_flight_graph<R>(
        &self,
        world: &World,
        action: impl FnOnce(&DragonFlightGraph) -> R,
    ) -> Option<R> {
        let mut graph = self.flight_graph.lock();
        // Not `get_or_insert_with`: a failed build off cold chunks stays uncached so
        // the next tick retries.
        if graph.is_none() {
            *graph = DragonFlightGraph::try_build(world);
        }
        graph.as_ref().map(action)
    }

    /// Vanilla `aiStep`: steers from the phase rather than movement input, and shoves
    /// with the wings instead of the usual entity pushing.
    fn dragon_ai_step(&self, world: &Arc<World>, entity: &SharedEntity) -> Option<MoveResult> {
        self.process_flapping_movement();

        // Latched above the dying split, as in vanilla: `process_flapping_movement`
        // still reads the beat while dying, so it must not freeze a stroke apart.
        *self.o_flap_time.lock() = *self.flap_time.lock();

        // A dying dragon is moved by `tick_death` instead.
        if self.is_dead_or_dying() {
            return None;
        }

        self.check_crystals(world);
        self.tick_flap_time();
        self.set_yaw(wrap_degrees(self.yaw()));

        if self.is_no_ai() {
            *self.flap_time.lock() = NO_AI_FLAP_TIME;
            return None;
        }

        {
            let mut history = self.flight_history.lock();
            history.record(self.position().y, self.yaw());
        }

        let phase = self.tick_phase(world);
        let result = self.steer_towards_phase_target(entity, phase);

        self.apply_effects_from_blocks();
        self.set_y_body_rot(self.yaw());
        self.tick_parts(world, entity);
        result
    }

    fn check_crystals(&self, world: &World) {
        let tracked = *self.nearest_crystal.lock();
        if let Some(id) = tracked {
            if world.get_accessible_entity_by_id(id).is_none() {
                *self.nearest_crystal.lock() = None;
            } else if self.tick_count() % CRYSTAL_HEAL_INTERVAL == 0
                && self.get_health() < self.get_max_health()
            {
                self.set_health(self.get_health() + CRYSTAL_HEAL_AMOUNT);
            }
        }

        if rand::random_range(0..CRYSTAL_RESCAN_CHANCE) != 0 {
            return;
        }

        // No per-type entity index, so `getEntitiesOfClass` is an area query + downcast.
        let search_area = self.bounding_box().inflate(CRYSTAL_SEARCH_RADIUS);
        let position = self.position();

        *self.nearest_crystal.lock() = world
            .get_entities_in_aabb(&search_area)
            .into_iter()
            .filter(|entity| entity.as_ref().downcast_ref::<EndCrystalEntity>().is_some())
            .min_by(|a, b| {
                a.distance_to_sqr(position)
                    .total_cmp(&b.distance_to_sqr(position))
            })
            .map(|crystal| crystal.id());
    }

    /// Places the hitboxes and runs the contact sweeps. Vanilla's order is load-bearing:
    /// the sweeps run after the body moves but before the head does, so the bite reads
    /// last tick's head box while `knock_back` centers on this tick's body box.
    #[expect(
        clippy::similar_names,
        reason = "the sample bindings are named for the vanilla latency indices they read"
    )]
    fn tick_parts(&self, world: &Arc<World>, entity: &SharedEntity) {
        let old_positions: Vec<DVec3> = self
            .sub_entities
            .iter()
            .map(|part| part.position())
            .collect();

        // Vanilla re-reads the ring per use; nothing here writes it, so one read suffices.
        let (sample_0, sample_5, sample_10, tail_samples) = {
            let history = self.flight_history.lock();
            (
                history.get(0),
                history.get(5),
                history.get(10),
                array::from_fn::<_, TAIL_COUNT, _>(|index| history.get(12 + index as i32 * 2)),
            )
        };

        // Vanilla re-reads this per `tickPart`; one read is sound because the sweeps
        // below only move *other* entities.
        let position = self.position();

        // Kept in `f32` until vanilla widens: the trig table indexes off the narrowed
        // value, and `f64` arithmetic would land on a different entry.
        let tilt = ((sample_5.y - sample_10.y) as f32 * 10.0).to_radians();
        let cc_tilt = trig::cos(f64::from(tilt));
        let ss_tilt = trig::sin(f64::from(tilt));
        let yaw_radians = self.yaw().to_radians();
        let ss1 = trig::sin(f64::from(yaw_radians));
        let cc1 = trig::cos(f64::from(yaw_radians));

        self.tick_part(
            BODY,
            position,
            DVec3::new(f64::from(ss1 * 0.5), 0.0, f64::from(-cc1 * 0.5)),
        );
        self.tick_part(
            WING1,
            position,
            DVec3::new(f64::from(cc1 * 4.5), 2.0, f64::from(ss1 * 4.5)),
        );
        self.tick_part(
            WING2,
            position,
            DVec3::new(f64::from(cc1 * -4.5), 2.0, f64::from(ss1 * -4.5)),
        );

        if self.hurt_time() == 0 {
            self.sweep_wings(world, entity);
            self.sweep_bite(world, entity);
        }

        let head_yaw = yaw_radians - *self.y_rot_a.lock() * 0.01;
        let ss2 = trig::sin(f64::from(head_yaw));
        let cc2 = trig::cos(f64::from(head_yaw));
        let y_offset = self.head_y_offset(sample_5, sample_0);
        for (index, reach) in [(HEAD, 6.5_f32), (NECK, 5.5)] {
            self.tick_part(
                index,
                position,
                DVec3::new(
                    f64::from(ss2 * reach * cc_tilt),
                    f64::from(y_offset + ss_tilt * reach),
                    f64::from(-cc2 * reach * cc_tilt),
                ),
            );
        }

        self.tick_tail(
            position,
            sample_5,
            &tail_samples,
            ss1,
            cc1,
            cc_tilt,
            ss_tilt,
        );

        // Non-short-circuiting `|`: every box must be scanned for its block breaking.
        *self.in_wall.lock() = Self::check_walls(world, self.part_box(HEAD))
            | Self::check_walls(world, self.part_box(NECK))
            | Self::check_walls(world, self.part_box(BODY));
        // TODO: Report to `EnderDragonFight::update_dragon` once the fight exists.

        // Covers both vanilla `xo/yo/zo` and `xOld/yOld/zOld`.
        for (part, old_position) in self.sub_entities.iter().zip(old_positions) {
            part.set_old_position(old_position);
        }
    }

    /// Places the tail segments, which trail the body through flight history.
    #[expect(
        clippy::too_many_arguments,
        reason = "vanilla's inlined tail block, threaded rather than re-derived per segment"
    )]
    fn tick_tail(
        &self,
        position: DVec3,
        sample_5: DragonFlightSample,
        trailing_samples: &[DragonFlightSample; TAIL_COUNT],
        ss1: f32,
        cc1: f32,
        cc_tilt: f32,
        ss_tilt: f32,
    ) {
        for (index, trailing) in trailing_samples.iter().enumerate() {
            // The yaw difference is taken in `f32` before widening, as in vanilla.
            let rotation = self.yaw().to_radians()
                + rot_wrap(f64::from(trailing.y_rot - sample_5.y_rot)).to_radians();
            let ss = trig::sin(f64::from(rotation));
            let cc = trig::cos(f64::from(rotation));
            let distance = (index + 1) as f32 * 2.0;

            self.tick_part(
                TAIL_START + index,
                position,
                DVec3::new(
                    f64::from(-(ss1 * 1.5 + ss * distance) * cc_tilt),
                    trailing.y - sample_5.y - f64::from((distance + 1.5) * ss_tilt) + 1.5,
                    f64::from((cc1 * 1.5 + cc * distance) * cc_tilt),
                ),
            );
        }
    }

    fn tick_part(&self, index: usize, position: DVec3, offset: DVec3) {
        self.sub_entities[index].set_part_position(position + offset);
    }

    fn part_box(&self, index: usize) -> WorldAabb {
        self.sub_entities[index].bounding_box()
    }

    fn head_y_offset(&self, sample_5: DragonFlightSample, sample_0: DragonFlightSample) -> f32 {
        if self.is_sitting() {
            return -1.0;
        }
        (sample_5.y - sample_0.y) as f32
    }

    /// Excluding the dragon (not just its id) also excludes its parts.
    fn entities_touching(&self, world: &World, box_: WorldAabb) -> Vec<SharedEntity> {
        world.get_entities_in_aabb_excluding(&box_, self, entity_selector::no_creative_or_spectator)
    }

    fn sweep_wings(&self, world: &Arc<World>, entity: &SharedEntity) {
        for wing in [WING1, WING2] {
            let sweep = self
                .part_box(wing)
                .inflate_xyz(4.0, 2.0, 4.0)
                .translate(DVec3::new(0.0, -2.0, 0.0));
            self.knock_back(world, entity, &self.entities_touching(world, sweep));
        }
    }

    fn sweep_bite(&self, world: &Arc<World>, entity: &SharedEntity) {
        for part in [HEAD, NECK] {
            let sweep = self.part_box(part).inflate(1.0);
            self.hurt_entities(world, entity, &self.entities_touching(world, sweep));
        }
    }

    fn knock_back(&self, world: &Arc<World>, attacker: &SharedEntity, entities: &[SharedEntity]) {
        let body = self.part_box(BODY);
        let center_x = f64::midpoint(body.min_x(), body.max_x());
        let center_z = f64::midpoint(body.min_z(), body.max_z());
        let sitting = self.is_sitting();

        for entity in entities {
            let Some(living) = entity.as_living_entity() else {
                continue;
            };

            let position = entity.position();
            let xd = position.x - center_x;
            let zd = position.z - center_z;
            // Divided by the *squared* distance, not normalized, as in vanilla.
            let distance_sqr = (xd * xd + zd * zd).max(MINIMUM_KNOCKBACK_DISTANCE_SQR);
            entity.push_impulse(DVec3::new(
                xd / distance_sqr * KNOCKBACK_STRENGTH,
                KNOCKBACK_LIFT,
                zd / distance_sqr * KNOCKBACK_STRENGTH,
            ));

            if sitting
                || living.last_hurt_by_mob_timestamp()
                    >= entity.tick_count() - REPEAT_ATTACK_GRACE_TICKS
            {
                continue;
            }
            self.attack(world, attacker, entity, WING_DAMAGE);
        }
    }

    /// Vanilla `hurt(ServerLevel, List<Entity>)`, renamed off the `Entity::hurt` overload.
    fn hurt_entities(
        &self,
        world: &Arc<World>,
        attacker: &SharedEntity,
        entities: &[SharedEntity],
    ) {
        for entity in entities {
            if entity.as_living_entity().is_some() {
                self.attack(world, attacker, entity, BITE_DAMAGE);
            }
        }
    }

    fn attack(
        &self,
        world: &Arc<World>,
        attacker: &SharedEntity,
        target: &SharedEntity,
        damage: f32,
    ) {
        let source = self.mob_attack_damage_source(attacker);
        target.hurt(world, &source, damage);

        let context = EnchantmentPostAttackContext::new(target, &source);
        enchantment_helper::do_post_attack_effects_with_item_source(
            world,
            target.as_ref(),
            &ItemStack::empty(),
            &context,
        );
    }

    fn award_death_experience(&self, world: &Arc<World>, share: f32) {
        if !world.get_game_rule(&MOB_DROPS) {
            return;
        }
        ExperienceOrbEntity::award(world, self.position(), share_of(DEATH_EXPERIENCE, share));
    }

    /// Not `Mob::mob_attack_damage_source`, which derives the type from a held weapon;
    /// vanilla's dragon always uses plain `mobAttack(this)`.
    fn mob_attack_damage_source(&self, attacker: &SharedEntity) -> DamageSource {
        DamageSource::environment(&vanilla_damage_types::MOB_ATTACK)
            .with_causing_entity(Arc::clone(attacker))
            .with_direct_entity(Arc::clone(attacker))
            .with_source_position(self.position())
    }

    /// Breaks blocks inside one hitbox; returns whether it hit one it could not break.
    fn check_walls(world: &Arc<World>, box_: WorldAabb) -> bool {
        let min = BlockPos::containing(box_.min_x(), box_.min_y(), box_.min_z());
        let max = BlockPos::containing(box_.max_x(), box_.max_y(), box_.max_z());

        // Hoisted out of vanilla's per-block read: it takes a level-data lock.
        let griefing = world.get_game_rule(&MOB_GRIEFING);
        let mut hit_wall = false;
        let mut destroyed = false;

        for x in min.x()..=max.x() {
            for y in min.y()..=max.y() {
                for z in min.z()..=max.z() {
                    let pos = BlockPos::new(x, y, z);
                    let state = world.get_block_state(pos);
                    let block = state.get_block();
                    if state.is_air() || block.has_tag(&BlockTag::DRAGON_TRANSPARENT) {
                        continue;
                    }

                    if griefing && !block.has_tag(&BlockTag::DRAGON_IMMUNE) {
                        destroyed |= world.remove_block(pos, false);
                    } else {
                        hit_wall = true;
                        // Without griefing nothing else can change; vanilla scans on
                        // but draws no RNG and changes no state.
                        if !griefing {
                            return true;
                        }
                    }
                }
            }
        }

        if destroyed {
            let between = |low: i32, high: i32| low + rand::random_range(0..=high - low);
            let particle_pos = BlockPos::new(
                between(min.x(), max.x()),
                between(min.y(), max.y()),
                between(min.z(), max.z()),
            );
            world.level_event(
                level_events::PARTICLES_DRAGON_BLOCK_BREAK,
                particle_pos,
                0,
                None,
            );
        }

        hit_wall
    }

    fn is_sitting(&self) -> bool {
        self.phase_manager.current().is_sitting()
    }

    fn yaw(&self) -> f32 {
        self.rotation().0
    }

    fn set_yaw(&self, yaw: f32) {
        self.set_rotation((yaw, self.rotation().1));
    }

    /// Advances the wing beat; `o_flap_time` is latched in [`Self::dragon_ai_step`].
    fn tick_flap_time(&self) {
        let velocity = self.velocity();
        let horizontal = velocity.x.hypot(velocity.z) as f32;
        let flap_speed = (0.2 / (horizontal * 10.0 + 1.0)) * 2.0_f32.powf(velocity.y as f32);

        let mut flap_time = self.flap_time.lock();
        *flap_time += if self.is_sitting() {
            SITTING_FLAP_RATE
        } else if *self.in_wall.lock() {
            flap_speed * 0.5
        } else {
            flap_speed
        };
    }

    /// Ticks the active phase, and the new one once more if it switched, so a successor
    /// sets its fly target in the same tick.
    fn tick_phase(&self, world: &Arc<World>) -> EnderDragonPhase {
        let before = self.phase_manager.current_phase();
        self.phase_manager
            .instance(before)
            .do_server_tick(self, world);

        let after = self.phase_manager.current_phase();
        if after != before {
            self.phase_manager
                .instance(after)
                .do_server_tick(self, world);
        }
        self.phase_manager.current_phase()
    }

    /// Flies toward the active phase's target. Orderings that look tidyable but are not:
    /// the distance uses the pre-clamp `dy`, the heading reads velocity after the climb,
    /// and thrust goes along `-Z` rather than through movement input.
    fn steer_towards_phase_target(
        &self,
        entity: &SharedEntity,
        phase: EnderDragonPhase,
    ) -> Option<MoveResult> {
        let instance = self.phase_manager.instance(phase);
        let target = instance.fly_target_location()?;

        let position = self.position();
        let dx = target.x - position.x;
        let mut dy = target.y - position.y;
        let dz = target.z - position.z;
        let dist_to_target = dx * dx + dy * dy + dz * dz;

        let max = f64::from(instance.fly_speed());
        let horizontal_dist = (dx * dx + dz * dz).sqrt();
        if horizontal_dist > 0.0 {
            dy = (dy / horizontal_dist).clamp(-max, max);
        }

        self.set_velocity(self.velocity() + DVec3::new(0.0, dy * 0.01, 0.0));
        self.set_yaw(wrap_degrees(self.yaw()));

        let aim = (target - position).normalize_or_zero();
        let yaw_radians = f64::from(self.yaw().to_radians());
        let heading = DVec3::new(
            f64::from(trig::sin(yaw_radians)),
            self.velocity().y,
            f64::from(-trig::cos(yaw_radians)),
        )
        .normalize_or_zero();
        let alignment = (((heading.dot(aim) as f32) + 0.5) / 1.5).max(0.0);

        if dx.abs() > STEERING_EPSILON || dz.abs() > STEERING_EPSILON {
            // TODO: Use a port of vanilla's table-based `Mth.atan2`; `f64::atan2`
            // turns fractionally differently.
            let desired = wrap_degrees(180.0 - (dx.atan2(dz) as f32).to_degrees() - self.yaw())
                .clamp(-MAX_TURN_DEGREES, MAX_TURN_DEGREES);
            let mut y_rot_a = self.y_rot_a.lock();
            *y_rot_a *= 0.8;
            *y_rot_a += desired * instance.turn_speed(self);
            let turn = *y_rot_a;
            drop(y_rot_a);
            self.set_yaw(self.yaw() + turn * 0.1);
        }

        let span = (2.0 / (dist_to_target + 1.0)) as f32;
        self.move_relative(
            FORWARD_THRUST * (alignment * span + (1.0 - span)),
            DVec3::new(0.0, 0.0, -1.0),
        );

        let velocity = self.velocity();
        let movement = if *self.in_wall.lock() {
            velocity * IN_WALL_MOVE_SCALE
        } else {
            velocity
        };
        let result = Arc::clone(entity).move_entity(MoverType::SelfMovement, movement);

        let moved = self.velocity();
        let slide = 0.8 + 0.15 * (moved.normalize_or_zero().dot(heading) + 1.0) / 2.0;
        self.set_velocity(moved * DVec3::new(slide, VERTICAL_DRAG, slide));
        result
    }

    /// Applies damage through a hitbox (vanilla's per-part `hurt` overload). Non-head
    /// parts take a quarter plus up to one point. Ignored sources still report handled,
    /// so e.g. a dispenser arrow lands and does nothing.
    pub fn hurt_part(
        &self,
        world: &World,
        part: &EnderDragonPart,
        source: &DamageSource,
        damage: f32,
    ) -> bool {
        let phase = self.phase_manager.current();
        if phase.phase() == EnderDragonPhase::Dying {
            return false;
        }

        let mut damage = phase.on_hurt(source, damage);
        if part.id() != self.head().id() {
            damage = damage / 4.0 + damage.min(1.0);
        }

        if damage < MINIMUM_EFFECTIVE_DAMAGE {
            return false;
        }

        if !Self::is_damageable_by(source) {
            return true;
        }

        let health_before = self.get_health();
        self.really_hurt(world, source, damage);
        if phase.is_sitting() {
            self.accumulate_sitting_damage(health_before - self.get_health());
        }
        true
    }

    fn is_damageable_by(source: &DamageSource) -> bool {
        source.is(&vanilla_damage_type_tags::DamageTypeTag::ALWAYS_HURTS_ENDER_DRAGONS)
            || source
                .causing_entity()
                .is_some_and(|entity| entity.as_player().is_some())
    }

    fn accumulate_sitting_damage(&self, taken: f32) {
        let mut received = self.sitting_damage_received.lock();
        *received += taken;
        if *received <= SITTING_ALLOWED_DAMAGE_FRACTION * self.get_max_health() {
            return;
        }

        *received = 0.0;
        drop(received);
        self.phase_manager
            .set_phase(self, EnderDragonPhase::Takeoff);
    }

    /// Vanilla `reallyHurt`, i.e. `super.hurtServer`.
    fn really_hurt(&self, world: &World, source: &DamageSource, damage: f32) {
        self.default_hurt_server(world, source, damage);
    }

    fn update_dirty_mob_effect_entity_data(&self) {
        if let Some(display) = sync_dirty_mob_effects(&self.living_base, &self.entity_data) {
            self.entity_data
                .set_base_glowing_flag(self.has_glowing_tag() || display.glowing);
        }
    }
}

impl Entity for EnderDragonEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn base_tick(&self) {
        Mob::base_tick_mob(self);
    }

    fn parts(&self) -> &[Arc<dyn PartEntity>] {
        &self.sub_entities
    }

    /// Removes outright, as vanilla does; the shared damage-based `kill` would start
    /// the 200-tick death flight instead.
    fn kill(&self, _world: &World) {
        // TODO: Report to `EnderDragonFight::update_dragon` and `set_dragon_killed`
        // once the fight exists.
        self.set_removed(RemovalReason::Killed);
        self.game_event(&vanilla_game_events::ENTITY_DIE);
    }

    /// Ignores pose and scale (vanilla `sanitizeScale` pins it to 1.0).
    fn dimensions_for_pose(&self, _pose: EntityPose) -> EntityDimensions {
        self.entity_type.dimensions
    }

    fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
        Some(&self.entity_data)
    }

    fn update_data_before_sync(&self) {
        self.update_dirty_mob_effect_entity_data();
    }

    fn is_flapping(&self) -> bool {
        // Table `trig::cos`, not `f32::cos`: the beat crosses -0.3 shallowly, so the
        // two disagree on which tick the sweep lands.
        let flap = trig::cos(f64::from(*self.flap_time.lock() * TAU));
        let previous = trig::cos(f64::from(*self.o_flap_time.lock() * TAU));
        previous <= -0.3 && flap >= -0.3
    }

    /// Only the parts are pickable, not the body.
    fn is_pickable(&self) -> bool {
        false
    }

    fn check_despawn(&self) {}

    fn sound_source(&self) -> SoundSource {
        SoundSource::Hostile
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.save_mob(nbt);
        nbt.insert(DRAGON_PHASE_KEY, self.phase_manager.current_phase().id());
        nbt.insert(DRAGON_DEATH_TIME_KEY, self.dragon_death_time());
        nbt.insert(
            SITTING_DAMAGE_RECEIVED_KEY,
            *self.sitting_damage_received.lock(),
        );
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.load_mob(nbt);
        if let Some(phase) = nbt.int(DRAGON_PHASE_KEY) {
            self.phase_manager
                .set_phase(self, EnderDragonPhase::by_id(phase));
        }
        if let Some(death_time) = nbt.int(DRAGON_DEATH_TIME_KEY) {
            *self.dragon_death_time.lock() = death_time;
        }
        if let Some(received) = nbt.float(SITTING_DAMAGE_RECEIVED_KEY) {
            *self.sitting_damage_received.lock() = received;
        }
    }
}

impl LivingEntity for EnderDragonEntity {
    fn living_base(&self) -> &LivingEntityBase {
        &self.living_base
    }

    fn get_health(&self) -> f32 {
        *self.entity_data.lock().living_entity().health.get()
    }

    fn set_health(&self, health: f32) {
        let max_health = self.get_max_health();
        let clamped = health.clamp(0.0, max_health);
        self.entity_data
            .lock()
            .living_entity_mut()
            .health
            .set(clamped);
    }

    /// Routes through the body hitbox, so a direct hit scales like a non-head part.
    fn hurt_server(&self, world: &World, source: &DamageSource, amount: f32) -> bool {
        let Some(body) = self.sub_entities.get(BODY) else {
            return false;
        };
        let Some(body) = body.as_ref().downcast_ref::<EnderDragonPart>() else {
            return false;
        };
        self.hurt_part(world, body, source, amount)
    }

    fn knockback(&self, power: f64, xd: f64, zd: f64) {
        if self.is_sitting() {
            return;
        }
        self.default_knockback(power, xd, zd);
    }

    /// An airborne dragon returns to 1 health and enters the Dying phase; the restored
    /// point keeps `is_dead_or_dying` false so the phase machine keeps steering. The
    /// default (vanilla `dead = true`) is skipped: Steel's `die` already set it.
    fn handle_killing_blow(&self) {
        if self.is_sitting() {
            return;
        }

        self.set_health(1.0);
        self.phase_manager.set_phase(self, EnderDragonPhase::Dying);
    }

    /// Replaces the shared death tick. Vanilla's tick 180-200 explosion particles use
    /// `Level.addParticle`, a server no-op, so they are absent.
    fn tick_death(&self, entity: &SharedEntity) {
        // TODO: Report to `EnderDragonFight::update_dragon` once the fight exists.
        let death_time = {
            let mut death_time = self.dragon_death_time.lock();
            *death_time += 1;
            *death_time
        };

        let Some(world) = self.level() else {
            return;
        };

        if death_time > DEATH_XP_TRICKLE_START && death_time % DEATH_XP_TRICKLE_INTERVAL == 0 {
            self.award_death_experience(&world, DEATH_XP_TRICKLE_SHARE);
        }

        if death_time == 1 && !self.is_silent() {
            world.global_level_event(level_events::SOUND_DRAGON_DEATH, self.block_position(), 0);
        }

        let drift = DVec3::new(0.0, f64::from(DEATH_DRIFT_PER_TICK), 0.0);
        let _ = Arc::clone(entity).move_entity(MoverType::SelfMovement, drift);
        for part in &self.sub_entities {
            // Unlike `tick_parts`, the old position is written before the move.
            part.set_old_position_to_current();
            part.set_part_position(part.position() + drift);
        }

        if death_time < DRAGON_DEATH_DURATION {
            return;
        }

        self.award_death_experience(&world, DEATH_XP_FINAL_SHARE);

        // TODO: Report to `EnderDragonFight::set_dragon_killed` once the fight exists.
        self.set_removed(RemovalReason::Killed);
        self.game_event(&vanilla_game_events::ENTITY_DIE);
    }

    fn ai_step(&self, entity: &SharedEntity) -> Option<MoveResult> {
        let world = self.level()?;
        self.dragon_ai_step(&world, entity)
    }

    /// Clears the line-of-sight cache, since the replaced `ai_step` never reaches
    /// `mob_server_ai_step` to do it.
    fn server_ai_step(&self, _entity: &SharedEntity) {
        self.tick_sensing();
    }

    fn sound_volume(&self) -> f32 {
        5.0
    }

    fn hurt_sound(&self, _source: &DamageSource) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_ENDER_DRAGON_HURT)
    }

    /// Vanilla `sanitizeScale`.
    fn get_scale(&self) -> f32 {
        1.0
    }

    fn add_mob_effect(&self, _effect: MobEffectInstance) -> bool {
        false
    }
}

impl Mob for EnderDragonEntity {
    fn mob_base(&self) -> &MobBase {
        &self.mob_base
    }

    fn mob_flags(&self) -> i8 {
        *self.entity_data.lock().mob.mob_flags.get()
    }

    fn set_mob_flags(&self, flags: i8) {
        self.entity_data.lock().mob.mob_flags.set(flags);
    }

    /// The dragon steers itself; the `PathNavigation` from `MobBase::new` stays unused.
    fn tick_path_navigation(&self) {}

    fn ambient_sound(&self) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_ENDER_DRAGON_AMBIENT)
    }

    fn can_attack(&self, target: &dyn LivingEntity) -> bool {
        target.can_be_seen_as_enemy()
    }
}
