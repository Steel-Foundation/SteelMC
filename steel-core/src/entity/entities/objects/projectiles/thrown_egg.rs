//! Thrown egg projectile entity (`ThrownEgg`).
//!
//! Mirrors vanilla `ThrownEgg` (yarn `EggEntity`) on the Steel
//! `Projectile → ThrowableProjectile → ThrowableItemProjectile` trait stack.
//! On impact it may hatch one chick (or four with a rarer roll), each born as a
//! baby that inherits the egg stack's `chicken/variant` component when present.
//! The egg then broadcasts the item-break entity event and discards itself.

use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::entity_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::data_components::vanilla_components::CHICKEN_VARIANT;
use steel_registry::entity_type::{EntityDimensions, EntityTypeRef};
use steel_registry::item_stack::ItemStack;
use steel_registry::items::ItemRef;
use steel_registry::vanilla_entity_data::EggEntityData;
use steel_registry::{vanilla_damage_types, vanilla_entities, vanilla_items};
use steel_utils::entity_events::EntityStatus;
use steel_utils::locks::SyncMutex;
use steel_utils::{DowncastType, DowncastTypeKey};

use crate::entity::damage::DamageSource;
use crate::entity::entities::ChickenEntity;
use crate::entity::{
    AgeableMob, Entity, EntityBase, EntityBaseLoad, EntitySyncedData, Projectile, ProjectileBase,
    ProjectileHit, RemovalReason, SharedEntity, ThrowableItemProjectile, ThrowableProjectile,
    next_entity_id,
};
use crate::world::World;

/// Baby chicken start age applied to hatched chicks (vanilla `getBabyStartAge`).
const BABY_CHICK_AGE: i32 = -24000;

/// Denomator of the one-in-eight hatch chance (vanilla `ThrownEgg.onHit`).
const HATCH_ROLL_DENOMINATOR: u32 = 8;
/// Denominator of the one-in-thirty-two quadruple-hatch chance.
const QUAD_HATCH_ROLL_DENOMINATOR: u32 = 32;
/// Chicks born when the quadruple roll succeeds.
const QUAD_HATCH_COUNT: usize = 4;

/// Zero-sized dimensions used as the pre-hatch footprint (vanilla
/// `ThrownEgg.ZERO_SIZED_DIMENSIONS`).
const ZERO_SIZED_DIMENSIONS: EntityDimensions = EntityDimensions::new(0.0, 0.0, 0.0);

/// A thrown egg.
#[entity_behavior(class = "ThrownEgg")]
pub struct ThrownEggEntity {
    /// Common entity fields (id, uuid, position, etc.).
    base: EntityBase,
    /// Vanilla entity type registered for this implementation.
    entity_type: EntityTypeRef,
    /// Synced data carrying the rendered item stack.
    entity_data: SyncMutex<EggEntityData>,
    /// Shared `Projectile` state (owner / left-owner / has-been-shot).
    projectile_base: ProjectileBase,
}

// SAFETY: This key is owned by Steel and uniquely identifies `ThrownEggEntity`.
unsafe impl DowncastType for ThrownEggEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/thrown_egg");
}

impl ThrownEggEntity {
    /// Creates a new thrown egg with no owner and the default rendered item.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self {
            base: EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
            entity_data: SyncMutex::new(EggEntityData::new()),
            projectile_base: ProjectileBase::new(),
        }
    }

    /// Creates a thrown egg from saved base data.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self {
            base: EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
            entity_data: SyncMutex::new(EggEntityData::new()),
            projectile_base: ProjectileBase::new(),
        }
    }

    /// Returns the number of chicks to hatch from the two vanilla rolls.
    ///
    /// Vanilla rolls lazily (the second roll only happens when the first
    /// succeeds); rolling both up front preserves the exact distribution.
    const fn hatch_count(eighth_roll: u32, thirty_second_roll: u32) -> usize {
        if eighth_roll != 0 {
            0
        } else if thirty_second_roll == 0 {
            QUAD_HATCH_COUNT
        } else {
            1
        }
    }

    /// Applies the egg stack's `chicken/variant` component to a hatched chick
    /// (vanilla `ThrownEgg.onHit` component inheritance).
    fn apply_hatchling_variant(item_stack: &ItemStack, chicken: &ChickenEntity) {
        if let Some(variant) = item_stack.get(CHICKEN_VARIANT) {
            chicken.set_variant(variant.value());
        }
    }

    /// Creates and adds one hatched chick, returning whether it was placed.
    ///
    /// Mirrors the per-chick body of vanilla `ThrownEgg.onHit`: the chick is
    /// born at the egg's position with the egg's yaw, aged into a baby, then
    /// fudged into the nearest free spot. Returns `false` when the chick does
    /// not fit, aborting the remaining hatch loop like vanilla.
    fn spawn_hatchling(&self, world: &Arc<World>) -> bool {
        let position = self.position();
        let chicken = Arc::new(ChickenEntity::new(
            &vanilla_entities::CHICKEN,
            next_entity_id(),
            position,
            Arc::downgrade(world),
        ));

        AgeableMob::set_age(chicken.as_ref(), BABY_CHICK_AGE);
        let (yaw, _) = self.rotation();
        chicken.set_rotation((yaw, 0.0));
        Self::apply_hatchling_variant(&self.get_item(), &chicken);

        if !chicken.fudge_position_after_size_change(ZERO_SIZED_DIMENSIONS) {
            return false;
        }

        let entity: SharedEntity = chicken;
        match world.try_add_entity(entity) {
            Ok(()) => true,
            Err(error) => {
                // Vanilla `addFreshEntity` drops the chick silently if the
                // destination chunk is not loaded; mirror that by continuing.
                log::debug!("failed to add hatched chick: {error}");
                true
            }
        }
    }
}

