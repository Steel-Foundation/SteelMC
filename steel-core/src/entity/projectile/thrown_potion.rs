//! Shared impact and water interaction logic for thrown potions

use std::sync::Arc;

use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::data_components::{PotionContents, vanilla_components};
use steel_registry::item_stack::ItemStack;
use steel_registry::potion::Potion;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_potion_tags::PotionTag;
use steel_registry::{level_events, vanilla_damage_types};
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
/// How far the splash search box extends past the potion's box on the X and Z axes.
pub const SPLASH_HORIZONTAL_REACH: f64 = 4.0;
/// How far the splash search box extends past the potion's box on the Y axis.
pub const SPLASH_VERTICAL_REACH: f64 = 2.0;
/// Gravity of a thrown potion.
const DEFAULT_GRAVITY: f64 = 0.05;

/// Shared behavior for splash and lingering potion projectiles
pub trait AbstractThrownPotion: ThrowableItemProjectile {
    /// Returns the gravity of thrown potions (default is 0.05)
    fn thrown_potion_default_gravity(&self) -> f64 {
        DEFAULT_GRAVITY
    }

    /// The effect-specific area-of-effect (splash) or cloud-spawning (lingering) behavior.
    fn on_hit_as_potion(
        self: &Arc<Self>,
        world: &Arc<World>,
        potion_item: &ItemStack,
        hit: &ProjectileHit,
    );

    /// Returns the horizontal direction from the potion to the hurt entity
    fn thrown_potion_knockback_direction(&self, hurt_entity: &dyn LivingEntity) -> (f64, f64) {
        let delta = hurt_entity.position() - self.position();
        (delta.x, delta.z)
    }

    /// Handles impact effects emits the break event and discards the potion
    fn thrown_potion_on_hit(self: Arc<Self>, hit: &ProjectileHit)
    where
        Self: Sized + 'static,
    {
        Arc::clone(&self).projectile_on_hit(hit);
        let Some(world) = self.level() else {
            return;
        };

        let potion_item = self.get_item();
        let contents = potion_item
            .get_or_default(vanilla_components::POTION_CONTENTS, PotionContents::empty());
        self.affect_entities_around(&world, &contents);
        if contents.has_effects() {
            self.on_hit_as_potion(&world, &potion_item, hit);
        }

        let has_instant_effects = contents
            .potion()
            .is_some_and(|potion| potion_has_instant_effects(potion.value()));
        let (event_type, sound_event) = if has_instant_effects {
            (
                level_events::PARTICLES_INSTANT_POTION_SPLASH,
                level_events::SOUND_INSTANT_POTION_SPLASH,
            )
        } else {
            (
                level_events::PARTICLES_SPELL_POTION_SPLASH,
                level_events::SOUND_SPELL_POTION_SPLASH,
            )
        };
        world.level_event(
            event_type,
            self.block_position(),
            contents.get_color(),
            None,
        );
        if !self.is_silent() {
            world.level_event(sound_event, self.block_position(), 0, None);
        }
        self.set_removed(RemovalReason::Discarded);
    }

    /// Hurts water-sensitive entities in range and extinguishes burning ones,
    /// as allowed by the potion's tags.
    fn affect_entities_around(self: &Arc<Self>, world: &Arc<World>, contents: &PotionContents)
    where
        Self: Sized + 'static,
    {
        let hurts_water_sensitive = contents.is_in_tag(&PotionTag::HURTS_WATER_SENSITIVE_ENTITIES);
        let extinguishes = contents.is_in_tag(&PotionTag::EXTINGUISHES_ENTITIES);
        // TODO: rehydrate nearby Axolotl entities for `REHYDRATES_AXOLOTLS` once
        // Axolotl is implemented.
        if !hurts_water_sensitive && !extinguishes {
            return;
        }

        let aabb = self.bounding_box().inflate_xyz(
            SPLASH_HORIZONTAL_REACH,
            SPLASH_VERTICAL_REACH,
            SPLASH_HORIZONTAL_REACH,
        );
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

            if hurts_water_sensitive && sensitive_to_water {
                let mut damage = DamageSource::environment(&vanilla_damage_types::INDIRECT_MAGIC)
                    .with_direct_entity(Arc::<Self>::clone(self));
                if let Some(owner) = self.get_owner() {
                    damage = damage.with_causing_entity(owner);
                }
                entity.hurt(world, &damage, 1.0);
            }

            if extinguishes && entity.is_on_fire() && entity.is_alive() {
                entity.extinguish_fire();
            }
        }
    }

    /// Handles block impact and douses nearby fire for `DOUSES_FIRE` potions.
    fn thrown_potion_on_hit_block(&self, hit: &ClipHitResult) {
        self.projectile_on_hit_block(hit);
        let Some(world) = self.level() else {
            return;
        };

        let contents = self
            .get_item()
            .get_or_default(vanilla_components::POTION_CONTENTS, PotionContents::empty());
        if !contents.is_in_tag(&PotionTag::DOUSES_FIRE) {
            return;
        }

        let block_effect_pos = hit.direction.relative(hit.block_pos);
        self.douse_fire(&world, block_effect_pos);
        self.douse_fire(&world, hit.direction.opposite().relative(block_effect_pos));
        for direction in Direction::HORIZONTAL {
            self.douse_fire(&world, direction.relative(block_effect_pos));
        }
    }

    /// Puts out fire, lit campfires and lit candles at `pos`.
    fn douse_fire(&self, world: &Arc<World>, pos: BlockPos) {
        let state = world.get_block_state(pos);
        if state.get_block().has_tag(&BlockTag::FIRE) {
            world.destroy_block_by_entity(pos, false, self.as_entity_event_source());
        } else if CandleBlock::is_lit(state) {
            CandleBlock::extinguish(None, state, world, pos);
        } else if CampfireBlock::is_lit_campfire(state) {
            douse_lit_campfire(world, pos, state, self.get_owner());
        }
    }
}

/// Checks for instant effects in the base potion excluding custom effects
fn potion_has_instant_effects(potion: &Potion) -> bool {
    potion.effects.iter().any(|effect| {
        MOB_EFFECT_BEHAVIORS
            .get_behavior(effect.effect)
            .as_instantaneous()
            .is_some()
    })
}

/// Plays the extinguish event, douses the campfire and clears `LIT`.
fn douse_lit_campfire(
    world: &Arc<World>,
    pos: BlockPos,
    state: BlockStateId,
    owner: Option<SharedEntity>,
) {
    world.level_event(level_events::SOUND_EXTINGUISH_FIRE, pos, 0, None);
    CampfireBlock::douse(owner.as_deref(), world, pos);
    world.set_block(
        pos,
        state.set_value(&BlockStateProperties::LIT, false),
        UpdateFlags::UPDATE_ALL,
    );
}
