//! Vanilla `AgeableMob`: age, forced growth and age locking.
//!
//! State lives in the [`Age`] component; behavior lives on the [`Ageable`]
//! view, which any code holding an entity can obtain with [`Ageable::of`].
//! The `aiStep`, `finalizeSpawn` and `mobInteract` layers are pipeline stages
//! in this module.

use crate::entity::prelude::*;
use std::sync::Arc;

use glam::DVec3;

use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_protocol::packets::game::SoundSource;
use steel_registry::particle_type::ParticleData;
use steel_registry::vanilla_entity_data::AgeableMobEntityData;
use steel_registry::vanilla_entity_type_tags::EntityTypeTag;
use steel_registry::{
    REGISTRY, TaggedRegistryExt, sound_events, vanilla_items, vanilla_particle_types,
};
use steel_utils::locks::CheckedMutex;
use steel_utils::{DowncastType, DowncastTypeKey};

use crate::behavior::InteractionResult;
use crate::entity::component::entries::{
    AiStep, FinalizeSpawn, FinalizeSpawnCtx, MobInteract, MobInteractCtx,
};
use crate::entity::component::pipeline::{Flow, Stage, StageId};
use crate::entity::component::{Component, Components as _, Has};
use crate::entity::next_entity_id;
use crate::entity::{AgeableMobGroupData, ENTITIES, Entity, SharedEntity, SpawnGroupData, mob};
use crate::world::World;

/// Age assigned to a newly spawned or bred baby (`AgeableMob.BABY_START_AGE`).
pub const BABY_START_AGE: i32 = -24_000;
const AGE_LOCK_COOLDOWN_TICKS: i32 = 40;
const FORCED_AGE_PARTICLE_TICKS: i32 = 40;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct AgeState {
    age: i32,
    forced_age: i32,
    forced_age_timer: i32,
    age_lock_particle_timer: i32,
}

/// Per-type properties of a vanilla `AgeableMob`.
pub struct AgeableProps {
    /// `AgeableMob.getBreedOffspring`: creates the offspring of `parent` and
    /// `partner`. Spawn eggs used on an adult pass the parent as partner.
    pub breed_offspring:
        fn(parent: &Ageable<'_>, world: &Arc<World>, partner: &Ageable<'_>) -> Option<SharedEntity>,
}

/// Age state of a vanilla `AgeableMob`. The baby and age-locked flags are
/// synchronized data and live in the entity's generated synced data.
pub struct Age {
    state: CheckedMutex<AgeState>,
    props: &'static AgeableProps,
}

impl Age {
    /// An adult with no forced growth, with `props` describing its type.
    #[must_use]
    pub const fn new(props: &'static AgeableProps) -> Self {
        Self {
            state: CheckedMutex::new(AgeState {
                age: 0,
                forced_age: 0,
                forced_age_timer: 0,
                age_lock_particle_timer: 0,
            }),
            props,
        }
    }
}

// SAFETY: This key is owned by Steel and uniquely identifies `Age`.
unsafe impl DowncastType for Age {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity_component/age");
}

impl Component for Age {
    fn save(&self, owner: &dyn Entity, nbt: &mut NbtCompound) {
        let ageable = Ageable::new(owner, self);
        nbt.insert("Age", ageable.age());
        nbt.insert("ForcedAge", ageable.forced_age());
        nbt.insert("AgeLocked", i8::from(ageable.is_age_locked()));
    }

    fn load(&self, owner: &dyn Entity, nbt: BorrowedNbtCompoundView<'_, '_>) {
        let ageable = Ageable::new(owner, self);
        ageable.set_age(nbt.int("Age").unwrap_or(0));
        self.state.lock().forced_age = nbt.int("ForcedAge").unwrap_or(0);
        ageable.set_age_locked(nbt.byte("AgeLocked").is_some_and(|value| value != 0));
    }
}

/// An entity seen as a vanilla `AgeableMob`.
#[derive(Clone, Copy)]
pub struct Ageable<'a> {
    entity: &'a dyn Entity,
    age: &'a Age,
}

impl<'a> Ageable<'a> {
    /// Pairs `entity` with its age component.
    #[must_use]
    pub const fn new(entity: &'a dyn Entity, age: &'a Age) -> Self {
        Self { entity, age }
    }

    /// Returns `entity` as an ageable mob, if it is one.
    #[must_use]
    pub fn of(entity: &'a dyn Entity) -> Option<Self> {
        Some(Self::new(entity, entity.query::<Age>()?))
    }

