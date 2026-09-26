//! Thrown splash potion projectile entity (`ThrownSplashPotion`).
//!
//! `Projectile → ThrowableProjectile → ThrowableItemProjectile → AbstractThrownPotion`
//! trait stack. On impact it applies its potion contents to every affected
//! `LivingEntity` in a 4-block radius, scaled by distance falloff, then
//! discards itself (shared [`AbstractThrownPotion::thrown_potion_on_hit`]
//! logic handles the water-splash branch, break level-event, and discard).

use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::entity_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::data_components::{PotionContents, vanilla_components};
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::items::ItemRef;
use steel_registry::vanilla_entity_data::SplashPotionEntityData;
use steel_registry::{MobEffectInstance as RegistryMobEffectInstance, vanilla_items};
use steel_utils::locks::SyncMutex;
use steel_utils::{DowncastType, DowncastTypeKey};

use crate::behavior::MOB_EFFECT_BEHAVIORS;
use crate::entity::damage::DamageSource;
use crate::entity::potion_contents::to_runtime_instance_icon_from_visibility;
use crate::entity::{
    AbstractThrownPotion, Entity, EntityBase, EntityBaseLoad, EntitySyncedData, LivingEntity,
    Projectile, ProjectileBase, ProjectileHit, SPLASH_RANGE_SQ, SharedEntity,
    ThrowableItemProjectile, ThrowableProjectile, compute_margin,
};
use crate::world::{ClipHitResult, World};

/// Vanilla `MobEffectInstance.endsWithin` cutoff applied after scaling duration.
const MIN_REMAINING_TICKS: i32 = 20;

/// A thrown splash potion.
#[entity_behavior(class = "ThrownSplashPotion")]
pub struct SplashPotionEntity {
    /// Common entity fields (id, uuid, position, etc.).
    base: EntityBase,
    /// Vanilla entity type registered for this implementation.
    entity_type: EntityTypeRef,
    /// Synced data carrying the rendered item stack.
    entity_data: SyncMutex<SplashPotionEntityData>,
    /// Shared `Projectile` state (owner / left-owner / has-been-shot).
    projectile_base: ProjectileBase,
}

// SAFETY: This key is owned by Steel and uniquely identifies `SplashPotionEntity`.
unsafe impl DowncastType for SplashPotionEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/splash_potion");
}

impl SplashPotionEntity {
    /// Creates a new thrown splash potion with no owner and the default rendered item.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self {
            base: EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
            entity_data: SyncMutex::new(SplashPotionEntityData::new()),
            projectile_base: ProjectileBase::new(),
        }
    }

    /// Creates a thrown splash potion from saved base data.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self {
            base: EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
            entity_data: SyncMutex::new(SplashPotionEntityData::new()),
            projectile_base: ProjectileBase::new(),
        }
    }

    /// Applies one potion effect to an affected entity (vanilla's per-effect
    /// body of `ThrownSplashPotion.onHitAsPotion`'s inner loop).
    fn apply_effect(
        &self,
        world: &World,
        living: &dyn LivingEntity,
        effect: &RegistryMobEffectInstance,
        scale: f64,
        duration_scale: f32,
        owner: Option<&SharedEntity>,
    ) {
        let behavior = MOB_EFFECT_BEHAVIORS.get_behavior(effect.effect());
        if let Some(instantaneous) = behavior.as_instantaneous() {
            instantaneous.apply_instantaneous(
                world,
                living,
                effect.amplifier(),
                Some(self.id()),
                owner.map(|owner| owner.id()),
                scale,
            );
            return;
        }

        let duration = effect.map_duration(|base_duration| {
            (scale * f64::from(base_duration) * f64::from(duration_scale) + 0.5) as i32
        });
        // Vanilla builds the new instance first and asks *it* whether it ends
        // within 20 ticks, so the check sees the scaled duration.
        let scaled_effect = to_runtime_instance_icon_from_visibility(effect, duration);
        if scaled_effect.ends_within(MIN_REMAINING_TICKS) {
            return;
        }
        // Vanilla `Projectile.getEffectSource()`: the thrower when it is still
        // resolvable, otherwise the potion itself.
        let effect_source: &dyn Entity = owner.map_or(self, |owner| owner.as_ref());
        living.add_mob_effect_with_source(scaled_effect, Some(effect_source));
    }
}

