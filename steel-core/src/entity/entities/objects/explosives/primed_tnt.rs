//! Primed TNT entity behavior.

use std::f64::consts::TAU;
use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::entity_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::fluid::FluidState;
use steel_registry::vanilla_blocks;
use steel_registry::vanilla_entity_data::PrimedTntEntityData;
use steel_registry::vanilla_game_rules::TNT_EXPLODES;
use steel_utils::locks::SyncMutex;
use steel_utils::random::Random;
use steel_utils::{BlockPos, BlockStateId, DowncastType, DowncastTypeKey};

use crate::block_entity::block_state_nbt;
use crate::entity::damage::DamageSource;
use crate::entity::{
    Entity, EntityBase, EntityBaseLoad, EntityMovementEmission, EntityReference, EntitySyncedData,
    RemovalReason, SharedEntity,
};
use crate::physics::MoverType;
use crate::world::explosion::default_explosion_damage_source_with_references;
use crate::world::{
    DefaultExplosionDamageCalculator, Explosion, ExplosionBlockReader, ExplosionDamageCalculator,
    ExplosionInteraction, ExplosionOptions, ImmutableExplosionBlockCalculator, World,
};

const DEFAULT_FUSE_TIME: i32 = 80;
const DEFAULT_EXPLOSION_POWER: f32 = 4.0;
const MAX_EXPLOSION_POWER: f32 = 128.0;
const GRAVITY: f64 = 0.04;
const AIR_DRAG: f32 = 0.98;
const INITIAL_HORIZONTAL_SPEED: f64 = 0.02;
const INITIAL_VERTICAL_SPEED: f32 = 0.2;
const SHORT_FUSE_RANDOM_RANGE_DIVISOR: i32 = 4;
const SHORT_FUSE_MINIMUM_DIVISOR: i32 = 8;
const MIN_SHORT_FUSE_RANDOM_RANGE: i32 = 1;
const EXPLOSION_HEIGHT_FRACTION: f32 = 0.0625;
const GROUND_HORIZONTAL_DRAG: f64 = 0.7;
const GROUND_VERTICAL_BOUNCE: f64 = -0.5;

struct PrimedTntState {
    owner: Option<EntityReference>,
    used_portal: bool,
    explosion_power: f32,
}

impl PrimedTntState {
    const fn new() -> Self {
        Self {
            owner: None,
            used_portal: false,
            explosion_power: DEFAULT_EXPLOSION_POWER,
        }
    }
}

struct UsedPortalDamageCalculator;

impl ExplosionDamageCalculator for UsedPortalDamageCalculator {
    fn block_explosion_resistance(
        &self,
        explosion: &dyn Explosion,
        world: &World,
        pos: BlockPos,
        state: BlockStateId,
        fluid: FluidState,
    ) -> Option<f32> {
        if state.get_block() == &vanilla_blocks::NETHER_PORTAL {
            return None;
        }
        DefaultExplosionDamageCalculator
            .block_explosion_resistance(explosion, world, pos, state, fluid)
    }

    fn should_block_explode(
        &self,
        explosion: &dyn Explosion,
        world: &World,
        pos: BlockPos,
        state: BlockStateId,
        power: f32,
    ) -> bool {
        if state.get_block() == &vanilla_blocks::NETHER_PORTAL {
            return false;
        }
        DefaultExplosionDamageCalculator.should_block_explode(explosion, world, pos, state, power)
    }
}

impl ImmutableExplosionBlockCalculator for UsedPortalDamageCalculator {
    fn bounded_block_read_radius(&self) -> Option<u32> {
        Some(0)
    }

    fn can_cache_explosion_resistance(&self) -> bool {
        true
    }

    fn explosion_resistance(
        &self,
        reader: &dyn ExplosionBlockReader,
        pos: BlockPos,
        state: BlockStateId,
        fluid: FluidState,
    ) -> Option<f32> {
        if state.get_block() == &vanilla_blocks::NETHER_PORTAL {
            return None;
        }
        <DefaultExplosionDamageCalculator as ImmutableExplosionBlockCalculator>::explosion_resistance(
            &DefaultExplosionDamageCalculator,
            reader,
            pos,
            state,
            fluid,
        )
    }

    fn should_explode(
        &self,
        reader: &dyn ExplosionBlockReader,
        pos: BlockPos,
        state: BlockStateId,
        power: f32,
    ) -> bool {
        if state.get_block() == &vanilla_blocks::NETHER_PORTAL {
            return false;
        }
        <DefaultExplosionDamageCalculator as ImmutableExplosionBlockCalculator>::should_explode(
            &DefaultExplosionDamageCalculator,
            reader,
            pos,
            state,
            power,
        )
    }
}

