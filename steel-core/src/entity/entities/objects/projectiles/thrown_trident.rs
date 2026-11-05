//! Thrown trident projectile entity: loyalty return flight, the single
//! non-piercing entity hit, and the trident-specific damage and pickup rules.

use std::sync::{Arc, Weak};

use glam::DVec3;
use parking_lot::{MappedMutexGuard, MutexGuard};
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::entity_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_entity_data::{AbstractArrowEntityData, TridentEntityData};
use steel_registry::{sound_events, vanilla_damage_types, vanilla_entities, vanilla_items};
use steel_utils::locks::SyncMutex;
use steel_utils::{BlockPos, DowncastType, DowncastTypeKey};

use crate::enchantment_helper::{self, EnchantmentDamageContext, EnchantmentPostAttackContext};
use crate::entity::damage::DamageSource;
use crate::entity::projectile::{
    ARROW_GRAVITY, AbstractArrow, ArrowBase, ArrowPickup, Projectile, ProjectileDeflection,
};
use crate::entity::{
    Entity, EntityBase, EntityBaseLoad, EntityHitResult, EntitySyncedData, ProjectileBase,
    RemovalReason, SharedEntity,
};
use crate::inventory::container::Container as _;
use crate::physics::{MoveResult, MoverType};
use crate::player::Player;
use crate::world::{ClipHitResult, World};

/// Inertia applied while the trident is in water.
const WATER_INERTIA: f32 = 0.99;

/// Base damage of a thrown trident hit, before enchantment modifiers.
const TRIDENT_DAMAGE: f32 = 8.0;

/// Upper clamp for the loyalty level.
const MAX_LOYALTY: i32 = 127;

/// Return-flight velocity dampening.
const RETURN_VELOCITY_SCALE: f64 = 0.95;

/// Return-flight lift per loyalty level.
const RETURN_Y_LIFT: f64 = 0.015;

/// Return-flight acceleration per loyalty level.
const RETURN_ACCELERATION: f64 = 0.05;

/// `normalize` treats vectors shorter than this as zero.
const NORMALIZE_EPSILON: f64 = 1.0e-4;

/// In-ground ticks after which the trident counts as having dealt damage.
const DEALT_DAMAGE_GROUND_TIME: i32 = 4;

/// Distance beyond the owner's width at which a non-player owner picks up the
/// returning trident.
const OWNER_PICKUP_DISTANCE: f64 = 1.0;

/// Volume of the return sound.
const RETURN_SOUND_VOLUME: f32 = 10.0;

/// Offset applied when dropping a trident as an item.
const PICKUP_SPAWN_OFFSET: DVec3 = DVec3::new(0.0, 0.1, 0.0);

/// Per-axis deflection power applied by a trident hit.
const HIT_DEFLECTION_POWER: DVec3 = DVec3::new(0.02, 0.2, 0.02);

/// A thrown trident.
#[entity_behavior(class = "ThrownTrident")]
pub struct ThrownTridentEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    /// Synced data carrying arrow flags plus trident loyalty and foil.
    entity_data: SyncMutex<TridentEntityData>,
    projectile_base: ProjectileBase,
    arrow_base: ArrowBase,
    /// Whether this trident has already dealt damage in this flight.
    dealt_damage: SyncMutex<bool>,
    /// Return-flight tick counter, used to play the return sound once per flight.
    return_tick_count: SyncMutex<i32>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `ThrownTridentEntity`.
unsafe impl DowncastType for ThrownTridentEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/thrown_trident");
}

