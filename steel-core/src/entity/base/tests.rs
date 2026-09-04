use super::{
    DEFAULT_MAX_AIR_SUPPLY, DEFAULT_TICKS_REQUIRED_TO_FREEZE, EntityBase, EntityBaseState,
    EntityFireFreezeState, EntityFluidContact, EntityMoveError, EntityMovement,
    EntityMovementEmission, EntityMovementFlags, EntityMovementProgress, EntityPhysicsStateInput,
    EntityPistonMovement, EntityVerticalMovementStateUpdate, MAX_ENTITY_TAGS,
};
use std::sync::{Arc, Weak};

use glam::DVec3;
use steel_registry::{
    entity_data::EntityPose, entity_type::EntityDimensions, entity_type::EntityTypeRef,
    init_vanilla_registry,
};
use steel_registry::{vanilla_damage_types, vanilla_entities};
use steel_utils::locks::SyncMutex;
use steel_utils::{BlockPos, WorldAabb};
use text_components::TextComponent;
use uuid::Uuid;

use crate::entity::damage::DamageSource;
use crate::entity::{
    Entity, EntityLevelCallback, InsideBlockEffectType, RemovalReason, SharedEntity,
    entities::RawEntity,
};
use crate::portal::PortalKind;
use crate::world::World;

fn assert_vec3_close(left: DVec3, right: DVec3) {
    let diff = left - right;
    assert!(
        diff.length_squared() < 1.0e-24,
        "expected {left:?} to equal {right:?}"
    );
}

fn assert_f32_close(left: f32, right: f32) {
    assert!(
        (left - right).abs() < 1.0e-6,
        "expected {left:?} to equal {right:?}"
    );
}

fn assert_f64_close(left: f64, right: f64) {
    assert!(
        (left - right).abs() < 1.0e-6,
        "expected {left:?} to equal {right:?}"
    );
}

fn raw_entity(id: i32) -> SharedEntity {
    Arc::new(RawEntity::new(
        id,
        DVec3::ZERO,
        Weak::<World>::new(),
        &vanilla_entities::ITEM,
    ))
}

fn link_vehicle_and_passenger(vehicle: &SharedEntity, passenger: &SharedEntity) {
    passenger.base().relationships.lock().vehicle = Some(Arc::downgrade(vehicle));
    vehicle
        .base()
        .relationships
        .lock()
        .passengers
        .push(Arc::downgrade(passenger));
}

struct FallDamageTestEntity {
    base: EntityBase,
    fall_damage_calls: SyncMutex<Vec<(f64, f32)>>,
}

impl FallDamageTestEntity {
    fn new(id: i32) -> Arc<Self> {
        Arc::new(Self {
            base: EntityBase::new(
                id,
                DVec3::ZERO,
                vanilla_entities::ITEM.dimensions,
                Weak::<World>::new(),
            ),
            fall_damage_calls: SyncMutex::new(Vec::new()),
        })
    }
}

crate::entity::impl_test_downcast_type!(FallDamageTestEntity);

impl Entity for FallDamageTestEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        &vanilla_entities::ITEM
    }

    fn cause_fall_damage(
        &self,
        fall_distance: f64,
        damage_modifier: f32,
        _source: &DamageSource,
    ) -> bool {
        self.fall_damage_calls
            .lock()
            .push((fall_distance, damage_modifier));
        true
    }
}

#[derive(Default)]
struct CountingCallback {
    removals: SyncMutex<Vec<RemovalReason>>,
    bounding_boxes: SyncMutex<Vec<WorldAabb>>,
}

impl EntityLevelCallback for CountingCallback {
    fn validate_move(&self, _old_pos: DVec3, _new_pos: DVec3) -> Result<(), EntityMoveError> {
        Ok(())
    }

    fn on_move_committed(&self, _old_pos: DVec3, _new_pos: DVec3) -> Result<(), EntityMoveError> {
        Ok(())
    }

    fn on_bounding_box_changed(&self, bounding_box: WorldAabb) {
        self.bounding_boxes.lock().push(bounding_box);
    }

    fn on_remove(&self, reason: RemovalReason) {
        self.removals.lock().push(reason);
    }
}

struct CommitRejectingCallback;

impl EntityLevelCallback for CommitRejectingCallback {
    fn validate_move(&self, _old_pos: DVec3, _new_pos: DVec3) -> Result<(), EntityMoveError> {
        Ok(())
    }

    fn on_move_committed(&self, _old_pos: DVec3, _new_pos: DVec3) -> Result<(), EntityMoveError> {
        Err(EntityMoveError::NotLive { entity_id: 1 })
    }

    fn on_remove(&self, _reason: RemovalReason) {}
}

#[test]
fn piston_movement_is_limited_per_axis_per_tick() {
    let mut piston_movement = EntityPistonMovement::new();

    assert_vec3_close(
        piston_movement.limit_movement(DVec3::new(0.4, 0.0, 0.0), 10),
        DVec3::new(0.4, 0.0, 0.0),
    );
    assert_vec3_close(
        piston_movement.limit_movement(DVec3::new(0.4, 0.0, 0.0), 10),
        DVec3::new(0.11, 0.0, 0.0),
    );
    assert_vec3_close(
        piston_movement.limit_movement(DVec3::new(0.4, 0.0, 0.0), 10),
        DVec3::ZERO,
    );
}

#[test]
fn piston_movement_resets_each_game_tick() {
    let mut piston_movement = EntityPistonMovement::new();

    assert_vec3_close(
        piston_movement.limit_movement(DVec3::new(0.51, 0.0, 0.0), 10),
        DVec3::new(0.51, 0.0, 0.0),
    );
    assert_vec3_close(
        piston_movement.limit_movement(DVec3::new(0.51, 0.0, 0.0), 11),
        DVec3::new(0.51, 0.0, 0.0),
    );
}

