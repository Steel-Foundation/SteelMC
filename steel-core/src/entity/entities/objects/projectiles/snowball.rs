//! Thrown snowball projectile entity (`Snowball`).
//!
//! Mirrors vanilla `Snowball` on the Steel
//! `Projectile → ThrowableProjectile → ThrowableItemProjectile` trait stack.
//! On entity impact it deals 0 thrown damage (3 to blazes) so the hit
//! registers, then broadcasts the item-break entity event and discards itself.

use std::sync::Weak;

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::entity_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::items::ItemRef;
use steel_registry::vanilla_entity_data::SnowballEntityData;
use steel_registry::{vanilla_damage_types, vanilla_entities, vanilla_items};
use steel_utils::entity_events::EntityStatus;
use steel_utils::locks::SyncMutex;
use steel_utils::{DowncastType, DowncastTypeKey};

use crate::entity::damage::DamageSource;
use crate::entity::{
    Entity, EntityBase, EntityBaseLoad, EntitySyncedData, Projectile, ProjectileBase,
    ProjectileHit, RemovalReason, SharedEntity, ThrowableItemProjectile, ThrowableProjectile,
};
use crate::world::World;

/// Thrown damage dealt to a blaze (vanilla `Snowball.onHitEntity`).
const BLAZE_HIT_DAMAGE: f32 = 3.0;

/// A thrown snowball.
#[entity_behavior(class = "Snowball")]
pub struct SnowballEntity {
    /// Common entity fields (id, uuid, position, etc.).
    base: EntityBase,
    /// Vanilla entity type registered for this implementation.
    entity_type: EntityTypeRef,
    /// Synced data carrying the rendered item stack.
    entity_data: SyncMutex<SnowballEntityData>,
    /// Shared `Projectile` state (owner / left-owner / has-been-shot).
    projectile_base: ProjectileBase,
}

// SAFETY: This key is owned by Steel and uniquely identifies `SnowballEntity`.
unsafe impl DowncastType for SnowballEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/snowball");
}

impl SnowballEntity {
    /// Creates a new thrown snowball with no owner and the default rendered item.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self {
            base: EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
            entity_data: SyncMutex::new(SnowballEntityData::new()),
            projectile_base: ProjectileBase::new(),
        }
    }

    /// Creates a thrown snowball from saved base data.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self {
            base: EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
            entity_data: SyncMutex::new(SnowballEntityData::new()),
            projectile_base: ProjectileBase::new(),
        }
    }

    /// Vanilla `Snowball.onHitEntity` damage: 3 against blazes, otherwise 0.
    fn impact_damage(entity_type: EntityTypeRef) -> f32 {
        if entity_type == &vanilla_entities::BLAZE {
            BLAZE_HIT_DAMAGE
        } else {
            0.0
        }
    }
}

impl Entity for SnowballEntity {
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

    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.save_projectile(nbt);
        self.save_throwable_item(nbt);
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.load_projectile(nbt);
        self.load_throwable_item(nbt);
    }
}

impl Projectile for SnowballEntity {
    fn projectile_base(&self) -> &ProjectileBase {
        &self.projectile_base
    }

    fn on_hit_entity(&self, entity: &SharedEntity, _location: DVec3) {
        // Vanilla `Snowball.onHitEntity`: super.onHitEntity() (no-op), then
        // `entity.hurt(thrown(this, owner), blaze ? 3 : 0)`.
        let mut damage =
            DamageSource::environment(&vanilla_damage_types::THROWN).with_direct_entity(self.id());
        if let Some(owner) = self.get_owner() {
            damage = damage.with_causing_entity(owner.id());
        }
        if let Some(world) = entity.level() {
            entity.hurt(&world, &damage, Self::impact_damage(entity.entity_type()));
        }
    }

    fn on_hit(&self, hit: &ProjectileHit) {
        // Vanilla `Snowball.onHit`: super.onHit() then the server-side break.
        self.projectile_on_hit(hit);

        // VANILLA CLIENT-LOCAL: entity event 3 renders the snowball break
        // particles on clients via `Snowball.handleEntityEvent`; the server
        // only relays the event. The shared `EntityStatus::Death` variant
        // carries byte 3.
        self.broadcast_entity_event(EntityStatus::Death);
        self.set_removed(RemovalReason::Discarded);
    }
}

impl ThrowableProjectile for SnowballEntity {}

impl ThrowableItemProjectile for SnowballEntity {
    fn get_default_item(&self) -> ItemRef {
        &vanilla_items::SNOWBALL
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
    use std::sync::Weak;

    use glam::DVec3;
    use steel_registry::{init_vanilla_registry, vanilla_damage_types, vanilla_entities};
    use steel_utils::{BlockPos, Direction};

    use crate::entity::damage::DamageSource;
    use crate::entity::{Entity, Projectile, ProjectileHit};
    use crate::test_support::test_world;
    use crate::world::{ClipHitResult, World};

    use super::*;

    #[test]
    fn hurt_marks_snowball_unless_base_invulnerable_and_always_returns_false() {
        init_vanilla_registry();

        let snowball = SnowballEntity::new(
            &vanilla_entities::SNOWBALL,
            1,
            DVec3::ZERO,
            Weak::<World>::new(),
        );
        let source = DamageSource::environment(&vanilla_damage_types::GENERIC);

        assert!(!Entity::hurt(&snowball, test_world(), &source, 1.0));
        assert!(snowball.hurt_marked());

        snowball.clear_hurt_mark();
        snowball.set_invulnerable(true);
        assert!(!Entity::hurt(&snowball, test_world(), &source, 1.0));
        assert!(!snowball.hurt_marked());
    }

    #[test]
    fn impact_damage_is_three_for_blazes_and_zero_otherwise() {
        init_vanilla_registry();

        assert_eq!(
            SnowballEntity::impact_damage(&vanilla_entities::BLAZE),
            BLAZE_HIT_DAMAGE
        );
        assert_eq!(SnowballEntity::impact_damage(&vanilla_entities::PIG), 0.0);
        assert_eq!(
            SnowballEntity::impact_damage(&vanilla_entities::SNOWBALL),
            0.0
        );
    }

    #[test]
    fn on_hit_discards_the_snowball() {
        init_vanilla_registry();

        let snowball = SnowballEntity::new(
            &vanilla_entities::SNOWBALL,
            1,
            DVec3::ZERO,
            Weak::<World>::new(),
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

        snowball.on_hit(&hit);
        assert!(snowball.is_removed());
    }
}
