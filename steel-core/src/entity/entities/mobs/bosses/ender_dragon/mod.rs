//! The Ender Dragon.
//!
//! Mirrors vanilla `net.minecraft.world.entity.boss.enderdragon.EnderDragon`.
//!
//! The dragon is a `Mob` that uses none of the mob AI machinery: no goal selectors,
//! no path navigation. It steers itself from a phase state machine over a fixed
//! 24-node graph, so its `ai_step` replaces the shared one outright rather than
//! delegating to it, exactly as vanilla's does.
//!
//! Client-only vanilla members are deliberately absent: `onFlap`, `doClientTick`,
//! `onSyncedDataUpdated`, `recreateFromPacket`, and the `growlTime` counter, whose
//! only use sits inside the `isClientSide` branch of `aiStep`. A vanilla client runs
//! its own copy of the phase machine off the synced phase id, so keeping that id
//! correct is what makes the visuals right.

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
    Entity, EntityBase, EntityBaseLoad, EntityEventSource as _, EntityPose, EntitySyncedData,
    LivingEntity, LivingEntityBase, Mob, MobBase, MobEffectInstance, PartEntity, RemovalReason,
    SharedEntity, entity_selector, part_entity_id, sync_dirty_mob_effects,
};
use crate::physics::{MoveResult, MoverType};
use crate::world::World;

/// The number of sub-entity hitboxes.
const SUB_ENTITY_COUNT: usize = 8;
/// Indices into `sub_entities`, in the vanilla construction order
/// head, neck, body, tail x3, wing x2.
const HEAD: usize = 0;
const NECK: usize = 1;
const BODY: usize = 2;
/// The first of the three tail segments, which are positioned in a loop.
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
/// Vertical velocity retained each tick.
const VERTICAL_DRAG: f64 = 0.91;

/// Damage a wing sweep deals on top of its knockback.
const WING_DAMAGE: f32 = 5.0;
/// Damage the head and neck deal on contact.
const BITE_DAMAGE: f32 = 10.0;
/// Lower bound on the squared horizontal distance used to scale knockback, which
/// keeps an entity standing exactly under the body from being flung.
const MINIMUM_KNOCKBACK_DISTANCE_SQR: f64 = 0.1;
/// Horizontal knockback strength, applied over the *squared* distance.
const KNOCKBACK_STRENGTH: f64 = 4.0;
/// Vertical lift added to every wing shove.
const KNOCKBACK_LIFT: f64 = 0.2;
/// A target hit within this many ticks is only shoved, not bitten again.
const REPEAT_ATTACK_GRACE_TICKS: i32 = 2;

/// Ticks the death animation runs before the dragon is removed.
const DRAGON_DEATH_DURATION: i32 = 200;
/// Experience trickles out once the death animation passes this tick.
const DEATH_XP_TRICKLE_START: i32 = 150;
/// ...and lands every this many ticks after that.
const DEATH_XP_TRICKLE_INTERVAL: i32 = 5;
/// Share of the prize each trickle awards.
const DEATH_XP_TRICKLE_SHARE: f32 = 0.08;
/// Share of the prize paid out when the animation ends.
const DEATH_XP_FINAL_SHARE: f32 = 0.2;
/// Experience a dragon is worth.
///
/// Vanilla awards 12000 instead on a fight's first kill.
// TODO: Read the fight's `hasPreviouslyKilledDragon` once `EnderDragonFight` exists.
const DEATH_EXPERIENCE: i32 = 500;
/// How far the dying dragon drifts upward each tick.
///
/// Declared `f32` because vanilla widens a float literal here, giving
/// `0.100000001...` rather than the `f64` `0.1`.
const DEATH_DRIFT_PER_TICK: f32 = 0.1;

/// How far the dragon looks for a crystal to heal from.
const CRYSTAL_SEARCH_RADIUS: f64 = 32.0;
/// The crystal heals the dragon on ticks divisible by this.
const CRYSTAL_HEAL_INTERVAL: i32 = 10;
/// Health restored per healing tick.
const CRYSTAL_HEAL_AMOUNT: f32 = 1.0;
/// One tick in this many rescans for a nearer crystal.
const CRYSTAL_RESCAN_CHANCE: i32 = 10;

