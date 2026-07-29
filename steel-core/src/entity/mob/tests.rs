use std::sync::{Arc, Weak};

use glam::DVec3;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_entities;
use steel_registry::vanilla_items;
use steel_registry::{
    REGISTRY, test_support::init_test_registry, vanilla_attributes, vanilla_blocks,
    vanilla_damage_types,
};
use steel_utils::locks::SyncMutex;
use steel_utils::{BlockPos, BlockStateId};

use super::{
    can_attempt_equipment_drop, find_ground_path_target_surface, path_end_node_can_reach_target,
};
use crate::behavior::init_behaviors;
use crate::entity::ai::control::{DEFAULT_LOOK_X_MAX_ROT_ANGLE, DEFAULT_LOOK_Y_MAX_ROT_SPEED};
use crate::entity::ai::goal::GoalControl;
use crate::entity::ai::node::Node;
use crate::entity::ai::path::{Path, PathType};
use crate::entity::damage::DamageSource;
use crate::entity::mob::{Mob, MobBase};
use crate::entity::{
    Entity, EntityBase, LivingEntity, LivingEntityBase, PathfinderMob, SharedEntity,
};
use crate::test_support::test_world;
use crate::world::{LevelReader, World};

#[test]
fn equipment_drop_attempt_gate_matches_vanilla_conditions() {
    assert!(!can_attempt_equipment_drop(0.0, true, true));
    assert!(!can_attempt_equipment_drop(0.085, false, false));
    assert!(can_attempt_equipment_drop(0.085, false, true));
    assert!(can_attempt_equipment_drop(2.0, true, false));
}

struct SurfaceLevel {
    default_state: BlockStateId,
    states: Vec<(BlockPos, BlockStateId)>,
}

impl SurfaceLevel {
    fn new(default_state: BlockStateId) -> Self {
        Self {
            default_state,
            states: Vec::new(),
        }
    }

    fn with(mut self, pos: BlockPos, state: BlockStateId) -> Self {
        self.states.push((pos, state));
        self
    }
}

impl LevelReader for SurfaceLevel {
    fn get_block_state(&self, pos: BlockPos) -> BlockStateId {
        self.states
            .iter()
            .find_map(|(state_pos, state)| (*state_pos == pos).then_some(*state))
            .unwrap_or(self.default_state)
    }

    fn raw_brightness(&self, _pos: BlockPos, _sky_darkening: u8) -> u8 {
        0
    }

    fn min_y(&self) -> i32 {
        -64
    }

    fn height(&self) -> i32 {
        384
    }
}

struct DespawnTestMob {
    base: EntityBase,
    entity_type: EntityTypeRef,
    living_base: LivingEntityBase,
    mob_base: MobBase,
    flags: SyncMutex<i8>,
    health: SyncMutex<f32>,
    nearest_player_distance_sqr: Option<f64>,
    remove_when_far_away: bool,
    controlling_passenger: SyncMutex<Option<SharedEntity>>,
}

impl DespawnTestMob {
    fn new(nearest_player_distance_sqr: Option<f64>, remove_when_far_away: bool) -> Self {
        Self::with_position(
            1,
            DVec3::ZERO,
            nearest_player_distance_sqr,
            remove_when_far_away,
        )
    }

    fn with_position(
        id: i32,
        position: DVec3,
        nearest_player_distance_sqr: Option<f64>,
        remove_when_far_away: bool,
    ) -> Self {
        Self::with_entity_type(
            id,
            position,
            &vanilla_entities::PIG,
            nearest_player_distance_sqr,
            remove_when_far_away,
        )
    }

    fn with_entity_type(
        id: i32,
        position: DVec3,
        entity_type: EntityTypeRef,
        nearest_player_distance_sqr: Option<f64>,
        remove_when_far_away: bool,
    ) -> Self {
        init_test_registry();

        Self {
            base: EntityBase::new(id, position, entity_type.dimensions, Weak::new()),
            entity_type,
            living_base: LivingEntityBase::new(entity_type),
            mob_base: MobBase::new(),
            flags: SyncMutex::new(0),
            health: SyncMutex::new(10.0),
            nearest_player_distance_sqr,
            remove_when_far_away,
            controlling_passenger: SyncMutex::new(None),
        }
    }

