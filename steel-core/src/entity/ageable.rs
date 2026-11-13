//! Vanilla `AgeableMob`: age, forced growth and age locking.
//!
//! State lives in the [`Age`] component; behavior lives on the [`Ageable`]
//! view, which any code holding an entity can obtain with [`Ageable::of`].
//! [`AgeableProps`] holds the methods `AgeableMob` declares, and
//! [`living_props`] and [`mob_props`] are the presets an ageable mob type
//! builds its other tables on.

use crate::entity::EntitySpawnReason;
use crate::entity::LivingProps;
use crate::entity::MobProps;
use crate::entity::prelude::*;
use crate::physics::MoveResult;
use crate::player::Player;
use std::sync::Arc;
use steel_utils::nbt::NbtValueInput as _;
use steel_utils::types::InteractionHand;

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
use crate::entity::component::props::layer_props;
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

layer_props! {
    /// Per-type properties of a vanilla `AgeableMob`.
    pub struct AgeableProps<E>: AgeableMethods for Ageable
    where E: Parts,
    table AgeableTable::AGEABLE,
    component Age {
        /// `AgeableMob.getBreedOffspring`: creates this mob's offspring with
        /// `partner`. Spawn eggs used on an adult pass the parent as partner.
        fn breed_offspring(world: &Arc<World>, partner: &Ageable<'_>) -> Option<SharedEntity>;
    }
}

/// `AgeableMob`'s [`AgeableProps::breed_offspring`]: vanilla's is abstract, so
/// a type without one has no offspring.
pub const fn breed_offspring<E: Parts>(
    _entity: &E,
    _world: &Arc<World>,
    _partner: &Ageable<'_>,
) -> Option<SharedEntity> {
    None
}

/// A concrete ageable mob type: its age component and its table.
pub trait AgeableTable: mob::Parts + Has<Age> {
    /// The `AgeableMob` properties this type runs.
    const AGEABLE: AgeableProps<Self>;
}

/// Every ageable mob type; keeps generic bounds short.
pub trait Parts: AgeableTable {
    /// Returns this entity as an ageable mob.
    fn ageable(&self) -> Ageable<'_>;
}

impl<E: AgeableTable> Parts for E {
    fn ageable(&self) -> Ageable<'_> {
        Ageable::new(self)
    }
}

/// Age state of a vanilla `AgeableMob`. The baby and age-locked flags are
/// synchronized data and live in the entity's generated synced data.
pub struct Age {
    state: CheckedMutex<AgeState>,
}

impl Age {
    /// An adult with no forced growth.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            state: CheckedMutex::new(AgeState {
                age: 0,
                forced_age: 0,
                forced_age_timer: 0,
                age_lock_particle_timer: 0,
            }),
        }
    }
}

// SAFETY: This key is owned by Steel and uniquely identifies `Age`.
unsafe impl DowncastType for Age {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity_component/age");
}

impl Component for Age {
    fn save(&self, owner: &dyn Entity, nbt: &mut NbtCompound) {
        let Some(ageable) = Ageable::of(owner) else {
            return;
        };
        nbt.insert("Age", ageable.age());
        nbt.insert("ForcedAge", ageable.forced_age());
        nbt.insert("AgeLocked", i8::from(ageable.is_age_locked()));
    }

    fn load(&self, owner: &dyn Entity, nbt: BorrowedNbtCompoundView<'_, '_>) {
        let Some(ageable) = Ageable::of(owner) else {
            return;
        };
        ageable.set_age(nbt.int_value("Age").unwrap_or(0));
        self.state.lock().forced_age = nbt.int_value("ForcedAge").unwrap_or(0);
        ageable.set_age_locked(nbt.byte_value("AgeLocked").is_some_and(|value| value != 0));
    }
}

/// An entity seen as a vanilla `AgeableMob`.
#[derive(Clone, Copy)]
pub struct Ageable<'a> {
    entity: &'a dyn Entity,
    age: &'a Age,
    methods: &'a dyn AgeableMethods,
}

impl<'a> Ageable<'a> {
    /// Pairs a concrete ageable mob with its state.
    #[must_use]
    pub fn new<E: Parts>(entity: &'a E) -> Self {
        Self {
            entity,
            age: entity.get::<Age>(),
            methods: entity,
        }
    }