impl ThrownTridentEntity {
    /// Creates a new thrown trident with no owner.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self {
            base: EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
            entity_data: SyncMutex::new(TridentEntityData::new()),
            projectile_base: ProjectileBase::new(),
            arrow_base: ArrowBase::new(
                &sound_events::ITEM_TRIDENT_HIT_GROUND,
                ItemStack::with_count(&vanilla_items::TRIDENT, 1),
            ),
            dealt_damage: SyncMutex::new(false),
            return_tick_count: SyncMutex::new(0),
        }
    }

    /// Creates a thrown trident from saved base data.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self {
            base: EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
            entity_data: SyncMutex::new(TridentEntityData::new()),
            projectile_base: ProjectileBase::new(),
            arrow_base: ArrowBase::new(
                &sound_events::ITEM_TRIDENT_HIT_GROUND,
                ItemStack::with_count(&vanilla_items::TRIDENT, 1),
            ),
            dealt_damage: SyncMutex::new(false),
            return_tick_count: SyncMutex::new(0),
        }
    }

    /// Creates a thrown trident owned by `owner`, with loyalty and foil taken
    /// from the trident item.
    #[must_use]
    pub fn from_owner(
        trident_item: &ItemStack,
        owner: &SharedEntity,
        position: DVec3,
        id: i32,
        world: Weak<World>,
    ) -> Self {
        let entity = Self {
            base: EntityBase::new(id, position, vanilla_entities::TRIDENT.dimensions, world),
            entity_type: &vanilla_entities::TRIDENT,
            entity_data: SyncMutex::new(TridentEntityData::new()),
            projectile_base: ProjectileBase::new(),
            arrow_base: ArrowBase::new(
                &sound_events::ITEM_TRIDENT_HIT_GROUND,
                ItemStack::with_count(&vanilla_items::TRIDENT, 1),
            ),
            dealt_damage: SyncMutex::new(false),
            return_tick_count: SyncMutex::new(0),
        };
        entity.arrow_set_pickup_item_stack(trident_item.clone());
        entity.set_owner_entity(Some(owner));
        let loyalty = entity.get_loyalty_from_item(trident_item);
        {
            let mut data = entity.entity_data.lock();
            data.thrown_trident.id_loyalty.set(loyalty);
            data.thrown_trident.id_foil.set(trident_item.has_foil());
        }
        entity
    }

    /// Returns whether the trident item is foiled.
    #[must_use]
    pub fn is_foil(&self) -> bool {
        *self.entity_data.lock().thrown_trident.id_foil.get()
    }

    fn get_loyalty_from_item(&self, trident_item: &ItemStack) -> i8 {
        if self.level().is_none() {
            return 0;
        }
        let acceleration =
            enchantment_helper::get_trident_return_to_owner_acceleration(trident_item, self);
        acceleration.clamp(0, MAX_LOYALTY) as i8
    }

    #[must_use]
    fn loyalty(&self) -> i32 {
        i32::from(*self.entity_data.lock().thrown_trident.id_loyalty.get())
    }

    fn is_acceptable_return_owner(&self) -> bool {
        let Some(current_owner) = self.get_owner() else {
            return false;
        };
        if !Entity::is_alive(current_owner.as_ref()) {
            return false;
        }
        current_owner.as_player().is_none() || !current_owner.is_spectator()
    }

    /// Returns `false` when a close non-player owner picked the trident up and the
    /// rest of the tick must be skipped.
    fn loyalty_tick(&self) -> bool {
        let loyalty = self.loyalty();
        let dealt_damage = *self.dealt_damage.lock();
        if loyalty <= 0 || !(dealt_damage || self.no_physics()) {
            return true;
        }
        let Some(current_owner) = self.get_owner() else {
            return true;
        };

        if !self.is_acceptable_return_owner() {
            if self.arrow_base().pickup() == ArrowPickup::Allowed {
                let _ = self
                    .spawn_at_location_with_offset(self.arrow_pickup_item(), PICKUP_SPAWN_OFFSET);
            }
            self.set_removed(RemovalReason::Discarded);
            return true;
        }

        let position = self.position();
        let eye_position = DVec3::new(position.x, current_owner.get_eye_y(), position.z);
        if current_owner.as_player().is_none() {
            let owner_box = current_owner.bounding_box();
            let owner_width = owner_box.max_x() - owner_box.min_x();
            if position.distance(eye_position) < owner_width + OWNER_PICKUP_DISTANCE {
                self.set_removed(RemovalReason::Discarded);
                return false;
            }
        }

        self.arrow_set_no_physics(true);
        let to_owner = eye_position - position;
        if let Err(error) = self.try_set_position(DVec3::new(
            position.x,
            position.y + to_owner.y * RETURN_Y_LIFT * f64::from(loyalty),
            position.z,
        )) {
            log::debug!(
                "Rejected trident loyalty reposition for entity {}: {error}",
                self.id()
            );
        }
        let direction = if to_owner.length() < NORMALIZE_EPSILON {
            DVec3::ZERO
        } else {
            to_owner.normalize()
        };
        self.set_velocity(
            self.velocity() * RETURN_VELOCITY_SCALE
                + direction * (RETURN_ACCELERATION * f64::from(loyalty)),
        );

        let mut return_tick_count = self.return_tick_count.lock();
        if *return_tick_count == 0 {
            self.play_sound(&sound_events::ITEM_TRIDENT_RETURN, RETURN_SOUND_VOLUME, 1.0);
        }
        *return_tick_count += 1;
        true
    }
}

