use std::sync::Weak;

use glam::DVec3;
use steel_registry::{
    init_vanilla_registry, vanilla_entities, vanilla_mob_effects, vanilla_villager_professions,
};
use uuid::Uuid;

use super::*;
use crate::behavior::init_behaviors;
use crate::entity::ai::goal::Goal;
use crate::entity::{LivingEntity, PathfinderMob, next_entity_id};

fn farmer_villager() -> VillagerEntity {
    init_vanilla_registry();
    init_behaviors();
    let villager = VillagerEntity::new(
        &vanilla_entities::VILLAGER,
        next_entity_id(),
        DVec3::ZERO,
        Weak::new(),
    );
    villager.set_profession(&vanilla_villager_professions::FARMER);
    villager
}

#[test]
fn claiming_a_profession_rolls_only_novice_offers() {
    let villager = farmer_villager();
    assert_eq!(villager.villager_level(), 1);
    assert_eq!(villager.villager_xp(), 0);
    assert_eq!(villager.offers.lock().len(), 2);
}

#[test]
fn trading_enough_xp_levels_up_after_the_gui_closes() {
    let villager = farmer_villager();
    let novice_count = villager.offers.lock().len();

    villager.reward_trade_xp(10);
    assert_eq!(villager.villager_xp(), 10);
    assert_eq!(villager.villager_level(), 1);

    villager.set_trading_player(Some(Uuid::nil()));
    for _ in 0..50 {
        villager.tick_merchant_career();
    }
    assert_eq!(
        villager.villager_level(),
        1,
        "vanilla only levels up after the trading screen closes"
    );
    assert_eq!(villager.offers.lock().len(), novice_count);

    villager.stop_trading();
    for _ in 0..40 {
        villager.tick_merchant_career();
    }

    assert_eq!(villager.villager_level(), 2);
    assert!(
        villager.offers.lock().len() > novice_count,
        "level-up should append the next career tier instead of replacing novice trades"
    );
    assert!(villager.has_mob_effect(vanilla_mob_effects::REGENERATION));
}

#[test]
fn career_xp_blocks_profession_reset() {
    let villager = farmer_villager();
    let farmer_id = villager.profession_id();
    villager.reward_trade_xp(1);

    let mut goal = super::job_site::ResetProfessionGoal;
    assert!(!goal.can_use(&villager as &dyn PathfinderMob));
    assert_eq!(villager.profession_id(), farmer_id);
}
