use steel_registry::vanilla_entities;

use super::{Player, PlayerGameModeState};
use steel_utils::types::GameType;

#[test]
fn invalid_attack_targets_include_xp_orbs() {
    assert!(Player::is_invalid_attack_target(
        1,
        1,
        &vanilla_entities::PLAYER
    ));
    assert!(Player::is_invalid_attack_target(
        1,
        2,
        &vanilla_entities::ITEM
    ));
    assert!(Player::is_invalid_attack_target(
        1,
        2,
        &vanilla_entities::EXPERIENCE_ORB
    ));
    assert!(!Player::is_invalid_attack_target(
        1,
        2,
        &vanilla_entities::PIG
    ));
}

#[test]
fn changing_current_records_previous_mode() {
    let mut state = PlayerGameModeState::new(GameType::Survival);

    assert!(state.change_current(GameType::Creative));

    assert_eq!(state.current(), GameType::Creative);
    assert_eq!(state.previous(), Some(GameType::Survival));
}

#[test]
fn setting_same_mode_keeps_previous_mode() {
    let mut state = PlayerGameModeState::new(GameType::Survival);
    state.change_current(GameType::Creative);

    assert!(!state.change_current(GameType::Creative));

    assert_eq!(state.current(), GameType::Creative);
    assert_eq!(state.previous(), Some(GameType::Survival));
}

#[test]
fn persistent_restore_sets_current_and_previous() {
    let mut state = PlayerGameModeState::new(GameType::Survival);

    state.set_pair(GameType::Adventure, Some(GameType::Creative));

    assert_eq!(state.current(), GameType::Adventure);
    assert_eq!(state.previous(), Some(GameType::Creative));
}

#[test]
fn initial_state_has_no_previous_mode() {
    let state = PlayerGameModeState::new(GameType::Survival);

    assert_eq!(state.current(), GameType::Survival);
    assert_eq!(state.previous(), None);
}

#[test]
fn destroy_progress_applies_efficiency_haste_and_the_airborne_penalty() {
    use super::block_breaking::get_destroy_progress;
    use crate::entity::{Entity as _, LivingEntity as _, MobEffectInstance};
    use crate::test_support::{TestPlayerBuilder, fresh_test_world};
    use std::sync::Arc;
    use steel_registry::{
        data_components::{components::ItemEnchantments, vanilla_components::ENCHANTMENTS},
        init_vanilla_registry,
        item_stack::ItemStack,
        vanilla_blocks, vanilla_items, vanilla_mob_effects,
    };
    use steel_utils::Identifier;

    init_vanilla_registry();
    crate::behavior::init_behaviors();
    let fixture = fresh_test_world("destroy_progress_modifiers");
    let player = TestPlayerBuilder::new(Arc::clone(&fixture.world), "Digger", 1).build();
    player.restore_game_modes(GameType::Survival, None);
    let ore = vanilla_blocks::DIAMOND_ORE.default_state();
    let hold = |efficiency: u32| {
        let mut pickaxe = ItemStack::new(&vanilla_items::DIAMOND_PICKAXE);
        if efficiency > 0 {
            let mut enchantments = ItemEnchantments::empty();
            enchantments
                .levels
                .insert(Identifier::vanilla_static("efficiency"), efficiency);
            pickaxe.set(ENCHANTMENTS, enchantments);
        }
        player.inventory.lock().set_selected_item(pickaxe);
    };

    player.base().set_on_ground(true);
    hold(0);
    let plain = get_destroy_progress(&player, ore);
    assert!(
        plain < 1.0,
        "a plain diamond pickaxe needs several ticks: {plain}"
    );
    hold(100);
    assert!(
        get_destroy_progress(&player, ore) >= 1.0,
        "Efficiency 100 is instant"
    );

    player.base().set_on_ground(false);
    assert!(
        get_destroy_progress(&player, ore) >= 1.0,
        "still instant while jumping or sprint-jumping"
    );
    hold(0);
    assert!((get_destroy_progress(&player, ore) - plain / 5.0).abs() < 1e-6);

    player.base().set_on_ground(true);
    player.add_mob_effect(MobEffectInstance::new(vanilla_mob_effects::HASTE, 1));
    assert!((get_destroy_progress(&player, ore) - plain * 1.4).abs() < 1e-5);
}