    /// Returns the entity this view belongs to.
    #[must_use]
    pub const fn entity(&self) -> &'a dyn Entity {
        self.entity
    }

    fn with_synced<R>(&self, f: impl FnOnce(&mut AgeableMobEntityData) -> R) -> Option<R> {
        let synced = self.entity.synced_data()?;
        let mut data = synced.lock_data();
        data.ageable_mob_layer_mut().map(f)
    }

    /// Age in ticks: negative while a baby, reaching zero at adulthood.
    #[must_use]
    pub fn age(&self) -> i32 {
        self.age.state.lock().age
    }

    /// Sets the age, updating the synchronized baby flag when the
    /// baby/adult boundary is crossed (`AgeableMob.setAge`).
    pub fn set_age(&self, age: i32) {
        let old_age = {
            let mut state = self.age.state.lock();
            let old_age = state.age;
            state.age = age;
            old_age
        };
        if (old_age < 0) != (age < 0) {
            let baby = age < 0;
            self.with_synced(|data| data.baby.set(baby));
            // Vanilla refreshes from `onSyncedDataUpdated(DATA_BABY_ID)`.
            // TODO: `ageBoundaryReached` also dismounts grown mobs from boats
            // without space for them.
            self.entity.refresh_dimensions();
        }
    }

    /// Returns whether this mob is a baby.
    #[must_use]
    pub fn is_baby(&self) -> bool {
        self.age() < 0
    }

    /// Makes this mob a baby or an adult (`AgeableMob.setBaby`).
    pub fn set_baby(&self, baby: bool) {
        self.set_age(if baby { BABY_START_AGE } else { 0 });
    }

    /// Returns the synchronized age-lock flag.
    #[must_use]
    pub fn is_age_locked(&self) -> bool {
        self.with_synced(|data| *data.age_locked.get())
            .unwrap_or(false)
    }

    /// Sets the synchronized age-lock flag.
    pub fn set_age_locked(&self, age_locked: bool) {
        self.with_synced(|data| data.age_locked.set(age_locked));
    }

    /// Extra age credited by forced growth, reapplied once natural age reaches zero.
    #[must_use]
    pub fn forced_age(&self) -> i32 {
        self.age.state.lock().forced_age
    }

    /// Ticks remaining in the forced-growth particle effect.
    #[must_use]
    pub fn forced_age_timer(&self) -> i32 {
        self.age.state.lock().forced_age_timer
    }

    /// Copies forced growth from `other`, as mob conversion does.
    pub fn copy_forced_growth_from(&self, other: &Ageable<'_>) {
        let (forced_age, forced_age_timer) = {
            let other = other.age.state.lock();
            (other.forced_age, other.forced_age_timer)
        };
        let mut state = self.age.state.lock();
        state.forced_age = forced_age;
        state.forced_age_timer = forced_age_timer;
    }

    /// Returns whether this mob can naturally age toward adulthood.
    #[must_use]
    pub fn can_age_up(&self) -> bool {
        self.is_baby() && !self.is_age_locked()
    }

    /// Seconds of growth gained from feeding: a tenth of the time left, in seconds.
    #[must_use]
    pub const fn speed_up_seconds_when_feeding(ticks_until_adult: i32) -> i32 {
        ((ticks_until_adult / 20) as f32 * 0.1) as i32
    }

    /// Advances age toward adulthood by `seconds`; forced growth is also
    /// credited to the forced age and restarts its particle timer.
    pub fn age_up(&self, seconds: i32, forced: bool) {
        let old_age = self.age();
        let age = (old_age + seconds * 20).min(0);
        self.set_age(age);
        if forced {
            let mut state = self.age.state.lock();
            state.forced_age += age - old_age;
            if state.forced_age_timer == 0 {
                state.forced_age_timer = FORCED_AGE_PARTICLE_TICKS;
            }
        }

        if self.age() == 0 {
            self.set_age(self.forced_age());
        }
    }

    /// Creates this mob's offspring with `partner` (`getBreedOffspring`).
    #[must_use]
    pub fn breed_offspring(
        &self,
        world: &Arc<World>,
        partner: &Ageable<'_>,
    ) -> Option<SharedEntity> {
        (self.age.props.breed_offspring)(self, world, partner)
    }

    /// Creates a same-type entity at this mob's position to become offspring.
    #[must_use]
    pub fn create_offspring(&self, world: &Arc<World>) -> Option<SharedEntity> {
        ENTITIES.create(
            self.entity.entity_type(),
            next_entity_id(),
            self.entity.position(),
            Arc::downgrade(world),
        )
    }

    /// `AgeableMob.aiStep` after `super`: forced-growth particles, natural growth
    /// and the age-lock particles.
    pub fn tick(&self) {
        let world = self.entity.level();
        let forced_age_particle = {
            let mut state = self.age.state.lock();
            let send = state.forced_age_timer > 0 && state.forced_age_timer % 4 == 0;
            if state.forced_age_timer > 0 {
                state.forced_age_timer -= 1;
            }
            send
        };
        if forced_age_particle && let Some(world) = &world {
            let entity = self.entity;
            world.send_particles(
                ParticleData::simple(&vanilla_particle_types::HAPPY_VILLAGER),
                DVec3::new(
                    entity.random_x(1.0),
                    entity.random_y() + 0.5,
                    entity.random_z(1.0),
                ),
                1,
                DVec3::ZERO,
                0.0,
            );
        }

        if self.entity.is_alive() {
            let age = self.age();
            if self.can_age_up() {
                self.set_age(age + 1);
            } else if age > 0 {
                self.set_age(age - 1);
            }
        }

        let age_lock_particle = {
            let mut state = self.age.state.lock();
            let send = state.age_lock_particle_timer > 0 && state.age_lock_particle_timer % 2 == 0;
            if state.age_lock_particle_timer > 0 {
                state.age_lock_particle_timer -= 1;
            }
            send
        };
        if age_lock_particle && let Some(world) = &world {
            self.send_age_lock_particle(world);
        }
    }

    /// One `AgeableMob.makeAgeLockedParticle` burst above the head.
    fn send_age_lock_particle(&self, world: &World) {
        let entity = self.entity;
        let age_locked = self.is_age_locked();
        let (particle, y_offset) = if age_locked {
            (
                &vanilla_particle_types::PAUSE_MOB_GROWTH,
                f64::from(0.2_f32),
            )
        } else {
            (&vanilla_particle_types::RESET_MOB_GROWTH, 0.0)
        };
        let height = f64::from(entity.base().dimensions().height);
        world.send_particles(
            ParticleData::simple(particle),
            DVec3::new(
                entity.random_x(1.0),
                entity.random_y_spread(0.2) + height + y_offset,
                entity.random_z(1.0),
            ),
            1,
            DVec3::ZERO,
            0.0,
        );
    }
}

