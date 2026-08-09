use glam::DVec3;
use steel_registry::{
    init_vanilla_registry, item_stack::ItemStack, vanilla_attributes,
    vanilla_damage_types, vanilla_entities, vanilla_entity_data::PlayerEntityData, vanilla_items,
    vanilla_mob_effects,
};
use steel_utils::{BlockPos, types::InteractionHand};

use crate::entity::damage::DamageSource;
use crate::inventory::equipment::EquipmentSlot;

use super::{
    ActiveMobEffect, DEFAULT_SWING_DURATION, LivingEntityBase, LivingTravelInput,
    MobEffectInstance, MobEffectSyncChange, POST_IMPULSE_GRACE_TICKS,
};

#[test]
fn living_constructor_initializes_health_from_max_health() {
    init_vanilla_registry();
    let base = LivingEntityBase::new(&vanilla_entities::PLAYER);
    let mut entity_data = PlayerEntityData::new();

    assert_eq!(
        entity_data.living_entity().health.get().to_bits(),
        1.0_f32.to_bits()
    );

    base.initialize_synced_data(&mut entity_data);

    assert_eq!(
        entity_data.living_entity().health.get().to_bits(),
        (vanilla_attributes::MAX_HEALTH.default_value as f32).to_bits()
    );
}

#[test]
fn absorption_amount_clamps_to_attribute_range() {
    init_vanilla_registry();
    let base = LivingEntityBase::new(&vanilla_entities::PLAYER);
    base.attributes()
        .lock()
        .set_base_value(vanilla_attributes::MAX_ABSORPTION, 4.0);

    base.set_absorption_amount(10.0);
    assert_eq!(base.absorption_amount().to_bits(), 4.0_f32.to_bits());

    base.set_absorption_amount(-1.0);
    assert_eq!(base.absorption_amount().to_bits(), 0.0_f32.to_bits());
}

#[test]
fn fall_damage_starts_above_safe_fall_distance() {
    assert_eq!(
        LivingEntityBase::calculate_fall_damage(3.0, 1.0, 3.0, 1.0),
        0
    );
    assert_eq!(
        LivingEntityBase::calculate_fall_damage(4.0, 1.0, 3.0, 1.0),
        1
    );
}

#[test]
fn last_damage_source_expires_after_vanilla_window() {
    init_vanilla_registry();
    let base = LivingEntityBase::new(&vanilla_entities::PIG);
    let source = DamageSource::environment(&vanilla_damage_types::GENERIC);

    assert!(base.last_damage_source(0).is_none());

    base.record_last_damage_source(&source, 10);

    let last_source = base
        .last_damage_source(50)
        .expect("last damage source should remain valid for 40 ticks");
    assert_eq!(last_source.damage_type, &vanilla_damage_types::GENERIC);
    assert!(base.last_damage_source(51).is_none());
}

#[test]
fn last_hurt_by_player_memory_ticks_down_then_clears_reference() {
    init_vanilla_registry();
    let base = LivingEntityBase::new(&vanilla_entities::PIG);
    let player_uuid = uuid::Uuid::from_u128(7);

    base.set_last_hurt_by_player(player_uuid, 2);

    assert_eq!(base.last_hurt_by_player_uuid(), Some(player_uuid));
    assert_eq!(base.last_hurt_by_player_memory_time(), 2);

    base.tick_last_hurt_by_player_memory();

    assert_eq!(base.last_hurt_by_player_uuid(), Some(player_uuid));
    assert_eq!(base.last_hurt_by_player_memory_time(), 1);

    base.tick_last_hurt_by_player_memory();

    assert_eq!(base.last_hurt_by_player_uuid(), Some(player_uuid));
    assert_eq!(base.last_hurt_by_player_memory_time(), 0);

    base.tick_last_hurt_by_player_memory();

    assert!(base.last_hurt_by_player_uuid().is_none());
    assert_eq!(base.last_hurt_by_player_memory_time(), 0);
}

