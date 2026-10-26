//! Lingering potion projectile that leaves an area effect cloud on impact

use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::entity_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::items::ItemRef;
use steel_registry::vanilla_entity_data::LingeringPotionEntityData;
use steel_registry::{vanilla_entities, vanilla_items};
use steel_utils::locks::SyncMutex;
use steel_utils::{DowncastType, DowncastTypeKey};

use crate::entity::damage::DamageSource;
use crate::entity::entities::AreaEffectCloudEntity;
use crate::entity::{
    AbstractThrownPotion, Entity, EntityBase, EntityBaseLoad, EntitySyncedData, LivingEntity,
    Projectile, ProjectileBase, ProjectileHit, SharedEntity, ThrowableItemProjectile,
    ThrowableProjectile, next_entity_id,
};
use crate::world::{ClipHitResult, World};

const CLOUD_RADIUS: f32 = 3.0;
const CLOUD_RADIUS_ON_USE: f32 = -0.5;
const CLOUD_DURATION: i32 = 600;
const CLOUD_WAIT_TIME: i32 = 10;

/// A thrown lingering potion.
#[entity_behavior(class = "ThrownLingeringPotion")]
pub struct LingeringPotionEntity {
    /// Common entity fields (id, uuid, position, etc.).
    base: EntityBase,
    /// Vanilla entity type registered for this implementation.
    entity_type: EntityTypeRef,
    /// Synced data carrying the rendered item stack.
    entity_data: SyncMutex<LingeringPotionEntityData>,
    /// Shared `Projectile` state (owner / left-owner / has-been-shot).
    projectile_base: ProjectileBase,
}

// SAFETY: This key is owned by Steel and uniquely identifies `LingeringPotionEntity`.
unsafe impl DowncastType for LingeringPotionEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/lingering_potion");
}

impl LingeringPotionEntity {
    /// Creates a new thrown lingering potion with no owner and the default rendered item.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self {
            base: EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
            entity_data: SyncMutex::new(LingeringPotionEntityData::new()),
            projectile_base: ProjectileBase::new(),
        }
    }

    /// Creates a thrown lingering potion from saved base data.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self {
            base: EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
            entity_data: SyncMutex::new(LingeringPotionEntityData::new()),
            projectile_base: ProjectileBase::new(),
        }
    }
}

impl Entity for LingeringPotionEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn tick(self: Arc<Self>) {
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

impl Projectile for LingeringPotionEntity {
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

    fn on_hit(self: Arc<Self>, hit: &ProjectileHit) {
        self.thrown_potion_on_hit(hit);
    }

    fn on_hit_block(self: Arc<Self>, hit: &ClipHitResult) {
        self.thrown_potion_on_hit_block(hit);
    }
}

impl ThrowableProjectile for LingeringPotionEntity {}

impl ThrowableItemProjectile for LingeringPotionEntity {
    fn get_default_item(&self) -> ItemRef {
        &vanilla_items::LINGERING_POTION
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

impl AbstractThrownPotion for LingeringPotionEntity {
    fn on_hit_as_potion(
        self: &Arc<Self>,
        world: &Arc<World>,
        potion_item: &ItemStack,
        hit: &ProjectileHit,
    ) {
        let position = match hit {
            ProjectileHit::Entity(entity_hit) => entity_hit.entity.position(),
            ProjectileHit::Block { .. } => self.position(),
        };
        let cloud = Arc::new(AreaEffectCloudEntity::new(
            &vanilla_entities::AREA_EFFECT_CLOUD,
            next_entity_id(),
            position,
            Arc::downgrade(world),
        ));
        cloud.set_owner(self.get_owner().as_ref());
        cloud.set_radius(CLOUD_RADIUS);
        cloud.set_radius_on_use(CLOUD_RADIUS_ON_USE);
        cloud.set_duration(CLOUD_DURATION);
        cloud.set_wait_time(CLOUD_WAIT_TIME);
        cloud.set_radius_per_tick(-cloud.radius() / cloud.duration() as f32);
        cloud.apply_components_from_item_stack(potion_item);

        let cloud: SharedEntity = cloud;
        if let Err(error) = world.try_add_entity(cloud) {
            log::debug!("world rejected a lingering potion cloud: {error}");
        }
    }
}
