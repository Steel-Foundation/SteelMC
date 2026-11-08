//! Lingering cloud that applies its potion effects to living entities standing inside it.

use rustc_hash::FxHashMap;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use simdnbt::{FromNbtTag as _, ToNbtTag as _};
use steel_macros::entity_behavior;
use steel_registry::MobEffectInstance as RegistryMobEffectInstance;
use steel_registry::blocks::behavior::PushReaction;
use steel_registry::data_components::{PotionContents, vanilla_components};
use steel_registry::entity_type::{EntityDimensions, EntityTypeRef};
use steel_registry::particle_type::{ColorParticleOption, ParticleData};
use steel_registry::vanilla_entity_data::AreaEffectCloudEntityData;
use steel_registry::vanilla_particle_types;
use steel_utils::RgbColor;
use steel_utils::locks::CheckedMutex;

use crate::behavior::MOB_EFFECT_BEHAVIORS;
use crate::entity::component::definition::Definition;
use crate::entity::component::pipeline::{Flow, Stage, StageId};
use crate::entity::potion_contents::to_runtime_instance;
use crate::entity::prelude::*;
use crate::entity::{
    EntityBase, EntityProps, EntityReference, Living, RemovalReason, SharedEntity,
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
#[derive(Entity)]
#[entity(hooks = Self::ENTITY, save = Self::save_cloud, load = Self::load_cloud)]
#[entity(key = "steel:entity/area_effect_cloud", constructors, definition = Self::define)]
pub struct AreaEffectCloudEntity {
    #[entity(base)]
    base: EntityBase,
    #[entity(entity_type)]
    entity_type: EntityTypeRef,
    state: CheckedMutex<AreaEffectCloudState>,
    #[entity(synced)]
    entity_data: CheckedMutex<AreaEffectCloudEntityData>,
}

impl AreaEffectCloudEntity {
    const TICK: StageId = StageId("area_effect_cloud.tick");

    fn define() -> Definition<Self> {
        let definition = Definition::new();
        Definition {
            tick: definition.tick.then([Stage::new(
                Self::TICK,
                |cloud: &Self, entity: &mut &SharedEntity| {
                    // Clients draw the particles from the synced radius, waiting
                    // flag and particle; only the server tick runs here.
                    if let Some(world) = cloud.level() {
                        cloud.server_tick(entity, &world);
                    }
                    Flow::Continue
                },
            )]),
            ..definition
        }
    }

    const ENTITY: EntityProps<Self> = EntityProps {
        piston_push_reaction: |_| PushReaction::IgnoreEntity,
        dimensions_for_pose: |cloud, _| {
            EntityDimensions::with_default_eye_height(cloud.radius() * 2.0, HEIGHT)
        },
        hurt: |_, _, _, _| false,
        restore_owner_reference: |cloud, owner| {
            if let Some(reference) = &cloud.state.lock().owner {
                reference.cache_entity(owner);
            }
        },
        apply_implicit_item_components: |cloud, item_stack| {
            if let Some(contents) = item_stack.get(vanilla_components::POTION_CONTENTS) {
                cloud.set_potion_contents(contents.clone());
            }
            if let Some(scale) = item_stack.get(vanilla_components::POTION_DURATION_SCALE) {
                cloud.set_potion_duration_scale(*scale);
            }
        },
        ..EntityProps::new()
    };

    fn from_base(base: EntityBase, entity_type: EntityTypeRef) -> Self {
        base.set_no_physics(true);
        Self {
            base,
            entity_type,
            state: CheckedMutex::new(AreaEffectCloudState::new()),
            entity_data: CheckedMutex::new(AreaEffectCloudEntityData::new()),
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
    pub fn owner(&self) -> Option<SharedEntity> {
        let world = self.level()?;
        let owner = self.state.lock().owner.clone()?;
        owner
            .get_entity(&world)
            .filter(|owner| owner.as_living_entity().is_some())
    }

    fn server_tick(&self, entity: &SharedEntity, world: &World) {
        let tick_count = self.tick_count();
        let (duration, wait_time, radius_per_tick) = {
            let state = self.state.lock();
            (state.duration, state.wait_time, state.radius_per_tick)
        };
        if duration != INFINITE_DURATION && tick_count.wrapping_sub(wait_time) >= duration {
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
            self.apply_effects(entity, world, tick_count, radius);
        }
    }

    /// Applies the cloud's effects to every eligible living entity inside it.
    fn apply_effects(&self, this: &SharedEntity, world: &World, tick_count: i32, mut radius: f32) {
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

        let owner = self.owner();
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
                    .insert(entity.id(), tick_count.wrapping_add(reapplication_delay));
                (state.radius_on_use, state.duration_on_use)
            };

            for effect in &effects {
                self.apply_effect(world, &living, effect, this, owner.as_ref());
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
        living: &Living<'_>,
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
                0.5,
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
        state.duration = state.duration.wrapping_add(delta);
        state.duration <= 0
    }
}

impl AreaEffectCloudEntity {
    fn save_cloud(&self, nbt: &mut NbtCompound) {
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

    fn load_cloud(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
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
