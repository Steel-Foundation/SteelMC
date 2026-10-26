//! Lingering cloud that applies its potion effects to living entities standing inside it.

use std::sync::{Arc, Weak};

use glam::DVec3;
use rustc_hash::FxHashMap;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use simdnbt::{FromNbtTag as _, ToNbtTag as _};
use steel_macros::entity_behavior;
use steel_registry::MobEffectInstance as RegistryMobEffectInstance;
use steel_registry::blocks::behavior::PushReaction;
use steel_registry::data_components::{PotionContents, vanilla_components};
use steel_registry::entity_data::EntityPose;
use steel_registry::entity_type::{EntityDimensions, EntityTypeRef};
use steel_registry::item_stack::ItemStack;
use steel_registry::particle_type::{ColorParticleOption, ParticleData};
use steel_registry::vanilla_entity_data::AreaEffectCloudEntityData;
use steel_registry::vanilla_particle_types;
use steel_utils::locks::SyncMutex;
use steel_utils::{DowncastType, DowncastTypeKey, RgbColor};

use crate::behavior::MOB_EFFECT_BEHAVIORS;
use crate::entity::damage::DamageSource;
use crate::entity::potion_contents::to_runtime_instance;
use crate::entity::{
    Entity, EntityBase, EntityBaseLoad, EntityReference, EntitySyncedData, LivingEntity,
    RemovalReason, SharedEntity,
};
use crate::world::World;

/// Ticks between two effect application passes.
const TIME_BETWEEN_APPLICATIONS: i32 = 5;
const MAX_RADIUS: f32 = 32.0;
/// The cloud is discarded once its radius shrinks below this.
const MINIMAL_RADIUS: f32 = 0.5;
const DEFAULT_RADIUS: f32 = 3.0;
const HEIGHT: f32 = 0.5;
/// `Duration` value for a cloud that never expires.
const INFINITE_DURATION: i32 = -1;
const DEFAULT_WAIT_TIME: i32 = 20;
const DEFAULT_REAPPLICATION_DELAY: i32 = 20;
const DEFAULT_POTION_DURATION_SCALE: f32 = 1.0;
/// Scale passed to instantaneous effects (instant health/damage) applied by the cloud.
const INSTANTANEOUS_EFFECT_SCALE: f64 = 0.5;

/// Server-side cloud state that is persisted but not synced.
struct AreaEffectCloudState {
    potion_contents: PotionContents,
    potion_duration_scale: f32,
    /// Entity id to the tick count at which it may be affected again.
    victims: FxHashMap<i32, i32>,
    duration: i32,
    wait_time: i32,
    reapplication_delay: i32,
    duration_on_use: i32,
    radius_on_use: f32,
    radius_per_tick: f32,
    owner: Option<EntityReference>,
}

impl AreaEffectCloudState {
    fn new() -> Self {
        Self {
            potion_contents: PotionContents::empty(),
            potion_duration_scale: DEFAULT_POTION_DURATION_SCALE,
            victims: FxHashMap::default(),
            duration: INFINITE_DURATION,
            wait_time: DEFAULT_WAIT_TIME,
            reapplication_delay: DEFAULT_REAPPLICATION_DELAY,
            duration_on_use: 0,
            radius_on_use: 0.0,
            radius_per_tick: 0.0,
            owner: None,
        }
    }
}

/// An area effect cloud, spawned by lingering potions.
// TODO: Persist `custom_particle` once particle options have an NBT codec. Vanilla
// only sets it for dragon breath and commands; potion clouds always derive their
// particle from the potion color.
#[entity_behavior(class = "AreaEffectCloud")]
pub struct AreaEffectCloudEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    entity_data: SyncMutex<AreaEffectCloudEntityData>,
    state: SyncMutex<AreaEffectCloudState>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `AreaEffectCloudEntity`.
unsafe impl DowncastType for AreaEffectCloudEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/area_effect_cloud");
}