    fn set_controlling_passenger(&self, passenger: SharedEntity) {
        *self.controlling_passenger.lock() = Some(passenger);
    }
}

crate::entity::impl_test_downcast_type!(DespawnTestMob);

impl Entity for DespawnTestMob {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn controlling_passenger(&self) -> Option<SharedEntity> {
        self.controlling_passenger.lock().clone()
    }

    fn hurt(&self, world: &World, source: &DamageSource, amount: f32) -> bool {
        LivingEntity::hurt_server(self, world, source, amount)
    }
}

impl LivingEntity for DespawnTestMob {
    fn living_base(&self) -> &LivingEntityBase {
        &self.living_base
    }

    fn get_health(&self) -> f32 {
        *self.health.lock()
    }

    fn set_health(&self, health: f32) {
        *self.health.lock() = health;
    }
}

struct HiddenTarget {
    base: EntityBase,
    living_base: LivingEntityBase,
    health: SyncMutex<f32>,
}

impl HiddenTarget {
    fn shared(id: i32) -> SharedEntity {
        Arc::new(Self {
            base: EntityBase::new(
                id,
                DVec3::ZERO,
                vanilla_entities::PIG.dimensions,
                Weak::new(),
            ),
            living_base: LivingEntityBase::new(&vanilla_entities::PIG),
            health: SyncMutex::new(10.0),
        })
    }
}

crate::entity::impl_test_downcast_type!(HiddenTarget);

impl Entity for HiddenTarget {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        &vanilla_entities::PIG
    }
}

impl LivingEntity for HiddenTarget {
    fn living_base(&self) -> &LivingEntityBase {
        &self.living_base
    }

    fn get_health(&self) -> f32 {
        *self.health.lock()
    }

    fn set_health(&self, health: f32) {
        *self.health.lock() = health;
    }

    fn can_be_seen_as_enemy(&self) -> bool {
        false
    }
}

impl Mob for DespawnTestMob {
    fn mob_base(&self) -> &MobBase {
        &self.mob_base
    }

    fn mob_flags(&self) -> i8 {
        *self.flags.lock()
    }

    fn set_mob_flags(&self, flags: i8) {
        *self.flags.lock() = flags;
    }

    fn remove_when_far_away(&self, _dist_sqr: f64) -> bool {
        self.remove_when_far_away
    }

    fn nearest_player_distance_sqr(&self) -> Option<f64> {
        self.nearest_player_distance_sqr
    }
}

impl PathfinderMob for DespawnTestMob {}

struct MobControlVehicleEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
}

impl MobControlVehicleEntity {
    fn new(id: i32, entity_type: EntityTypeRef) -> Self {
        Self {
            base: EntityBase::new(id, DVec3::ZERO, entity_type.dimensions, Weak::new()),
            entity_type,
        }
    }
}

crate::entity::impl_test_downcast_type!(MobControlVehicleEntity);

impl Entity for MobControlVehicleEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }
}

#[test]
fn mob_base_uses_vanilla_fire_path_malus_defaults() {
    let base = MobBase::new();
    let malus = base.pathfinding_malus().lock();

    assert_eq!(
        malus.get(PathType::FireInNeighbor).to_bits(),
        8.0_f32.to_bits()
    );
    assert_eq!(malus.get(PathType::Fire).to_bits(), 16.0_f32.to_bits());
    assert_eq!(malus.get(PathType::Water).to_bits(), 8.0_f32.to_bits());
}

#[test]
fn pathfinder_mob_reads_below_surface_capability_from_navigation() {
    let mob = DespawnTestMob::new(None, false);

    assert!(!mob.can_path_to_targets_below_surface());

    mob.mob_base()
        .navigation()
        .lock()
        .set_can_path_to_targets_below_surface(true);

    assert!(mob.can_path_to_targets_below_surface());
}

