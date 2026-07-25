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
