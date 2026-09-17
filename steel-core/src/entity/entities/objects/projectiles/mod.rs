//! Projectile entity implementations.

mod ender_pearl;
mod eye_of_ender;
mod firework_rocket;
mod fishing_hook;
mod snowball;
mod thrown_egg;

pub use ender_pearl::EnderPearlEntity;
pub use eye_of_ender::EyeOfEnderEntity;
pub use firework_rocket::FireworkRocketEntity;
pub use fishing_hook::{FishingHookEntity, FishingHookState};
pub use snowball::SnowballEntity;
pub use thrown_egg::ThrownEggEntity;
