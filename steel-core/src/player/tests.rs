use std::sync::{Arc, Weak};

use glam::DVec3;
use steel_protocol::packet_traits::{CompressionInfo, EncodedPacket};
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, Direction};
use steel_registry::data_component_predicate::DataComponentMatchers;
use steel_registry::data_components::vanilla_components::CAN_BREAK;
use steel_registry::data_components::{AdventureModePredicate, BlockPredicate};
use steel_registry::{
    RegistryHolderSet, item_stack::ItemStack, test_support::init_test_registry, vanilla_attributes,
    vanilla_blocks, vanilla_damage_types, vanilla_game_rules, vanilla_items,
};
use steel_utils::types::{Difficulty, GameType, UpdateFlags};
use steel_utils::{BlockPos, ChunkPos};
use text_components::TextComponent;
use uuid::Uuid;

use crate::behavior::init_behaviors;
use crate::config::RuntimeConfig;
use crate::entity::{Entity, EntitySyncedData, LivingEntity, damage::DamageSource};
use crate::inventory::{container::Container as _, equipment::EquipmentSlot, menu::Menu as _};
use crate::permission::{PermissionEntry, PermissionKey, PermissionMetadataSet, PermissionSet};
use crate::player::connection::NetworkConnection;
use crate::server::Server;
use crate::test_support::{
    fresh_test_world, hard_damage_test_world, insert_ready_full_chunk, test_world,
};
use crate::world::World;

use super::{
    ClientInformation, GameProfile, Player, PlayerConnection, PlayerPermissionState, ResetReason,
    experience::Experience, experience::first_point_level_up_sound,
    game_mode::block_breaking::BlockBreakAction, lifecycle::nullable_game_mode_id,
    player_data::PersistentPlayerData,
};

struct TestConnection;

impl NetworkConnection for TestConnection {
    fn compression(&self) -> Option<CompressionInfo> {
        None
    }

    fn send_encoded(&self, _packet: EncodedPacket) {}

    fn send_encoded_bundle(&self, _packets: Vec<EncodedPacket>) {}

    fn disconnect_with_reason(&self, _reason: TextComponent) {}

    fn tick(&self) {}

    fn latency(&self) -> i32 {
        0
    }

    fn close(&self) {}

    fn closed(&self) -> bool {
        false
    }
}

fn test_runtime_config() -> Arc<RuntimeConfig> {
    Arc::new(RuntimeConfig {
        max_players: 1,
        view_distance: 2,
        simulation_distance: 2,
        max_chained_neighbor_updates: 1_000_000,
        online_mode: false,
        auth_server: None,
        profile_server: None,
        encryption: false,
        allow_flight: false,
        motd: String::new(),
        use_favicon: false,
        favicon: String::new(),
        enforce_secure_chat: false,
        chat_spam_threshold_seconds: 10,
        command_spam_threshold_seconds: 10,
        compression: None,
        server_links: None,
        packet_workers: Some(1),
        chunk_generation_threads: Some(1),
        chunk_encoding_threads: Some(1),
    })
}

fn test_player(world: Arc<World>) -> Arc<Player> {
    let connection = Arc::new(PlayerConnection::Other(Box::new(TestConnection)));
    let config = test_runtime_config();
    let player = Arc::new_cyclic(|weak_player| {
        Player::new(
            GameProfile {
                id: Uuid::from_u128(1),
                name: "TestPlayer".to_owned(),
                properties: Vec::new(),
                profile_actions: None,
            },
            Arc::clone(&connection),
            Arc::clone(&world),
            Weak::<Server>::new(),
            Arc::clone(&config),
            1,
            weak_player,
            ClientInformation::default(),
        )
    });
    player.set_client_loaded(true);
    player
}

fn permission_key(value: &str) -> PermissionKey {
    match PermissionKey::parse(value) {
        Ok(key) => key,
        Err(error) => panic!("test permission key should parse: {error}"),
    }
}

