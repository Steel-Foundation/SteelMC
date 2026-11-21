//! Arrow projectile entity: potion contents carried by the pickup stack, the
//! synced effect color, the tipped-arrow decay, and the post-hurt effect.

use std::sync::{Arc, Weak};

use glam::DVec3;
use parking_lot::{MappedMutexGuard, MutexGuard};
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::entity_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::data_components::PotionContents;
use steel_registry::data_components::vanilla_components::{
    INTANGIBLE_PROJECTILE, POTION_CONTENTS, POTION_DURATION_SCALE,
};
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_entity_data::{AbstractArrowEntityData, ArrowEntityData};
use steel_registry::{sound_events, vanilla_entities, vanilla_items};
use steel_utils::entity_events::EntityStatus;
use steel_utils::locks::SyncMutex;
use steel_utils::{BlockPos, DowncastType, DowncastTypeKey};

use crate::enchantment_helper;
use crate::entity::potion_contents::to_runtime_instance;
use crate::entity::projectile::{ARROW_GRAVITY, AbstractArrow, ArrowBase, ArrowPickup, Projectile};
use crate::entity::{
    Entity, EntityBase, EntityBaseLoad, EntitySyncedData, LivingEntity, LivingEntityRef,
    ProjectileBase, SharedEntity,
};
use crate::physics::{MoveResult, MoverType};
use crate::player::Player;
use crate::world::{ClipHitResult, World};

/// Ticks a tipped arrow must stay in the ground before its effect decays.
const EXPOSED_POTION_DECAY_TIME: i32 = 600;

/// Effect color used when the arrow carries no potion effect.
const NO_EFFECT_COLOR: i32 = -1;

/// Spawn offset below the shooter's eye.
const SPAWN_EYE_OFFSET: f64 = 0.1;

/// A plain (possibly tipped) arrow.
#[entity_behavior(class = "Arrow")]
pub struct ArrowEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    /// Synced data carrying arrow flags and the potion effect color.
    entity_data: SyncMutex<ArrowEntityData>,
    projectile_base: ProjectileBase,
    arrow_base: ArrowBase,
}

// SAFETY: This key is owned by Steel and uniquely identifies `ArrowEntity`.
unsafe impl DowncastType for ArrowEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/arrow");
}

impl ArrowEntity {
    /// Creates a new arrow with no owner.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self {
            base: EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
            entity_data: SyncMutex::new(ArrowEntityData::new()),
            projectile_base: ProjectileBase::new(),
            arrow_base: ArrowBase::new(
                &sound_events::ENTITY_ARROW_HIT,
                ItemStack::with_count(&vanilla_items::ARROW, 1),
            ),
        }
    }

    /// Creates an arrow from saved base data.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self {
            base: EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
            entity_data: SyncMutex::new(ArrowEntityData::new()),
            projectile_base: ProjectileBase::new(),
            arrow_base: ArrowBase::new(
                &sound_events::ENTITY_ARROW_HIT,
                ItemStack::with_count(&vanilla_items::ARROW, 1),
            ),
        }
    }

    /// Creates a fired arrow at the shooter's eye, owned by the shooter.
    #[must_use]
    pub fn from_owner(
        id: i32,
        world: Weak<World>,
        shooter: LivingEntityRef<'_>,
        pickup_item_stack: &mut ItemStack,
        weapon: Option<&ItemStack>,
        is_crit: bool,
    ) -> Self {
        let position = shooter.entity().position();
        let spawn_position = DVec3::new(
            position.x,
            shooter.entity().get_eye_y() - SPAWN_EYE_OFFSET,
            position.z,
        );

        let entity = Self::new(&vanilla_entities::ARROW, id, spawn_position, world);
        entity
            .arrow_base()
            .set_pickup_item_stack(pickup_item_stack.clone());
        entity.apply_components_from_item_stack(pickup_item_stack);

        if pickup_item_stack.has(INTANGIBLE_PROJECTILE) {
            pickup_item_stack.remove(INTANGIBLE_PROJECTILE);
            entity.arrow_base().set_pickup(ArrowPickup::CreativeOnly);
        }

        if is_crit {
            entity.set_crit_arrow(true);
        }

        // Skip the piercing roll for an empty weapon instead of panicking on a
        // caller bug.
        if let Some(weapon) = weapon
            && !weapon.is_empty()
        {
            let pierce_level = enchantment_helper::get_piercing_count(
                weapon,
                &entity.arrow_base().pickup_item_stack(),
            );
            entity.arrow_base().set_weapon(Some(weapon.clone()));
            if pierce_level > 0 {
                entity.set_pierce_level(pierce_level as i8);
            }
        }

        entity.arrow_set_owner(Some(shooter.entity()));
        entity.update_color();
        entity
    }

    fn get_potion_contents(&self) -> PotionContents {
        self.arrow_base()
            .pickup_item_stack()
            .get(POTION_CONTENTS)
            .cloned()
            .unwrap_or_default()
    }

    fn get_potion_duration_scale(&self) -> f32 {
        self.arrow_base()
            .pickup_item_stack()
            .get(POTION_DURATION_SCALE)
            .copied()
            .unwrap_or(1.0)
    }

    fn update_color(&self) {
        let contents = self.get_potion_contents();
        let color = if contents == PotionContents::empty() {
            NO_EFFECT_COLOR
        } else {
            contents.get_color()
        };
        self.entity_data.lock().id_effect_color.set(color);
    }

    /// Returns this arrow's synced effect color (`ID_EFFECT_COLOR`).
    #[must_use]
    pub fn get_color(&self) -> i32 {
        *self.entity_data.lock().id_effect_color.get()
    }
}

