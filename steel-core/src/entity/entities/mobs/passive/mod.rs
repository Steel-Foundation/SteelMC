//! Passive entity implementations.

use crate::behavior::InteractionResult;
use crate::entity::{Entity, LivingEntity, SharedEntity};
use crate::player::Player;
use steel_utils::types::InteractionHand;

/// Vanilla `AbstractHorse`-style mounting: right-clicking an adult
/// horse-family mob (horse, donkey, mule, camel, ...) with an empty hand
/// mounts the player. (Taming/breeding is not implemented yet, so any
/// adult is mountable; sneak + interact still dismounts first.)
pub(crate) fn mob_interact_mountable(
    entity: &(impl Entity + LivingEntity),
    player: &Player,
    _hand: InteractionHand,
) -> InteractionResult {
    if LivingEntity::is_baby(entity) || player.is_secondary_use_active() {
        return InteractionResult::Pass;
    }
    if let Some(world) = entity.level()
        && let Some(vehicle) = world.get_entity_by_id(entity.id())
        && !entity.is_vehicle()
        && player.start_riding(&vehicle)
    {
        return InteractionResult::Success;
    }
    InteractionResult::Pass
}

pub mod allay;
pub mod armadillo;
pub mod axolotl;
pub mod bee;
pub mod camel;
pub mod cat;
/// Those mobs are passive creatures that run away when attacked by a player.
pub mod chicken;
pub mod cod;
pub mod copper_golem;
pub mod cow;
pub mod dolphin;
pub mod donkey;
pub mod fox;
pub mod frog;
pub mod glow_squid;
pub mod goat;
pub mod happy_ghast;
pub mod horse;
pub mod iron_golem;
pub mod llama;
pub mod mooshroom;
pub mod mule;
pub mod nautilus;
pub mod ocelot;
pub mod panda;
pub mod parrot;
pub mod pig;
pub mod polar_bear;
pub mod pufferfish;
pub mod rabbit;
pub mod salmon;
pub mod sheep;
pub mod skeleton_horse;
pub mod sniffer;
pub mod snow_golem;
pub mod squid;
pub mod strider;
pub mod tadpole;
pub mod trader_llama;
pub mod tropical_fish;
pub mod turtle;
pub mod villager;
pub mod wandering_trader;
pub mod wolf;
pub mod zombie_horse;

pub use allay::AllayEntity;
pub use armadillo::ArmadilloEntity;
pub use axolotl::AxolotlEntity;
pub use bee::BeeEntity;
pub use camel::CamelEntity;
pub use cat::CatEntity;
pub use chicken::ChickenEntity;
pub use cod::CodEntity;
pub use copper_golem::CopperGolemEntity;
pub use cow::CowEntity;
pub use dolphin::DolphinEntity;
pub use donkey::DonkeyEntity;
pub use fox::FoxEntity;
pub use frog::FrogEntity;
pub use glow_squid::GlowSquidEntity;
pub use goat::GoatEntity;
pub use happy_ghast::HappyGhastEntity;
pub use horse::HorseEntity;
pub use iron_golem::IronGolemEntity;
pub use llama::LlamaEntity;
pub use mooshroom::{MooshroomEntity, MooshroomVariant};
pub use mule::MuleEntity;
pub use nautilus::NautilusEntity;
pub use ocelot::OcelotEntity;
pub use panda::PandaEntity;
pub use parrot::ParrotEntity;
pub use pig::PigEntity;
pub use polar_bear::PolarBearEntity;
pub use pufferfish::PufferfishEntity;
pub use rabbit::{RabbitEntity, RabbitVariant};
pub use salmon::SalmonEntity;
pub use sheep::SheepEntity;
pub use skeleton_horse::SkeletonHorseEntity;
pub use sniffer::SnifferEntity;
pub use snow_golem::SnowGolemEntity;
pub use squid::SquidEntity;
pub use strider::StriderEntity;
pub use tadpole::TadpoleEntity;
pub use trader_llama::TraderLlamaEntity;
pub use tropical_fish::TropicalFishEntity;
pub use turtle::TurtleEntity;
pub use villager::VillagerEntity;
pub use wandering_trader::WanderingTraderEntity;
pub use wolf::WolfEntity;
pub use zombie_horse::ZombieHorseEntity;

/// Vanilla `AbstractHorse.getControllingPassenger`: the first passenger steers
/// the horse-family mob when it is a player and the mount is saddled.
pub(crate) fn controlling_passenger_mountable(
    entity: &impl Entity,
    saddled: bool,
) -> Option<SharedEntity> {
    if !saddled {
        return None;
    }
    let passenger = entity.first_passenger()?;
    passenger.as_player().is_some().then_some(passenger)
}
