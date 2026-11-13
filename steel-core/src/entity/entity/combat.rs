//! Explosions, damage, kills and projectile hits.

use super::*;

/// Explosions, damage, kills and projectile hits.
///
/// Implemented for every [`Entity`]; entity types change behavior through
/// [`EntityProps`], never by implementing this trait.
pub trait EntityCombat: Entity {
    /// Default for [`EntityHooks::attackable`].
    fn attackable_default(&self) -> bool {
        true
    }

    /// Default for [`EntityHooks::skip_attack_interaction`].
    fn skip_attack_interaction_default(&self, _source: &SharedEntity) -> bool {
        false
    }

    /// Default for [`EntityHooks::explosion_indirect_source`].
    fn explosion_indirect_source_default(&self) -> Option<SharedEntity> {
        self.projectile_owner()
            .filter(|owner| owner.as_living_entity().is_some())
    }

    /// Default for [`EntityHooks::explosion_damage_origin`].
    fn explosion_damage_origin_default(&self) -> DVec3 {
        let position = self.position();
        DVec3::new(position.x, self.eye_y(), position.z)
    }

    /// Default for [`EntityHooks::ignore_explosion`].
    fn ignore_explosion_default(&self, _explosion: &dyn Explosion) -> bool {
        false
    }

    /// Default for [`EntityHooks::block_explosion_resistance`].
    #[expect(
        unused_variables,
        reason = "default implementation preserves resistance"
    )]
    fn block_explosion_resistance_default(
        &self,
        explosion: &dyn Explosion,
        world: &World,
        pos: BlockPos,
        state: BlockStateId,
        fluid: FluidState,
        resistance: f32,
    ) -> f32 {
        resistance
    }

    /// Default for [`EntityHooks::should_block_explode`].
    #[expect(unused_variables, reason = "default implementation allows destruction")]
    fn should_block_explode_default(
        &self,
        explosion: &dyn Explosion,
        world: &World,
        pos: BlockPos,
        state: BlockStateId,
        power: f32,
    ) -> bool {
        true
    }

    /// Default for [`EntityHooks::on_explosion_hit`].
    fn on_explosion_hit_default(&self, _explosion_source: Option<&dyn Entity>) {}

    /// Default for [`EntityHooks::can_be_hit_by_projectile`].
    fn can_be_hit_by_projectile_default(&self) -> bool {
        !self.is_removed() && self.is_pickable()
    }

    /// Returns this entity's Vanilla projectile deflection behavior.
    fn deflection(&self, _projectile: &Projectile<'_>) -> ProjectileDeflection {
        if REGISTRY
            .entity_types
            .is_in_tag(self.entity_type(), &EntityTypeTag::DEFLECTS_PROJECTILES)
        {
            ProjectileDeflection::Reverse
        } else {
            ProjectileDeflection::None
        }
    }

    /// Default for [`EntityHooks::kill`].
    fn kill_default(&self, _world: &World) {
        self.set_removed(RemovalReason::Killed);
        self.game_event(&vanilla_game_events::ENTITY_DIE);
    }

    /// Returns whether this entity is immune to damage from `source`: already
    /// removed, or invulnerable and the source doesn't bypass invulnerability.
    fn is_invulnerable_to_base(&self, source: &DamageSource) -> bool {
        self.is_removed()
            || self.is_invulnerable()
                && !source.bypasses_invulnerability()
                && !source.is_creative_player()
            || source.is(&vanilla_damage_type_tags::DamageTypeTag::IS_FIRE) && self.fire_immune()
            || source.is(&vanilla_damage_type_tags::DamageTypeTag::IS_FALL)
                && self.is_fall_damage_immune()
    }

    /// Default for [`EntityHooks::killed_entity`].
    fn killed_entity_default(
        &self,
        _world: &World,
        _entity: &Living<'_>,
        _source: &DamageSource,
    ) -> bool {
        true
    }

    /// Default for [`EntityHooks::award_kill_score`].
    fn award_kill_score_default(&self, _victim: &dyn Entity, _killing_blow: &DamageSource) {
        // TODO: Trigger advancement criterion ENTITY_KILLED_PLAYER if the victim is a player.
    }
}

impl<T: Entity + ?Sized> EntityCombat for T {}