const DRAGON_PHASE_KEY: &str = "DragonPhase";
const DRAGON_DEATH_TIME_KEY: &str = "DragonDeathTime";
const SITTING_DAMAGE_RECEIVED_KEY: &str = "sitting_damage_received";

/// Where the exit portal sits for an arena centered on `origin`.
///
/// Mirrors vanilla `EndPodiumFeature.getLocation`. That method offsets a constant
/// `END_PODIUM_LOCATION`, which is `BlockPos.ZERO`, so this is an identity today; it
/// exists so the call sites read like vanilla and so the constant has one home when
/// the podium feature itself lands.
// TODO: Move this onto `EndPodiumFeature` once runtime feature placement exists.
#[must_use]
pub const fn end_podium_location(origin: BlockPos) -> BlockPos {
    origin
}

/// Vanilla `Mth.floor(total * share)`, where the multiplication happens in `f32`.
fn share_of(total: i32, share: f32) -> i32 {
    (total as f32 * share).floor() as i32
}

/// Wraps an angle the way vanilla `EnderDragon.rotWrap` does.
///
/// Vanilla's is `(float)Mth.wrapDegrees(double)`, so it wraps in `f64` and narrows
/// afterwards. Steel's [`wrap_degrees`] is `f32`-only, which moves the narrowing one
/// step earlier. Unlike the trig tables that decide the flight graph's `floor`, this
/// cannot change an observable: the inputs are yaw differences, and both widths wrap
/// them to the same value.
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
    /// Vanilla `subEntities`, in vanilla order: head, neck, body, tail x3, wing x2.
    sub_entities: Vec<Arc<dyn PartEntity>>,
    /// Vanilla `dragonDeathTime`.
    dragon_death_time: SyncMutex<i32>,
    /// Vanilla `sittingDamageReceived`.
    sitting_damage_received: SyncMutex<f32>,
    /// Vanilla `flightHistory`.
    flight_history: SyncMutex<DragonFlightHistory>,
    /// Vanilla `phaseManager`.
    phase_manager: EnderDragonPhaseManager,
    /// Vanilla `flapTime` and `oFlapTime`.
    ///
    /// Cosmetic in itself, but `is_flapping` reads it and that drives the FLAP game
    /// event from inside the shared move path.
    flap_time: SyncMutex<f32>,
    o_flap_time: SyncMutex<f32>,
    /// Vanilla `yRotA`, the accumulated turn rate.
    y_rot_a: SyncMutex<f32>,
    /// Vanilla `inWall`, set by the wall scan once that lands.
    in_wall: SyncMutex<bool>,
    /// Vanilla `fightOrigin`, the arena center this dragon belongs to.
    ///
    /// Set by the fight rather than persisted: vanilla saves only the phase, the
    /// death timer and the sitting damage.
    fight_origin: SyncMutex<BlockPos>,
    /// Vanilla `nodes` and `nodeAdjacency`, built on first use because the layout
    /// samples the terrain.
    flight_graph: SyncMutex<Option<DragonFlightGraph>>,
    /// Vanilla `nearestCrystal`, held as an entity id rather than a reference.
    ///
    /// Storing the crystal itself would mean an `Arc` cycle through the world and
    /// would keep a destroyed crystal alive; vanilla instead nulls the field once the
    /// crystal reports removed, which an id lookup reproduces for free.
    nearest_crystal: SyncMutex<Option<i32>>,
}

// SAFETY: The owner-scoped type key uniquely identifies EnderDragonEntity.
unsafe impl DowncastType for EnderDragonEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/ender_dragon");
}