/// Stage running [`Ageable::tick`].
pub const AI_STEP: StageId = StageId("ageable.ai_step");
/// Stage rolling whether a spawned group member is a baby.
pub const FINALIZE_SPAWN: StageId = StageId("ageable.finalize_spawn");
/// Stage toggling the age lock with a golden dandelion.
pub const MOB_INTERACT: StageId = StageId("ageable.mob_interact");

/// The `AgeableMob.aiStep` stage.
pub fn ai_step<E: Entity + Has<Age>>() -> Stage<E, AiStep> {
    fn run<E: Entity + Has<Age>>(entity: &E) {
        Ageable::new(entity, entity.get::<Age>()).tick();
    }
    Stage::effect(AI_STEP, run::<E>)
}

/// The `AgeableMob.finalizeSpawn` stage, before `Mob.finalizeSpawn`.
pub fn finalize_spawn<E: Entity + Has<Age>>() -> Stage<E, FinalizeSpawn> {
    fn run<E: Entity + Has<Age>>(entity: &E, ctx: &mut FinalizeSpawnCtx<'_>) -> Flow {
        let mut group_data = match ctx.group_data.take() {
            Some(SpawnGroupData::AgeableMob(group_data)) => group_data,
            None => AgeableMobGroupData::with_should_spawn_baby(true),
        };
        if group_data.finalize_ageable_spawn(rand::random::<f32>) {
            Ageable::new(entity, entity.get::<Age>()).set_age(BABY_START_AGE);
        }
        ctx.group_data = Some(SpawnGroupData::AgeableMob(group_data));
        Flow::Continue
    }
    Stage::new(FINALIZE_SPAWN, run::<E>)
}