#[test]
fn fall_damage_applies_block_and_attribute_multipliers() {
    assert_eq!(
        LivingEntityBase::calculate_fall_damage(8.0, 0.5, 3.0, 2.0),
        5
    );
    assert_eq!(
        LivingEntityBase::calculate_fall_damage(8.0, 0.2, 3.0, 1.0),
        1
    );
}

#[test]
fn post_impulse_grace_counts_down_by_tick() {
    init_vanilla_registry();
    let base = LivingEntityBase::new(&vanilla_entities::PLAYER);

    base.apply_post_impulse_grace_time(2);

    assert!(base.is_in_post_impulse_grace_time());
    base.tick_post_impulse_grace_time();
    assert!(base.is_in_post_impulse_grace_time());
    base.tick_post_impulse_grace_time();
    assert!(!base.is_in_post_impulse_grace_time());
}

#[test]
fn post_impulse_grace_keeps_larger_existing_window() {
    init_vanilla_registry();
    let base = LivingEntityBase::new(&vanilla_entities::PLAYER);

    base.apply_post_impulse_grace_time(5);
    base.apply_post_impulse_grace_time(2);

    for _ in 0..4 {
        base.tick_post_impulse_grace_time();
        assert!(base.is_in_post_impulse_grace_time());
    }

    base.tick_post_impulse_grace_time();
    assert!(!base.is_in_post_impulse_grace_time());
}

#[test]
fn current_impulse_context_tracks_fall_damage_impact_position() {
    init_vanilla_registry();
    let base = LivingEntityBase::new(&vanilla_entities::PLAYER);
    let impact_pos = DVec3::new(1.0, 72.0, -3.0);

    base.set_ignore_fall_damage_from_current_impulse(true, impact_pos);

    assert!(base.is_ignoring_fall_damage_from_current_impulse());
    assert_eq!(base.current_impulse_impact_pos(), Some(impact_pos));
    assert!(base.is_in_post_impulse_grace_time());
}

#[test]
fn current_impulse_context_resets_after_grace_window() {
    init_vanilla_registry();
    let base = LivingEntityBase::new(&vanilla_entities::PLAYER);

    base.set_ignore_fall_damage_from_current_impulse(true, DVec3::new(0.0, 72.0, 0.0));
    base.apply_post_impulse_grace_time(1);
    base.try_reset_current_impulse_context();
    assert!(base.is_ignoring_fall_damage_from_current_impulse());

    for _ in 0..POST_IMPULSE_GRACE_TICKS {
        base.tick_post_impulse_grace_time();
    }
    base.try_reset_current_impulse_context();

    assert!(!base.is_ignoring_fall_damage_from_current_impulse());
    assert_eq!(base.current_impulse_impact_pos(), None);
}

#[test]
fn fall_flying_is_living_entity_state() {
    init_vanilla_registry();
    let base = LivingEntityBase::new(&vanilla_entities::PLAYER);

    assert!(!base.is_fall_flying());
    base.set_fall_flying(true);
    assert!(base.is_fall_flying());
    base.set_fall_flying(false);
    assert!(!base.is_fall_flying());
}

#[test]
fn fall_flying_ticks_are_living_entity_state() {
    init_vanilla_registry();
    let base = LivingEntityBase::new(&vanilla_entities::PLAYER);

    assert_eq!(base.fall_flying_ticks(), 0);
    base.tick_fall_flying_state(true);
    base.tick_fall_flying_state(true);
    assert_eq!(base.fall_flying_ticks(), 2);
    base.tick_fall_flying_state(false);
    assert_eq!(base.fall_flying_ticks(), 0);
}