static USED_PORTAL_DAMAGE_CALCULATOR: UsedPortalDamageCalculator = UsedPortalDamageCalculator;
static DEFAULT_TNT_BLOCK_CALCULATOR: DefaultExplosionDamageCalculator =
    DefaultExplosionDamageCalculator;

/// Vanilla primed TNT entity.
#[entity_behavior(class = "PrimedTnt")]
pub struct PrimedTntEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    entity_data: SyncMutex<PrimedTntEntityData>,
    state: SyncMutex<PrimedTntState>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `PrimedTntEntity`.
unsafe impl DowncastType for PrimedTntEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/primed_tnt");
}

impl PrimedTntEntity {
    /// Default fuse duration in ticks.
    pub const DEFAULT_FUSE_TIME: i32 = DEFAULT_FUSE_TIME;
    /// Sentinel used by Vanilla callers for an absent fuse.
    pub const NO_FUSE: i32 = -1;

    /// Creates an unprimed TNT entity for the entity factory.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self {
            base: EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
            entity_data: SyncMutex::new(PrimedTntEntityData::new()),
            state: SyncMutex::new(PrimedTntState::new()),
        }
    }

    /// Creates a normally primed TNT entity with Vanilla's randomized launch motion.
    #[must_use]
    pub fn primed(
        entity_type: EntityTypeRef,
        id: i32,
        position: DVec3,
        world: &Arc<World>,
        owner: Option<&dyn Entity>,
    ) -> Self {
        let entity = Self::new(entity_type, id, position, Arc::downgrade(world));
        let angle = world.with_random(Random::next_f64) * TAU;
        entity.set_velocity(DVec3::new(
            -angle.sin() * INITIAL_HORIZONTAL_SPEED,
            f64::from(INITIAL_VERTICAL_SPEED),
            -angle.cos() * INITIAL_HORIZONTAL_SPEED,
        ));
        entity.state.lock().owner = owner.map(|owner| EntityReference::from_uuid(owner.uuid()));
        entity
    }

    /// Creates a primed TNT entity from saved data.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self {
            base: EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
            entity_data: SyncMutex::new(PrimedTntEntityData::new()),
            state: SyncMutex::new(PrimedTntState::new()),
        }
    }

    /// Returns the remaining fuse time in ticks.
    #[must_use]
    pub fn fuse(&self) -> i32 {
        *self.entity_data.lock().fuse.get()
    }

    /// Sets the remaining fuse time in ticks.
    pub fn set_fuse(&self, fuse: i32) {
        self.entity_data.lock().fuse.set(fuse);
    }

    fn decrement_fuse(&self) -> i32 {
        let mut entity_data = self.entity_data.lock();
        let fuse = *entity_data.fuse.get() - 1;
        entity_data.fuse.set(fuse);
        fuse
    }

    /// Returns the block state rendered by the client.
    #[must_use]
    pub fn block_state(&self) -> BlockStateId {
        *self.entity_data.lock().block_state.get()
    }

    /// Sets the block state rendered by the client.
    pub fn set_block_state(&self, state: BlockStateId) {
        self.entity_data.lock().block_state.set(state);
    }

    /// Returns Vanilla's shortened chain-reaction fuse for `fuse`.
    #[must_use]
    pub fn get_random_short_fuse(world: &World, fuse: i32) -> i32 {
        let random_range =
            (fuse / SHORT_FUSE_RANDOM_RANGE_DIVISOR).max(MIN_SHORT_FUSE_RANDOM_RANGE);
        world.with_random(|random| random.next_i32_bounded(random_range))
            + fuse / SHORT_FUSE_MINIMUM_DIVISOR
    }

    fn owner_reference(&self) -> Option<EntityReference> {
        self.state.lock().owner.clone()
    }

    fn owner(&self) -> Option<SharedEntity> {
        let world = self.level()?;
        self.owner_reference()?.get_living_entity(&world)
    }

    fn explode(
        &self,
        world: &Arc<World>,
        direct_source: Option<&SharedEntity>,
        owner: Option<&SharedEntity>,
    ) {
        if !world.get_game_rule(&TNT_EXPLODES) {
            return;
        }

        let position = self.position();
        let center = DVec3::new(
            position.x,
            position.y
                + f64::from(self.base.dimensions().height) * f64::from(EXPLOSION_HEIGHT_FRACTION),
            position.z,
        );
        let (explosion_power, used_portal) = {
            let state = self.state.lock();
            (state.explosion_power, state.used_portal)
        };
        let mut options = ExplosionOptions::new(center, explosion_power, ExplosionInteraction::Tnt);
        options.source = Some(self);
        if let Some(direct_source) = direct_source {
            options.damage_source = Some(default_explosion_damage_source_with_references(
                direct_source,
                owner,
            ));
        }
        options.immutable_block_calculator = Some(if used_portal {
            &USED_PORTAL_DAMAGE_CALCULATOR
        } else {
            &DEFAULT_TNT_BLOCK_CALCULATOR
        });
        if used_portal {
            options.damage_calculator = Some(&USED_PORTAL_DAMAGE_CALCULATOR);
        }
        world.explode(options);
    }
}

