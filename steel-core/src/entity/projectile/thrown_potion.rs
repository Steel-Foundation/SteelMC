//! Shared splash/lingering potion projectile logic. Concrete potions ([`SplashPotionEntity`](crate::entity::entities::SplashPotionEntity))
//! implement [`AbstractThrownPotion::on_hit_as_potion`] for their distinct
//! area-of-effect behavior; everything else (gravity, the water-splash branch,
//! block-hit fire dowsing, the hurt-knockback direction, and the break
//! level-event) is shared here.

use std::sync::Arc;

use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::data_components::{PotionContents, vanilla_components};
use steel_registry::item_stack::ItemStack;
use steel_registry::potion::Potion;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::{level_events, vanilla_damage_types, vanilla_potions};
use steel_utils::{BlockPos, BlockStateId, Direction, types::UpdateFlags};

use crate::behavior::MOB_EFFECT_BEHAVIORS;
use crate::behavior::blocks::{CampfireBlock, CandleBlock};
use crate::entity::damage::DamageSource;
use crate::entity::projectile::ThrowableItemProjectile;
use crate::entity::{LivingEntity, ProjectileHit, RemovalReason, SharedEntity};
use crate::world::ClipHitResult;
use crate::world::World;

/// Squared distance within which a splash reaches an entity.
pub const SPLASH_RANGE_SQ: f64 = 16.0;
/// Gravity of a thrown potion.
const DEFAULT_GRAVITY: f64 = 0.05;

/// Vanilla-shaped behavior shared by `ThrownSplashPotion` and `ThrownLingeringPotion`.
pub trait AbstractThrownPotion: ThrowableItemProjectile {
    /// Vanilla `AbstractThrownPotion.getDefaultGravity()` (0.05, overriding the
    /// throwable-projectile default of 0.03).
    fn thrown_potion_default_gravity(&self) -> f64 {
        DEFAULT_GRAVITY
    }

    /// The effect-specific area-of-effect (splash) or cloud-spawning (lingering) behavior.
    fn on_hit_as_potion(&self, world: &Arc<World>, potion_item: &ItemStack, hit: &ProjectileHit);

    /// Vanilla `AbstractThrownPotion.calculateHorizontalHurtKnockbackDirection`:
    /// knock the hurt entity radially away from the potion's impact point rather
    /// than along the potion's flight direction (the `Projectile` default).
    fn thrown_potion_knockback_direction(&self, hurt_entity: &dyn LivingEntity) -> (f64, f64) {
        let delta = hurt_entity.position() - self.position();
        (delta.x, delta.z)
    }

    /// Vanilla `AbstractThrownPotion.onHit`.
    fn thrown_potion_on_hit(&self, hit: &ProjectileHit) {
        self.projectile_on_hit(hit);
        let Some(world) = self.level() else {
            return;
        };

        let potion_item = self.get_item();
        let contents = potion_item
            .get_or_default(vanilla_components::POTION_CONTENTS, PotionContents::empty());
        if contents.is(&vanilla_potions::WATER) {
            self.on_hit_as_water(&world);
        } else if contents.has_effects() {
            self.on_hit_as_potion(&world, &potion_item, hit);
        }

        let has_instant_effects = contents
            .potion()
            .is_some_and(|potion| potion_has_instant_effects(potion.value()));
        let event_type = if has_instant_effects {
            level_events::PARTICLES_INSTANT_POTION_SPLASH
        } else {
            level_events::PARTICLES_SPELL_POTION_SPLASH
        };
        world.level_event(
            event_type,
            self.block_position(),
            contents.get_color(),
            None,
        );
        self.set_removed(RemovalReason::Discarded);
    }

    /// Splashes water: hurts water-sensitive entities in range and
    /// extinguishes burning ones.
    fn on_hit_as_water(&self, world: &Arc<World>) {
        let aabb = self.bounding_box().inflate_xyz(4.0, 2.0, 4.0);
        let potion_pos = self.position();

        for entity in world.get_entities_in_aabb(&aabb) {
            if entity.as_living_entity().is_none() {
                continue;
            }
            let sensitive_to_water = entity.entity_type().flags.is_sensitive_to_water;
            if !sensitive_to_water && !entity.is_on_fire() {
                continue;
            }
            if potion_pos.distance_squared(entity.position()) >= SPLASH_RANGE_SQ {
                continue;
            }

            if sensitive_to_water {
                let mut damage = DamageSource::environment(&vanilla_damage_types::INDIRECT_MAGIC)
                    .with_direct_entity(self.id());
                if let Some(owner) = self.get_owner() {
                    damage = damage.with_causing_entity(owner.id());
                }
                entity.hurt(world, &damage, 1.0);
            }

            if entity.is_on_fire() && entity.is_alive() {
                entity.extinguish_fire();
            }
        }

        // TODO: rehydrate nearby Axolotl entities once Axolotl is implemented
        // (vanilla `AbstractThrownPotion.onHitAsWater` also calls `Axolotl.rehydrate()`).
    }

    /// Vanilla `AbstractThrownPotion.onHitBlock`.
    fn thrown_potion_on_hit_block(&self, hit: &ClipHitResult) {
        self.projectile_on_hit_block(hit);
        let Some(world) = self.level() else {
            return;
        };

        let contents = self
            .get_item()
            .get_or_default(vanilla_components::POTION_CONTENTS, PotionContents::empty());
        if !contents.is(&vanilla_potions::WATER) {
            return;
        }

        let block_effect_pos = hit.direction.relative(hit.block_pos);
        self.dowse_fire(&world, block_effect_pos);
        self.dowse_fire(&world, hit.direction.opposite().relative(block_effect_pos));
        for direction in Direction::HORIZONTAL {
            self.dowse_fire(&world, direction.relative(block_effect_pos));
        }
    }

    /// Puts out fire, lit campfires and lit candles at `pos`.
    fn dowse_fire(&self, world: &Arc<World>, pos: BlockPos) {
        let state = world.get_block_state(pos);
        if state.get_block().has_tag(&BlockTag::FIRE) {
            world.destroy_block_by_entity(pos, false, self.as_entity_event_source());
        } else if CandleBlock::is_lit(state) {
            CandleBlock::extinguish(None, state, world, pos);
        } else if CampfireBlock::is_lit_campfire(state) {
            dowse_lit_campfire(world, pos, state, self.get_owner());
        }
    }
}

/// Vanilla `Potion.hasInstantEffects()`: true when any of the *base* potion's
/// own effects (not the item stack's `custom_effects`) is instantaneous.
fn potion_has_instant_effects(potion: &Potion) -> bool {
    potion.effects.iter().any(|effect| {
        MOB_EFFECT_BEHAVIORS
            .get_behavior(effect.effect)
            .as_instantaneous()
            .is_some()
    })
}

/// Vanilla `AbstractThrownPotion.dowseFire`'s campfire branch: the level event,
/// `CampfireBlock.dowse`, then unsetting `LIT`.
fn dowse_lit_campfire(
    world: &Arc<World>,
    pos: BlockPos,
    state: BlockStateId,
    owner: Option<SharedEntity>,
) {
    world.level_event(level_events::SOUND_EXTINGUISH_FIRE, pos, 0, None);
    CampfireBlock::dowse(owner.as_deref(), world, pos);
    world.set_block(
        pos,
        state.set_value(&BlockStateProperties::LIT, false),
        UpdateFlags::UPDATE_ALL,
    );
}