#[test]
fn living_rotation_is_base_tick_snapshot_state() {
    init_vanilla_registry();
    let base = LivingEntityBase::new(&vanilla_entities::PIG);

    base.set_y_body_rot(30.0);
    base.set_y_head_rot(45.0);
    let rotation = base.rotation_state();
    assert_eq!(rotation.y_body_rot().to_bits(), 30.0_f32.to_bits());
    assert_eq!(rotation.y_body_rot_o().to_bits(), 0.0_f32.to_bits());
    assert_eq!(rotation.y_head_rot().to_bits(), 45.0_f32.to_bits());
    assert_eq!(rotation.y_head_rot_o().to_bits(), 0.0_f32.to_bits());

    base.advance_rotation_for_base_tick();
    let rotation = base.rotation_state();
    assert_eq!(rotation.y_body_rot_o().to_bits(), 30.0_f32.to_bits());
    assert_eq!(rotation.y_head_rot_o().to_bits(), 45.0_f32.to_bits());

    base.set_y_body_rot(60.0);
    base.set_y_head_rot(75.0);
    let rotation = base.rotation_state();
    assert_eq!(rotation.y_body_rot().to_bits(), 60.0_f32.to_bits());
    assert_eq!(rotation.y_body_rot_o().to_bits(), 30.0_f32.to_bits());
    assert_eq!(rotation.y_head_rot().to_bits(), 75.0_f32.to_bits());
    assert_eq!(rotation.y_head_rot_o().to_bits(), 45.0_f32.to_bits());
}

#[test]
fn living_swing_uses_vanilla_restart_gate() {
    init_vanilla_registry();
    let base = LivingEntityBase::new(&vanilla_entities::PIG);

    assert!(base.start_swing(InteractionHand::MainHand, DEFAULT_SWING_DURATION));
    let state = base.swing_state();
    assert!(state.swinging());
    assert_eq!(state.swinging_arm(), Some(InteractionHand::MainHand));
    assert_eq!(state.swing_time(), -1);

    base.update_swing_time(DEFAULT_SWING_DURATION);
    assert!(!base.start_swing(InteractionHand::OffHand, DEFAULT_SWING_DURATION));
    assert_eq!(
        base.swing_state().swinging_arm(),
        Some(InteractionHand::MainHand)
    );

    for _ in 0..3 {
        base.update_swing_time(DEFAULT_SWING_DURATION);
    }
    assert!(base.start_swing(InteractionHand::OffHand, DEFAULT_SWING_DURATION));
    let state = base.swing_state();
    assert_eq!(state.swinging_arm(), Some(InteractionHand::OffHand));
    assert_eq!(state.swing_time(), -1);
}

#[test]
fn living_swing_time_updates_attack_animation() {
    init_vanilla_registry();
    let base = LivingEntityBase::new(&vanilla_entities::PIG);

    assert!(base.start_swing(InteractionHand::MainHand, DEFAULT_SWING_DURATION));
    base.update_swing_time(DEFAULT_SWING_DURATION);
    base.update_swing_time(DEFAULT_SWING_DURATION);
    let state = base.swing_state();
    assert!(state.swinging());
    assert_eq!(state.swing_time(), 1);
    assert_eq!(
        state.attack_anim().to_bits(),
        (1.0_f32 / DEFAULT_SWING_DURATION as f32).to_bits()
    );

    base.advance_attack_animation_for_base_tick();
    assert_eq!(
        base.swing_state().old_attack_anim().to_bits(),
        (1.0_f32 / DEFAULT_SWING_DURATION as f32).to_bits()
    );

    for _ in 0..5 {
        base.update_swing_time(DEFAULT_SWING_DURATION);
    }
    let state = base.swing_state();
    assert!(!state.swinging());
    assert_eq!(state.swing_time(), 0);
    assert_eq!(state.attack_anim().to_bits(), 0.0_f32.to_bits());
}

#[test]
fn equipment_is_living_entity_state() {
    init_vanilla_registry();
    let base = LivingEntityBase::new(&vanilla_entities::PLAYER);

    assert!(base.equipment().lock().non_empty_items().is_empty());

    base.equipment()
        .lock()
        .set(EquipmentSlot::Chest, ItemStack::new(&vanilla_items::ELYTRA));

    assert!(
        base.equipment()
            .lock()
            .get_ref(EquipmentSlot::Chest)
            .is(&vanilla_items::ELYTRA)
    );
}