#[test]
fn piston_movement_uses_first_non_zero_axis() {
    let mut piston_movement = EntityPistonMovement::new();

    assert_vec3_close(
        piston_movement.limit_movement(DVec3::new(0.2, 0.2, 0.2), 10),
        DVec3::new(0.2, 0.0, 0.0),
    );
}

#[test]
fn piston_movement_keeps_sub_threshold_movement() {
    let mut piston_movement = EntityPistonMovement::new();
    let movement = DVec3::new(0.0, 0.0, 1.0e-4);

    assert_vec3_close(piston_movement.limit_movement(movement, 10), movement);
}

#[test]
fn collision_flags_clear_without_changing_ground_state() {
    let flags = EntityMovementFlags::after_move(true, true, true, DVec3::new(0.0, -1.0, 0.0))
        .without_collisions();

    assert!(flags.on_ground());
    assert!(!flags.horizontal_collision());
    assert!(!flags.vertical_collision());
    assert!(!flags.vertical_collision_below());
}

#[test]
fn movement_emission_flags_match_vanilla_variants() {
    assert!(!EntityMovementEmission::None.emits_anything());
    assert!(EntityMovementEmission::Sounds.emits_anything());
    assert!(EntityMovementEmission::Events.emits_anything());
    assert!(EntityMovementEmission::All.emits_anything());

    assert!(EntityMovementEmission::Sounds.emits_sounds());
    assert!(!EntityMovementEmission::Sounds.emits_events());
    assert!(!EntityMovementEmission::Events.emits_sounds());
    assert!(EntityMovementEmission::Events.emits_events());
    assert!(EntityMovementEmission::All.emits_sounds());
    assert!(EntityMovementEmission::All.emits_events());
}

#[test]
fn movement_progress_accumulates_vanilla_step_and_fly_distance() {
    let mut progress = EntityMovementProgress::new();

    progress.add_movement(DVec3::new(3.0, 4.0, 0.0), false);
    assert_f32_close(progress.move_dist(), 1.8);
    assert_f32_close(progress.fly_dist(), 3.0);
    assert!(progress.crossed_next_step());

    progress.add_movement(DVec3::new(0.0, 4.0, 3.0), true);
    assert_f32_close(progress.move_dist(), 4.8);
    assert_f32_close(progress.fly_dist(), 6.0);
}

#[test]
fn base_tick_count_advances_like_vanilla_entity_tick_count() {
    let base = EntityBase::new(
        1,
        DVec3::ZERO,
        EntityDimensions::new(0.25, 0.25, 0.125),
        Weak::<World>::new(),
    );

    assert_eq!(base.tick_count(), 0);
    base.advance_tick_count();

    assert_eq!(base.tick_count(), 1);
}

#[test]
fn movement_flags_can_preserve_vertical_state_for_client_authored_horizontal_moves() {
    let previous = EntityMovementFlags::after_move(true, false, true, DVec3::new(0.0, -1.0, 0.0));
    let flags = EntityMovementFlags::after_move_with_previous(
        previous,
        EntityVerticalMovementStateUpdate::Preserve,
        false,
        true,
        false,
        DVec3::new(1.0, 0.0, 0.0),
    );

    assert!(flags.on_ground());
    assert!(flags.horizontal_collision());
    assert!(flags.vertical_collision());
    assert!(flags.vertical_collision_below());
}

#[test]
fn lifecycle_state_tracks_removal() {
    let base = EntityBase::new(
        1,
        DVec3::ZERO,
        EntityDimensions::new(0.25, 0.25, 0.125),
        Weak::<World>::new(),
    );
    let callback = Arc::new(CountingCallback::default());
    base.set_level_callback(callback.clone());

    assert!(!base.is_removed());
    let Some(pending_token) = base.begin_pending_world_change() else {
        panic!("fresh entity should accept a pending world change");
    };
    assert!(base.is_world_change_token_pending(pending_token));

    base.set_removed(RemovalReason::Discarded);
    base.set_removed(RemovalReason::Killed);
    assert!(base.is_removed());
    assert!(!base.is_world_change_pending());
    assert_eq!(base.removal_reason(), Some(RemovalReason::Discarded));
    assert_eq!(*callback.removals.lock(), vec![RemovalReason::Discarded]);
    assert!(base.clear_removed());
    assert!(!base.clear_removed());
    assert!(!base.is_removed());
    assert_eq!(base.removal_reason(), None);
}

#[test]
fn lifecycle_state_tracks_pending_world_change_tokens() {
    let base = EntityBase::new(
        1,
        DVec3::ZERO,
        EntityDimensions::new(0.25, 0.25, 0.125),
        Weak::<World>::new(),
    );

    let Some(first) = base.begin_pending_world_change() else {
        panic!("fresh entity should accept a pending world change");
    };
    assert!(base.is_world_change_pending());
    assert!(base.is_world_change_token_pending(first));
    assert_eq!(base.begin_pending_world_change(), None);
    assert!(base.finish_pending_world_change(first));
    assert!(!base.is_world_change_pending());

    let Some(second) = base.begin_pending_world_change() else {
        panic!("entity should accept a second pending world change after finishing the first");
    };
    assert_ne!(first, second);
    assert!(!base.finish_pending_world_change(first));
    assert!(base.is_world_change_token_pending(second));
    assert!(base.finish_pending_world_change(second));
    assert!(!base.is_world_change_pending());
}

#[test]
fn killed_player_respawn_can_retain_admission_ownership() {
    let dimensions = EntityDimensions::new(0.6, 1.8, 1.62);
    let base = EntityBase::new(1, DVec3::ZERO, dimensions, Weak::<World>::new());
    base.set_removed(RemovalReason::Killed);

    assert_eq!(base.begin_pending_world_change(), None);
    let Some(pending_token) = base.begin_pending_player_respawn() else {
        panic!("a killed player should be able to reserve respawn preparation");
    };
    assert!(base.clear_removed());
    base.reset_for_player_respawn_during_world_change(dimensions, pending_token);

    assert!(base.is_world_change_token_pending(pending_token));
    assert!(base.finish_pending_world_change(pending_token));
}