impl EnderDragonEntity {
    /// Creates a new dragon.
    ///
    /// `id` must come from an [`crate::entity::EntityIdBlock`] wide enough for the
    /// eight parts, which the entity registry guarantees for this type.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::new_with_base(
            EntityBase::new(id, position, entity_type.dimensions, world.clone()),
            entity_type,
            world,
        )
    }

    /// Loads a saved dragon.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        let world = load.world.clone();
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
        // Seeds health from the MAX_HEALTH attribute; without this the dragon would
        // spawn on the synced default of 1.0 rather than 200.
        living_base.initialize_synced_data(&mut entity_data);

        // Vanilla sets `noPhysics` in the constructor, so the dragon passes through
        // terrain and does its own wall handling in `check_walls`.
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

    /// Builds the eight hitboxes, in vanilla's order and at vanilla's sizes.
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
                    world.clone(),
                ));
                part
            })
            .collect()
    }

    /// Returns the dragon's hitboxes. Mirrors vanilla `getSubEntities`.
    #[must_use]
    pub fn sub_entities(&self) -> &[Arc<dyn PartEntity>] {
        &self.sub_entities
    }

    /// Returns the head, which vanilla treats as the only full-damage hitbox.
    #[must_use]
    pub fn head(&self) -> &Arc<dyn PartEntity> {
        &self.sub_entities[HEAD]
    }

    /// Returns vanilla `dragonDeathTime`.
    #[must_use]
    pub fn dragon_death_time(&self) -> i32 {
        *self.dragon_death_time.lock()
    }

    /// Returns the dragon's phase machine. Mirrors vanilla `getPhaseManager`.
    #[must_use]
    pub const fn phase_manager(&self) -> &EnderDragonPhaseManager {
        &self.phase_manager
    }

    /// Publishes the active phase to watching clients.
    ///
    /// A vanilla client runs its own copy of the phase machine off this value, so it
    /// is what makes the dragon animate correctly rather than any server-side work.
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

    /// How many end crystals are still feeding the dragon.
    ///
    /// `None` means there is no fight at all, which vanilla treats differently from a
    /// fight with zero crystals left in one of its three call sites.
    // TODO: Read this from `EnderDragonFight` once that exists.
    #[expect(
        clippy::unused_self,
        reason = "reads the dragon's fight once EnderDragonFight lands"
    )]
    #[must_use]
    pub const fn alive_crystals(&self) -> Option<i32> {
        None
    }

    /// the arena center this dragon belongs to. Vanilla `getFightOrigin`.
    #[must_use]
    pub fn fight_origin(&self) -> BlockPos {
        *self.fight_origin.lock()
    }

    /// Sets the arena center. Vanilla `setFightOrigin`.
    pub fn set_fight_origin(&self, origin: BlockPos) {
        *self.fight_origin.lock() = origin;
    }

    /// Returns the flight-graph node nearest the dragon.
    ///
    /// Mirrors vanilla `findClosestNode()`, whose no-argument overload also builds the
    /// graph on first use. The build samples the terrain, so it needs the world, and it
    /// yields `None` while the arena's chunks are still cold.
    pub fn find_closest_node(&self, world: &World) -> Option<usize> {
        let position = self.position();
        let crystals = self.alive_crystals();
        self.with_flight_graph(world, |graph| graph.closest_node(position, crystals))
    }

    /// Paths between two flight-graph nodes. Mirrors vanilla `findPath`.
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

    /// Runs `action` against the flight graph, building it if this is the first use.
    ///
    /// `action` runs under the graph lock, so it must stay confined to the graph. The
    /// two callers above satisfy that: a search reads node positions and nothing else.
    ///
    /// # Lock order
    ///
    /// A phase may hold its own state lock across a call here, so the order is
    /// phase state -> flight graph -> chunk map, and nothing may take them the other
    /// way round. The build's world reads sit at the deepest point of that chain, but
    /// they never run under a phase lock in practice: `pick_new_path` warms the graph
    /// through `find_closest_node` before it locks its own state.
    fn with_flight_graph<R>(
        &self,
        world: &World,
        action: impl FnOnce(&DragonFlightGraph) -> R,
    ) -> Option<R> {
        let mut graph = self.flight_graph.lock();
        // Deliberately not `get_or_insert_with`: a build off cold chunks must not be
        // cached, because nothing would ever rebuild it. Leaving the slot empty makes
        // the next tick try again.
        if graph.is_none() {
            *graph = DragonFlightGraph::try_build(world);
        }
        graph.as_ref().map(action)
    }

    /// Runs vanilla `EnderDragon.aiStep`.
    ///
    /// A full replacement for the shared living step, as vanilla's is: the dragon
    /// steers from its phase rather than from movement input, and it shoves entities
    /// with its wings instead of the usual entity pushing.
    fn dragon_ai_step(&self, world: &Arc<World>) -> Option<MoveResult> {
        self.process_flapping_movement();

        // Above the `isDeadOrDying` split, as vanilla does, so the beat keeps tracking
        // while the dragon dies. Below it the two would freeze a stroke apart, and
        // `process_flapping_movement` still reads them on every dying tick.
        *self.o_flap_time.lock() = *self.flap_time.lock();

        // Everything below is inside vanilla's `else`; a dying dragon is moved by
        // `tick_death` instead.
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
        let result = self.steer_towards_phase_target(phase);

        self.apply_effects_from_blocks();
        self.set_y_body_rot(self.yaw());
        self.tick_parts(world);
        result
    }

    /// Heals the dragon from a nearby end crystal. Mirrors vanilla `checkCrystals`.
    ///
    /// The crystal is tracked by id, so a destroyed one simply stops resolving, which
    /// is what vanilla's `isRemoved` check does with a reference.
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

        // Steel has no per-type entity index, so vanilla's `getEntitiesOfClass` becomes
        // an area query filtered by downcast.
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

    /// Places the eight hitboxes and runs the contact sweeps they drive.
    ///
    /// Vanilla's order is load-bearing and is reproduced exactly. The head and neck
    /// damage sweep sits *between* positioning the body and positioning the head, so
    /// it reads the head box from the previous tick; and `knock_back` centers on the
    /// body box, which by then holds this tick's position.
    #[expect(
        clippy::similar_names,
        reason = "the sample bindings are named for the vanilla latency indices they read"
    )]
    fn tick_parts(&self, world: &Arc<World>) {
        let old_positions: Vec<DVec3> = self
            .sub_entities
            .iter()
            .map(|part| part.position())
            .collect();

        // One pass over the history covers every sample this tick needs: the body's own
        // two, the head's lag reference, and one per tail segment. Vanilla re-reads the
        // ring per use, but nothing here writes to it, so the values are identical.
        let (sample_0, sample_5, sample_10, tail_samples) = {
            let history = self.flight_history.lock();
            (
                history.get(0),
                history.get(5),
                history.get(10),
                array::from_fn::<_, TAIL_COUNT, _>(|index| history.get(12 + index as i32 * 2)),
            )
        };

        // Vanilla reads this afresh inside every `tickPart`. Holding one value is only
        // sound because nothing between the first and last call moves the dragon: the
        // sweeps below push and hurt *other* entities and never reposition `self`.
        let position = self.position();

        // Every expression below is `f32` up to the point vanilla widens it, because
        // the trig lookup indexes off the narrowed value. Doing the arithmetic in `f64`
        // and narrowing afterwards would land on a different table entry.
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
            self.sweep_wings(world);
            self.sweep_bite(world);
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

        // Non-short-circuiting `|`: every box must be scanned for its block-breaking
        // side effect, not just until one reports a wall.
        *self.in_wall.lock() = Self::check_walls(world, self.part_box(HEAD))
            | Self::check_walls(world, self.part_box(NECK))
            | Self::check_walls(world, self.part_box(BODY));
        // TODO: Report to `EnderDragonFight::update_dragon` once the fight exists.

        // Vanilla writes the pre-tick position into both `xo/yo/zo` and
        // `xOld/yOld/zOld`, which is manual interpolation bookkeeping for entities that
        // are never ticked themselves. Steel has a single `old_position`, so one write
        // covers both.
        for (part, old_position) in self.sub_entities.iter().zip(old_positions) {
            part.set_old_position(old_position);
        }
    }

    /// Places the three tail segments, which trail the body through flight history.
    ///
    /// `trailing` holds one sample per segment, already read by [`Self::tick_parts`].
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
            // The yaw difference is taken in `f32` before it widens, as vanilla's is.
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

    /// Moves one hitbox to an offset from the dragon. Mirrors vanilla `tickPart`.
    ///
    /// `position` is the dragon's, read once per tick by [`Self::tick_parts`].
    fn tick_part(&self, index: usize, position: DVec3, offset: DVec3) {
        self.sub_entities[index].set_part_position(position + offset);
    }

    /// Returns a hitbox's current bounding box.
    fn part_box(&self, index: usize) -> WorldAabb {
        self.sub_entities[index].bounding_box()
    }

    /// Mirrors vanilla `getHeadYOffset`.
    ///
    /// A perched dragon lowers its head to a fixed offset; a flying one lets the head
    /// lag behind the body's recent climb.
    fn head_y_offset(&self, sample_5: DragonFlightSample, sample_0: DragonFlightSample) -> f32 {
        if self.is_sitting() {
            return -1.0;
        }
        (sample_5.y - sample_0.y) as f32
    }

    /// Mirrors vanilla's `level.getEntities(this, box, NO_CREATIVE_OR_SPECTATOR)`.
    ///
    /// Excluding the dragon also excludes its own parts, which is the reason
    /// [`World::get_entities_in_aabb_excluding`] takes an entity rather than an id.
    fn entities_touching(&self, world: &World, box_: WorldAabb) -> Vec<SharedEntity> {
        world.get_entities_in_aabb_excluding(&box_, self, entity_selector::no_creative_or_spectator)
    }

    /// Shoves and cuts everything under either wing. Mirrors vanilla `knockBack`.
    fn sweep_wings(&self, world: &Arc<World>) {
        for wing in [WING1, WING2] {
            let sweep = self
                .part_box(wing)
                .inflate_xyz(4.0, 2.0, 4.0)
                .translate(DVec3::new(0.0, -2.0, 0.0));
            self.knock_back(world, &self.entities_touching(world, sweep));
        }
    }

    /// Bites everything against the head and neck. Mirrors vanilla `hurt(level, list)`.
    fn sweep_bite(&self, world: &Arc<World>) {
        for part in [HEAD, NECK] {
            let sweep = self.part_box(part).inflate(1.0);
            self.hurt_entities(world, &self.entities_touching(world, sweep));
        }
    }

    /// Mirrors vanilla `knockBack`.
    ///
    /// The shove lands on every living entity, but the damage additionally needs the
    /// dragon to be airborne and the target not to have been hit moments ago.
    fn knock_back(&self, world: &Arc<World>, entities: &[SharedEntity]) {
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
            // Vanilla divides by the *squared* distance rather than normalizing, so the
            // shove weakens sharply with range instead of staying constant.
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
            self.attack(world, entity, WING_DAMAGE);
        }
    }

    /// Mirrors vanilla `EnderDragon.hurt(ServerLevel, List<Entity>)`.
    ///
    /// Renamed for the same reason as [`Self::hurt_part`]: Rust has no overloading and
    /// the vanilla name collides with `Entity::hurt`.
    fn hurt_entities(&self, world: &Arc<World>, entities: &[SharedEntity]) {
        for entity in entities {
            if entity.as_living_entity().is_some() {
                self.attack(world, entity, BITE_DAMAGE);
            }
        }
    }

    /// Applies one contact hit, including the post-attack enchantment effects.
    fn attack(&self, world: &Arc<World>, target: &SharedEntity, damage: f32) {
        let source = self.mob_attack_damage_source();
        target.hurt(world, &source, damage);

        let attacker = self.as_entity_event_source();
        let context = EnchantmentPostAttackContext::new(
            target.as_ref(),
            Some(attacker),
            Some(attacker),
            &source,
        );
        enchantment_helper::do_post_attack_effects_with_item_source(
            world,
            target.as_ref(),
            &ItemStack::empty(),
            &context,
        );
    }

    /// Drops a share of the death prize, if the world allows mob drops at all.
    ///
    /// Vanilla spells the gamerule check out at both award sites; the two are the same
    /// test over the same tick, so it lives here instead.
    fn award_death_experience(&self, world: &Arc<World>, share: f32) {
        if !world.get_game_rule(&MOB_DROPS) {
            return;
        }
        ExperienceOrbEntity::award(world, self.position(), share_of(DEATH_EXPERIENCE, share));
    }

    /// Builds vanilla's `damageSources().mobAttack(this)`.
    ///
    /// Deliberately not `Mob::mob_attack_damage_source`, which derives the damage type
    /// from a held weapon; vanilla's dragon always attacks as a bare mob.
    fn mob_attack_damage_source(&self) -> DamageSource {
        DamageSource::environment(&vanilla_damage_types::MOB_ATTACK)
            .with_causing_entity(self.id())
            .with_direct_entity(self.id())
            .with_source_position(self.position())
    }

    /// Carves through, or bumps into, everything inside one hitbox.
    ///
    /// Mirrors vanilla `checkWalls`. Returns whether the dragon hit something it could
    /// not break, which is what stalls it; blocks that *were* broken are reported to
    /// clients as particles instead.
    fn check_walls(world: &Arc<World>, box_: WorldAabb) -> bool {
        let min = BlockPos::containing(box_.min_x(), box_.min_y(), box_.min_z());
        let max = BlockPos::containing(box_.max_x(), box_.max_y(), box_.max_z());

        // Vanilla re-reads the game rule for every block. That takes a level-data read
        // lock, and the body box alone spans dozens of blocks each tick, so the
        // loop-invariant read is hoisted; the result is identical either way.
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
                        // Without griefing the particle event below is already dead and
                        // the rest of the box can only set `hit_wall` again. Vanilla
                        // scans on, but draws no RNG and changes no state doing it.
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

    /// Whether the dragon is perched.
    ///
    /// Vanilla spells out `phaseManager.getCurrentPhase().isSitting()` at each of its
    /// five use sites; Rust can name it once.
    fn is_sitting(&self) -> bool {
        self.phase_manager.current().is_sitting()
    }

    /// Returns the dragon's yaw.
    fn yaw(&self) -> f32 {
        self.rotation().0
    }

    /// Sets the dragon's yaw, leaving its pitch alone.
    ///
    /// Stands in for vanilla `setYRot`; Steel's setter takes both components, and the
    /// dragon only ever steers in yaw.
    fn set_yaw(&self, yaw: f32) {
        self.set_rotation((yaw, self.rotation().1));
    }

    /// Advances the wing beat. Mirrors the `flapTime` increment in `aiStep`.
    ///
    /// The previous beat is latched by [`Self::dragon_ai_step`] instead, because vanilla
    /// does that above the dying split while this increment sits below it.
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

    /// Ticks the active phase, re-ticking once if it switched.
    ///
    /// Vanilla chases exactly one switch, and the steering that follows uses the new
    /// phase, so a phase that hands off gets to set its successor's fly target in the
    /// same tick.
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

    /// Flies the dragon toward the active phase's target.
    ///
    /// Ported expression by expression from vanilla. Three orderings matter and are
    /// easy to "tidy" into something that still looks right: the squared distance is
    /// taken from the pre-clamp height delta; the heading vector reads the vertical
    /// velocity *after* the climb has been added; and the forward thrust is applied
    /// along `-Z` rather than through a movement input.
    fn steer_towards_phase_target(&self, phase: EnderDragonPhase) -> Option<MoveResult> {
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
            // TODO: Vanilla uses `Mth.atan2`, a table approximation that `steel-math`
            // does not port yet; `f64::atan2` is exact and so turns fractionally
            // differently.
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
        let result = self.move_entity(MoverType::SelfMovement, movement);

        let moved = self.velocity();
        let slide = 0.8 + 0.15 * (moved.normalize_or_zero().dot(heading) + 1.0) / 2.0;
        self.set_velocity(moved * DVec3::new(slide, VERTICAL_DRAG, slide));
        result
    }

    /// Applies damage routed through one of the dragon's hitboxes.
    ///
    /// Mirrors vanilla `EnderDragon.hurt(ServerLevel, EnderDragonPart, DamageSource,
    /// float)`, which Rust cannot name as an overload of `Entity::hurt`.
    ///
    /// Everything but the head takes a quarter of the damage plus a flat point, which
    /// is what makes aiming for the head worthwhile.
    ///
    /// Note the dragon reports a hit as handled even when it ignores the damage, so
    /// an arrow from a dispenser still lands and simply does nothing.
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

        if !Self::is_damageable_by(world, source) {
            return true;
        }

        let health_before = self.get_health();
        self.really_hurt(world, source, damage);
        if phase.is_sitting() {
            self.accumulate_sitting_damage(health_before - self.get_health());
        }
        true
    }

    /// Whether a damage source is allowed to hurt the dragon at all.
    ///
    /// Vanilla admits only players and the explosion-shaped damage types, which is
    /// what stops the dragon being whittled down by fire, drowning or a stray mob.
    fn is_damageable_by(world: &World, source: &DamageSource) -> bool {
        if source.is(&vanilla_damage_type_tags::DamageTypeTag::ALWAYS_HURTS_ENDER_DRAGONS) {
            return true;
        }

        source
            .causing_entity_id
            .and_then(|id| world.get_entity_by_id(id))
            .is_some_and(|entity| entity.as_player().is_some())
    }

    /// Tracks damage taken while perched, and takes off once enough has landed.
    ///
    /// Mirrors the accumulator in vanilla's per-part `hurt`: a quarter of max health
    /// is what dislodges a sitting dragon.
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

    /// Applies the damage the shared living path would have applied.
    ///
    /// Mirrors vanilla `EnderDragon.reallyHurt`, which calls `super.hurtServer`.
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

    /// Mirrors vanilla `EnderDragon.kill`, which removes the dragon outright.
    ///
    /// The shared `kill` damages a living entity instead, and the dragon turns damage
    /// into the 200-tick death flight, so without this override `/kill` would take ten
    /// seconds to take effect.
    fn kill(&self, _world: &World) {
        // TODO: Report to `EnderDragonFight::update_dragon` and `set_dragon_killed`
        // once the fight exists.
        self.set_removed(RemovalReason::Killed);
        self.game_event(&vanilla_game_events::ENTITY_DIE);
    }

    /// Mirrors vanilla `EnderDragon.sanitizeScale`, which pins the dragon to 1.0.
    fn dimensions_for_pose(&self, _pose: EntityPose) -> EntityDimensions {
        self.entity_type.dimensions
    }

    fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
        Some(&self.entity_data)
    }

    fn update_data_before_sync(&self) {
        self.update_dirty_mob_effect_entity_data();
    }

    /// Mirrors vanilla `EnderDragon.isFlapping`.
    ///
    /// Detects the point in the beat where the wings sweep down, which is what makes
    /// the shared move path emit the flap game event.
    fn is_flapping(&self) -> bool {
        // The beat crosses the threshold shallowly, so `Mth.cos`'s table and `f32::cos`
        // disagree on which tick the sweep lands.
        let flap = trig::cos(f64::from(*self.flap_time.lock() * TAU));
        let previous = trig::cos(f64::from(*self.o_flap_time.lock() * TAU));
        previous <= -0.3 && flap >= -0.3
    }

    /// Mirrors vanilla `EnderDragon.isPickable`.
    ///
    /// The dragon body itself is not a target; its eight parts are.
    fn is_pickable(&self) -> bool {
        false
    }

    /// Mirrors vanilla `EnderDragon.checkDespawn`, which is empty.
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

    /// Mirrors vanilla `EnderDragon.hurtServer`, which routes everything through the
    /// body hitbox so that a direct hit is scaled like any other non-head part.
    fn hurt_server(&self, world: &World, source: &DamageSource, amount: f32) -> bool {
        let Some(body) = self.sub_entities.get(BODY) else {
            return false;
        };
        let Some(body) = body.as_ref().downcast_ref::<EnderDragonPart>() else {
            return false;
        };
        self.hurt_part(world, body, source, amount)
    }

    /// Mirrors vanilla `EnderDragon.knockback`, which a perched dragon shrugs off.
    ///
    /// Vanilla's override also takes the damage source and amount; Steel's signature
    /// carries neither, and the dragon's body uses neither.
    fn knockback(&self, power: f64, xd: f64, zd: f64) {
        if self.is_sitting() {
            return;
        }
        self.default_knockback(power, xd, zd);
    }

    /// Mirrors vanilla `EnderDragon.handleKillingBlow`.
    ///
    /// Rather than dying, an airborne dragon claws back to a single point of health and
    /// enters its death flight. Because [`LivingEntity::is_dead_or_dying`] is
    /// health-based, restoring that point is exactly what keeps [`Self::tick_death`]
    /// from starting and lets the phase machine keep steering.
    ///
    /// The default is deliberately not called. Vanilla's body is `dead = true`, which
    /// Steel's `die` has already claimed atomically before this hook runs.
    fn handle_killing_blow(&self) {
        if self.is_sitting() {
            return;
        }

        self.set_health(1.0);
        self.phase_manager.set_phase(self, EnderDragonPhase::Dying);
    }

    /// Mirrors vanilla `EnderDragon.tickDeath`.
    ///
    /// A full replacement for the shared death tick, which counts a different field to
    /// a different limit. Vanilla's explosion particles between ticks 180 and 200 go
    /// through `Level.addParticle`, a no-op on the server, so they are absent here.
    fn tick_death(&self) {
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
        let _ = self.move_entity(MoverType::SelfMovement, drift);
        for part in &self.sub_entities {
            // The old position is recorded *before* the move here, the opposite way
            // round from `tick_parts`, which snapshots first and writes back after.
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

    /// Mirrors vanilla `EnderDragon.aiStep`, which does not call `super`.
    fn ai_step(&self) -> Option<MoveResult> {
        let world = self.level()?;
        self.dragon_ai_step(&world)
    }

    /// Flushes the line-of-sight cache the shared AI path would have cleared.
    ///
    /// The dragon replaces `ai_step` wholesale, so it never reaches
    /// `mob_server_ai_step`, and without this every targeting check would answer from
    /// a cache populated once and never invalidated.
    fn server_ai_step(&self) {
        self.tick_sensing();
    }

    /// Mirrors vanilla `EnderDragon.getSoundVolume`.
    fn sound_volume(&self) -> f32 {
        5.0
    }

    fn hurt_sound(&self, _source: &DamageSource) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_ENDER_DRAGON_HURT)
    }

    /// Mirrors vanilla `EnderDragon.sanitizeScale`, which always returns 1.0.
    fn get_scale(&self) -> f32 {
        1.0
    }

    /// Mirrors vanilla `EnderDragon.addEffect`, which refuses every effect.
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

    /// The dragon steers itself, so it never ticks a path navigation.
    ///
    /// The shared default would tick the `PathNavigation` that `MobBase::new` creates
    /// for every mob. The dragon keeps that unused navigation, exactly as it inherits
    /// an unused goal selector from vanilla's `Mob`.
    fn tick_path_navigation(&self) {}

    fn ambient_sound(&self) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_ENDER_DRAGON_AMBIENT)
    }

    /// Mirrors vanilla `EnderDragon.canAttack`.
    fn can_attack(&self, target: &dyn LivingEntity) -> bool {
        target.can_be_seen_as_enemy()
    }
}
