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
use crate::world::{
    DefaultExplosionDamageCalculator, Explosion, ExplosionBlockReader, ExplosionDamageCalculator,
    ExplosionInteraction, ExplosionOptions, ImmutableExplosionBlockCalculator, World,
};

const DEFAULT_FUSE_TIME: i32 = 80;
const DEFAULT_EXPLOSION_POWER: f32 = 4.0;
const MAX_EXPLOSION_POWER: f32 = 128.0;
const GRAVITY: f64 = 0.04;
const AIR_DRAG: f32 = 0.98;

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
            -angle.sin() * 0.02,
            f64::from(0.2_f32),
            -angle.cos() * 0.02,
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
        world.with_random(|random| random.next_i32_bounded((fuse / 4).max(1))) + fuse / 8
    }

    fn owner_reference(&self) -> Option<EntityReference> {
        self.state.lock().owner.clone()
    }

    fn owner(&self) -> Option<SharedEntity> {
        let world = self.level()?;
        self.owner_reference()?.get_living_entity(&world)
    }

    fn explode(&self, world: &Arc<World>) {
        if !world.get_game_rule(&TNT_EXPLODES) {
            return;
        }

        let position = self.position();
        let center = DVec3::new(
            position.x,
            position.y + f64::from(self.base.dimensions().height) * 0.0625,
            position.z,
        );
        let (explosion_power, used_portal) = {
            let state = self.state.lock();
            (state.explosion_power, state.used_portal)
        };
        let mut options = ExplosionOptions::new(center, explosion_power, ExplosionInteraction::Tnt);
        options.source = Some(self);
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
            velocity = DVec3::new(velocity.x * 0.7, velocity.y * -0.5, velocity.z * 0.7);
        }
        self.set_velocity(velocity);

        let fuse = self.decrement_fuse();
        if fuse <= 0 {
            self.set_removed(RemovalReason::Discarded);
            if let Some(world) = self.level() {
                self.explode(&world);
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

    fn restore_owner_reference(&self, owner: &SharedEntity) {
        if let Some(reference) = self.owner_reference() {
            reference.cache_entity(owner);
        }
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
mod tests {
    use std::io::Cursor;

    use simdnbt::borrow::read_compound as read_borrowed_compound;
    use steel_registry::blocks::properties::BlockStateProperties;
    use steel_registry::init_vanilla_registry;
    use steel_registry::{vanilla_damage_types, vanilla_entities};
    use steel_utils::random::legacy_random::LegacyRandom;
    use steel_utils::types::UpdateFlags;
    use steel_utils::{ChunkPos, WorldAabb};
    use uuid::Uuid;

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::entity::{EntityFluidContact, next_entity_id};
    use crate::physics::{CollisionWorld, WorldCollisionProvider};
    use crate::player::ResetReason;
    use crate::test_support::{TestPlayerBuilder, fresh_test_world, insert_ready_full_chunk};

    #[test]
    fn priming_applies_vanilla_motion_and_entity_properties() {
        const SEED: i64 = 0x7A71;

        init_vanilla_registry();
        let world = fresh_test_world("primed_tnt_initial_motion");
        world.set_random_seed_for_test(SEED);
        let mut expected = LegacyRandom::from_seed(SEED as u64);
        let angle = expected.next_f64() * TAU;
        let entity = PrimedTntEntity::primed(
            &vanilla_entities::TNT,
            1,
            DVec3::new(1.5, 64.0, 2.5),
            &world,
            None,
        );

        let velocity = entity.velocity();
        assert_eq!(velocity.x.to_bits(), (-angle.sin() * 0.02).to_bits());
        assert_eq!(velocity.y.to_bits(), f64::from(0.2_f32).to_bits());
        assert_eq!(velocity.z.to_bits(), (-angle.cos() * 0.02).to_bits());
        assert_eq!(world.with_random(Random::next_i64), expected.next_i64());
        assert_eq!(entity.fuse(), DEFAULT_FUSE_TIME);
        assert_eq!(entity.block_state(), vanilla_blocks::TNT.default_state());
        assert!(entity.blocks_building());
        assert!(entity.is_pickable());
        assert_eq!(entity.movement_emission(), EntityMovementEmission::None);
        assert!((entity.get_default_gravity() - GRAVITY).abs() <= f64::EPSILON);
    }

    #[test]
    fn primed_tnt_does_not_create_hard_movement_collision_shapes() {
        init_vanilla_registry();
        let world = fresh_test_world("primed_tnt_hard_collision_broad_phase");
        insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
        let first = Arc::new(PrimedTntEntity::new(
            &vanilla_entities::TNT,
            1,
            DVec3::new(8.25, 64.0, 8.5),
            Arc::downgrade(&world),
        ));
        let second = Arc::new(PrimedTntEntity::new(
            &vanilla_entities::TNT,
            2,
            DVec3::new(8.75, 64.0, 8.5),
            Arc::downgrade(&world),
        ));
        for entity in [Arc::clone(&first), Arc::clone(&second)] {
            let entity: SharedEntity = entity;
            world
                .try_add_entity(entity)
                .expect("primed TNT should enter the loaded test chunk");
        }

        let query = WorldAabb::new(7.0, 63.0, 7.0, 10.0, 66.0, 10.0);
        let collision_world = WorldCollisionProvider::for_entity(&world, first.as_ref());

        assert!(collision_world.get_entity_collisions(&query).is_empty());
        assert!(!collision_world.has_entity_collision(&query));
    }

    #[test]
    fn shortened_fuse_uses_one_bounded_level_random_draw() {
        const SEED: i64 = 0xF05E;

        init_vanilla_registry();
        let world = fresh_test_world("primed_tnt_short_fuse_random");
        world.set_random_seed_for_test(SEED);
        let mut expected = LegacyRandom::from_seed(SEED as u64);
        let expected_fuse = expected.next_i32_bounded(20) + 10;

        let actual_fuse = PrimedTntEntity::get_random_short_fuse(&world, DEFAULT_FUSE_TIME);

        assert_eq!(actual_fuse, expected_fuse);
        assert_eq!(world.with_random(Random::next_i64), expected.next_i64());
    }

    #[test]
    fn type_specific_nbt_round_trip_preserves_fuse_block_power_and_owner() {
        init_vanilla_registry();
        let owner = Uuid::from_u128(0x1234);
        let entity = PrimedTntEntity::new(&vanilla_entities::TNT, 1, DVec3::ZERO, Weak::new());
        entity.set_fuse(37);
        entity.set_block_state(
            vanilla_blocks::TNT
                .default_state()
                .set_value(&BlockStateProperties::UNSTABLE, true),
        );
        {
            let mut state = entity.state.lock();
            state.explosion_power = 6.5;
            state.owner = Some(EntityReference::from_uuid(owner));
        }

        let mut encoded = NbtCompound::new();
        entity.save_additional(&mut encoded);
        let mut bytes = Vec::new();
        encoded.write(&mut bytes);
        let borrowed = read_borrowed_compound(&mut Cursor::new(bytes.as_slice()))
            .expect("test TNT NBT should reborrow");

        let loaded = PrimedTntEntity::new(&vanilla_entities::TNT, 2, DVec3::ZERO, Weak::new());
        loaded.load_additional(BorrowedNbtCompoundView::from(&borrowed));

        assert_eq!(loaded.fuse(), 37);
        assert_eq!(loaded.block_state(), entity.block_state());
        let state = loaded.state.lock();
        assert!((state.explosion_power - 6.5).abs() <= f32::EPSILON);
        assert_eq!(state.owner.as_ref().map(EntityReference::uuid), Some(owner));
    }

    #[test]
    fn loaded_explosion_power_is_clamped_to_vanilla_bounds() {
        init_vanilla_registry();
        let entity = PrimedTntEntity::new(&vanilla_entities::TNT, 1, DVec3::ZERO, Weak::new());
        let mut encoded = NbtCompound::new();
        encoded.insert("explosion_power", 500.0_f32);
        let mut bytes = Vec::new();
        encoded.write(&mut bytes);
        let borrowed = read_borrowed_compound(&mut Cursor::new(bytes.as_slice()))
            .expect("test TNT NBT should reborrow");

        entity.load_additional(BorrowedNbtCompoundView::from(&borrowed));

        assert!((entity.state.lock().explosion_power - MAX_EXPLOSION_POWER).abs() <= f32::EPSILON);
    }

    #[test]
    fn tick_applies_physics_before_decrementing_the_fuse() {
        init_vanilla_registry();
        init_behaviors();
        let world = fresh_test_world("primed_tnt_tick_order");
        let pos = BlockPos::new(8, 64, 8);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
        let entity = PrimedTntEntity::new(
            &vanilla_entities::TNT,
            1,
            DVec3::new(8.5, 64.0, 8.5),
            Arc::downgrade(&world),
        );

        entity.tick();

        assert_eq!(entity.position().y.to_bits(), (64.0 - GRAVITY).to_bits());
        assert_eq!(
            entity.velocity().y.to_bits(),
            (-GRAVITY * f64::from(AIR_DRAG)).to_bits()
        );
        assert_eq!(entity.fuse(), DEFAULT_FUSE_TIME - 1);
        assert!(!entity.is_removed());
    }

    #[test]
    fn surviving_tick_updates_fluid_without_advancing_base_tick_eye_history() {
        init_vanilla_registry();
        init_behaviors();
        let world = fresh_test_world("primed_tnt_direct_fluid_update");
        let pos = BlockPos::new(8, 64, 8);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
        let entity = PrimedTntEntity::new(
            &vanilla_entities::TNT,
            1,
            DVec3::new(8.5, 64.0, 8.5),
            Arc::downgrade(&world),
        );
        entity
            .base
            .set_fluid_contact(EntityFluidContact::from_parts(1.0, 0.0, true, false));

        entity.tick();

        assert_eq!(entity.fluid_contact(), EntityFluidContact::default());
        assert!(!entity.base.was_eye_in_water());
    }

    #[test]
    fn fuse_zero_discards_tnt_and_disabled_rule_suppresses_the_explosion() {
        init_vanilla_registry();
        init_behaviors();
        let world = fresh_test_world("primed_tnt_disabled_explosion");
        let pos = BlockPos::new(8, 64, 8);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
        assert!(world.set_game_rule(&TNT_EXPLODES, false));
        assert!(world.set_block(
            pos,
            vanilla_blocks::GLASS.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        let entity = PrimedTntEntity::new(
            &vanilla_entities::TNT,
            1,
            DVec3::new(8.5, 64.0, 8.5),
            Arc::downgrade(&world),
        );
        entity.set_fuse(1);
        let cached_contact = EntityFluidContact::from_parts(1.0, 0.0, true, false);
        entity.base.set_fluid_contact(cached_contact);

        entity.tick();

        assert!(entity.is_removed());
        assert_eq!(entity.fuse(), 0);
        assert_eq!(entity.fluid_contact(), cached_contact);
        assert_eq!(
            world.get_block_state(pos).get_block(),
            &vanilla_blocks::GLASS
        );
    }

    #[test]
    fn primed_tnt_is_immune_to_damage() {
        init_vanilla_registry();
        let entity = PrimedTntEntity::new(&vanilla_entities::TNT, 1, DVec3::ZERO, Weak::new());
        let source = DamageSource::environment(&vanilla_damage_types::GENERIC);

        assert!(!entity.hurt(
            &fresh_test_world("primed_tnt_damage_immunity"),
            &source,
            100.0
        ));
        assert_eq!(entity.fuse(), DEFAULT_FUSE_TIME);
        assert!(!entity.is_removed());
    }

    #[test]
    fn persisted_owner_restores_to_the_live_living_entity() {
        init_vanilla_registry();
        let world = fresh_test_world("primed_tnt_owner_restore");
        let player = TestPlayerBuilder::new(
            Arc::clone(&world),
            Uuid::from_u128(0x0A11),
            "Owner",
            next_entity_id(),
        )
        .build();
        assert!(world.add_player(Arc::clone(&player), ResetReason::InitialJoin));
        let entity = PrimedTntEntity::new(
            &vanilla_entities::TNT,
            1,
            DVec3::ZERO,
            Arc::downgrade(&world),
        );
        entity.state.lock().owner = Some(EntityReference::from_uuid(player.uuid()));

        assert_eq!(entity.owner().map(|owner| owner.id()), Some(player.id()));
    }

    #[test]
    fn flowing_water_pushes_primed_tnt_trajectory() {
        init_vanilla_registry();
        init_behaviors();
        let world = fresh_test_world("primed_tnt_fluid_current");
        let pos = BlockPos::new(8, 64, 8);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
        let flags = UpdateFlags::UPDATE_NONE
            | UpdateFlags::UPDATE_KNOWN_SHAPE
            | UpdateFlags::UPDATE_SKIP_ON_PLACE;
        assert!(world.set_block(pos.below(), vanilla_blocks::STONE.default_state(), flags,));
        assert!(world.set_block(pos, vanilla_blocks::WATER.default_state(), flags,));
        assert!(
            world.set_block(
                pos.east(),
                vanilla_blocks::WATER
                    .default_state()
                    .set_value(&BlockStateProperties::LEVEL, 4),
                flags,
            )
        );
        let entity = PrimedTntEntity::new(
            &vanilla_entities::TNT,
            1,
            DVec3::new(8.5, 64.0, 8.5),
            Arc::downgrade(&world),
        );

        entity.tick();
        assert!(entity.velocity().x > 0.0);

        entity.tick();
        assert!(entity.position().x > 8.5);
    }

    #[test]
    fn teleported_tnt_preserves_nether_portal_but_explodes_other_blocks() {
        init_vanilla_registry();
        init_behaviors();
        let world = fresh_test_world("primed_tnt_portal_explosion");
        let portal_pos = BlockPos::new(8, 64, 8);
        let glass_pos = portal_pos.south();
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(portal_pos));
        let flags = UpdateFlags::UPDATE_NONE
            | UpdateFlags::UPDATE_KNOWN_SHAPE
            | UpdateFlags::UPDATE_SKIP_ON_PLACE;
        assert!(world.set_block(
            portal_pos,
            vanilla_blocks::NETHER_PORTAL.default_state(),
            flags,
        ));
        assert!(world.set_block(glass_pos, vanilla_blocks::GLASS.default_state(), flags,));
        assert_eq!(
            world.get_block_state(portal_pos).get_block(),
            &vanilla_blocks::NETHER_PORTAL
        );
        let entity = PrimedTntEntity::new(
            &vanilla_entities::TNT,
            1,
            DVec3::new(8.5, 64.0, 8.5),
            Arc::downgrade(&world),
        );

        entity.on_teleported();
        entity.explode(&world);

        assert_eq!(
            world.get_block_state(portal_pos).get_block(),
            &vanilla_blocks::NETHER_PORTAL
        );
        assert!(world.get_block_state(glass_pos).is_air());
    }

    #[test]
    fn horizontally_aligned_explosion_does_not_push_primed_tnt_upward() {
        init_vanilla_registry();
        init_behaviors();
        let world = fresh_test_world("primed_tnt_explosion_origin");
        let position = DVec3::new(9.5, 64.0, 8.5);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(BlockPos::from(position)));
        let entity = Arc::new(PrimedTntEntity::new(
            &vanilla_entities::TNT,
            1,
            position,
            Arc::downgrade(&world),
        ));
        let shared: SharedEntity = entity.clone();
        world
            .try_add_entity(shared)
            .expect("primed TNT should enter the loaded test chunk");

        world.explode(ExplosionOptions::new(
            DVec3::new(8.5, 64.0, 8.5),
            2.0,
            ExplosionInteraction::None,
        ));

        assert!(entity.velocity().x > 0.0);
        assert!(entity.velocity().y.abs() <= f64::EPSILON);
    }
}
