//! Area effect cloud entity (`AreaEffectCloud`).
//!
//! The server owns wait/duration, radius growth, potion application, and
//! reapplication tracking. Cloud particles are created by the client from
//! synchronized radius, waiting, and particle data.

use std::sync::Weak;

use glam::DVec3;
use rustc_hash::FxHashMap;
use simdnbt::ToNbtTag;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_macros::entity_behavior;
use steel_registry::data_components::PotionContents;
use steel_registry::data_components::vanilla_components::{POTION_CONTENTS, POTION_DURATION_SCALE};
use steel_registry::entity_data::EntityPose;
use steel_registry::entity_type::{EntityDimensions, EntityTypeRef};
use steel_registry::item_stack::ItemStack;
use steel_registry::mob_effect_instance::MobEffectInstance as RegistryMobEffectInstance;
use steel_registry::particle_type::{ColorParticleOption, ParticleData};
use steel_registry::vanilla_entity_data::AreaEffectCloudEntityData;
use steel_registry::vanilla_particle_types;
use steel_utils::ArgbColor;
use steel_utils::locks::SyncMutex;
use steel_utils::{DowncastType, DowncastTypeKey, UuidExt};
use uuid::Uuid;

use crate::entity::{
    Entity, EntityBase, EntityBaseLoad, EntitySyncedData, LivingEntity, MobEffectInstance,
    RemovalReason, SharedEntity,
};
use crate::world::World;

const TIME_BETWEEN_APPLICATIONS: i32 = 5;
const MAX_RADIUS: f32 = 32.0;
const MINIMAL_RADIUS: f32 = 0.5;
const DEFAULT_RADIUS: f32 = 3.0;
const HEIGHT: f32 = 0.5;
const INFINITE_DURATION: i32 = -1;
const DEFAULT_WAIT_TIME: i32 = 20;
const DEFAULT_REAPPLICATION_DELAY: i32 = 20;
const DEFAULT_DURATION_ON_USE: i32 = 0;
const DEFAULT_RADIUS_ON_USE: f32 = 0.0;
const DEFAULT_RADIUS_PER_TICK: f32 = 0.0;
const DEFAULT_POTION_DURATION_SCALE: f32 = 1.0;
const INSTANTANEOUS_EFFECT_SCALE: f64 = 0.5;

struct AreaEffectCloudState {
    potion_contents: PotionContents,
    potion_duration_scale: f32,
    custom_particle: Option<ParticleData>,
    victims: FxHashMap<i32, i32>,
    duration: i32,
    wait_time: i32,
    reapplication_delay: i32,
    duration_on_use: i32,
    radius_on_use: f32,
    radius_per_tick: f32,
    owner: Option<Uuid>,
}

impl AreaEffectCloudState {
    fn new() -> Self {
        Self {
            potion_contents: PotionContents::empty(),
            potion_duration_scale: DEFAULT_POTION_DURATION_SCALE,
            custom_particle: None,
            victims: FxHashMap::default(),
            duration: INFINITE_DURATION,
            wait_time: DEFAULT_WAIT_TIME,
            reapplication_delay: DEFAULT_REAPPLICATION_DELAY,
            duration_on_use: DEFAULT_DURATION_ON_USE,
            radius_on_use: DEFAULT_RADIUS_ON_USE,
            radius_per_tick: DEFAULT_RADIUS_PER_TICK,
            owner: None,
        }
    }
}