#[test]
fn mob_server_ai_step_increments_no_action_time() {
    let mob = DespawnTestMob::new(None, false);

    mob.set_no_action_time(12);
    mob.mob_server_ai_step();

    assert_eq!(mob.no_action_time(), 13);
}

#[test]
fn mob_control_flags_enable_goals_without_controller_or_boat() {
    let mob = DespawnTestMob::new(None, false);
    {
        let mut selector = mob.mob_base().goal_selector().lock();
        selector.disable_control(GoalControl::Move);
        selector.disable_control(GoalControl::Jump);
        selector.disable_control(GoalControl::Look);
    }

    mob.update_control_flags();

    let selector = mob.mob_base().goal_selector().lock();
    assert!(!selector.is_control_disabled(GoalControl::Move));
    assert!(!selector.is_control_disabled(GoalControl::Jump));
    assert!(!selector.is_control_disabled(GoalControl::Look));
}

#[test]
fn mob_control_flags_disable_goals_for_mob_controller() {
    let mob = DespawnTestMob::new(None, false);
    let controller: SharedEntity =
        Arc::new(DespawnTestMob::with_position(2, DVec3::ZERO, None, false));
    mob.set_controlling_passenger(controller);

    mob.update_control_flags();

    let selector = mob.mob_base().goal_selector().lock();
    assert!(selector.is_control_disabled(GoalControl::Move));
    assert!(selector.is_control_disabled(GoalControl::Jump));
    assert!(selector.is_control_disabled(GoalControl::Look));
}

#[test]
fn mob_control_flags_disable_jump_when_riding_boat() {
    let mob = Arc::new(DespawnTestMob::new(None, false));
    let mob_entity: SharedEntity = mob.clone();
    let boat: SharedEntity = Arc::new(MobControlVehicleEntity::new(2, &vanilla_entities::OAK_BOAT));
    EntityBase::restore_passenger_relationship(&boat, &mob_entity);

    mob.update_control_flags();

    let selector = mob.mob_base().goal_selector().lock();
    assert!(!selector.is_control_disabled(GoalControl::Move));
    assert!(selector.is_control_disabled(GoalControl::Jump));
    assert!(!selector.is_control_disabled(GoalControl::Look));
}

#[test]
fn mob_attack_damage_source_uses_item_damage_type_component() {
    let mob = DespawnTestMob::new(None, false);
    let spear = ItemStack::new(&vanilla_items::WOODEN_SPEAR);

    let source = mob.mob_attack_damage_source(&spear, &mob);

    assert_eq!(source.damage_type.key, vanilla_damage_types::SPEAR.key);
    assert_eq!(source.causing_entity_id, Some(mob.id()));
    assert_eq!(source.direct_entity_id, Some(mob.id()));
}

#[test]
fn mob_target_stores_living_target_weakly() {
    let mob = DespawnTestMob::new(None, false);
    let target: SharedEntity = Arc::new(DespawnTestMob::with_position(2, DVec3::ZERO, None, false));

    assert!(mob.set_target(Some(&target)));

    let stored = mob.target().expect("living target should be stored");
    assert!(Arc::ptr_eq(&stored, &target));
}

#[test]
fn mob_target_can_be_cleared() {
    let mob = DespawnTestMob::new(None, false);
    let target: SharedEntity = Arc::new(DespawnTestMob::with_position(2, DVec3::ZERO, None, false));
    assert!(mob.set_target(Some(&target)));

    assert!(mob.set_target(None));

    assert!(mob.target().is_none());
}

#[test]
fn mob_target_expires_with_target_entity() {
    let mob = DespawnTestMob::new(None, false);
    {
        let target: SharedEntity =
            Arc::new(DespawnTestMob::with_position(2, DVec3::ZERO, None, false));
        assert!(mob.set_target(Some(&target)));
    }

    assert!(mob.target().is_none());
}

#[test]
fn mob_target_rejects_non_living_entities() {
    let mob = DespawnTestMob::new(None, false);
    let target: SharedEntity =
        Arc::new(MobControlVehicleEntity::new(2, &vanilla_entities::OAK_BOAT));

    assert!(!mob.set_target(Some(&target)));

    assert!(mob.target().is_none());
}