impl AreaEffectCloudEntity {
    /// Creates a new area effect cloud with vanilla defaults.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self {
            base: EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
            entity_data: SyncMutex::new(AreaEffectCloudEntityData::new()),
            state: SyncMutex::new(AreaEffectCloudState::new()),
        }
    }

    /// Creates an area effect cloud from saved base data.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self {
            base: EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
            entity_data: SyncMutex::new(AreaEffectCloudEntityData::new()),
            state: SyncMutex::new(AreaEffectCloudState::new()),
        }
    }

    /// Returns the current radius.
    #[must_use]
    pub fn radius(&self) -> f32 {
        *self.entity_data.lock().radius.get()
    }

    /// Sets the radius, clamped to `0..=32`, and resizes the bounding box.
    pub fn set_radius(&self, radius: f32) {
        self.entity_data
            .lock()
            .radius
            .set(radius.clamp(0.0, MAX_RADIUS));
        // Refreshing is idempotent: the cloud has no physics, so it never fudges its position.
        self.refresh_dimensions();
    }

    /// Returns whether the cloud is still in its initial wait period.
    #[must_use]
    pub fn is_waiting(&self) -> bool {
        *self.entity_data.lock().waiting.get()
    }

    fn set_waiting(&self, waiting: bool) {
        self.entity_data.lock().waiting.set(waiting);
    }

    /// Sets the potion contents and updates the synced particle color.
    pub fn set_potion_contents(&self, contents: PotionContents) {
        let color = contents.get_color();
        self.state.lock().potion_contents = contents;
        self.update_particle(color);
    }

    /// Sets the duration scale applied to the potion's effects.
    pub fn set_potion_duration_scale(&self, scale: f32) {
        self.state.lock().potion_duration_scale = scale;
    }

    fn update_particle(&self, potion_color: i32) {
        let color = RgbColor::new(potion_color).with_alpha(u8::MAX);
        self.entity_data.lock().particle.set(ParticleData::new(
            &vanilla_particle_types::ENTITY_EFFECT,
            ColorParticleOption::new(color),
        ));
    }

    /// Applies the potion components carried by `stack` to this cloud.
    pub fn apply_components_from_item_stack(&self, stack: &ItemStack) {
        if let Some(contents) = stack.get(vanilla_components::POTION_CONTENTS) {
            self.set_potion_contents(contents.clone());
        }
        if let Some(scale) = stack.get(vanilla_components::POTION_DURATION_SCALE) {
            self.set_potion_duration_scale(*scale);
        }
        if let Some(name) = stack.get(vanilla_components::CUSTOM_NAME) {
            self.set_custom_name(Some(name.clone()));
        }
        // TODO: Apply `CUSTOM_DATA` once entities store custom data.
    }

    /// Returns the duration in ticks, or `-1` for a cloud that never expires.
    #[must_use]
    pub fn duration(&self) -> i32 {
        self.state.lock().duration
    }

    /// Sets the duration in ticks, counted after the wait time.
    pub fn set_duration(&self, duration: i32) {
        self.state.lock().duration = duration;
    }

    /// Sets the radius change applied each time an entity is affected.
    pub fn set_radius_on_use(&self, radius_on_use: f32) {
        self.state.lock().radius_on_use = radius_on_use;
    }

    /// Sets the radius change applied every tick after the wait time.
    pub fn set_radius_per_tick(&self, radius_per_tick: f32) {
        self.state.lock().radius_per_tick = radius_per_tick;
    }

    /// Sets the number of ticks before the cloud starts applying effects.
    pub fn set_wait_time(&self, wait_time: i32) {
        self.state.lock().wait_time = wait_time;
    }

    /// Sets the living entity credited for the cloud's effects.
    pub fn set_owner(&self, owner: Option<&SharedEntity>) {
        self.state.lock().owner = owner
            .filter(|owner| owner.as_living_entity().is_some())
            .map(EntityReference::from_entity);
    }

    /// Resolves the owner, if it is a loaded living entity.
    #[must_use]
    pub fn get_owner(&self) -> Option<SharedEntity> {
        let world = self.level()?;
        let owner = self.state.lock().owner.clone()?;
        owner
            .get_entity(&world)
            .filter(|owner| owner.as_living_entity().is_some())
    }

    fn server_tick(self: &Arc<Self>, world: &World) {
        let tick_count = self.tick_count();
        let (duration, wait_time, radius_per_tick) = {
            let state = self.state.lock();
            (state.duration, state.wait_time, state.radius_per_tick)
        };
        if duration != INFINITE_DURATION && tick_count - wait_time >= duration {
            self.set_removed(RemovalReason::Discarded);
            return;
        }

        let should_wait = tick_count < wait_time;
        if self.is_waiting() != should_wait {
            self.set_waiting(should_wait);
        }
        if should_wait {
            return;
        }

        let mut radius = self.radius();
        if radius_per_tick != 0.0 {
            radius += radius_per_tick;
            if radius < MINIMAL_RADIUS {
                self.set_removed(RemovalReason::Discarded);
                return;
            }
            self.set_radius(radius);
        }

        if tick_count % TIME_BETWEEN_APPLICATIONS == 0 {
            self.apply_effects(world, tick_count, radius);
        }
    }

    /// Applies the cloud's effects to every eligible living entity inside it.
    fn apply_effects(self: &Arc<Self>, world: &World, tick_count: i32, mut radius: f32) {
        let (effects, reapplication_delay) = {
            let mut state = self.state.lock();
            state.victims.retain(|_, until| tick_count < *until);
            if !state.potion_contents.has_effects() {
                state.victims.clear();
                return;
            }
            let scale = state.potion_duration_scale;
            let effects: Vec<_> = state
                .potion_contents
                .all_effects()
                .iter()
                .map(|effect| effect.with_scaled_duration(scale))
                .collect();
            (effects, state.reapplication_delay)
        };

        let entities = world.get_entities_in_aabb_matching(&self.bounding_box(), |entity| {
            !entity.is_spectator() && entity.as_living_entity().is_some()
        });
        if entities.is_empty() {
            return;
        }

        let this: SharedEntity = Arc::<Self>::clone(self);
        let owner = self.get_owner();
        let position = self.position();

        for entity in entities {
            let Some(living) = entity.as_living_entity() else {
                continue;
            };
            if self.state.lock().victims.contains_key(&entity.id())
                || !living.is_affected_by_potions()
                || !effects.iter().any(|effect| {
                    living.can_be_affected(&to_runtime_instance(effect, effect.duration()))
                })
            {
                continue;
            }

            let offset = entity.position() - position;
            let dist = offset.x * offset.x + offset.z * offset.z;
            if dist > f64::from(radius * radius) {
                continue;
            }

            let (radius_on_use, duration_on_use) = {
                let mut state = self.state.lock();
                state
                    .victims
                    .insert(entity.id(), tick_count + reapplication_delay);
                (state.radius_on_use, state.duration_on_use)
            };

            for effect in &effects {
                self.apply_effect(world, living, effect, &this, owner.as_ref());
            }

            if radius_on_use != 0.0 {
                radius += radius_on_use;
                if radius < MINIMAL_RADIUS {
                    self.set_removed(RemovalReason::Discarded);
                    return;
                }
                self.set_radius(radius);
            }

            if duration_on_use != 0 && self.add_duration(duration_on_use) {
                self.set_removed(RemovalReason::Discarded);
                return;
            }
        }
    }

    fn apply_effect(
        &self,
        world: &World,
        living: &dyn LivingEntity,
        effect: &RegistryMobEffectInstance,
        this: &SharedEntity,
        owner: Option<&SharedEntity>,
    ) {
        let behavior = MOB_EFFECT_BEHAVIORS.get_behavior(effect.effect());
        if let Some(instantaneous) = behavior.as_instantaneous() {
            instantaneous.apply_instantaneous(
                world,
                living,
                effect.amplifier(),
                Some(this),
                owner,
                INSTANTANEOUS_EFFECT_SCALE,
            );
        } else {
            living.add_mob_effect_with_source(
                to_runtime_instance(effect, effect.duration()),
                Some(self as &dyn Entity),
            );
        }
    }

    /// Adds `delta` to a finite duration, returning whether the cloud has expired.
    fn add_duration(&self, delta: i32) -> bool {
        let mut state = self.state.lock();
        if state.duration == INFINITE_DURATION {
            return false;
        }
        state.duration += delta;
        state.duration <= 0
    }
}