#[test]
fn sprinting_is_living_entity_state_and_speed_modifier() {
    init_vanilla_registry();
    let base = LivingEntityBase::new(&vanilla_entities::PLAYER);
    let movement_speed = vanilla_attributes::MOVEMENT_SPEED;
    let base_speed = base
        .attributes()
        .lock()
        .get_value(movement_speed)
        .expect("player should have movement speed");

    assert!(!base.is_sprinting());
    base.set_sprinting(true);
    assert!(base.is_sprinting());
    assert!(
        base.attributes()
            .lock()
            .get_value(movement_speed)
            .expect("player should have movement speed")
            > base_speed
    );

    base.set_sprinting(false);
    assert!(!base.is_sprinting());
    assert_eq!(
        base.attributes()
            .lock()
            .get_value(movement_speed)
            .expect("player should have movement speed")
            .to_bits(),
        base_speed.to_bits()
    );
}

#[test]
fn active_mob_effect_presence_is_living_entity_state() {
    init_vanilla_registry();
    let base = LivingEntityBase::new(&vanilla_entities::PLAYER);

    assert!(!base.has_mob_effect(vanilla_mob_effects::DOLPHINS_GRACE));
    base.set_mob_effect_active(vanilla_mob_effects::DOLPHINS_GRACE, true);
    assert!(base.has_mob_effect(vanilla_mob_effects::DOLPHINS_GRACE));
    assert_eq!(
        base.mob_effect(vanilla_mob_effects::DOLPHINS_GRACE),
        Some(ActiveMobEffect::new(vanilla_mob_effects::DOLPHINS_GRACE, 0))
    );
    base.set_mob_effect_active(vanilla_mob_effects::DOLPHINS_GRACE, false);
    assert!(!base.has_mob_effect(vanilla_mob_effects::DOLPHINS_GRACE));
}

#[test]
fn active_mob_effect_amplifier_is_living_entity_state() {
    init_vanilla_registry();
    let base = LivingEntityBase::new(&vanilla_entities::PLAYER);

    base.set_mob_effect(vanilla_mob_effects::JUMP_BOOST, 2);

    assert_eq!(
        base.mob_effect(vanilla_mob_effects::JUMP_BOOST),
        Some(ActiveMobEffect::new(vanilla_mob_effects::JUMP_BOOST, 2))
    );
}

#[test]
fn mob_effect_attribute_modifiers_use_extracted_vanilla_data() {
    init_vanilla_registry();
    let base = LivingEntityBase::new(&vanilla_entities::PLAYER);
    let movement_speed = vanilla_attributes::MOVEMENT_SPEED;
    let base_speed = base
        .attributes()
        .lock()
        .get_value(movement_speed)
        .expect("player should have movement speed");
    let speed_modifier = &vanilla_mob_effects::SPEED.attribute_modifiers[0];

    assert_eq!(speed_modifier.attribute.key, movement_speed.key);
    assert!(base.add_mob_effect(MobEffectInstance::with_duration(
        vanilla_mob_effects::SPEED,
        200,
        1
    )));

    let boosted_speed = base
        .attributes()
        .lock()
        .get_value(movement_speed)
        .expect("player should have movement speed");
    let expected = base_speed * (1.0 + speed_modifier.amount * 2.0);
    assert!((boosted_speed - expected).abs() < f64::EPSILON);

    assert!(base.remove_mob_effect(vanilla_mob_effects::SPEED));
    assert_eq!(
        base.attributes()
            .lock()
            .get_value(movement_speed)
            .expect("player should have movement speed")
            .to_bits(),
        base_speed.to_bits()
    );
}