#[test]
fn permission_state_replacement_is_versioned_and_keeps_both_rule_sets() {
    let mut state = PlayerPermissionState::default();
    let overrides =
        PermissionSet::from_entries([PermissionEntry::deny(permission_key("steel.fly"))]);
    let effective =
        PermissionSet::from_entries([PermissionEntry::allow(permission_key("steel.build"))]);

    let first = state.replace(
        vec!["builder".to_owned()],
        overrides.clone(),
        PermissionMetadataSet::new(),
        effective.clone(),
        PermissionMetadataSet::new(),
    );
    let second = state.replace(
        vec!["moderator".to_owned()],
        overrides,
        PermissionMetadataSet::new(),
        effective,
        PermissionMetadataSet::new(),
    );

    assert_eq!(first, 1);
    assert_eq!(second, 2);
    assert_eq!(state.groups, ["moderator"]);
    assert!(!state.overrides.allows_key(&permission_key("steel.fly")));
    assert!(state.effective.allows_key(&permission_key("steel.build")));
}

#[test]
fn respawn_request_is_allowed_after_dead_reconnect() {
    assert!(Player::should_process_respawn(0.0));
}

#[test]
fn ai_step_copies_player_yaw_to_head_yaw() {
    init_test_registry();
    init_behaviors();
    let player = test_player(Arc::clone(test_world()));
    player.set_rotation((90.0, 15.0));
    player.set_y_head_rot(-45.0);

    let _ = player.ai_step();

    assert_eq!(player.y_head_rot().to_bits(), 90.0_f32.to_bits());
}

#[test]
fn respawn_request_is_ignored_while_alive() {
    assert!(!Player::should_process_respawn(20.0));
}

#[test]
fn respawn_request_uses_health_not_death_processed_guard() {
    struct RespawnGateInput {
        health: f32,
        death_processed: bool,
    }

    let input = RespawnGateInput {
        health: 20.0,
        death_processed: true,
    };

    assert!(input.death_processed);
    assert!(!Player::should_process_respawn(input.health));
}

#[test]
fn end_credits_respawn_keeps_vanilla_attribute_data_only() {
    assert_eq!(ResetReason::InitialJoin.respawn_data_kept(), 0x00);
    assert_eq!(ResetReason::Respawn.respawn_data_kept(), 0x00);
    assert_eq!(ResetReason::EndCredits.respawn_data_kept(), 0x01);
    assert_eq!(ResetReason::WorldChange.respawn_data_kept(), 0x03);
}

#[test]
fn disabled_damage_game_rule_matches_vanilla_player_damage_gates() {
    init_test_registry();

    let cases = [
        (
            &vanilla_damage_types::DROWN,
            &vanilla_game_rules::DROWNING_DAMAGE,
        ),
        (
            &vanilla_damage_types::FALL,
            &vanilla_game_rules::FALL_DAMAGE,
        ),
        (
            &vanilla_damage_types::LAVA,
            &vanilla_game_rules::FIRE_DAMAGE,
        ),
        (
            &vanilla_damage_types::FREEZE,
            &vanilla_game_rules::FREEZE_DAMAGE,
        ),
    ];

    for (damage_type, rule) in cases {
        let source = DamageSource::environment(damage_type);
        let mapped = Player::disabled_damage_game_rule(&source);
        assert!(mapped.is_some_and(|mapped| mapped.key() == rule.key()));
    }
}

#[test]
fn disabled_damage_game_rule_ignores_unrelated_damage() {
    init_test_registry();
    let source = DamageSource::environment(&vanilla_damage_types::GENERIC);

    assert!(Player::disabled_damage_game_rule(&source).is_none());
}

#[test]
fn hurt_uses_explicit_world_difficulty() {
    let attached_world = Arc::clone(test_world());
    let damage_world = hard_damage_test_world();
    let player = test_player(attached_world);
    let source = DamageSource::environment(&vanilla_damage_types::EXPLOSION);

    assert_eq!(player.get_world().difficulty(), Difficulty::Normal);
    assert_eq!(damage_world.difficulty(), Difficulty::Hard);
    assert_eq!(player.get_health().to_bits(), 20.0_f32.to_bits());

    assert!(player.hurt(damage_world, &source, 4.0));
    assert_eq!(player.get_health().to_bits(), 14.0_f32.to_bits());
}