#[test]
fn mob_target_rejects_targets_it_cannot_attack() {
    let mob = DespawnTestMob::new(None, false);
    let target = HiddenTarget::shared(2);

    assert!(!mob.set_target(Some(&target)));

    assert!(mob.target().is_none());
}

#[test]
fn mob_target_filters_invalid_target_without_clearing_stored_target() {
    let mob = DespawnTestMob::new(None, false);
    let target: SharedEntity = Arc::new(DespawnTestMob::with_position(2, DVec3::ZERO, None, false));

    assert!(mob.mob_base().set_target(Some(&target), |_| true));

    assert!(mob.mob_base().target(|_| false).is_none());

    let stored = mob
        .mob_base()
        .target(|_| true)
        .expect("temporary invalidity must not clear the stored target");
    assert!(Arc::ptr_eq(&stored, &target));
}

#[test]
fn mob_target_clears_previous_target_when_new_target_is_invalid() {
    let mob = DespawnTestMob::new(None, false);
    let previous: SharedEntity =
        Arc::new(DespawnTestMob::with_position(2, DVec3::ZERO, None, false));
    let invalid = HiddenTarget::shared(3);

    assert!(mob.set_target(Some(&previous)));
    assert!(!mob.set_target(Some(&invalid)));

    assert!(mob.target().is_none());
}

#[test]
fn melee_attack_range_uses_vanilla_default_reach() {
    let mob = DespawnTestMob::new(None, false);
    let close_target = DespawnTestMob::with_position(2, DVec3::new(1.7, 0.0, 0.0), None, false);
    let far_target = DespawnTestMob::with_position(3, DVec3::new(1.8, 0.0, 0.0), None, false);

    assert!(mob.is_within_melee_attack_range(&close_target));
    assert!(!mob.is_within_melee_attack_range(&far_target));
}

#[test]
fn melee_attack_range_uses_vehicle_expanded_attack_box() {
    let mob = Arc::new(DespawnTestMob::with_position(
        1,
        DVec3::new(4.0, 0.0, 0.0),
        None,
        false,
    ));
    let target = DespawnTestMob::with_position(2, DVec3::new(1.1, 0.0, 0.0), None, false);

    assert!(!mob.is_within_melee_attack_range(&target));

    let mob_entity: SharedEntity = mob.clone();
    let vehicle: SharedEntity = Arc::new(MobControlVehicleEntity::new(3, &vanilla_entities::PIG));
    EntityBase::restore_passenger_relationship(&vehicle, &mob_entity);

    assert!(mob.is_within_melee_attack_range(&target));
}

#[test]
fn target_reach_uses_vanilla_horizontal_endpoint_distance() {
    let reachable = Path::new(vec![Node::new(1, 0, 1)], BlockPos::new(2, 64, 2), false);
    let too_far = Path::new(vec![Node::new(3, 64, 0)], BlockPos::new(0, 64, 0), false);

    assert!(path_end_node_can_reach_target(
        &reachable,
        BlockPos::new(2, 64, 2)
    ));
    assert!(!path_end_node_can_reach_target(
        &too_far,
        BlockPos::new(0, 64, 0)
    ));
}

#[test]
fn mob_can_attack_excludes_ghasts() {
    let mob =
        DespawnTestMob::with_entity_type(1, DVec3::ZERO, &vanilla_entities::ZOMBIE, None, false);
    let ghast = DespawnTestMob::with_entity_type(
        2,
        DVec3::new(1.0, 0.0, 0.0),
        &vanilla_entities::GHAST,
        None,
        false,
    );

    assert!(LivingEntity::can_attack(&mob, &ghast));
    assert!(!Mob::can_attack(&mob, &ghast));
}

#[test]
fn mob_base_tick_increments_ambient_sound_time_when_roll_fails() {
    let mob = DespawnTestMob::new(None, false);

    mob.mob_base_tick();

    assert_eq!(mob.mob_base().ambient_sound_time(), 1);
}