/// The `AgeableMob.mobInteract` stage: toggles a baby's age lock when fed a
/// golden dandelion.
pub fn mob_interact<E: mob::Parts + Has<Age>>() -> Stage<E, MobInteract> {
    fn run<E: mob::Parts + Has<Age>>(entity: &E, ctx: &mut MobInteractCtx<'_>) -> Flow {
        let ageable = Ageable::new(entity, entity.get::<Age>());
        let holds_dandelion = ctx
            .player
            .inventory
            .lock()
            .get_item_in_hand(ctx.hand)
            .is(&vanilla_items::GOLDEN_DANDELION);
        if !holds_dandelion
            || !ageable.is_baby()
            || ageable.age.state.lock().age_lock_particle_timer != 0
            || REGISTRY
                .entity_types
                .is_in_tag(entity.entity_type(), &EntityTypeTag::CANNOT_BE_AGE_LOCKED)
        {
            return Flow::Continue;
        }

        let age_locked = !ageable.is_age_locked();
        ageable.set_age_locked(age_locked);
        ageable.set_age(BABY_START_AGE);
        ageable.age.state.lock().age_lock_particle_timer = AGE_LOCK_COOLDOWN_TICKS;
        entity.mob().use_player_item(ctx.player, ctx.hand);

        if age_locked {
            entity.mob().set_persistence_required();
        }
        if let Some(world) = entity.level() {
            let sound = if age_locked {
                &sound_events::ITEM_GOLDEN_DANDELION_USE
            } else {
                &sound_events::ITEM_GOLDEN_DANDELION_UNUSE
            };
            world.play_sound(
                sound,
                SoundSource::Players,
                entity.block_position(),
                1.0,
                1.0,
                None,
            );
        }

        ctx.result = InteractionResult::Success;
        Flow::Stop
    }
    Stage::new(MOB_INTERACT, run::<E>)
}

#[cfg(test)]
mod tests {
    use std::sync::Weak;

    use glam::DVec3;
    use steel_registry::{init_vanilla_registry, vanilla_entities};

    use super::{Age, Ageable};
    use crate::entity::Entity;
    use crate::entity::component::Components as _;
    use crate::entity::entities::ChickenEntity;

    fn chicken() -> ChickenEntity {
        init_vanilla_registry();
        ChickenEntity::new(&vanilla_entities::CHICKEN, 1, DVec3::ZERO, Weak::new())
    }

    fn ageable(chicken: &ChickenEntity) -> Ageable<'_> {
        Ageable::new(chicken, chicken.get::<Age>())
    }

    #[test]
    fn age_boundary_updates_synced_baby_flag() {
        let chicken = chicken();
        let ageable = ageable(&chicken);

        ageable.set_age(-1);
        assert!(ageable.is_baby());
        assert!(
            chicken
                .synced_data()
                .and_then(|data| data.lock_data().ageable_mob_layer().map(|l| *l.baby.get()))
                .expect("chickens have ageable synced data")
        );

        ageable.set_age(0);
        assert!(!ageable.is_baby());
    }

    #[test]
    fn age_tick_grows_babies_and_reduces_positive_age() {
        let chicken = chicken();
        let ageable = ageable(&chicken);

        ageable.set_age(-2);
        ageable.tick();
        assert_eq!(ageable.age(), -1);

        ageable.set_age(2);
        ageable.tick();
        assert_eq!(ageable.age(), 1);
    }

    #[test]
    fn age_lock_blocks_baby_growth() {
        let chicken = chicken();
        let ageable = ageable(&chicken);

        ageable.set_age(-2);
        ageable.set_age_locked(true);
        ageable.tick();

        assert_eq!(ageable.age(), -2);
    }

    #[test]
    fn age_up_adds_forced_age_and_timer() {
        let chicken = chicken();
        let ageable = ageable(&chicken);

        ageable.set_age(-100);
        ageable.age_up(1, true);

        assert_eq!(ageable.age(), -80);
        assert_eq!(ageable.forced_age(), 20);
        assert_eq!(ageable.forced_age_timer(), 40);
    }

    #[test]
    fn age_up_applies_forced_age_when_reaching_adulthood() {
        let chicken = chicken();
        let ageable = ageable(&chicken);

        ageable.set_age(-10);
        ageable.age_up(1, true);

        assert_eq!(ageable.age(), 10);
        assert_eq!(ageable.forced_age(), 10);
    }

    #[test]
    fn feeding_speed_up_seconds_matches_vanilla_integer_order() {
        assert_eq!(Ageable::speed_up_seconds_when_feeding(24_000), 120);
        assert_eq!(Ageable::speed_up_seconds_when_feeding(199), 0);
    }

    #[test]
    fn dynamic_lookup_finds_the_age_component() {
        let chicken = chicken();
        let entity: &dyn Entity = &chicken;
        assert!(Ageable::of(entity).is_some());
    }
}
