//! Explosions, damage, kills and projectile hits.

use super::*;

/// Explosions, damage, kills and projectile hits.
///
/// Implemented for every [`Entity`]; entity types change behavior through
/// [`EntityProps`], never by implementing this trait.
pub trait EntityCombat: Entity {
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
}

impl<T: Entity + ?Sized> EntityCombat for T {}

/// `Entity`'s [`EntityProps::attackable`].
pub const fn attackable<E: Entity + ?Sized>(_entity: &E) -> bool {
    true
}

/// `Entity`'s [`EntityProps::skip_attack_interaction`].
pub fn skip_attack_interaction<E: Entity + ?Sized>(_entity: &E, _source: &SharedEntity) -> bool {
    false
}

/// `Entity`'s [`EntityProps::explosion_indirect_source`].
pub fn explosion_indirect_source<E: Entity + ?Sized>(entity: &E) -> Option<SharedEntity> {
    entity
        .projectile_owner()
        .filter(|owner| owner.as_living_entity().is_some())
}

/// `Entity`'s [`EntityProps::explosion_damage_origin`].
pub fn explosion_damage_origin<E: Entity + ?Sized>(entity: &E) -> DVec3 {
    let position = entity.position();
    DVec3::new(position.x, entity.eye_y(), position.z)
}

/// `Entity`'s [`EntityProps::ignore_explosion`].
pub fn ignore_explosion<E: Entity + ?Sized>(_entity: &E, _explosion: &dyn Explosion) -> bool {
    false
}

/// `Entity`'s [`EntityProps::block_explosion_resistance`].
#[expect(
    unused_variables,
    reason = "default implementation preserves resistance"
)]
pub fn block_explosion_resistance<E: Entity + ?Sized>(
    _entity: &E,
    explosion: &dyn Explosion,
    world: &World,
    pos: BlockPos,
    state: BlockStateId,
    fluid: FluidState,
    resistance: f32,
) -> f32 {
    resistance
}

/// `Entity`'s [`EntityProps::should_block_explode`].
#[expect(unused_variables, reason = "default implementation allows destruction")]
pub fn should_block_explode<E: Entity + ?Sized>(
    _entity: &E,
    explosion: &dyn Explosion,
    world: &World,
    pos: BlockPos,
    state: BlockStateId,
    power: f32,
) -> bool {
    true
}

/// `Entity`'s [`EntityProps::on_explosion_hit`].
pub fn on_explosion_hit<E: Entity + ?Sized>(_entity: &E, _explosion_source: Option<&dyn Entity>) {}

/// `Entity`'s [`EntityProps::can_be_hit_by_projectile`].
pub fn can_be_hit_by_projectile<E: Entity + ?Sized>(entity: &E) -> bool {
    !entity.is_removed() && entity.is_pickable()
}

/// `Entity`'s [`EntityProps::kill`].
pub fn kill<E: Entity + ?Sized>(entity: &E, _world: &World) {
    entity.set_removed(RemovalReason::Killed);
    entity.game_event(&vanilla_game_events::ENTITY_DIE);
}

/// `Entity`'s [`EntityProps::killed_entity`].
pub const fn killed_entity<E: Entity + ?Sized>(
    _entity: &E,
    _world: &World,
    _victim: &Living<'_>,
    _source: &DamageSource,
) -> bool {
    true
}

/// `Entity`'s [`EntityProps::award_kill_score`].
pub fn award_kill_score<E: Entity + ?Sized>(
    _entity: &E,
    _victim: &dyn Entity,
    _killing_blow: &DamageSource,
) {
    // TODO: Trigger advancement criterion ENTITY_KILLED_PLAYER if the victim is a player.
}