/// Vanilla area-effect cloud entity.
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
    /// Creates a new area-effect cloud for the entity factory.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        let cloud = Self {
            base: EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
            entity_data: SyncMutex::new(AreaEffectCloudEntityData::new()),
            state: SyncMutex::new(AreaEffectCloudState::new()),
        };
        cloud.set_no_physics(true);
        cloud
    }

    /// Creates an area-effect cloud from saved base data.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        let cloud = Self {
            base: EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
            entity_data: SyncMutex::new(AreaEffectCloudEntityData::new()),
            state: SyncMutex::new(AreaEffectCloudState::new()),
        };
        cloud.set_no_physics(true);
        cloud
    }

    /// Vanilla lingering-potion duration (`AreaEffectCloud.DEFAULT_LINGERING_DURATION`).
    pub const DEFAULT_LINGERING_DURATION: i32 = 600;

    /// Returns the synchronized cloud radius.
    #[must_use]
    pub fn radius(&self) -> f32 {
        *self.entity_data.lock().radius.get()
    }

    /// Sets the synchronized cloud radius, matching vanilla `setRadius`.
    pub fn set_radius(&self, radius: f32) {
        self.entity_data
            .lock()
            .radius
            .set(radius.clamp(0.0, MAX_RADIUS));
        self.refresh_dimensions();
    }

    /// Returns whether the cloud is in its wait period.
    #[must_use]
    pub fn is_waiting(&self) -> bool {
        *self.entity_data.lock().waiting.get()
    }

    /// Sets whether the cloud is waiting before applying effects.
    pub fn set_waiting(&self, waiting: bool) {
        self.entity_data.lock().waiting.set(waiting);
    }

    /// Returns the synchronized particle options.
    #[must_use]
    pub fn particle(&self) -> ParticleData {
        self.entity_data.lock().particle.get().clone()
    }

    /// Returns the current potion contents.
    #[must_use]
    pub fn potion_contents(&self) -> PotionContents {
        self.state.lock().potion_contents.clone()
    }

    /// Sets potion contents and refreshes the synchronized particle.
    pub fn set_potion_contents(&self, contents: PotionContents) {
        self.state.lock().potion_contents = contents;
        self.update_particle();
    }

    /// Sets an optional custom particle, matching vanilla `setCustomParticle`.
    pub fn set_custom_particle(&self, particle: Option<ParticleData>) {
        self.state.lock().custom_particle = particle;
        self.update_particle();
    }

    /// Adds a custom effect, matching vanilla `addEffect`.
    pub fn add_effect(&self, effect: RegistryMobEffectInstance) {
        let contents = self.state.lock().potion_contents.with_effect_added(effect);
        self.set_potion_contents(contents);
    }

    /// Returns the potion duration scale.
    #[must_use]
    pub fn potion_duration_scale(&self) -> f32 {
        self.state.lock().potion_duration_scale
    }

    /// Sets the potion duration scale.
    pub fn set_potion_duration_scale(&self, scale: f32) {
        self.state.lock().potion_duration_scale = scale;
    }

    /// Returns remaining duration in ticks, or `-1` for infinite.
    #[must_use]
    pub fn duration(&self) -> i32 {
        self.state.lock().duration
    }

    /// Sets remaining duration in ticks, or `-1` for infinite.
    pub fn set_duration(&self, duration: i32) {
        self.state.lock().duration = duration;
    }

    /// Returns wait time before the cloud starts applying effects.
    #[must_use]
    pub fn wait_time(&self) -> i32 {
        self.state.lock().wait_time
    }

    /// Sets wait time before the cloud starts applying effects.
    pub fn set_wait_time(&self, wait_time: i32) {
        self.state.lock().wait_time = wait_time;
    }

    /// Returns radius change applied when an entity is affected.
    #[must_use]
    pub fn radius_on_use(&self) -> f32 {
        self.state.lock().radius_on_use
    }

    /// Sets radius change applied when an entity is affected.
    pub fn set_radius_on_use(&self, radius_on_use: f32) {
        self.state.lock().radius_on_use = radius_on_use;
    }

    /// Returns radius change applied each tick after waiting.
    #[must_use]
    pub fn radius_per_tick(&self) -> f32 {
        self.state.lock().radius_per_tick
    }

    /// Sets radius change applied each tick after waiting.
    pub fn set_radius_per_tick(&self, radius_per_tick: f32) {
        self.state.lock().radius_per_tick = radius_per_tick;
    }

    /// Returns duration change applied when an entity is affected.
    #[must_use]
    pub fn duration_on_use(&self) -> i32 {
        self.state.lock().duration_on_use
    }

    /// Sets duration change applied when an entity is affected.
    pub fn set_duration_on_use(&self, duration_on_use: i32) {
        self.state.lock().duration_on_use = duration_on_use;
    }

    /// Sets the living owner UUID, matching vanilla `setOwner`.
    pub fn set_owner(&self, owner: Option<&dyn LivingEntity>) {
        self.state.lock().owner = owner.map(Entity::uuid);
    }

    /// Sets the owner UUID directly.
    pub fn set_owner_uuid(&self, owner: Option<Uuid>) {
        self.state.lock().owner = owner;
    }

    /// Returns the owner UUID.
    #[must_use]
    pub fn owner_uuid(&self) -> Option<Uuid> {
        self.state.lock().owner
    }

    /// Applies potion contents and duration scale from an item stack.
    pub fn apply_components_from_item_stack(&self, stack: &ItemStack) {
        if let Some(contents) = stack.get(POTION_CONTENTS).cloned() {
            self.set_potion_contents(contents);
        }
        if let Some(&scale) = stack.get(POTION_DURATION_SCALE) {
            self.set_potion_duration_scale(scale);
        }
    }

    fn owner(&self) -> Option<SharedEntity> {
        let uuid = self.state.lock().owner?;
        let world = self.level()?;
        let owner = world.get_entity_by_uuid(&uuid)?;
        owner.as_living_entity()?;
        Some(owner)
    }

    fn update_particle(&self) {
        let state = self.state.lock();
        let particle = if let Some(custom_particle) = &state.custom_particle {
            custom_particle.clone()
        } else {
            ParticleData::new(
                &vanilla_particle_types::ENTITY_EFFECT,
                ColorParticleOption::new(ArgbColor::new(opaque_argb(
                    state.potion_contents.color(),
                ))),
            )
        };
        drop(state);
        self.entity_data.lock().particle.set(particle);
    }

    fn discard(&self) {
        self.set_removed(RemovalReason::Discarded);
    }

    fn server_tick(&self) {
        let Some(radius) = self.tick_lifetime() else {
            return;
        };
        self.apply_effects_pulse(radius);
    }

    fn tick_lifetime(&self) -> Option<f32> {
        let tick_count = self.tick_count();
        let (duration, wait_time, radius_per_tick) = {
            let state = self.state.lock();
            (state.duration, state.wait_time, state.radius_per_tick)
        };

        if duration != INFINITE_DURATION && tick_count - wait_time >= duration {
            self.discard();
            return None;
        }

        let should_wait = tick_count < wait_time;
        if self.is_waiting() != should_wait {
            self.set_waiting(should_wait);
        }
        if should_wait {
            return None;
        }

        let mut radius = self.radius();
        if radius_per_tick != 0.0 {
            radius += radius_per_tick;
            if radius < MINIMAL_RADIUS {
                self.discard();
                return None;
            }
            self.set_radius(radius);
        }

        (tick_count % TIME_BETWEEN_APPLICATIONS == 0).then_some(radius)
    }

    fn apply_effects_pulse(&self, mut radius: f32) {
        let tick_count = self.tick_count();
        let (effects, reapplication_delay, radius_on_use, duration_on_use, has_effects) = {
            let mut state = self.state.lock();
            state.victims.retain(|_, expire_at| tick_count < *expire_at);
            let has_effects = state.potion_contents.has_effects();
            if !has_effects {
                state.victims.clear();
            }
            (
                state
                    .potion_contents
                    .effects_with_duration_scale(state.potion_duration_scale),
                state.reapplication_delay,
                state.radius_on_use,
                state.duration_on_use,
                has_effects,
            )
        };
        if !has_effects {
            return;
        }

        let Some(world) = self.level() else {
            return;
        };

        let cloud_pos = self.position();
        let entities = world.get_entities_in_aabb_matching(&self.bounding_box(), |entity| {
            entity.as_living_entity().is_some()
        });
        for entity in entities {
            let Some(living) = entity.as_living_entity() else {
                continue;
            };
            if self.state.lock().victims.contains_key(&entity.id()) {
                continue;
            }
            if !living.is_affected_by_potions() {
                continue;
            }
            let living_effects = effects
                .iter()
                .map(MobEffectInstance::from_potion_contents)
                .collect::<Vec<_>>();
            if living_effects
                .iter()
                .all(|effect| !living.can_be_affected(effect))
            {
                continue;
            }

            let dx = living.position().x - cloud_pos.x;
            let dz = living.position().z - cloud_pos.z;
            if dx * dx + dz * dz > f64::from(radius * radius) {
                continue;
            }

            self.state
                .lock()
                .victims
                .insert(entity.id(), tick_count + reapplication_delay);

            let owner = self.owner();
            let owner_ref = owner.as_deref();
            for (codec_effect, living_effect) in effects.iter().zip(living_effects) {
                if codec_effect.effect().is_instantaneous() {
                    living.apply_instantaneous_effect(
                        &world,
                        Some(self),
                        owner_ref,
                        codec_effect.effect(),
                        codec_effect.amplifier(),
                        INSTANTANEOUS_EFFECT_SCALE,
                    );
                } else {
                    living.add_mob_effect(living_effect);
                }
            }

            if radius_on_use != 0.0 {
                radius += radius_on_use;
                if radius < MINIMAL_RADIUS {
                    self.discard();
                    return;
                }
                self.set_radius(radius);
            }

            if duration_on_use != 0 {
                let new_duration = {
                    let mut state = self.state.lock();
                    if state.duration == INFINITE_DURATION {
                        continue;
                    }
                    state.duration += duration_on_use;
                    state.duration
                };
                if new_duration <= 0 {
                    self.discard();
                    return;
                }
            }
        }
    }
}