#[test]
fn mob_base_tick_resets_ambient_sound_time_after_vanilla_roll() {
    let mob = DespawnTestMob::new(None, false);

    mob.mob_base().set_ambient_sound_time(1000);
    mob.mob_base_tick();

    assert_eq!(mob.mob_base().ambient_sound_time(), -80);
}

#[test]
fn mob_hurt_sound_resets_ambient_sound_time() {
    let mob = DespawnTestMob::new(None, false);
    let source = DamageSource::environment(&vanilla_damage_types::GENERIC);

    mob.mob_base().set_ambient_sound_time(12);
    LivingEntity::play_hurt_sound(&mob, &source);

    assert_eq!(mob.mob_base().ambient_sound_time(), -80);
}

#[test]
fn mob_do_hurt_target_applies_attack_damage_and_records_target() {
    init_test_registry();
    init_behaviors();

    let mob =
        DespawnTestMob::with_entity_type(1, DVec3::ZERO, &vanilla_entities::ZOMBIE, None, false);
    mob.attributes()
        .lock()
        .set_base_value(vanilla_attributes::ATTACK_DAMAGE, 4.0);
    let target = Arc::new(DespawnTestMob::with_position(
        2,
        DVec3::new(1.0, 0.0, 0.0),
        None,
        false,
    ));
    let target_entity: SharedEntity = target.clone();

    assert!(mob.do_hurt_target(test_world(), &target_entity));

    assert_eq!(target.get_health().to_bits(), 6.0_f32.to_bits());
    let stored_target = mob
        .last_hurt_mob()
        .expect("successful mob attack should record target");
    assert!(Arc::ptr_eq(&stored_target, &target_entity));
}

#[test]
fn mob_do_hurt_target_applies_vanilla_extra_knockback() {
    init_test_registry();
    init_behaviors();

    let mob =
        DespawnTestMob::with_entity_type(1, DVec3::ZERO, &vanilla_entities::ZOMBIE, None, false);
    {
        let mut attributes = mob.attributes().lock();
        attributes.set_base_value(vanilla_attributes::ATTACK_DAMAGE, 4.0);
        attributes.set_base_value(vanilla_attributes::ATTACK_KNOCKBACK, 2.0);
    }
    mob.set_velocity(DVec3::new(1.0, 0.0, 1.0));
    let target = Arc::new(DespawnTestMob::with_position(
        2,
        DVec3::new(1.0, 0.0, 0.0),
        None,
        false,
    ));
    let target_entity: SharedEntity = target.clone();

    assert!(mob.do_hurt_target(test_world(), &target_entity));

    assert_eq!(mob.velocity().x.to_bits(), 0.6_f64.to_bits());
    assert_eq!(mob.velocity().z.to_bits(), 0.6_f64.to_bits());
    assert!(target.velocity().length_squared() > 0.0);
    assert!(target.needs_velocity_sync());
}

#[test]
fn mob_look_control_rotates_head_yaw_without_turning_body_yaw() {
    let mob = DespawnTestMob::new(None, false);
    mob.set_rotation((0.0, 0.0));
    mob.set_y_body_rot(0.0);
    mob.set_y_head_rot(0.0);
    let position = mob.position();
    mob.mob_base().controls().lock().look_control.set_look_at(
        DVec3::new(position.x + 1.0, mob.get_eye_y(), position.z),
        DEFAULT_LOOK_Y_MAX_ROT_SPEED,
        DEFAULT_LOOK_X_MAX_ROT_ANGLE,
    );

    Mob::tick_look_control(&mob);

    assert_eq!(mob.rotation(), (0.0, 0.0));
    assert_eq!(mob.y_body_rot().to_bits(), 0.0_f32.to_bits());
    assert_eq!(mob.y_head_rot().to_bits(), (-10.0_f32).to_bits());
}

#[test]
fn mob_look_control_returns_head_yaw_toward_body_when_idle() {
    let mob = DespawnTestMob::new(None, false);
    mob.set_rotation((0.0, 20.0));
    mob.set_y_body_rot(90.0);
    mob.set_y_head_rot(0.0);

    Mob::tick_look_control(&mob);

    assert_eq!(mob.rotation(), (0.0, 0.0));
    assert_eq!(mob.y_body_rot().to_bits(), 90.0_f32.to_bits());
    assert_eq!(mob.y_head_rot().to_bits(), 10.0_f32.to_bits());
}