#[test]
fn conditional_damage_does_not_scale_for_player_or_unresolved_causes() {
    let world = hard_damage_test_world();
    let causing_player = test_player(Arc::clone(world));
    let source = DamageSource::environment(&vanilla_damage_types::FIREWORKS);

    assert!(!source.scales_with_difficulty(Some(causing_player.as_ref())));

    let target = test_player(Arc::clone(world));
    let unresolved_source = source.with_causing_entity(2);
    assert!(target.hurt(world, &unresolved_source, 4.0));
    assert_eq!(target.get_health().to_bits(), 16.0_f32.to_bits());
}

#[test]
fn player_damage_applies_armor_and_absorption() {
    init_test_registry();
    let world = Arc::clone(test_world());
    let player = test_player(Arc::clone(&world));
    {
        let mut attributes = player.attributes().lock();
        attributes.set_base_value(vanilla_attributes::ARMOR, 20.0);
        attributes.set_base_value(vanilla_attributes::MAX_ABSORPTION, 3.0);
    }
    player.set_absorption_amount(3.0);
    let source = DamageSource::environment(&vanilla_damage_types::FIREWORKS);

    assert!(player.hurt(&world, &source, 10.0));

    assert_eq!(player.get_health().to_bits(), 19.0_f32.to_bits());
    assert_eq!(player.get_absorption_amount().to_bits(), 0.0_f32.to_bits());
}

#[test]
fn player_absorption_amount_clamps_to_attribute_range() {
    let world = Arc::clone(test_world());
    let player = test_player(world);
    player
        .attributes()
        .lock()
        .set_base_value(vanilla_attributes::MAX_ABSORPTION, 4.0);

    player.set_absorption_amount(10.0);
    assert_eq!(player.get_absorption_amount().to_bits(), 4.0_f32.to_bits());

    player.set_absorption_amount(-1.0);
    assert_eq!(player.get_absorption_amount().to_bits(), 0.0_f32.to_bits());
}

#[test]
fn player_damage_hurts_armor_equipment() {
    init_test_registry();
    let world = Arc::clone(test_world());
    let player = test_player(Arc::clone(&world));
    player.inventory.lock().equipment_mut().set(
        EquipmentSlot::Chest,
        ItemStack::new(&vanilla_items::DIAMOND_CHESTPLATE),
    );
    let source = DamageSource::environment(&vanilla_damage_types::FIREWORKS);

    assert!(player.hurt(&world, &source, 8.0));

    let inventory = player.inventory.lock();
    assert_eq!(
        inventory
            .equipment()
            .get_ref(EquipmentSlot::Chest)
            .get_damage_value(),
        2,
    );
}

#[test]
fn nullable_game_mode_id_matches_vanilla_encoding() {
    assert_eq!(nullable_game_mode_id(None), -1);
    assert_eq!(nullable_game_mode_id(Some(GameType::Creative)), 1);
}

#[test]
fn clear_matching_items_uses_inventory_crafting_then_carried_order() {
    init_test_registry();
    let player = test_player(Arc::clone(test_world()));
    player
        .inventory
        .lock()
        .set_item(0, ItemStack::with_count(&vanilla_items::STONE, 3));
    {
        let inventory_menu = player.inventory_menu.lock();
        inventory_menu
            .crafting_container()
            .lock()
            .set_item(0, ItemStack::with_count(&vanilla_items::STONE, 2));
    }
    player
        .inventory_menu
        .lock()
        .behavior_mut()
        .set_carried(ItemStack::with_count(&vanilla_items::STONE, 4));

    let stone = |stack: &ItemStack| stack.is(&vanilla_items::STONE);
    assert_eq!(player.clear_or_count_matching_items(&stone, 5), 5);
    assert!(player.inventory.lock().get_item(0).is_empty());
    assert!(
        player
            .inventory_menu
            .lock()
            .crafting_container()
            .lock()
            .get_item(0)
            .is_empty()
    );
    assert_eq!(
        player
            .inventory_menu
            .lock()
            .behavior()
            .get_carried()
            .count(),
        4
    );

    assert_eq!(player.clear_or_count_matching_items(&stone, 0), 4);
    assert_eq!(
        player
            .inventory_menu
            .lock()
            .behavior()
            .get_carried()
            .count(),
        4
    );
    assert_eq!(player.clear_or_count_matching_items(&stone, -1), 4);
    assert!(
        player
            .inventory_menu
            .lock()
            .behavior()
            .get_carried()
            .is_empty()
    );
}