#[test]
fn try_set_position_rolls_back_when_commit_fails() {
    let base = EntityBase::new(
        1,
        DVec3::new(1.0, 2.0, 3.0),
        EntityDimensions::new(0.25, 0.25, 0.125),
        Weak::<World>::new(),
    );
    base.set_level_callback(Arc::new(CommitRejectingCallback));

    let result = base.try_set_position(DVec3::new(4.0, 5.0, 6.0));

    assert!(matches!(
        result,
        Err(EntityMoveError::NotLive { entity_id: 1 })
    ));
    assert_vec3_close(base.position(), DVec3::new(1.0, 2.0, 3.0));
}

#[test]
#[should_panic(expected = "entity 1 local position update bypassed world entity manager")]
fn set_position_local_panics_when_callback_requires_manager_commit() {
    let base = EntityBase::new(
        1,
        DVec3::new(1.0, 2.0, 3.0),
        EntityDimensions::new(0.25, 0.25, 0.125),
        Weak::<World>::new(),
    );
    base.set_level_callback(Arc::new(CountingCallback::default()));

    base.set_position_local(DVec3::new(4.0, 5.0, 6.0));
}

#[test]
fn dimension_change_notifies_the_level_callback_of_new_bounds() {
    let base = EntityBase::new(
        1,
        DVec3::new(1.0, 2.0, 3.0),
        EntityDimensions::new(0.25, 0.25, 0.125),
        Weak::<World>::new(),
    );
    let callback = Arc::new(CountingCallback::default());
    base.set_level_callback(callback.clone());

    base.set_pose_and_dimensions(EntityPose::Standing, EntityDimensions::new(2.0, 3.0, 2.5));

    assert_eq!(*callback.bounding_boxes.lock(), vec![base.bounding_box()]);
}

#[test]
fn base_state_caches_fluid_contact() {
    let base = EntityBase::new(
        1,
        DVec3::ZERO,
        EntityDimensions::new(0.25, 0.25, 0.125),
        Weak::<World>::new(),
    );
    let water_contact = EntityFluidContact::from_parts(0.4, 0.0, true, false);
    let air_contact = EntityFluidContact::default();

    base.set_fluid_contact(water_contact);

    assert_eq!(base.fluid_contact(), water_contact);
    assert!(!base.was_eye_in_water());

    base.set_fluid_contact(air_contact);

    assert_eq!(base.fluid_contact(), air_contact);
    assert!(!base.was_eye_in_water());

    base.set_fluid_contact(water_contact);
    base.set_fluid_contact_for_base_tick(air_contact);

    assert_eq!(base.fluid_contact(), air_contact);
    assert!(base.was_eye_in_water());
}

#[test]
fn fire_freeze_state_applies_inside_block_effects() {
    let base = EntityBase::new(
        1,
        DVec3::ZERO,
        EntityDimensions::new(0.25, 0.25, 0.125),
        Weak::<World>::new(),
    );

    base.apply_inside_block_effect(
        InsideBlockEffectType::Freeze,
        true,
        false,
        0,
        DEFAULT_TICKS_REQUIRED_TO_FREEZE,
        None,
    );
    assert!(base.is_in_powder_snow());
    assert_eq!(base.ticks_frozen(), 1);

    base.apply_inside_block_effect(
        InsideBlockEffectType::LavaIgnite,
        true,
        false,
        0,
        DEFAULT_TICKS_REQUIRED_TO_FREEZE,
        None,
    );
    assert_eq!(base.remaining_fire_ticks(), 300);
    assert_eq!(base.ticks_frozen(), 0);

    base.apply_inside_block_effect(
        InsideBlockEffectType::Extinguish,
        true,
        false,
        0,
        DEFAULT_TICKS_REQUIRED_TO_FREEZE,
        None,
    );
    assert_eq!(base.remaining_fire_ticks(), 0);

    base.apply_inside_block_effect(
        InsideBlockEffectType::ClearFreeze,
        true,
        false,
        0,
        DEFAULT_TICKS_REQUIRED_TO_FREEZE,
        None,
    );
    assert_eq!(base.ticks_frozen(), 0);
}

#[test]
fn fire_ignite_respects_remaining_fire_tick_cap() {
    let base = EntityBase::new(
        1,
        DVec3::ZERO,
        EntityDimensions::new(0.25, 0.25, 0.125),
        Weak::<World>::new(),
    );

    base.apply_inside_block_effect(
        InsideBlockEffectType::LavaIgnite,
        true,
        false,
        0,
        DEFAULT_TICKS_REQUIRED_TO_FREEZE,
        Some(1),
    );
    assert_eq!(base.remaining_fire_ticks(), 1);

    base.set_remaining_fire_ticks(10);
    base.apply_inside_block_effect(
        InsideBlockEffectType::LavaIgnite,
        true,
        false,
        0,
        DEFAULT_TICKS_REQUIRED_TO_FREEZE,
        Some(1),
    );
    assert_eq!(base.remaining_fire_ticks(), 1);

    base.set_remaining_fire_ticks(0);
    base.apply_inside_block_effect(
        InsideBlockEffectType::FireIgnite,
        true,
        false,
        2,
        DEFAULT_TICKS_REQUIRED_TO_FREEZE,
        Some(1),
    );
    assert_eq!(base.remaining_fire_ticks(), 1);
}