impl Entity for ThrownTridentEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn tick(self: Arc<Self>) {
        if self.arrow_in_ground_time() > DEALT_DAMAGE_GROUND_TIME {
            *self.dealt_damage.lock() = true;
        }
        if !self.loyalty_tick() {
            return;
        }
        Arc::clone(&self).arrow_tick();
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
        if self.owned_by(player.as_ref()) || self.get_owner().is_none() {
            self.arrow_player_touch(player);
        }
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.save_projectile(nbt);
        self.arrow_save(nbt);
        nbt.insert("DealtDamage", i8::from(*self.dealt_damage.lock()));
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.load_projectile(nbt);
        self.arrow_load(nbt);
        *self.dealt_damage.lock() = nbt.byte("DealtDamage").is_some_and(|value| value != 0);
        let pickup_item = self.arrow_pickup_item_stack_origin();
        let loyalty = self.get_loyalty_from_item(&pickup_item);
        self.entity_data
            .lock()
            .thrown_trident
            .id_loyalty
            .set(loyalty);
    }
}

impl Projectile for ThrownTridentEntity {
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

    fn on_item_break(&self, _item: &ItemStack) {
        self.arrow_base().set_weapon(None);
    }

    fn on_hit_entity(self: Arc<Self>, entity: &SharedEntity, _location: DVec3) {
        // A trident deals fixed damage and never discards itself on impact, so
        // it does not reuse the abstract-arrow hit path.
        let Some(world) = entity.level() else {
            return;
        };

        let owner = self.get_owner();
        let causing_entity = owner
            .clone()
            .unwrap_or_else(|| Arc::<Self>::clone(&self) as SharedEntity);
        let damage_source = DamageSource::environment(&vanilla_damage_types::TRIDENT)
            .with_direct_entity(Arc::<Self>::clone(&self))
            .with_causing_entity(causing_entity);

        let weapon_item = self.arrow_pickup_item_stack_origin();
        let context = EnchantmentDamageContext::new(
            entity.entity_type(),
            owner.as_ref().map(|owner| owner.entity_type()),
            Some(self.entity_type()),
            &damage_source,
        );
        let damage = enchantment_helper::modify_damage(&weapon_item, &context, TRIDENT_DAMAGE);

        *self.dealt_damage.lock() = true;
        let was_hurt = entity.hurt(&world, &damage_source, damage);
        if was_hurt {
            let post_attack = EnchantmentPostAttackContext::new(entity, &damage_source);
            let on_break = |_item: &ItemStack| self.kill(&world);
            enchantment_helper::do_post_attack_effects_with_item_source_on_break(
                &world,
                entity.as_ref(),
                &weapon_item,
                &post_attack,
                Some(&on_break),
            );

            if let Some(mob) = entity.as_living_entity() {
                self.arrow_do_knockback(mob, &damage_source);
                Arc::clone(&self).arrow_do_post_hurt_effects(mob);
            }
        }

        if entity.projectile_receives_side_effects_on_hit(was_hurt) {
            self.play_sound(&sound_events::ITEM_TRIDENT_HIT, 1.0, 1.0);
            let owner_reference = self.owner_reference();
            self.deflect(
                ProjectileDeflection::Reverse,
                Some(entity.as_ref()),
                owner_reference,
                false,
                HIT_DEFLECTION_POWER,
            );
        }
    }

    fn on_hit_block(self: Arc<Self>, hit: &ClipHitResult) {
        Arc::clone(&self).arrow_on_hit_block(hit);
    }
}

impl AbstractArrow for ThrownTridentEntity {
    fn arrow_base(&self) -> &ArrowBase {
        &self.arrow_base
    }

    fn abstract_arrow_data(&self) -> MappedMutexGuard<'_, AbstractArrowEntityData> {
        MutexGuard::map(self.entity_data.lock(), |data| {
            data.thrown_trident.abstract_arrow_mut()
        })
    }

    fn tick_despawn(&self) {
        if self.arrow_base().pickup() != ArrowPickup::Allowed || self.loyalty() <= 0 {
            self.arrow_tick_despawn();
        }
    }

    fn try_pickup(&self, player: &Arc<Player>) -> bool {
        self.arrow_try_pickup(player)
            || (self.no_physics()
                && self.owned_by(player.as_ref())
                && player.inventory.lock().add(&mut self.arrow_pickup_item()))
    }

    fn find_hit_entity(&self, from: DVec3, to: DVec3) -> Option<EntityHitResult> {
        if *self.dealt_damage.lock() {
            return None;
        }
        self.arrow_find_hit_entity(from, to)
    }

    fn find_hit_entities(&self, from: DVec3, to: DVec3) -> Vec<EntityHitResult> {
        self.find_hit_entity(from, to).into_iter().collect()
    }

    fn arrow_water_inertia(&self) -> f32 {
        WATER_INERTIA
    }

    fn get_weapon_item(&self) -> Option<ItemStack> {
        Some(self.arrow_pickup_item_stack_origin())
    }
}
