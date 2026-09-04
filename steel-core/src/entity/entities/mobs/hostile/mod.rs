//! Hostile entity implementations.

use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_items;

use crate::entity::LivingEntityBase;
use crate::entity::ai::goal::{GoalSelector, RangedBowAttackGoal};
use crate::inventory::equipment::EquipmentSlot;

/// Gives a skeleton-type mob a held bow and the ranged-bow-attack goal.
/// Takes the already-locked goal selector: the goal selector is a
/// non-reentrant `parking_lot::Mutex` and callers hold its lock.
pub(crate) fn apply_bow_ai(living_base: &LivingEntityBase, goal_selector: &mut GoalSelector) {
    let _ = living_base
        .equipment()
        .lock()
        .set(EquipmentSlot::MainHand, ItemStack::new(&vanilla_items::BOW));
    goal_selector.add_goal(2, RangedBowAttackGoal::new(1.0, 20));
}

pub mod bat;
pub mod blaze;
pub mod bogged;
pub mod breeze;
pub mod cave_spider;
pub mod creaking;
pub mod creeper;
pub mod elder_guardian;
pub mod enderman;
/// The endermite module.
pub mod endermite;
pub mod evoker;
pub mod ghast;
pub mod giant;
pub mod guardian;
pub mod hoglin;
pub mod illusioner;
pub mod magma_cube;
pub mod parched;
pub mod phantom;
pub mod piglin;
pub mod piglin_brute;
pub mod pillager;
pub mod ravager;
pub mod shulker;
pub mod silverfish;
pub mod skeleton;
pub mod slime;
pub mod spider;
pub mod stray;
pub mod sulfur_cube;
pub mod vex;
pub mod vindicator;
pub mod warden;
pub mod witch;
pub mod wither_skeleton;
pub mod zoglin;
pub mod zombie;
pub mod zombified_piglin;

pub use bat::BatEntity;
pub use blaze::BlazeEntity;
pub use bogged::BoggedEntity;
pub use breeze::BreezeEntity;
pub use cave_spider::CaveSpiderEntity;
pub use creaking::CreakingEntity;
pub use creeper::CreeperEntity;
pub use elder_guardian::ElderGuardianEntity;
pub use enderman::EndermanEntity;
pub use endermite::EndermiteEntity;
pub use evoker::EvokerEntity;
pub use ghast::GhastEntity;
pub use giant::GiantEntity;
pub use guardian::GuardianEntity;
pub use hoglin::HoglinEntity;
pub use illusioner::IllusionerEntity;
pub use magma_cube::MagmaCubeEntity;
pub use parched::ParchedEntity;
pub use phantom::PhantomEntity;
pub use piglin::PiglinEntity;
pub use piglin_brute::PiglinBruteEntity;
pub use pillager::PillagerEntity;
pub use ravager::RavagerEntity;
pub use shulker::ShulkerEntity;
pub use silverfish::SilverfishEntity;
pub use skeleton::SkeletonEntity;
pub use slime::SlimeEntity;
pub use spider::SpiderEntity;
pub use stray::StrayEntity;
pub use sulfur_cube::SulfurCubeEntity;
pub use vex::VexEntity;
pub use vindicator::VindicatorEntity;
pub use warden::WardenEntity;
pub use witch::WitchEntity;
pub use wither_skeleton::WitherSkeletonEntity;
pub use zoglin::ZoglinEntity;
pub use zombie::{DrownedEntity, HuskEntity, ZombieEntity, ZombieVillagerEntity};
pub use zombified_piglin::ZombifiedPiglinEntity;