#[test]
fn fire_ignite_respects_vanilla_cooldown_shape() {
    let base = EntityBase::new(
        1,
        DVec3::ZERO,
        EntityDimensions::new(0.25, 0.25, 0.125),
        Weak::<World>::new(),
    );

    base.set_remaining_fire_ticks(-2);
    base.apply_inside_block_effect(
        InsideBlockEffectType::FireIgnite,
        true,
        false,
        0,
        DEFAULT_TICKS_REQUIRED_TO_FREEZE,
        None,
    );
    assert_eq!(base.remaining_fire_ticks(), -1);

    base.apply_inside_block_effect(
        InsideBlockEffectType::FireIgnite,
        true,
        false,
        0,
        DEFAULT_TICKS_REQUIRED_TO_FREEZE,
        None,
    );
    assert_eq!(base.remaining_fire_ticks(), 160);

    base.set_remaining_fire_ticks(4);
    base.apply_inside_block_effect(
        InsideBlockEffectType::FireIgnite,
        true,
        false,
        2,
        DEFAULT_TICKS_REQUIRED_TO_FREEZE,
        None,
    );
    assert_eq!(base.remaining_fire_ticks(), 160);

    base.set_remaining_fire_ticks(0);
    base.apply_inside_block_effect(
        InsideBlockEffectType::FireIgnite,
        true,
        true,
        0,
        DEFAULT_TICKS_REQUIRED_TO_FREEZE,
        None,
    );
    assert_eq!(base.remaining_fire_ticks(), 0);
}

#[test]
fn base_tick_advances_powder_snow_and_fire_state() {
    let base = EntityBase::new(
        1,
        DVec3::ZERO,
        EntityDimensions::new(0.25, 0.25, 0.125),
        Weak::<World>::new(),
    );

    base.apply_inside_block_effect(
        InsideBlockEffectType::Freeze,
        true,
        false,
        0,
        DEFAULT_TICKS_REQUIRED_TO_FREEZE,
        None,
    );
    base.advance_powder_snow_contact_for_base_tick();
    assert!(!base.is_in_powder_snow());
    assert!(base.was_in_powder_snow());

    base.set_remaining_fire_ticks(21);
    assert!(!base.advance_fire_tick(false, false));
    assert_eq!(base.remaining_fire_ticks(), 20);
    assert!(base.advance_fire_tick(false, false));
    assert_eq!(base.remaining_fire_ticks(), 19);

    base.set_remaining_fire_ticks(20);
    assert!(!base.advance_fire_tick(false, true));
    assert_eq!(base.remaining_fire_ticks(), 19);
}

#[test]
fn player_respawn_reset_restores_fresh_base_state_and_preserves_tags() {
    let dimensions = EntityDimensions::new(0.6, 1.8, 1.62);
    let base = EntityBase::new(1, DVec3::new(1.0, 64.0, 1.0), dimensions, Weak::new());

    base.set_velocity(DVec3::new(0.4, -0.2, 0.3));
    base.set_no_physics(true);
    base.set_air_supply(12);
    base.set_portal_cooldown(9);
    base.set_as_inside_portal(PortalKind::Nether, BlockPos::new(2, 64, 2));
    base.set_no_gravity(true);
    base.set_invulnerable(true);
    base.set_custom_name(Some(TextComponent::plain("stale")));
    base.set_custom_name_visible(true);
    base.set_silent(true);
    base.set_glowing(true);
    base.add_tag("keep".to_owned());
    base.set_remaining_fire_ticks(80);
    base.set_ticks_frozen(40);
    base.set_visual_fire(true);
    base.set_fall_distance(7.0);
    base.set_fluid_contact(EntityFluidContact::from_parts(0.25, 0.5, true, true));
    base.make_stuck_in_block(DVec3::splat(0.2));
    base.mark_velocity_sync();
    base.mark_hurt();
    base.record_movement_this_tick(EntityMovement::new(
        DVec3::new(1.0, 64.0, 1.0),
        DVec3::new(2.0, 64.0, 1.0),
    ));
    base.set_position_local(DVec3::new(2.0, 64.0, 1.0));
    assert_ne!(base.take_movements_for_block_effects().len(), 0);
    base.record_movement_this_tick(EntityMovement::new(
        DVec3::new(2.0, 64.0, 1.0),
        DVec3::new(3.0, 64.0, 1.0),
    ));
    base.set_position_local(DVec3::new(3.0, 64.0, 1.0));
    assert!(base.begin_pending_world_change().is_some());

    let reset_dimensions = EntityDimensions::new(0.6, 1.8, 1.62);
    base.reset_for_player_respawn(reset_dimensions);

    let reset_position = DVec3::new(3.0, 64.0, 1.0);
    assert_vec3_close(base.velocity(), DVec3::ZERO);
    assert!(!base.no_physics());
    assert_eq!(base.air_supply(), DEFAULT_MAX_AIR_SUPPLY);
    assert_eq!(base.portal_cooldown(), 0);
    assert_eq!(base.portal_process(), None);
    assert!(!base.is_world_change_pending());
    assert!(!base.no_gravity());
    assert!(!base.invulnerable());
    assert_eq!(base.custom_name(), None);
    assert!(!base.custom_name_visible());
    assert!(!base.silent());
    assert!(!base.glowing());
    assert!(base.save_data().tags.contains("keep"));
    assert_eq!(base.remaining_fire_ticks(), 0);
    assert_eq!(base.ticks_frozen(), 0);
    assert!(!base.has_visual_fire());
    assert_eq!(base.fall_distance().to_bits(), 0.0_f64.to_bits());
    assert_eq!(base.fluid_contact(), EntityFluidContact::default());
    assert!(!base.needs_velocity_sync());
    assert!(!base.hurt_marked());
    assert_eq!(base.dimensions(), reset_dimensions);
    assert_eq!(base.last_movements_for_block_effects().len(), 0);
    assert_eq!(
        base.take_movements_for_block_effects(),
        vec![EntityMovement::new(reset_position, reset_position)]
    );
}