#[test]
fn player_respawn_reset_clears_living_runtime_and_effect_state() {
    init_vanilla_registry();
    let base = LivingEntityBase::new(&vanilla_entities::PLAYER);
    let movement_speed = vanilla_attributes::MOVEMENT_SPEED;
    let base_speed = base
        .attributes()
        .lock()
        .get_value(movement_speed)
        .expect("player should have movement speed");

    base.set_sprinting(true);
    base.set_sleeping_pos(BlockPos::new(1, 64, 1));
    base.set_fall_flying(true);
    base.tick_fall_flying_state(true);
    base.attributes()
        .lock()
        .set_base_value(vanilla_attributes::MAX_ABSORPTION, 4.0);
    base.set_absorption_amount(4.0);
    base.skip_drop_experience();
    base.set_no_action_time(80);
    base.set_last_hurt_by_player(uuid::Uuid::from_u128(9), 100);
    base.record_last_damage_source(
        &DamageSource::environment(&vanilla_damage_types::GENERIC),
        7,
    );
    assert!(base.apply_damage_cooldown(4.0, false).is_some());
    assert!(base.mark_death_processed());
    assert_eq!(base.increment_death_time(), 1);
    base.set_mob_effect(vanilla_mob_effects::SPEED, 1);
    base.set_mob_effect(vanilla_mob_effects::INVISIBILITY, 0);
    base.drain_dirty_mob_effects();

    base.reset_for_player_respawn();

    assert!(!base.is_sprinting());
    assert_eq!(base.sleeping_pos(), None);
    assert!(!base.is_fall_flying());
    assert_eq!(base.fall_flying_ticks(), 0);
    assert_eq!(base.absorption_amount().to_bits(), 0.0_f32.to_bits());
    assert!(!base.was_experience_consumed());
    assert_eq!(base.no_action_time(), 0);
    assert!(base.last_hurt_by_player_uuid().is_none());
    assert!(base.last_damage_source(7).is_none());
    assert!(!base.has_mob_effect(vanilla_mob_effects::SPEED));
    assert!(!base.has_mob_effect(vanilla_mob_effects::INVISIBILITY));
    assert_eq!(
        base.attributes()
            .lock()
            .get_value(movement_speed)
            .expect("player should have movement speed")
            .to_bits(),
        base_speed.to_bits()
    );

    let state = base.state.lock();
    assert!(!state.death_processed);
    assert_eq!(state.death_time, 0);
    assert_eq!(state.last_hurt.to_bits(), 0.0_f32.to_bits());
    drop(state);

    let changes = base.drain_dirty_mob_effects();
    assert!(changes.contains(&MobEffectSyncChange::Remove {
        effect: vanilla_mob_effects::SPEED
    }));
    assert!(changes.contains(&MobEffectSyncChange::Remove {
        effect: vanilla_mob_effects::INVISIBILITY
    }));
    assert!(base.take_effects_dirty());
}

#[test]
fn mob_effect_duration_tick_removes_expired_effect() {
    init_vanilla_registry();
    let base = LivingEntityBase::new(&vanilla_entities::PLAYER);

    base.add_mob_effect(MobEffectInstance::with_duration(
        vanilla_mob_effects::DOLPHINS_GRACE,
        1,
        0,
    ));
    base.drain_dirty_mob_effects();

    base.tick_mob_effect_duration(vanilla_mob_effects::DOLPHINS_GRACE);

    assert!(!base.has_mob_effect(vanilla_mob_effects::DOLPHINS_GRACE));
    assert_eq!(
        base.drain_dirty_mob_effects(),
        vec![MobEffectSyncChange::Remove {
            effect: vanilla_mob_effects::DOLPHINS_GRACE
        }]
    );
}

#[test]
fn stronger_shorter_effect_downgrades_to_hidden_effect() {
    init_vanilla_registry();
    let base = LivingEntityBase::new(&vanilla_entities::PLAYER);

    base.add_mob_effect(MobEffectInstance::with_duration(
        vanilla_mob_effects::SPEED,
        10,
        0,
    ));
    base.add_mob_effect(MobEffectInstance::with_duration(
        vanilla_mob_effects::SPEED,
        2,
        1,
    ));
    base.drain_dirty_mob_effects();

    base.tick_mob_effect_duration(vanilla_mob_effects::SPEED);
    base.tick_mob_effect_duration(vanilla_mob_effects::SPEED);

    let effect = base
        .mob_effect(vanilla_mob_effects::SPEED)
        .expect("speed should downgrade to hidden effect");
    assert_eq!(effect.amplifier(), 0);
    assert_eq!(effect.duration(), 8);
    assert_eq!(
        base.drain_dirty_mob_effects(),
        vec![MobEffectSyncChange::Update {
            effect,
            blend_for_self: false,
        }]
    );
}