impl Entity for SplashPotionEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn tick(&self) {
        self.throwable_projectile_tick();
    }

    fn get_default_gravity(&self) -> f64 {
        self.thrown_potion_default_gravity()
    }

    fn sound_source(&self) -> SoundSource {
        SoundSource::Neutral
    }

    fn spawn_data(&self) -> i32 {
        self.get_owner().map_or(0, |owner| owner.id())
    }

    fn restore_owner_reference(&self, owner: &SharedEntity) {
        self.cache_owner_entity(owner);
    }

    fn projectile_owner_uuid(&self) -> Option<uuid::Uuid> {
        self.owner_uuid()
    }

    fn projectile_owner(&self) -> Option<SharedEntity> {
        self.get_owner()
    }

    fn attackable(&self) -> bool {
        false
    }

    fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
        Some(&self.entity_data)
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.save_projectile(nbt);
        self.save_throwable_item(nbt);
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.load_projectile(nbt);
        self.load_throwable_item(nbt);
    }
}

impl Projectile for SplashPotionEntity {
    fn projectile_base(&self) -> &ProjectileBase {
        &self.projectile_base
    }

    fn calculate_horizontal_hurt_knockback_direction(
        &self,
        hurt_entity: &dyn LivingEntity,
        _damage_source: &DamageSource,
    ) -> (f64, f64) {
        self.thrown_potion_knockback_direction(hurt_entity)
    }

    fn on_hit(&self, hit: &ProjectileHit) {
        self.thrown_potion_on_hit(hit);
    }

    fn on_hit_block(&self, hit: &ClipHitResult) {
        self.thrown_potion_on_hit_block(hit);
    }
}

impl ThrowableProjectile for SplashPotionEntity {}

impl ThrowableItemProjectile for SplashPotionEntity {
    fn get_default_item(&self) -> ItemRef {
        &vanilla_items::SPLASH_POTION
    }

    fn set_item(&self, item: ItemStack) {
        self.entity_data
            .lock()
            .throwable_item_projectile
            .item_stack
            .set(item);
    }

    fn get_item(&self) -> ItemStack {
        self.entity_data
            .lock()
            .throwable_item_projectile
            .item_stack
            .get()
            .clone()
    }
}

impl AbstractThrownPotion for SplashPotionEntity {
    fn on_hit_as_potion(&self, world: &Arc<World>, potion_item: &ItemStack, hit: &ProjectileHit) {
        let contents = potion_item
            .get_or_default(vanilla_components::POTION_CONTENTS, PotionContents::empty());
        let duration_scale =
            potion_item.get_or_default(vanilla_components::POTION_DURATION_SCALE, 1.0);
        let mob_effects = contents.all_effects();

        let potion_aabb = self
            .bounding_box()
            .translate(hit.location() - self.position());
        let effect_aabb = potion_aabb.inflate_xyz(4.0, 2.0, 4.0);
        let entities = world.get_entities_in_aabb(&effect_aabb);
        if entities.is_empty() {
            return;
        }

        let margin = compute_margin(self.tick_count());
        // Vanilla hoists `getEffectSource()` out of the loop; Steel resolves the
        // owner once here and derives the source per application.
        let owner = self.get_owner();

        for entity in entities {
            let Some(living) = entity.as_living_entity() else {
                continue;
            };
            if !living.is_affected_by_potions() {
                continue;
            }

            let dist = potion_aabb.distance_to_sqr_aabb(entity.bounding_box().inflate(margin));
            if dist >= SPLASH_RANGE_SQ {
                continue;
            }
            let scale = 1.0 - dist.sqrt() / 4.0;

            for effect in &mob_effects {
                self.apply_effect(world, living, effect, scale, duration_scale, owner.as_ref());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use glam::DVec3;
    use steel_registry::{init_vanilla_registry, vanilla_entities};
    use steel_utils::{BlockPos, Direction};

    use crate::behavior::init_behaviors;
    use crate::entity::{Entity, Projectile, ProjectileHit};
    use crate::test_support::test_world;
    use crate::world::ClipHitResult;

    use super::SplashPotionEntity;

    #[test]
    fn on_hit_discards_the_splash_potion() {
        init_vanilla_registry();
        init_behaviors();

        let world = test_world();
        let potion = SplashPotionEntity::new(
            &vanilla_entities::SPLASH_POTION,
            1,
            DVec3::ZERO,
            Arc::downgrade(world),
        );
        let hit = ProjectileHit::Block {
            location: DVec3::ZERO,
            hit: ClipHitResult {
                location: DVec3::ZERO,
                direction: Direction::Up,
                block_pos: BlockPos::new(0, 0, 0),
                miss: false,
                inside: false,
                world_border_hit: false,
            },
        };

        potion.on_hit(&hit);
        assert!(potion.is_removed());
    }
}