    /// Returns `entity` as an ageable mob, if it is one.
    #[must_use]
    pub fn of(entity: &'a dyn Entity) -> Option<Self> {
        let methods = entity.as_ageable_methods()?;
        Some(Self {
            entity,
            age: methods.layer_component(),
            methods,
        })
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

/// `AgeableMob`'s [`LivingProps::ai_step`].
pub fn ai_step<E: Parts>(entity: &E) -> Option<MoveResult> {
    let movement = mob::ai_step(entity);
    entity.ageable().tick();
    movement
}

/// `AgeableMob`'s changes to the living defaults.
#[must_use]
pub const fn living_props<E: Parts>() -> LivingProps<E> {
    LivingProps {
        ai_step: ai_step::<E>,
        ..mob::living_props()
    }
}

/// `AgeableMob`'s [`MobProps::mob_interact`].
pub fn mob_interact<E: Parts>(
    entity: &E,
    player: &Player,
    hand: InteractionHand,
) -> InteractionResult {
    if let Some(result) = toggle_age_lock(entity, player, hand) {
        return result;
    }
    mob::mob_interact(entity, player, hand)
}

/// `AgeableMob`'s [`MobProps::finalize_spawn`].
///
/// `AgeableMob.finalizeSpawn` rolls whether a spawned group member is
/// a baby before `super`.
pub fn finalize_spawn<E: Parts>(
    entity: &E,
    world: &Arc<World>,
    reason: EntitySpawnReason,
    group_data: Option<SpawnGroupData>,
) -> Option<SpawnGroupData> {
    let mut group_data = match group_data {
        Some(SpawnGroupData::AgeableMob(group_data)) => group_data,
        None => AgeableMobGroupData::with_should_spawn_baby(true),
    };
    if group_data.finalize_ageable_spawn(rand::random::<f32>) {
        entity.ageable().set_age(BABY_START_AGE);
    }
    mob::finalize_spawn(
        entity,
        world,
        reason,
        Some(SpawnGroupData::AgeableMob(group_data)),
    )
}

/// `AgeableMob`'s changes to the mob defaults.
#[must_use]
pub const fn mob_props<E: Parts>() -> MobProps<E> {
    MobProps {
        mob_interact: mob_interact::<E>,
        finalize_spawn: finalize_spawn::<E>,
        ..mob::pathfinder::mob_props()
    }
}

/// `AgeableMob.mobInteract` before `super`: toggles a baby's age lock when fed
/// a golden dandelion. `None` when the interaction is not this one.
fn toggle_age_lock<E: Parts>(
    entity: &E,
    player: &Player,
    hand: InteractionHand,
) -> Option<InteractionResult> {
    let ageable = entity.ageable();
    let holds_dandelion = player
        .inventory
        .lock()
        .get_item_in_hand(hand)
        .is(&vanilla_items::GOLDEN_DANDELION);
    if !holds_dandelion
        || !ageable.is_baby()
        || ageable.age.state.lock().age_lock_particle_timer != 0
        || REGISTRY
            .entity_types
            .is_in_tag(entity.entity_type(), &EntityTypeTag::CANNOT_BE_AGE_LOCKED)
    {
        return None;
    }

    let age_locked = !ageable.is_age_locked();
    ageable.set_age_locked(age_locked);
    ageable.set_age(BABY_START_AGE);
    ageable.age.state.lock().age_lock_particle_timer = AGE_LOCK_COOLDOWN_TICKS;
    entity.mob().use_player_item(player, hand);

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

    Some(InteractionResult::Success)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Weak};

    use glam::DVec3;
    use steel_registry::{init_vanilla_registry, vanilla_entities};

    use super::Ageable;
    use crate::entity::Entity;
    use crate::entity::entities::ChickenEntity;

    fn chicken() -> Arc<ChickenEntity> {
        init_vanilla_registry();
        ChickenEntity::new(&vanilla_entities::CHICKEN, 1, DVec3::ZERO, Weak::new())
    }

    fn ageable(chicken: &ChickenEntity) -> Ageable<'_> {
        Ageable::new(chicken)
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
        let entity: &dyn Entity = &*chicken;
        assert!(Ageable::of(entity).is_some());
    }
}