#[test]
fn fire_freeze_state_round_trips_through_base_load() {
    let load = super::EntityBaseLoad {
        id: 1,
        position: DVec3::ZERO,
        uuid: Uuid::nil(),
        velocity: DVec3::ZERO,
        rotation: (0.0, 0.0),
        fall_distance: 0.0,
        fire_freeze: EntityFireFreezeState::from_parts(12, 34, true, false, true),
        on_ground: false,
        save_data: super::EntityBaseSaveData {
            no_gravity: true,
            invulnerable: true,
            ..super::EntityBaseSaveData::new()
        },
        world: Weak::<World>::new(),
    };

    let base = EntityBase::from_load(load, EntityDimensions::new(0.25, 0.25, 0.125));
    let state = base.fire_freeze_state();

    assert_eq!(state.remaining_fire_ticks(), 12);
    assert_eq!(state.ticks_frozen(), 34);
    assert!(state.is_in_powder_snow());
    assert!(state.has_visual_fire());
    assert!(base.no_gravity());
    assert!(base.invulnerable());
}

#[test]
fn no_physics_is_stored_on_base_state() {
    let base = EntityBase::new(
        1,
        DVec3::ZERO,
        EntityDimensions::new(0.25, 0.25, 0.125),
        Weak::<World>::new(),
    );

    assert!(!base.no_physics());
    base.set_no_physics(true);
    assert!(base.no_physics());
}

#[test]
fn relationship_state_tracks_direct_vehicle_and_passengers() {
    let vehicle = raw_entity(1);
    let passenger = raw_entity(2);

    link_vehicle_and_passenger(&vehicle, &passenger);

    assert!(passenger.is_passenger());
    assert_eq!(passenger.vehicle().map(|entity| entity.id()), Some(1));
    assert!(vehicle.is_vehicle());
    assert_eq!(vehicle.first_passenger().map(|entity| entity.id()), Some(2));
    assert_eq!(
        vehicle
            .passengers()
            .iter()
            .map(|entity| entity.id())
            .collect::<Vec<_>>(),
        vec![2]
    );
    assert!(vehicle.has_passenger(passenger.as_ref()));
    assert_eq!(passenger.root_vehicle_id(), 1);
    assert!(passenger.is_passenger_of_same_vehicle(vehicle.as_ref()));
}

#[test]
fn relationship_queries_follow_indirect_vehicle_chain() {
    let root = raw_entity(1);
    let middle = raw_entity(2);
    let passenger = raw_entity(3);

    link_vehicle_and_passenger(&root, &middle);
    link_vehicle_and_passenger(&middle, &passenger);

    assert_eq!(passenger.root_vehicle_id(), 1);
    assert_eq!(middle.root_vehicle_id(), 1);
    assert!(root.has_indirect_passenger(passenger.as_ref()));
    assert!(middle.has_indirect_passenger(passenger.as_ref()));
    assert!(!passenger.has_indirect_passenger(root.as_ref()));
    assert!(middle.is_passenger_of_same_vehicle(passenger.as_ref()));
}

#[test]
fn removal_cleans_up_relationship_state() {
    let vehicle = raw_entity(1);
    let passenger = raw_entity(2);

    link_vehicle_and_passenger(&vehicle, &passenger);

    vehicle.set_removed(RemovalReason::UnloadedToChunk);

    assert!(vehicle.is_removed());
    assert!(!vehicle.is_vehicle());
    assert!(!passenger.is_passenger());
    assert_eq!(passenger.base().boarding_cooldown(), 60);
}

#[test]
fn base_fall_damage_propagates_to_passengers() {
    init_vanilla_registry();
    let vehicle = raw_entity(1);
    let passenger = FallDamageTestEntity::new(2);
    let passenger_entity: SharedEntity = passenger.clone();

    link_vehicle_and_passenger(&vehicle, &passenger_entity);

    assert!(!vehicle.cause_fall_damage(
        8.0,
        1.5,
        &DamageSource::environment(&vanilla_damage_types::FALL),
    ));
    assert_eq!(*passenger.fall_damage_calls.lock(), vec![(8.0, 1.5)]);
}

#[test]
fn physics_state_uses_current_base_bounding_box() {
    let position = DVec3::new(10.0, 64.0, -5.0);
    let custom_box = WorldAabb::new(9.75, 64.0, -5.75, 10.75, 66.0, -4.75);
    let base = EntityBase::new_with_state(
        1,
        EntityBaseState::new_with_bounding_box(
            position,
            EntityDimensions::new(0.25, 0.25, 0.125),
            custom_box,
        )
        .with_on_ground(true)
        .with_fall_distance(3.5),
        Weak::<World>::new(),
    );

    let physics_state = base.physics_state(EntityPhysicsStateInput {
        max_up_step: 0.6,
        backs_off_from_edge: true,
        descending: true,
        can_walk_on_powder_snow: true,
        is_falling_block: false,
    });
    let block_collision_context = physics_state.block_collision_context();

    assert_vec3_close(physics_state.position(), position);
    assert_eq!(physics_state.bounding_box(), custom_box);
    assert_eq!(physics_state.max_up_step().to_bits(), 0.6_f32.to_bits());
    assert!(physics_state.backs_off_from_edge());
    assert!(physics_state.on_ground());
    assert_f64_close(physics_state.fall_distance(), 3.5);
    assert!(block_collision_context.is_descending());
    assert!(block_collision_context.can_walk_on_powder_snow());
}

#[test]
fn old_position_is_explicit_movement_trace_state() {
    let base = EntityBase::new(
        1,
        DVec3::new(1.0, 2.0, 3.0),
        EntityDimensions::new(0.25, 0.25, 0.125),
        Weak::<World>::new(),
    );

    assert_vec3_close(base.old_position(), DVec3::new(1.0, 2.0, 3.0));
    base.set_position_local(DVec3::new(4.0, 5.0, 6.0));
    assert_vec3_close(base.position(), DVec3::new(4.0, 5.0, 6.0));
    assert_vec3_close(base.old_position(), DVec3::new(1.0, 2.0, 3.0));

    base.set_old_position_to_current();
    assert_vec3_close(base.old_position(), DVec3::new(4.0, 5.0, 6.0));
    base.set_old_position(DVec3::new(7.0, 8.0, 9.0));
    assert_vec3_close(base.old_position(), DVec3::new(7.0, 8.0, 9.0));
}