const fn opaque_argb(color: i32) -> i32 {
    (color as u32 | 0xFF00_0000) as i32
}

impl Entity for AreaEffectCloudEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn tick(&self) {
        self.default_tick();
        self.server_tick();
    }

    fn dimensions_for_pose(&self, _pose: EntityPose) -> EntityDimensions {
        EntityDimensions::scalable(self.radius() * 2.0, HEIGHT)
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
        if let Some(custom_particle) = &state.custom_particle {
            nbt.insert("custom_particle", custom_particle.to_nbt_tag_ref());
        }
        if let Some(owner) = state.owner {
            nbt.insert("Owner", NbtTag::IntArray(owner.to_int_array().to_vec()));
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
            state.duration_on_use = nbt.int("DurationOnUse").unwrap_or(DEFAULT_DURATION_ON_USE);
            state.radius_on_use = nbt.float("RadiusOnUse").unwrap_or(DEFAULT_RADIUS_ON_USE);
            state.radius_per_tick = nbt
                .float("RadiusPerTick")
                .unwrap_or(DEFAULT_RADIUS_PER_TICK);
            if let Some(owner) = nbt
                .int_array("Owner")
                .and_then(|arr| Uuid::from_int_array(&arr))
            {
                state.owner = Some(owner);
            }
        }

        if let Some(particle) = nbt.compound("custom_particle").and_then(|compound| {
            ParticleData::from_owned_nbt(&NbtTag::Compound(compound.to_owned()))
        }) {
            self.set_custom_particle(Some(particle));
        }

        let contents = nbt
            .compound("potion_contents")
            .and_then(|compound| {
                PotionContents::from_owned_nbt(&NbtTag::Compound(compound.to_owned()))
            })
            .or_else(|| {
                nbt.string("potion_contents").and_then(|value| {
                    PotionContents::from_owned_nbt(&NbtTag::String(value.to_string().into()))
                })
            })
            .unwrap_or_else(PotionContents::empty);
        self.set_potion_contents(contents);

        if let Some(scale) = nbt.float("potion_duration_scale") {
            self.set_potion_duration_scale(scale);
        }

        self.set_radius(nbt.float("Radius").unwrap_or(DEFAULT_RADIUS));
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::sync::{Arc, Weak};

    use simdnbt::borrow::read_compound as read_borrowed_compound;
    use steel_registry::RegistryReference;
    use steel_registry::init_vanilla_registry;
    use steel_registry::vanilla_entities;
    use steel_registry::vanilla_mob_effects;
    use steel_registry::vanilla_potions;
    use steel_utils::{ChunkPos, Downcast};

    use crate::entity::entities::PigEntity;
    use crate::entity::{ENTITIES, init_entities, next_entity_id};
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};

    use super::*;

    fn new_cloud() -> AreaEffectCloudEntity {
        init_vanilla_registry();
        AreaEffectCloudEntity::new(
            &vanilla_entities::AREA_EFFECT_CLOUD,
            1,
            DVec3::ZERO,
            Weak::new(),
        )
    }

    #[test]
    fn factory_creates_area_effect_cloud() {
        init_vanilla_registry();
        init_entities();
        let Some(entity) = ENTITIES.create(
            &vanilla_entities::AREA_EFFECT_CLOUD,
            1,
            DVec3::new(1.0, 2.0, 3.0),
            Weak::new(),
        ) else {
            panic!("area effect cloud factory should be registered");
        };
        assert!(entity.downcast_ref::<AreaEffectCloudEntity>().is_some());
        assert!(entity.no_physics());
    }

    #[test]
    fn defaults_match_vanilla() {
        let cloud = new_cloud();
        assert_eq!(cloud.radius().to_bits(), DEFAULT_RADIUS.to_bits());
        assert!(!cloud.is_waiting());
        assert_eq!(cloud.duration(), INFINITE_DURATION);
        assert_eq!(cloud.wait_time(), DEFAULT_WAIT_TIME);
        assert_eq!(
            cloud.potion_duration_scale().to_bits(),
            DEFAULT_POTION_DURATION_SCALE.to_bits()
        );
        assert!(cloud.no_physics());
        assert_eq!(
            cloud.particle().particle_type().key,
            vanilla_particle_types::ENTITY_EFFECT.key
        );
    }

    #[test]
    fn radius_clamps_and_refreshes_dimensions() {
        let cloud = new_cloud();
        cloud.set_radius(40.0);
        assert_eq!(cloud.radius().to_bits(), MAX_RADIUS.to_bits());
        assert_eq!(
            cloud
                .dimensions_for_pose(EntityPose::Standing)
                .width
                .to_bits(),
            (MAX_RADIUS * 2.0).to_bits()
        );

        cloud.set_radius(-1.0);
        assert_eq!(cloud.radius().to_bits(), 0.0_f32.to_bits());
    }

    #[test]
    fn wait_then_duration_discards_the_cloud() {
        let cloud = new_cloud();
        cloud.set_wait_time(2);
        cloud.set_duration(3);

        for _ in 0..4 {
            cloud.advance_tick_count();
            cloud.tick();
        }
        assert!(!cloud.is_removed());

        cloud.advance_tick_count();
        cloud.tick();
        assert!(cloud.is_removed());
    }

    #[test]
    fn shrinking_below_minimal_radius_discards() {
        let cloud = new_cloud();
        cloud.set_wait_time(0);
        cloud.set_radius(0.6);
        cloud.set_radius_per_tick(-0.2);

        cloud.advance_tick_count();
        cloud.tick();
        assert!(cloud.is_removed());
    }

    #[test]
    fn state_persists_with_vanilla_keys() {
        let cloud = new_cloud();
        cloud.base.set_tick_count(17);
        cloud.set_duration(600);
        cloud.set_wait_time(10);
        cloud.set_radius_on_use(-0.5);
        cloud.set_radius_per_tick(-0.005);
        cloud.set_duration_on_use(-1);
        cloud.set_radius(2.5);
        cloud.set_potion_duration_scale(0.25);
        cloud.set_owner_uuid(Some(Uuid::from_u128(42)));
        cloud.set_potion_contents(PotionContents::new(
            Some(RegistryReference::new(&vanilla_potions::POISON)),
            None,
            Vec::new(),
            None,
        ));
        cloud.set_custom_particle(Some(ParticleData::simple(&vanilla_particle_types::FLAME)));

        let mut nbt = NbtCompound::new();
        cloud.save_additional(&mut nbt);
        assert_eq!(nbt.int("Age"), Some(17));
        assert_eq!(nbt.int("Duration"), Some(600));
        assert_eq!(nbt.int("WaitTime"), Some(10));
        assert_eq!(nbt.float("RadiusOnUse"), Some(-0.5));
        assert_eq!(nbt.float("potion_duration_scale"), Some(0.25));

        let mut bytes = Vec::new();
        nbt.write(&mut bytes);
        let borrowed = read_borrowed_compound(&mut Cursor::new(&bytes))
            .unwrap_or_else(|error| panic!("test NBT should reborrow: {error}"));
        let loaded = AreaEffectCloudEntity::new(
            &vanilla_entities::AREA_EFFECT_CLOUD,
            2,
            DVec3::ZERO,
            Weak::new(),
        );
        loaded.load_additional((&borrowed).into());

        assert_eq!(loaded.tick_count(), 17);
        assert_eq!(loaded.duration(), 600);
        assert_eq!(loaded.wait_time(), 10);
        assert_eq!(loaded.radius().to_bits(), 2.5_f32.to_bits());
        assert_eq!(loaded.potion_duration_scale().to_bits(), 0.25_f32.to_bits());
        assert_eq!(loaded.owner_uuid(), Some(Uuid::from_u128(42)));
        assert!(loaded.potion_contents().is(&vanilla_potions::POISON));
        assert_eq!(
            loaded.particle().particle_type().key,
            vanilla_particle_types::FLAME.key
        );
    }

    #[test]
    fn applies_scaled_effects_inside_horizontal_radius() {
        init_vanilla_registry();
        let world = fresh_test_world("area_effect_cloud_poison");
        insert_ready_full_chunk(&world, ChunkPos::new(0, 0));

        let pig = Arc::new(PigEntity::new(
            &vanilla_entities::PIG,
            next_entity_id(),
            DVec3::new(8.0, 64.0, 8.0),
            Arc::downgrade(&world),
        ));
        world
            .try_add_entity(Arc::clone(&pig) as SharedEntity)
            .expect("pig should attach");

        let cloud = Arc::new(AreaEffectCloudEntity::new(
            &vanilla_entities::AREA_EFFECT_CLOUD,
            next_entity_id(),
            DVec3::new(8.0, 64.0, 8.0),
            Arc::downgrade(&world),
        ));
        cloud.set_wait_time(0);
        cloud.set_potion_contents(PotionContents::new(
            Some(RegistryReference::new(&vanilla_potions::POISON)),
            None,
            Vec::new(),
            None,
        ));
        world
            .try_add_entity(Arc::clone(&cloud) as SharedEntity)
            .expect("cloud should attach");

        for _ in 0..5 {
            cloud.advance_tick_count();
            cloud.tick();
        }

        assert!(pig.has_mob_effect(vanilla_mob_effects::POISON));
        let poison = pig
            .mob_effect(vanilla_mob_effects::POISON)
            .expect("poison should be applied");
        assert_eq!(poison.duration(), 900);
    }

    #[test]
    fn applies_instantaneous_damage_at_vanilla_cloud_scale() {
        init_vanilla_registry();
        let world = fresh_test_world("area_effect_cloud_harm");
        insert_ready_full_chunk(&world, ChunkPos::new(0, 0));

        let pig = Arc::new(PigEntity::new(
            &vanilla_entities::PIG,
            next_entity_id(),
            DVec3::new(8.0, 64.0, 8.0),
            Arc::downgrade(&world),
        ));
        world
            .try_add_entity(Arc::clone(&pig) as SharedEntity)
            .expect("pig should attach");

        let cloud = Arc::new(AreaEffectCloudEntity::new(
            &vanilla_entities::AREA_EFFECT_CLOUD,
            next_entity_id(),
            DVec3::new(8.0, 64.0, 8.0),
            Arc::downgrade(&world),
        ));
        cloud.set_wait_time(0);
        cloud.set_potion_contents(PotionContents::new(
            Some(RegistryReference::new(&vanilla_potions::HARMING)),
            None,
            Vec::new(),
            None,
        ));
        world
            .try_add_entity(Arc::clone(&cloud) as SharedEntity)
            .expect("cloud should attach");

        for _ in 0..5 {
            cloud.advance_tick_count();
            cloud.tick();
        }

        assert_eq!(pig.get_health().to_bits(), 7.0_f32.to_bits());
    }

    #[test]
    fn empty_potion_contents_are_not_saved() {
        let cloud = new_cloud();
        let mut nbt = NbtCompound::new();
        cloud.save_additional(&mut nbt);
        assert!(nbt.get("potion_contents").is_none());
        assert!(nbt.get("potion_duration_scale").is_none());
        assert_eq!(nbt.int("Duration"), Some(INFINITE_DURATION));
    }
}