impl Entity for PrimedTntEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn tick(&self) {
        self.handle_portal();
        self.apply_gravity();
        self.move_entity(MoverType::SelfMovement, self.velocity());
        self.apply_effects_from_blocks();
        let mut velocity = self.velocity() * f64::from(AIR_DRAG);
        if self.on_ground() {
            velocity = DVec3::new(
                velocity.x * GROUND_HORIZONTAL_DRAG,
                velocity.y * GROUND_VERTICAL_BOUNCE,
                velocity.z * GROUND_HORIZONTAL_DRAG,
            );
        }
        self.set_velocity(velocity);

        let fuse = self.decrement_fuse();
        if fuse <= 0 {
            let world = self.level();
            let direct_source = world.as_ref().and_then(|world| {
                world
                    .get_entity_by_id(self.id())
                    .filter(|entity| entity.uuid() == self.uuid())
            });
            let owner = self.owner();
            self.set_removed(RemovalReason::Discarded);
            if let Some(world) = world {
                self.explode(&world, direct_source.as_ref(), owner.as_ref());
            }
        } else {
            self.refresh_fluid_contact_with_currents();
            self.base.reset_fall_distance_in_water();
            // Vanilla's remaining-fuse smoke particle is client-local.
        }
    }

    fn blocks_building(&self) -> bool {
        true
    }

    fn is_pickable(&self) -> bool {
        !self.is_removed()
    }

    fn is_hard_collision_relevant(&self) -> bool {
        false
    }

    fn movement_emission(&self) -> EntityMovementEmission {
        EntityMovementEmission::None
    }

    fn get_default_gravity(&self) -> f64 {
        GRAVITY
    }

    fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
        Some(&self.entity_data)
    }

    fn explosion_indirect_source(&self) -> Option<SharedEntity> {
        self.owner()
    }

    fn explosion_damage_origin(&self) -> DVec3 {
        self.position()
    }

    fn cache_owner_reference(&self, owner: &SharedEntity) {
        if let Some(reference) = self.owner_reference() {
            reference.cache_entity(owner);
        }
    }

    fn restore_additional_references_from(&self, previous: &dyn Entity) {
        let Some(previous) = steel_utils::Downcast::downcast_ref::<Self>(previous) else {
            return;
        };
        let owner = previous.owner_reference();
        self.state.lock().owner = owner;
    }

    fn on_teleported(&self) {
        self.state.lock().used_portal = true;
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        nbt.insert("fuse", self.fuse() as i16);
        nbt.insert("block_state", block_state_nbt::save(self.block_state()));
        let state = self.state.lock();
        if state.explosion_power != DEFAULT_EXPLOSION_POWER {
            nbt.insert("explosion_power", state.explosion_power);
        }
        if let Some(owner) = &state.owner {
            owner.store(nbt, "owner");
        }
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.set_fuse(i32::from(
            nbt.short("fuse").unwrap_or(DEFAULT_FUSE_TIME as i16),
        ));
        self.set_block_state(
            nbt.compound("block_state")
                .and_then(block_state_nbt::load)
                .unwrap_or_else(|| vanilla_blocks::TNT.default_state()),
        );
        let mut state = self.state.lock();
        state.explosion_power = nbt
            .float("explosion_power")
            .unwrap_or(DEFAULT_EXPLOSION_POWER)
            .clamp(0.0, MAX_EXPLOSION_POWER);
        state.owner = EntityReference::read(&nbt, "owner");
    }

    fn hurt(&self, _world: &World, _source: &DamageSource, _amount: f32) -> bool {
        false
    }
}

#[cfg(test)]
#[path = "primed_tnt/tests.rs"]
mod tests;