#[test]
fn set_velocity_ignores_non_finite_updates_like_vanilla() {
    let base = EntityBase::new(
        1,
        DVec3::ZERO,
        EntityDimensions::new(0.25, 0.25, 0.125),
        Weak::<World>::new(),
    );

    let velocity = DVec3::new(0.25, -0.5, 0.75);
    base.set_velocity(velocity);
    base.set_velocity(DVec3::new(f64::NAN, 0.0, 0.0));
    assert_vec3_close(base.velocity(), velocity);

    let state = EntityBaseState::new(DVec3::ZERO, EntityDimensions::new(0.25, 0.25, 0.125))
        .with_velocity(DVec3::new(f64::INFINITY, 0.0, 0.0));
    assert_vec3_close(state.velocity, DVec3::ZERO);
}

#[test]
fn set_rotation_wraps_yaw_and_clamps_pitch_like_vanilla_snap() {
    let base = EntityBase::new(
        1,
        DVec3::ZERO,
        EntityDimensions::new(0.25, 0.25, 0.125),
        Weak::<World>::new(),
    );

    base.set_rotation((450.0, 120.0));
    let rotation = base.rotation();
    assert_f32_close(rotation.0, 90.0);
    assert_f32_close(rotation.1, 90.0);

    base.set_rotation((-450.0, -120.0));
    let rotation = base.rotation();
    assert_f32_close(rotation.0, -90.0);
    assert_f32_close(rotation.1, -90.0);
}

#[test]
fn with_rotation_initializes_old_rotation_to_current_rotation() {
    let state = EntityBaseState::new(DVec3::ZERO, EntityDimensions::new(0.25, 0.25, 0.125))
        .with_rotation((450.0, 120.0));

    assert_f32_close(state.rotation.0, 90.0);
    assert_f32_close(state.rotation.1, 90.0);
    assert_eq!(state.old_rotation, state.rotation);
}

#[test]
fn old_rotation_is_base_tick_snapshot_state() {
    let base = EntityBase::new(
        1,
        DVec3::ZERO,
        EntityDimensions::new(0.25, 0.25, 0.125),
        Weak::<World>::new(),
    );

    base.set_rotation((30.0, 40.0));
    assert_eq!(base.old_rotation(), (0.0, 0.0));

    base.advance_base_tick_state();
    assert_eq!(base.old_rotation(), (30.0, 40.0));

    base.set_rotation((60.0, 70.0));
    assert_eq!(base.old_rotation(), (30.0, 40.0));

    base.set_old_yaw_to_current();
    assert_eq!(base.old_rotation(), (60.0, 40.0));

    base.set_old_rotation((450.0, 120.0));
    assert_eq!(base.old_rotation(), (90.0, 90.0));
}

#[test]
#[should_panic(expected = "entity position must be finite")]
fn set_position_rejects_non_finite_values() {
    let base = EntityBase::new(
        1,
        DVec3::ZERO,
        EntityDimensions::new(0.25, 0.25, 0.125),
        Weak::<World>::new(),
    );

    base.set_position_local(DVec3::new(f64::NAN, 0.0, 0.0));
}

#[test]
#[should_panic(expected = "entity old position must be finite")]
fn set_old_position_rejects_non_finite_values() {
    let base = EntityBase::new(
        1,
        DVec3::ZERO,
        EntityDimensions::new(0.25, 0.25, 0.125),
        Weak::<World>::new(),
    );

    base.set_old_position(DVec3::new(0.0, f64::INFINITY, 0.0));
}

#[test]
#[should_panic(expected = "entity rotation must be finite")]
fn set_rotation_rejects_non_finite_values() {
    let base = EntityBase::new(
        1,
        DVec3::ZERO,
        EntityDimensions::new(0.25, 0.25, 0.125),
        Weak::<World>::new(),
    );

    base.set_rotation((f32::NAN, 0.0));
}

#[test]
fn known_speed_is_base_tick_position_delta() {
    let base = EntityBase::new(
        1,
        DVec3::new(1.0, 2.0, 3.0),
        EntityDimensions::new(0.25, 0.25, 0.125),
        Weak::<World>::new(),
    );

    base.set_position_local(DVec3::new(4.0, 2.0, 3.0));
    base.advance_base_tick_state();
    assert_vec3_close(base.known_speed(), DVec3::ZERO);

    base.set_position_local(DVec3::new(7.0, 1.5, -1.0));
    base.advance_base_tick_state();
    assert_vec3_close(base.known_speed(), DVec3::new(3.0, -0.5, -4.0));
}

#[test]
fn base_tick_state_decrements_boarding_cooldown() {
    let base = EntityBase::new(
        1,
        DVec3::ZERO,
        EntityDimensions::new(0.25, 0.25, 0.125),
        Weak::<World>::new(),
    );

    base.set_boarding_cooldown(2);
    base.advance_base_tick_state();
    assert_eq!(base.boarding_cooldown(), 1);
    base.advance_base_tick_state();
    assert_eq!(base.boarding_cooldown(), 0);
    base.advance_base_tick_state();
    assert_eq!(base.boarding_cooldown(), 0);
}

#[test]
fn portal_cooldown_tick_decrements_portal_cooldown() {
    let base = EntityBase::new(
        1,
        DVec3::ZERO,
        EntityDimensions::new(0.25, 0.25, 0.125),
        Weak::<World>::new(),
    );

    base.set_portal_cooldown(2);
    base.process_portal_cooldown();
    assert_eq!(base.portal_cooldown(), 1);
    base.process_portal_cooldown();
    assert_eq!(base.portal_cooldown(), 0);
    base.process_portal_cooldown();
    assert_eq!(base.portal_cooldown(), 0);
}