#[test]
fn point_level_up_sound_uses_first_crossed_five_level_boundary() {
    assert_eq!(first_point_level_up_sound(0, 4, 100), None);
    assert_eq!(first_point_level_up_sound(0, 5, 100), Some(5));
    assert_eq!(first_point_level_up_sound(4, 12, 100), Some(5));
    assert_eq!(first_point_level_up_sound(5, 10, 100), Some(10));
    assert_eq!(first_point_level_up_sound(5, 10, -100), None);
}

#[test]
fn point_grants_update_entity_score_with_java_wrapping() {
    let player = test_player(Arc::clone(test_world()));
    player.set_score(i32::MAX - 10);

    player.give_experience_points(100);

    assert_eq!(player.score(), (i32::MAX - 10).wrapping_add(100));
    assert_eq!(player.experience.lock().total_points(), 100);
}

#[test]
fn persistent_player_data_restores_independent_experience_fields_and_score() {
    init_test_registry();
    let player = test_player(Arc::clone(test_world()));
    *player.experience.lock() = Experience::from_parts(7, 0.5, 32);
    player.set_score(19);
    let persistent = PersistentPlayerData::from_player(&player);

    *player.experience.lock() = Experience::default();
    player.set_score(-1);
    persistent.apply_to_player_without_location(&player);

    let experience = player.experience.lock();
    assert_eq!(experience.level(), 7);
    assert_eq!(experience.progress().to_bits(), 0.5_f32.to_bits());
    assert_eq!(experience.total_points(), 32);
    drop(experience);
    assert_eq!(player.score(), 19);
}

#[test]
fn effect_visibility_refresh_preserves_spectator_invisibility() {
    init_test_registry();
    let player = test_player(Arc::clone(test_world()));

    player.restore_game_modes(GameType::Spectator, Some(GameType::Survival));
    player.living_base.mark_effects_dirty();
    player.update_dirty_mob_effect_entity_data();
    assert!(player.entity_data.is_base_invisible_flag());

    player.restore_game_modes(GameType::Survival, Some(GameType::Spectator));
    player.living_base.mark_effects_dirty();
    player.update_dirty_mob_effect_entity_data();
    assert!(!player.entity_data.is_base_invisible_flag());
}

#[test]
fn block_action_restriction_precedes_redstone_ore_attack() {
    init_test_registry();
    init_behaviors();
    let world = fresh_test_world("redstone_ore_block_action_restriction");
    let pos = BlockPos::new(1, 64, 0);
    insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
    assert!(world.set_block(
        pos,
        vanilla_blocks::REDSTONE_ORE.default_state(),
        UpdateFlags::UPDATE_ALL,
    ));

    let player = test_player(Arc::clone(&world));
    player.base.set_position_local(DVec3::new(1.0, 64.0, 0.0));

    for game_mode in [GameType::Spectator, GameType::Adventure] {
        player.restore_game_modes(game_mode, None);
        player.abilities.lock().update_for_game_mode(game_mode);
        player.block_breaking.lock().handle_block_break_action(
            &player,
            &world,
            pos,
            BlockBreakAction::Start,
            Direction::Up,
        );
        assert!(
            !world
                .get_block_state(pos)
                .get_value(&BlockStateProperties::LIT)
        );
    }

    let predicate = BlockPredicate::new(
        Some(RegistryHolderSet::Direct(vec![
            &vanilla_blocks::REDSTONE_ORE,
        ])),
        None,
        None,
        DataComponentMatchers::ANY,
    );
    let can_break =
        AdventureModePredicate::new(vec![predicate]).expect("one block predicate is valid");
    let mut tool = ItemStack::new(&vanilla_items::DIAMOND_PICKAXE);
    tool.set(CAN_BREAK, can_break);
    player.inventory.lock().set_selected_item(tool);

    player.block_breaking.lock().handle_block_break_action(
        &player,
        &world,
        pos,
        BlockBreakAction::Start,
        Direction::Up,
    );
    assert!(
        world
            .get_block_state(pos)
            .get_value(&BlockStateProperties::LIT)
    );
}