#[test]
fn sleeping_uses_living_entity_sleeping_position() {
    init_vanilla_registry();
    let base = LivingEntityBase::new(&vanilla_entities::PLAYER);
    let bed_pos = BlockPos::new(12, 64, -4);

    assert!(!base.is_sleeping());
    assert_eq!(base.sleeping_pos(), None);

    base.set_sleeping_pos(bed_pos);
    assert!(base.is_sleeping());
    assert_eq!(base.sleeping_pos(), Some(bed_pos));

    base.clear_sleeping_pos();
    assert!(!base.is_sleeping());
    assert_eq!(base.sleeping_pos(), None);
}

#[test]
fn last_climbable_pos_is_living_entity_state() {
    init_vanilla_registry();
    let base = LivingEntityBase::new(&vanilla_entities::PLAYER);
    let climbable_pos = BlockPos::new(-5, 72, 3);

    assert_eq!(base.last_climbable_pos(), None);
    base.set_last_climbable_pos(climbable_pos);
    assert_eq!(base.last_climbable_pos(), Some(climbable_pos));
}

#[test]
fn discard_friction_is_living_entity_state() {
    init_vanilla_registry();
    let base = LivingEntityBase::new(&vanilla_entities::PLAYER);

    assert!(!base.should_discard_friction());
    base.set_discard_friction(true);
    assert!(base.should_discard_friction());
    base.set_discard_friction(false);
    assert!(!base.should_discard_friction());
}

#[test]
fn living_travel_input_is_shared_living_state() {
    init_vanilla_registry();
    let base = LivingEntityBase::new(&vanilla_entities::PLAYER);

    assert_eq!(base.travel_input(), LivingTravelInput::ZERO);
    base.set_travel_input(LivingTravelInput::new(1.0, 0.5, -1.0));
    assert_eq!(base.travel_input(), LivingTravelInput::new(1.0, 0.5, -1.0));

    base.dampen_travel_input();
    assert_eq!(
        base.travel_input(),
        LivingTravelInput::new(0.98, 0.5, -0.98)
    );
}

#[test]
fn jumping_and_jump_delay_are_shared_living_state() {
    init_vanilla_registry();
    let base = LivingEntityBase::new(&vanilla_entities::PLAYER);

    assert!(!base.is_jumping());
    base.set_jumping(true);
    assert!(base.is_jumping());

    assert_eq!(base.no_jump_delay(), 0);
    base.set_no_jump_delay(2);
    base.tick_no_jump_delay();
    assert_eq!(base.no_jump_delay(), 1);
    base.tick_no_jump_delay();
    base.tick_no_jump_delay();
    assert_eq!(base.no_jump_delay(), 0);
}

#[test]
fn starting_item_use_does_not_replace_active_use() {
    init_vanilla_registry();
    let base = LivingEntityBase::new(&vanilla_entities::PLAYER);

    assert!(base.start_using_item(
        InteractionHand::MainHand,
        &ItemStack::new(&vanilla_items::BRUSH),
        200,
    ));
    assert!(!base.start_using_item(
        InteractionHand::OffHand,
        &ItemStack::new(&vanilla_items::BOW),
        72_000,
    ));

    let Some(active) = base.active_item_use() else {
        panic!("first active item use should remain present");
    };
    assert_eq!(active.hand(), InteractionHand::MainHand);
    assert_eq!(active.item(), &*vanilla_items::BRUSH);
    assert_eq!(active.duration(), 200);
    assert_eq!(active.remaining_ticks(), 200);
}
