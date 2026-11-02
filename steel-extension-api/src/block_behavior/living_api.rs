//! Living-entity operations on borrowed entities in block and fluid callbacks.
//!
//! A separate host module from `values_api`, so adding these bindings leaves the
//! existing ones unchanged.

#[stabby::host_api]
pub mod api {
    use crate::block_behavior::types::Entity;
    use crate::effects::Effect;
    use crate::players::Uuid;
    use crate::{ApiResult, Position};
    use stabby::str::Str;
    #[cfg(feature = "host")]
    use stabby::string::String as ApiString;
    #[cfg(feature = "host")]
    use steel_core::entity::damage::DamageSource;
    #[cfg(feature = "host")]
    use steel_registry::{REGISTRY, RegistryExt as _};
    #[cfg(feature = "host")]
    use steel_utils::{Identifier, types::GameType};

    impl<'a> Entity<'a> {
        /// Whether this entity is alive and can take damage and effects (mobs and players).
        #[must_use]
        pub fn is_living(&self) -> bool {
            self.host().as_living_entity().is_some()
        }

        /// Whether this is a player in creative or spectator mode.
        #[must_use]
        pub fn is_creative_or_spectator(&self) -> bool {
            self.host().as_player().is_some_and(|player| {
                matches!(player.game_mode(), GameType::Creative | GameType::Spectator)
            })
        }

        /// The entity's position in its world.
        #[must_use]
        pub fn position(&self) -> Position {
            let position = self.host().position();
            Position {
                x: position.x,
                y: position.y,
                z: position.z,
            }
        }

        /// The entity's persistent UUID.
        #[must_use]
        pub fn uuid(&self) -> Uuid {
            Uuid {
                bytes: *self.host().uuid().as_bytes(),
            }
        }

        /// Damages the entity with an environmental source of `damage_type`, such as
        /// `minecraft:magic`. Vanilla armor, immunity, and invulnerability rules
        /// apply; returns whether damage was dealt.
        pub fn hurt(&self, damage_type: Str<'_>, amount: f32) -> ApiResult<bool> {
            hurt(*self.host(), damage_type.as_str(), amount)
                .map_err(ApiString::from)
                .into()
        }

        /// Adds a status effect. Returns `false` when the entity is not living or is
        /// immune to the effect, as vanilla undead are to poison.
        pub fn add_effect(&self, effect: Effect) -> ApiResult<bool> {
            add_effect(*self.host(), &effect)
                .map_err(ApiString::from)
                .into()
        }
    }

    #[cfg(feature = "host")]
    fn hurt(
        entity: &dyn steel_core::entity::Entity,
        damage_type: &str,
        amount: f32,
    ) -> Result<bool, String> {
        if !amount.is_finite() || amount < 0.0 {
            return Err(format!(
                "damage {amount} must be a finite, non-negative number"
            ));
        }
        let key: Identifier = damage_type
            .parse()
            .map_err(|error: &str| error.to_owned())?;
        let damage_type = REGISTRY
            .damage_types
            .by_key(&key)
            .ok_or_else(|| format!("unknown damage type {key}"))?;
        let world = entity.level().ok_or("the entity is not in a world")?;
        Ok(entity.hurt(&world, &DamageSource::environment(damage_type), amount))
    }

    #[cfg(feature = "host")]
    fn add_effect(
        entity: &dyn steel_core::entity::Entity,
        effect: &Effect,
    ) -> Result<bool, String> {
        let instance = crate::host::effects::mob_effect_instance(effect)?;
        Ok(entity
            .as_living_entity()
            .is_some_and(|living| living.add_mob_effect(instance)))
    }
}
