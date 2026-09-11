//! Projectile entity implementations.

mod ender_pearl;
mod experience_bottle;
mod eye_of_ender;
mod firework_rocket;
mod fishing_hook;
mod snowball;
mod thrown_egg;

pub use ender_pearl::EnderPearlEntity;
pub use experience_bottle::ThrownExperienceBottleEntity;
pub use eye_of_ender::EyeOfEnderEntity;
pub use firework_rocket::FireworkRocketEntity;
pub use fishing_hook::{FishingHookEntity, FishingHookState};
pub use snowball::SnowballEntity;
pub use thrown_egg::ThrownEggEntity;