impl Entity for AreaEffectCloudEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn tick(self: Arc<Self>) {
        self.default_tick();
        // Client-side particle rendering is driven by the synced radius, waiting
        // flag and particle, so only the server tick runs here.
        if let Some(world) = self.level() {
            self.server_tick(&world);
        }
    }

    fn no_physics(&self) -> bool {
        true
    }

    fn piston_push_reaction(&self) -> PushReaction {
        PushReaction::IgnoreEntity
    }

    fn dimensions_for_pose(&self, _pose: EntityPose) -> EntityDimensions {
        EntityDimensions::with_default_eye_height(self.radius() * 2.0, HEIGHT)
    }

    fn hurt(&self, _world: &World, _source: &DamageSource, _amount: f32) -> bool {
        false
    }

    fn restore_owner_reference(&self, owner: &SharedEntity) {
        if let Some(reference) = &self.state.lock().owner {
            reference.cache_entity(owner);
        }
    }

    fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
        Some(&self.entity_data)
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        nbt.insert("Age", self.tick_count());
        let state = self.state.lock();
        nbt.insert("Duration", state.duration);
        nbt.insert("WaitTime", state.wait_time);
        nbt.insert("ReapplicationDelay", state.reapplication_delay);
        nbt.insert("DurationOnUse", state.duration_on_use);
        nbt.insert("RadiusOnUse", state.radius_on_use);
        nbt.insert("RadiusPerTick", state.radius_per_tick);
        nbt.insert("Radius", self.radius());
        if let Some(owner) = &state.owner {
            owner.store(nbt, "Owner");
        }
        if state.potion_contents != PotionContents::empty() {
            nbt.insert(
                "potion_contents",
                state.potion_contents.clone().to_nbt_tag(),
            );
        }
        if state.potion_duration_scale != DEFAULT_POTION_DURATION_SCALE {
            nbt.insert("potion_duration_scale", state.potion_duration_scale);
        }
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.base.set_tick_count(nbt.int("Age").unwrap_or(0));
        {
            let mut state = self.state.lock();
            state.duration = nbt.int("Duration").unwrap_or(INFINITE_DURATION);
            state.wait_time = nbt.int("WaitTime").unwrap_or(DEFAULT_WAIT_TIME);
            state.reapplication_delay = nbt
                .int("ReapplicationDelay")
                .unwrap_or(DEFAULT_REAPPLICATION_DELAY);
            state.duration_on_use = nbt.int("DurationOnUse").unwrap_or(0);
            state.radius_on_use = nbt.float("RadiusOnUse").unwrap_or(0.0);
            state.radius_per_tick = nbt.float("RadiusPerTick").unwrap_or(0.0);
            state.owner = EntityReference::read(&nbt, "Owner");
            state.potion_duration_scale = nbt
                .float("potion_duration_scale")
                .unwrap_or(DEFAULT_POTION_DURATION_SCALE);
        }
        self.set_radius(nbt.float("Radius").unwrap_or(DEFAULT_RADIUS));
        self.set_potion_contents(
            nbt.get("potion_contents")
                .and_then(PotionContents::from_nbt_tag)
                .unwrap_or_else(PotionContents::empty),
        );
    }
}

#[cfg(test)]
mod tests;
