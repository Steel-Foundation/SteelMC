//! Projectile entity implementations.

mod arrow;
mod ender_pearl;
mod eye_of_ender;
mod firework_rocket;
mod fishing_hook;
mod lingering_potion;
mod snowball;
mod spectral_arrow;
mod splash_potion;
mod thrown_egg;
mod thrown_trident;

pub use arrow::ArrowEntity;
pub use ender_pearl::EnderPearlEntity;
pub use eye_of_ender::EyeOfEnderEntity;
pub use firework_rocket::FireworkRocketEntity;
pub use fishing_hook::{FishingHookEntity, FishingHookState};
pub use lingering_potion::LingeringPotionEntity;
pub use snowball::SnowballEntity;
pub use spectral_arrow::SpectralArrowEntity;
pub use splash_potion::SplashPotionEntity;
pub use thrown_egg::ThrownEggEntity;
pub use thrown_trident::ThrownTridentEntity;