impl Entity for ArrowEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn tick(self: Arc<Self>) {
        Arc::clone(&self).arrow_tick();

        if self.in_ground()
            && self.arrow_in_ground_time() != 0
            && self.get_potion_contents() != PotionContents::empty()
            && self.arrow_in_ground_time() >= EXPOSED_POTION_DECAY_TIME
        {
            self.broadcast_entity_event(EntityStatus::TippedArrow);
            self.arrow_set_pickup_item_stack(ItemStack::new(&vanilla_items::ARROW));
        }
    }

    fn get_default_gravity(&self) -> f64 {
        ARROW_GRAVITY
    }

    fn sound_source(&self) -> SoundSource {
        SoundSource::Neutral
    }

    fn is_pushed_by_fluid(&self) -> bool {
        self.arrow_is_pushed_by_fluid()
    }

    fn push_impulse(&self, impulse: DVec3) {
        self.arrow_push_impulse(impulse);
    }

    fn on_above_bubble_column(&self, drag_down: bool, pos: BlockPos) {
        self.arrow_on_above_bubble_column(drag_down, pos);
    }

    fn on_inside_bubble_column(&self, drag_down: bool) {
        self.arrow_on_inside_bubble_column(drag_down);
    }

    fn is_pickable(&self) -> bool {
        self.arrow_is_pickable()
    }

    fn attackable(&self) -> bool {
        self.arrow_is_attackable()
    }

    fn set_no_physics(&self, no_physics: bool) {
        self.arrow_set_no_physics(no_physics);
    }

    fn move_entity(self: Arc<Self>, mover_type: MoverType, delta: DVec3) -> Option<MoveResult> {
        self.arrow_move(mover_type, delta)
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

    fn player_touch(self: Arc<Self>, player: &Arc<Player>) {
        self.arrow_player_touch(player);
    }

    fn with_weapon_item(&self, visitor: &mut dyn FnMut(Option<&ItemStack>)) {
        self.arrow_with_weapon_item(visitor);
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.save_projectile(nbt);
        self.arrow_save(nbt);
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.load_projectile(nbt);
        self.arrow_load(nbt);
    }
}

impl Projectile for ArrowEntity {
    fn projectile_base(&self) -> &ProjectileBase {
        &self.projectile_base
    }

    fn shoot(&self, direction: DVec3, power: f32, uncertainty: f32) {
        self.arrow_shoot(direction, power, uncertainty);
    }

    fn set_owner_entity(&self, owner: Option<&SharedEntity>) {
        self.arrow_set_owner(owner);
    }

    fn apply_on_projectile_spawned(&self, world: &World, pickup_item_stack: &mut ItemStack) {
        self.arrow_apply_on_projectile_spawned(world, pickup_item_stack);
    }

    fn can_hit_entity(&self, entity: &dyn Entity) -> bool {
        self.arrow_can_hit_entity(entity)
    }

    fn on_hit_entity(self: Arc<Self>, entity: &SharedEntity, location: DVec3) {
        self.arrow_on_hit_entity(entity, location);
    }

    fn on_hit_block(self: Arc<Self>, hit: &ClipHitResult) {
        self.arrow_on_hit_block(hit);
    }

    fn should_bounce_on_world_border(&self) -> bool {
        self.arrow_should_bounce_on_world_border()
    }
}

impl AbstractArrow for ArrowEntity {
    fn arrow_base(&self) -> &ArrowBase {
        &self.arrow_base
    }

    fn abstract_arrow_data(&self) -> MappedMutexGuard<'_, AbstractArrowEntityData> {
        MutexGuard::map(self.entity_data.lock(), |data| data.abstract_arrow_mut())
    }

    fn arrow_set_pickup_item_stack(&self, item_stack: ItemStack) {
        self.arrow_base().set_pickup_item_stack(item_stack);
        self.update_color();
    }

    fn arrow_do_post_hurt_effects(self: Arc<Self>, mob: &dyn LivingEntity) {
        let source = Arc::clone(&self).arrow_get_effect_source();
        let contents = self.get_potion_contents();
        let duration_scale = self.get_potion_duration_scale();
        for effect in contents.all_effects() {
            let scaled = effect.with_scaled_duration(duration_scale);
            mob.add_mob_effect_with_source(
                to_runtime_instance(&scaled, scaled.duration()),
                Some(source.as_ref()),
            );
        }
    }
}