#[test]
fn base_tick_state_does_not_decrement_portal_cooldown() {
    let base = EntityBase::new(
        1,
        DVec3::ZERO,
        EntityDimensions::new(0.25, 0.25, 0.125),
        Weak::<World>::new(),
    );

    base.set_portal_cooldown(2);
    base.advance_base_tick_state();

    assert_eq!(base.portal_cooldown(), 2);
}

#[test]
fn active_portal_process_reuses_same_portal_after_tick_is_consumed() {
    let base = EntityBase::new(
        1,
        DVec3::ZERO,
        EntityDimensions::new(0.25, 0.25, 0.125),
        Weak::<World>::new(),
    );

    base.set_as_inside_portal(PortalKind::Nether, BlockPos::new(1, 64, 1));
    let mut process = base.portal_process().expect("portal process should exist");
    assert_eq!(process.entry_position(), BlockPos::new(1, 64, 1));

    base.set_as_inside_portal(PortalKind::Nether, BlockPos::new(2, 64, 2));
    process = base
        .portal_process()
        .expect("portal process should still exist");
    assert_eq!(process.entry_position(), BlockPos::new(1, 64, 1));

    base.process_portal_teleportation(true, 80);
    base.set_as_inside_portal(PortalKind::Nether, BlockPos::new(2, 64, 2));

    assert_eq!(
        base.portal_process()
            .expect("portal process should still exist")
            .entry_position(),
        BlockPos::new(2, 64, 2)
    );
}

#[test]
fn active_portal_process_restarts_for_different_portal_kind() {
    let base = EntityBase::new(
        1,
        DVec3::ZERO,
        EntityDimensions::new(0.25, 0.25, 0.125),
        Weak::<World>::new(),
    );

    base.set_as_inside_portal(PortalKind::Nether, BlockPos::new(1, 64, 1));
    base.set_as_inside_portal(PortalKind::End, BlockPos::new(2, 70, 2));

    let process = base.portal_process().expect("portal process should exist");
    assert_eq!(process.portal(), PortalKind::End);
    assert_eq!(process.entry_position(), BlockPos::new(2, 70, 2));
    assert_eq!(process.portal_time(), 0);
    assert!(process.is_inside_portal_this_tick());
}

#[test]
fn entity_tags_respect_vanilla_limit() {
    let base = EntityBase::new(
        1,
        DVec3::ZERO,
        EntityDimensions::new(0.25, 0.25, 0.125),
        Weak::<World>::new(),
    );

    for index in 0..MAX_ENTITY_TAGS {
        assert!(base.add_tag(format!("tag_{index}")));
    }

    assert!(!base.add_tag("overflow".to_owned()));
    assert_eq!(base.tags().len(), MAX_ENTITY_TAGS);
    assert!(base.remove_tag("tag_0"));
    assert!(base.add_tag("replacement".to_owned()));
    assert!(base.tags().iter().any(|tag| tag == "replacement"));
}

#[test]
fn movement_trace_falls_back_to_old_position_when_no_moves_were_recorded() {
    let base = EntityBase::new(
        1,
        DVec3::new(1.0, 2.0, 3.0),
        EntityDimensions::new(0.25, 0.25, 0.125),
        Weak::<World>::new(),
    );
    base.set_old_position(DVec3::new(-1.0, 2.0, -3.0));

    let movements = base.take_movements_for_block_effects();

    assert_eq!(
        movements,
        vec![EntityMovement::new(
            DVec3::new(-1.0, 2.0, -3.0),
            DVec3::new(1.0, 2.0, 3.0)
        )]
    );
}

#[test]
fn movement_trace_replays_last_finalized_movements() {
    let base = EntityBase::new(
        1,
        DVec3::ZERO,
        EntityDimensions::new(0.25, 0.25, 0.125),
        Weak::<World>::new(),
    );
    assert_eq!(base.last_movements_for_block_effects().len(), 0);

    base.record_movement_this_tick(EntityMovement::new(DVec3::ZERO, DVec3::new(1.0, 0.0, 0.0)));
    base.set_position_local(DVec3::new(1.0, 0.0, 0.0));
    let finalized = base.take_movements_for_block_effects();
    assert_eq!(base.last_movements_for_block_effects(), finalized);

    base.record_movement_this_tick(EntityMovement::new(
        DVec3::new(1.0, 0.0, 0.0),
        DVec3::new(2.0, 0.0, 0.0),
    ));
    assert_eq!(base.last_movements_for_block_effects(), finalized);
}

#[test]
fn movement_trace_appends_direct_position_change_after_recorded_moves() {
    let base = EntityBase::new(
        1,
        DVec3::new(0.0, 64.0, 0.0),
        EntityDimensions::new(0.25, 0.25, 0.125),
        Weak::<World>::new(),
    );
    base.record_movement_this_tick(EntityMovement::with_axis_dependent_original_movement(
        DVec3::new(0.0, 64.0, 0.0),
        DVec3::new(1.0, 64.0, 0.0),
        DVec3::new(1.0, 0.0, 0.0),
    ));
    base.set_position_local(DVec3::new(2.0, 64.0, 0.0));

    let movements = base.take_movements_for_block_effects();

    assert_eq!(
        movements,
        vec![
            EntityMovement::with_axis_dependent_original_movement(
                DVec3::new(0.0, 64.0, 0.0),
                DVec3::new(1.0, 64.0, 0.0),
                DVec3::new(1.0, 0.0, 0.0)
            ),
            EntityMovement::new(DVec3::new(1.0, 64.0, 0.0), DVec3::new(2.0, 64.0, 0.0))
        ]
    );
}

#[test]
fn movement_trace_removes_latest_movement_recording() {
    let base = EntityBase::new(
        1,
        DVec3::ZERO,
        EntityDimensions::new(0.25, 0.25, 0.125),
        Weak::<World>::new(),
    );
    base.record_movement_this_tick(EntityMovement::new(DVec3::ZERO, DVec3::new(1.0, 0.0, 0.0)));
    base.record_movement_this_tick(EntityMovement::new(
        DVec3::new(1.0, 0.0, 0.0),
        DVec3::new(2.0, 0.0, 0.0),
    ));

    base.remove_latest_movement_recording();
    base.set_position_local(DVec3::new(1.0, 0.0, 0.0));

    let movements = base.take_movements_for_block_effects();

    assert_eq!(
        movements,
        vec![EntityMovement::new(DVec3::ZERO, DVec3::new(1.0, 0.0, 0.0))]
    );
}

