//! Entity damage, knockback, and post-explosion callbacks.

use glam::DVec3;
use steel_registry::vanilla_entity_type_tags::EntityTypeTag;
use steel_registry::{REGISTRY, TaggedRegistryExt as _, vanilla_attributes};
use steel_utils::types::GameType;
use steel_utils::{BlockPos, WorldAabb};

use crate::behavior::BlockCollisionContext;
use crate::entity::Entity;
use crate::entity::entities::PrimedTntEntity;
use crate::world::explosion::ExplosionDamageCalculator as _;
use crate::world::raycast::ExplosionExposureRaycast;

use super::ServerExplosion;
use super::exposure::EntityExplosionExposure;

const MIN_DAMAGE_RADIUS: f32 = 1.0e-5;
const DAMAGE_RADIUS_SCALE: f32 = 2.0;
const ENTITY_QUERY_PADDING: f64 = 1.0;
const NORMALIZE_EPSILON: f64 = 1.0e-5_f32 as f64;

impl ServerExplosion<'_> {
    pub(super) fn hurt_entities(&mut self) {
        if self.radius < MIN_DAMAGE_RADIUS {
            return;
        }

        let double_radius = self.radius * DAMAGE_RADIUS_SCALE;
        let radius = f64::from(double_radius);
        let bounds = WorldAabb::from_min_max(
            DVec3::new(
                (self.center.x - radius - ENTITY_QUERY_PADDING).floor(),
                (self.center.y - radius - ENTITY_QUERY_PADDING).floor(),
                (self.center.z - radius - ENTITY_QUERY_PADDING).floor(),
            ),
            DVec3::new(
                (self.center.x + radius + ENTITY_QUERY_PADDING).floor(),
                (self.center.y + radius + ENTITY_QUERY_PADDING).floor(),
                (self.center.z + radius + ENTITY_QUERY_PADDING).floor(),
            ),
        );
        let source_id = self.source.map(Entity::id);
        let entities = self.world.get_entities_in_aabb_matching(&bounds, |entity| {
            source_id != Some(entity.id()) && !entity.is_spectator()
        });
        let redirect_owner = self.damage_source.causing_entity(self.world);
        let builtin_entity_effects = self.damage_calculator.has_builtin_entity_effects();
        let mut exposure_raycast =
            ExplosionExposureRaycast::new(self.world.as_ref(), BlockCollisionContext::empty());
        exposure_raycast.configure_clear_grid(
            BlockPos::from(bounds.min_corner()),
            BlockPos::from(bounds.max_corner()),
        );
        // The freshly constructed cache is already safe for the first target.
        let mut reusable_from_previous_tnt = true;

        for entity in entities {
            // Exact Steel PrimedTNT rejects damage, keeps the base no-op explosion callback, and
            // only accepts the impulse. With built-in entity effects, no block mutation can occur
            // between these targets, so their static exposure shapes remain current.
            let inert_primed_tnt = builtin_entity_effects
                && steel_utils::Downcast::downcast_ref::<PrimedTntEntity>(entity.as_ref())
                    .is_some();
            if !reusable_from_previous_tnt || !inert_primed_tnt {
                exposure_raycast.clear();
            }
            reusable_from_previous_tnt = inert_primed_tnt;

            if entity.ignore_explosion(self) {
                continue;
            }
            let distance = entity.position().distance(self.center) / radius;
            if distance > 1.0 {
                continue;
            }

            let delta = entity.explosion_damage_origin() - self.center;
            let delta_length = delta.length();
            let direction = if delta_length < NORMALIZE_EPSILON {
                DVec3::ZERO
            } else {
                delta / delta_length
            };
            let should_damage = self
                .damage_calculator
                .should_damage_entity(self, entity.as_ref());
            let knockback_multiplier = self.damage_calculator.knockback_multiplier(entity.as_ref());
            let exposure = if !should_damage && knockback_multiplier == 0.0 {
                0.0
            } else {
                EntityExplosionExposure::capture(entity.as_ref())
                    .calculate_cached_with(&mut exposure_raycast, self.center)
            };

            if should_damage {
                let amount =
                    self.damage_calculator
                        .entity_damage_amount(self, entity.as_ref(), exposure);
                entity.hurt(self.world, &self.damage_source, amount);
            }

            let knockback_resistance = entity.as_living_entity().map_or(0.0, |living| {
                living
                    .attributes()
                    .lock()
                    .required_value(vanilla_attributes::EXPLOSION_KNOCKBACK_RESISTANCE)
            });
            let knockback_power = (1.0 - distance)
                * f64::from(exposure)
                * f64::from(knockback_multiplier)
                * (1.0 - knockback_resistance);
            let knockback = direction * knockback_power;
            entity.push_impulse(knockback);

            if REGISTRY.entity_types.is_in_tag(
                entity.entity_type(),
                &EntityTypeTag::REDIRECTABLE_PROJECTILE,
            ) {
                if let Some(projectile) = entity.as_projectile() {
                    projectile.set_owner_entity(redirect_owner.as_ref());
                }
            } else if let Some(player) = entity.as_player()
                && !player.is_spectator()
                && (player.game_mode() != GameType::Creative || !player.abilities.lock().flying)
            {
                self.hit_players.insert(player.id(), knockback);
            }

            entity.on_explosion_hit(self.source);
        }
    }
}