#[test]
fn mob_body_rotation_control_uses_tick_position_delta() {
    let mob = DespawnTestMob::new(None, false);
    mob.set_old_position(DVec3::ZERO);
    mob.base.set_position_local(DVec3::new(1.0, 0.0, 0.0));
    mob.set_rotation((90.0, 0.0));
    mob.set_y_body_rot(0.0);
    mob.set_y_head_rot(200.0);

    Mob::tick_body_rotation_control(&mob);

    assert_eq!(mob.y_body_rot().to_bits(), 90.0_f32.to_bits());
    assert_eq!(mob.y_head_rot().to_bits(), 165.0_f32.to_bits());
}

#[test]
fn mob_tick_leash_applies_default_elastic_pull() {
    let mob = Arc::new(DespawnTestMob::with_position(1, DVec3::ZERO, None, false));
    let holder = Arc::new(DespawnTestMob::with_position(
        2,
        DVec3::new(7.0, 0.0, 0.0),
        None,
        false,
    ));
    let holder_entity: SharedEntity = holder.clone();
    assert!(mob.set_leashed_to(&holder_entity));

    mob.tick_leash();

    assert!(mob.velocity().x > 0.0);
    assert!(mob.velocity().z < 0.0);
    assert!(mob.needs_velocity_sync());
    assert!(mob.rotation().0 < 0.0);
    assert!(mob.is_leashed());
}

#[test]
fn mob_despawn_resets_no_action_time_near_player() {
    let mob = DespawnTestMob::new(Some(31.0 * 31.0), false);

    mob.set_no_action_time(42);
    mob.check_mob_despawn();

    assert_eq!(mob.no_action_time(), 0);
    assert!(!mob.is_removed());
}

#[test]
fn mob_despawn_discards_far_removable_mob() {
    let mob = DespawnTestMob::new(Some(129.0 * 129.0), true);

    mob.check_mob_despawn();

    assert!(mob.is_removed());
}

#[test]
fn mob_persistence_resets_no_action_time_and_blocks_removal() {
    let mob = DespawnTestMob::new(Some(129.0 * 129.0), true);

    mob.set_no_action_time(42);
    mob.set_persistence_required();
    mob.check_mob_despawn();

    assert_eq!(mob.no_action_time(), 0);
    assert!(!mob.is_removed());
}

#[test]
fn mob_home_restriction_uses_vanilla_radius() {
    let mob = DespawnTestMob::new(None, false);

    assert!(mob.is_within_home_pos(BlockPos::new(1000, 64, 1000)));

    mob.set_home_to(BlockPos::ZERO, 4);
    assert!(mob.has_home());
    assert!(mob.is_within_home_pos(BlockPos::new(3, 0, 0)));
    assert!(!mob.is_within_home_pos(BlockPos::new(4, 0, 0)));

    mob.clear_home();
    assert!(!mob.has_home());
    assert!(mob.is_within_home_pos(BlockPos::new(1000, 64, 1000)));
}

#[test]
fn ground_path_target_air_rewrites_to_surface_above_ground() {
    init_test_registry();

    let air = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);
    let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);
    let level = SurfaceLevel::new(air).with(BlockPos::new(4, 63, 4), stone);

    assert_eq!(
        find_ground_path_target_surface(&level, BlockPos::new(4, 70, 4)),
        BlockPos::new(4, 64, 4)
    );
}

#[test]
fn ground_path_target_solid_rewrites_to_first_open_block_above() {
    init_test_registry();

    let air = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);
    let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);
    let level = SurfaceLevel::new(air)
        .with(BlockPos::new(4, 64, 4), stone)
        .with(BlockPos::new(4, 65, 4), stone);

    assert_eq!(
        find_ground_path_target_surface(&level, BlockPos::new(4, 64, 4)),
        BlockPos::new(4, 66, 4)
    );
}