#[test]
fn movement_trace_compacts_oldest_moves_at_vanilla_limit() {
    let base = EntityBase::new(
        1,
        DVec3::ZERO,
        EntityDimensions::new(0.25, 0.25, 0.125),
        Weak::<World>::new(),
    );

    for x in 0..101 {
        let from = DVec3::new(f64::from(x), 0.0, 0.0);
        let to = DVec3::new(f64::from(x + 1), 0.0, 0.0);
        base.record_movement_this_tick(EntityMovement::new(from, to));
    }
    base.set_position_local(DVec3::new(101.0, 0.0, 0.0));

    let movements = base.take_movements_for_block_effects();

    assert_eq!(movements.len(), 100);
    assert_eq!(
        movements[0],
        EntityMovement::new(DVec3::new(0.0, 0.0, 0.0), DVec3::new(2.0, 0.0, 0.0))
    );
    assert_eq!(
        movements[99],
        EntityMovement::new(DVec3::new(100.0, 0.0, 0.0), DVec3::new(101.0, 0.0, 0.0))
    );
}

#[test]
fn fall_distance_is_stored_on_base_state() {
    let base = EntityBase::new(
        1,
        DVec3::ZERO,
        EntityDimensions::new(0.25, 0.25, 0.125),
        Weak::<World>::new(),
    );

    base.set_fall_distance(4.5);
    assert_f64_close(base.fall_distance(), 4.5);
    base.reset_fall_distance();
    assert_f64_close(base.fall_distance(), 0.0);
}

#[test]
fn fall_distance_accumulation_uses_vanilla_float_cast() {
    let base = EntityBase::new(
        1,
        DVec3::ZERO,
        EntityDimensions::new(0.25, 0.25, 0.125),
        Weak::<World>::new(),
    );

    let vertical_movement = -1.0 / 3.0;
    base.accumulate_fall_distance(vertical_movement);

    let vanilla_delta = -f64::from(vertical_movement as f32);
    assert_f64_close(base.fall_distance(), vanilla_delta);
    assert!(
        (base.fall_distance() + vertical_movement).abs() > f64::EPSILON,
        "fall distance should preserve vanilla's f32 cast before widening"
    );
}

#[test]
fn base_tick_lava_contact_dampens_fall_distance() {
    let base = EntityBase::new(
        1,
        DVec3::ZERO,
        EntityDimensions::new(0.25, 0.25, 0.125),
        Weak::<World>::new(),
    );

    base.set_fall_distance(8.0);
    base.set_fluid_contact(EntityFluidContact::from_parts(0.0, 0.25, false, false));
    base.set_first_tick(false);
    base.dampen_fall_distance_in_lava();

    assert_f64_close(base.fall_distance(), 4.0);
}

#[test]
fn base_tick_water_contact_resets_fall_distance() {
    let base = EntityBase::new(
        1,
        DVec3::ZERO,
        EntityDimensions::new(0.25, 0.25, 0.125),
        Weak::<World>::new(),
    );

    base.set_fall_distance(8.0);
    base.set_fluid_contact(EntityFluidContact::from_parts(0.25, 0.0, false, false));
    base.reset_fall_distance_in_water();

    assert_f64_close(base.fall_distance(), 0.0);
}

#[test]
fn water_reset_runs_before_lava_fall_distance_damping() {
    let base = EntityBase::new(
        1,
        DVec3::ZERO,
        EntityDimensions::new(0.25, 0.25, 0.125),
        Weak::<World>::new(),
    );

    base.set_fall_distance(8.0);
    base.set_fluid_contact(EntityFluidContact::from_parts(0.25, 0.25, false, false));
    base.set_first_tick(false);
    base.reset_fall_distance_in_water();
    base.dampen_fall_distance_in_lava();

    assert_f64_close(base.fall_distance(), 0.0);
}

#[test]
fn stuck_speed_multiplier_resets_fall_distance_and_applies_once() {
    let base = EntityBase::new(
        1,
        DVec3::ZERO,
        EntityDimensions::new(0.25, 0.25, 0.125),
        Weak::<World>::new(),
    );
    base.set_velocity(DVec3::new(0.4, -0.2, 0.3));
    base.set_fall_distance(3.0);
    base.make_stuck_in_block(DVec3::new(0.8, 0.75, 0.8));

    assert_f64_close(base.fall_distance(), 0.0);
    assert_vec3_close(
        base.consume_stuck_speed_multiplier(DVec3::new(1.0, -1.0, 0.5), true),
        DVec3::new(0.8, -0.75, 0.4),
    );
    assert_vec3_close(base.velocity(), DVec3::ZERO);
    assert_vec3_close(
        base.consume_stuck_speed_multiplier(DVec3::new(1.0, -1.0, 0.5), true),
        DVec3::new(1.0, -1.0, 0.5),
    );
}

#[test]
fn stuck_speed_multiplier_can_be_consumed_without_applying_for_pistons() {
    let base = EntityBase::new(
        1,
        DVec3::ZERO,
        EntityDimensions::new(0.25, 0.25, 0.125),
        Weak::<World>::new(),
    );
    base.set_velocity(DVec3::new(0.4, -0.2, 0.3));
    base.make_stuck_in_block(DVec3::new(0.8, 0.75, 0.8));

    let movement = DVec3::new(1.0, -1.0, 0.5);
    assert_vec3_close(
        base.consume_stuck_speed_multiplier(movement, false),
        movement,
    );
    assert_vec3_close(base.velocity(), DVec3::ZERO);
}