impl Entity for ThrownEggEntity {
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
        self.throwable_default_gravity()
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

    fn hurt(&self, _world: &World, _source: &DamageSource, _amount: f32) -> bool {
        // Vanilla `Projectile.hurtServer` marks hurt but never takes damage.
        false
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

impl Projectile for ThrownEggEntity {
    fn projectile_base(&self) -> &ProjectileBase {
        &self.projectile_base
    }

    fn on_hit_entity(&self, entity: &SharedEntity, _location: DVec3) {
        // Vanilla `ThrownEgg.onHitEntity`: super.onHitEntity() (no-op), then
        // deal 0 damage with a `thrown` source so the hit registers the impact.
        let mut damage =
            DamageSource::environment(&vanilla_damage_types::THROWN).with_direct_entity(self.id());
        if let Some(owner) = self.get_owner() {
            damage = damage.with_causing_entity(owner.id());
        }
        if let Some(world) = entity.level() {
            entity.hurt(&world, &damage, 0.0);
        }
    }

    fn on_hit(&self, hit: &ProjectileHit) {
        // Vanilla `ThrownEgg.onHit`: super.onHit() then the server-side hatch.
        self.projectile_on_hit(hit);

        let Some(world) = self.level() else {
            return;
        };

        let count = Self::hatch_count(
            rand::random_range(0..HATCH_ROLL_DENOMINATOR),
            rand::random_range(0..QUAD_HATCH_ROLL_DENOMINATOR),
        );
        for _ in 0..count {
            if !self.spawn_hatchling(&world) {
                break;
            }
        }

        // VANILLA CLIENT-LOCAL: entity event 3 renders the egg break particles
        // on clients via `ThrownEgg.handleEntityEvent`; the server only relays
        // the event. The shared `EntityStatus::Death` variant carries byte 3.
        self.broadcast_entity_event(EntityStatus::Death);
        self.set_removed(RemovalReason::Discarded);
    }
}

impl ThrowableProjectile for ThrownEggEntity {}

impl ThrowableItemProjectile for ThrownEggEntity {
    fn get_default_item(&self) -> ItemRef {
        &vanilla_items::EGG
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

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Weak};

    use glam::DVec3;
    use steel_registry::{
        RegistryReference, init_vanilla_registry, vanilla_blocks, vanilla_chicken_variants,
        vanilla_entities, vanilla_items,
    };
    use steel_utils::types::UpdateFlags;
    use steel_utils::{BlockPos, ChunkPos, Downcast};

    use crate::behavior::init_behaviors;
    use crate::entity::entities::ChickenEntity;
    use crate::entity::{AgeableMob, Entity, ThrowableItemProjectile, WorldAabb};
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};

    use super::*;

    #[test]
    fn hatch_count_matches_vanilla_odds() {
        assert_eq!(ThrownEggEntity::hatch_count(1, 0), 0);
        assert_eq!(ThrownEggEntity::hatch_count(7, 5), 0);
        assert_eq!(ThrownEggEntity::hatch_count(0, 3), 1);
        assert_eq!(ThrownEggEntity::hatch_count(0, 0), QUAD_HATCH_COUNT);
    }

    #[test]
    fn hatchling_inherits_egg_variant_component() {
        init_vanilla_registry();

        let egg =
            ThrownEggEntity::new(&vanilla_entities::EGG, 1, DVec3::ZERO, Weak::<World>::new());
        let mut stack = ItemStack::new(&vanilla_items::EGG);
        stack.set(
            CHICKEN_VARIANT,
            RegistryReference::new(&vanilla_chicken_variants::COLD),
        );
        egg.set_item(stack);

        let chicken = ChickenEntity::new(
            &vanilla_entities::CHICKEN,
            2,
            DVec3::ZERO,
            Weak::<World>::new(),
        );
        ThrownEggEntity::apply_hatchling_variant(&egg.get_item(), &chicken);

        assert_eq!(chicken.variant().key, vanilla_chicken_variants::COLD.key);
    }

    #[test]
    fn hatchling_without_variant_keeps_default() {
        init_vanilla_registry();

        let egg =
            ThrownEggEntity::new(&vanilla_entities::EGG, 1, DVec3::ZERO, Weak::<World>::new());
        egg.set_item(ItemStack::new(&vanilla_items::EGG));

        let chicken = ChickenEntity::new(
            &vanilla_entities::CHICKEN,
            2,
            DVec3::ZERO,
            Weak::<World>::new(),
        );
        ThrownEggEntity::apply_hatchling_variant(&egg.get_item(), &chicken);

        assert_eq!(
            chicken.variant().key,
            vanilla_chicken_variants::TEMPERATE.key
        );
    }

    #[test]
    fn hatchling_spawns_baby_chicken_at_egg_position() {
        init_vanilla_registry();

        let world = fresh_test_world("thrown_egg_hatchling");
        let pos = DVec3::new(0.5, 80.0, 0.5);
        insert_ready_full_chunk(&world, ChunkPos::from_entity_pos(pos));
        let egg = Arc::new(ThrownEggEntity::new(
            &vanilla_entities::EGG,
            1,
            pos,
            Arc::downgrade(&world),
        ));
        egg.set_item(ItemStack::new(&vanilla_items::EGG));

        assert!(egg.spawn_hatchling(&world));

        let chicks = world.get_entities_in_aabb(&WorldAabb::of_size(pos, 1.0, 2.0, 1.0));
        let chick = chicks
            .iter()
            .find(|entity| entity.entity_type() == &vanilla_entities::CHICKEN)
            .expect("hatched chick should be in the world");
        let chicken = chick
            .as_ref()
            .downcast_ref::<ChickenEntity>()
            .expect("chick should be a chicken");
        assert!(AgeableMob::is_baby(chicken));
        assert!((chicken.position() - pos).length() < 1.0);
    }

    #[test]
    fn hatchling_does_not_spawn_fully_enclosed_in_solid_block() {
        init_vanilla_registry();
        init_behaviors();

        let world = fresh_test_world("thrown_egg_enclosed");
        insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
        assert!(world.set_block(
            BlockPos::new(0, 80, 0),
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));

        // The egg lands at the center of the stone block, where the chick box
        // cannot fit; the fudge finds no free position and the hatch aborts.
        let egg = Arc::new(ThrownEggEntity::new(
            &vanilla_entities::EGG,
            1,
            DVec3::new(0.5, 80.5, 0.5),
            Arc::downgrade(&world),
        ));
        egg.set_item(ItemStack::new(&vanilla_items::EGG));

        assert!(!egg.spawn_hatchling(&world));
        assert!(
            world
                .get_entities_in_aabb(&WorldAabb::of_size(
                    DVec3::new(0.5, 80.5, 0.5),
                    1.0,
                    2.0,
                    1.0,
                ))
                .iter()
                .all(|entity| entity.entity_type() != &vanilla_entities::CHICKEN)
        );
    }

    #[test]
    fn hatchling_fits_when_egg_lands_on_top_of_solid_block() {
        init_vanilla_registry();
        init_behaviors();

        let world = fresh_test_world("thrown_egg_on_block");
        insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
        assert!(world.set_block(
            BlockPos::new(0, 80, 0),
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));

        // The egg rests on the block surface, so the chick fits right there.
        let pos = DVec3::new(0.5, 81.0, 0.5);
        let egg = Arc::new(ThrownEggEntity::new(
            &vanilla_entities::EGG,
            1,
            pos,
            Arc::downgrade(&world),
        ));
        egg.set_item(ItemStack::new(&vanilla_items::EGG));

        assert!(egg.spawn_hatchling(&world));
        assert!(
            world
                .get_entities_in_aabb(&WorldAabb::of_size(pos, 1.0, 2.0, 1.0))
                .iter()
                .any(|entity| entity.entity_type() == &vanilla_entities::CHICKEN)
        );
    }
}
