//! Shared vanilla `AgeableMob` state and hooks.

use std::sync::Arc;

use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_protocol::packets::game::SoundSource;
use steel_registry::vanilla_entity_type_tags::EntityTypeTag;
use steel_registry::{REGISTRY, TaggedRegistryExt, sound_events, vanilla_items};
use steel_utils::locks::SyncMutex;
use steel_utils::types::InteractionHand;

use crate::behavior::InteractionResult;
use crate::entity::{AgeableMobGroupData, Entity, EntitySpawnReason, Mob, SpawnGroupData};
use crate::player::Player;
use crate::world::World;

const BABY_START_AGE: i32 = -24_000;
const AGE_LOCK_COOLDOWN_TICKS: i32 = 40;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AgeableMobState {
    age: i32,
    forced_age: i32,
    forced_age_timer: i32,
    age_lock_particle_timer: i32,
}

impl AgeableMobState {
    const fn new() -> Self {
        Self {
            age: 0,
            forced_age: 0,
            forced_age_timer: 0,
            age_lock_particle_timer: 0,
        }
    }
}

/// Runtime fields shared by vanilla ageable mobs.
#[derive(Debug)]
pub struct AgeableMobBase {
    state: SyncMutex<AgeableMobState>,
}

impl AgeableMobBase {
    /// Creates default ageable runtime state.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            state: SyncMutex::new(AgeableMobState::new()),
        }
    }

    /// Returns vanilla `AgeableMob.age`.
    #[must_use]
    pub fn age(&self) -> i32 {
        self.state.lock().age
    }

    /// Sets vanilla `AgeableMob.age`, returning whether the baby/adult boundary changed.
    pub fn set_age(&self, age: i32) -> bool {
        let mut state = self.state.lock();
        let old_age = state.age;
        state.age = age;
        old_age < 0 && age >= 0 || old_age >= 0 && age < 0
    }

    /// Returns vanilla `AgeableMob.forcedAge`.
    #[must_use]
    pub fn forced_age(&self) -> i32 {
        self.state.lock().forced_age
    }

    /// Sets vanilla `AgeableMob.forcedAge`.
    pub fn set_forced_age(&self, forced_age: i32) {
        self.state.lock().forced_age = forced_age;
    }

    /// Returns vanilla `AgeableMob.forcedAgeTimer`.
    #[must_use]
    pub fn forced_age_timer(&self) -> i32 {
        self.state.lock().forced_age_timer
    }

    /// Sets vanilla `AgeableMob.forcedAgeTimer`.
    pub fn set_forced_age_timer(&self, forced_age_timer: i32) {
        self.state.lock().forced_age_timer = forced_age_timer;
    }

    /// Returns vanilla `AgeableMob.ageLockParticleTimer`.
    #[must_use]
    pub fn age_lock_particle_timer(&self) -> i32 {
        self.state.lock().age_lock_particle_timer
    }

    /// Sets vanilla `AgeableMob.ageLockParticleTimer`.
    pub fn set_age_lock_particle_timer(&self, timer: i32) {
        self.state.lock().age_lock_particle_timer = timer;
    }

    /// Adds to vanilla `AgeableMob.forcedAge`.
    pub fn add_forced_age(&self, delta: i32) {
        self.state.lock().forced_age += delta;
    }

    /// Returns vanilla `AgeableMob.getSpeedUpSecondsWhenFeeding`.
    #[must_use]
    pub fn get_speed_up_seconds_when_feeding(ticks_until_adult: i32) -> i32 {
        ((ticks_until_adult / 20) as f32 * 0.1) as i32
    }
}

impl Default for AgeableMobBase {
    fn default() -> Self {
        Self::new()
    }
}

/// Vanilla-shaped behavior shared by entities that extend `AgeableMob`.
pub trait AgeableMob: Mob {
    /// Returns shared ageable runtime state.
    fn ageable_base(&self) -> &AgeableMobBase;

    /// Returns the synchronized vanilla age-lock flag.
    fn is_age_locked(&self) -> bool;

    /// Sets the synchronized vanilla age-lock flag.
    fn set_age_locked(&self, age_locked: bool);

    /// Sets the synchronized baby flag.
    fn set_synced_baby(&self, baby: bool);

    /// Hook called after the baby/adult boundary changes.
    fn age_boundary_changed(&self, _baby: bool) {}

    /// Returns vanilla `AgeableMob.getBabyStartAge`.
    fn get_baby_start_age(&self) -> i32 {
        BABY_START_AGE
    }

    /// Returns vanilla `AgeableMob.age`.
    fn get_age(&self) -> i32 {
        self.ageable_base().age()
    }

    /// Sets vanilla `AgeableMob.age` and updates synchronized baby state.
    fn set_age(&self, age: i32) {
        if self.ageable_base().set_age(age) {
            let baby = age < 0;
            self.set_synced_baby(baby);
            self.age_boundary_changed(baby);
        }
    }

    /// Returns whether this ageable mob is a baby.
    fn is_baby(&self) -> bool {
        self.get_age() < 0
    }

    /// Sets the vanilla baby state using the `AgeableMob` start age.
    fn set_baby(&self, baby: bool) {
        self.set_age(if baby { self.get_baby_start_age() } else { 0 });
    }

    /// Returns vanilla `AgeableMob.forcedAge`.
    fn forced_age(&self) -> i32 {
        self.ageable_base().forced_age()
    }

    /// Sets vanilla `AgeableMob.forcedAge`.
    fn set_forced_age(&self, forced_age: i32) {
        self.ageable_base().set_forced_age(forced_age);
    }

    /// Returns vanilla `AgeableMob.forcedAgeTimer`.
    fn forced_age_timer(&self) -> i32 {
        self.ageable_base().forced_age_timer()
    }

    /// Sets vanilla `AgeableMob.forcedAgeTimer`.
    fn set_forced_age_timer(&self, forced_age_timer: i32) {
        self.ageable_base().set_forced_age_timer(forced_age_timer);
    }

    /// Returns vanilla `AgeableMob.ageLockParticleTimer`.
    fn age_lock_particle_timer(&self) -> i32 {
        self.ageable_base().age_lock_particle_timer()
    }

    /// Sets vanilla `AgeableMob.ageLockParticleTimer`.
    fn set_age_lock_particle_timer(&self, timer: i32) {
        self.ageable_base().set_age_lock_particle_timer(timer);
    }

    /// Returns whether this mob can naturally age toward adulthood this tick.
    fn can_age_up(&self) -> bool {
        AgeableMob::is_baby(self) && !self.is_age_locked()
    }

    /// Returns vanilla `AgeableMob.getSpeedUpSecondsWhenFeeding`.
    fn get_speed_up_seconds_when_feeding(ticks_until_adult: i32) -> i32
    where
        Self: Sized,
    {
        AgeableMobBase::get_speed_up_seconds_when_feeding(ticks_until_adult)
    }

    /// Applies vanilla `AgeableMob.ageUp`.
    fn age_up(&self, seconds: i32, forced: bool) {
        let old_age = self.get_age();
        let mut age = old_age + seconds * 20;
        if age > 0 {
            age = 0;
        }

        let delta = age - old_age;
        self.set_age(age);
        if forced {
            self.ageable_base().add_forced_age(delta);
            if self.forced_age_timer() == 0 {
                self.set_forced_age_timer(40);
            }
        }

        if self.get_age() == 0 {
            self.set_age(self.forced_age());
        }
    }

    /// Runs vanilla `AgeableMob.finalizeSpawn`, then delegates to `Mob.finalizeSpawn`.
    fn finalize_spawn_ageable_mob(
        &self,
        world: &Arc<World>,
        spawn_reason: EntitySpawnReason,
        group_data: Option<SpawnGroupData>,
    ) -> Option<SpawnGroupData> {
        let mut group_data = match group_data {
            Some(SpawnGroupData::AgeableMob(group_data)) => group_data,
            None => AgeableMobGroupData::with_should_spawn_baby(true),
        };
        if group_data.finalize_ageable_spawn(rand::random::<f32>) {
            self.set_age(self.get_baby_start_age());
        }

        self.finalize_spawn_mob_base(
            world,
            spawn_reason,
            Some(SpawnGroupData::AgeableMob(group_data)),
        )
    }

    /// Handles vanilla `AgeableMob.mobInteract`.
    fn mob_interact_ageable(&self, player: &Player, hand: InteractionHand) -> InteractionResult {
        let item_stack = {
            let inventory = player.inventory.lock();
            let item_stack = inventory.get_item_in_hand(hand);
            item_stack.copy_with_count(item_stack.count())
        };

        if !item_stack.is(&vanilla_items::ITEMS.golden_dandelion)
            || !AgeableMob::is_baby(self)
            || self.age_lock_particle_timer() != 0
            || REGISTRY
                .entity_types
                .is_in_tag(self.entity_type(), &EntityTypeTag::CANNOT_BE_AGE_LOCKED)
        {
            return InteractionResult::Pass;
        }

        self.set_age_locked(!self.is_age_locked());
        self.set_age(self.get_baby_start_age());
        self.set_age_lock_particle_timer(AGE_LOCK_COOLDOWN_TICKS);
        Mob::use_player_item(self, player, hand);

        let is_age_locked = self.is_age_locked();
        if is_age_locked {
            self.set_persistence_required();
        }
        if let Some(world) = self.level() {
            let sound = if is_age_locked {
                &sound_events::ITEM_GOLDEN_DANDELION_USE
            } else {
                &sound_events::ITEM_GOLDEN_DANDELION_UNUSE
            };
            world.play_sound(
                sound,
                SoundSource::Players,
                self.block_position(),
                1.0,
                1.0,
                None,
            );
        }

        InteractionResult::Success
    }

    /// Ticks vanilla age progression.
    fn tick_ageable_mob(&self) {
        if !Entity::is_alive(self) {
            return;
        }

        let age = self.get_age();
        if self.can_age_up() {
            self.set_age(age + 1);
        } else if age > 0 {
            self.set_age(age - 1);
        }

        let age_lock_particle_timer = self.age_lock_particle_timer();
        if age_lock_particle_timer > 0 {
            self.set_age_lock_particle_timer(age_lock_particle_timer - 1);
        }
    }

    /// Saves vanilla ageable mob fields.
    fn save_ageable_mob(&self, nbt: &mut NbtCompound) {
        nbt.insert("Age", self.get_age());
        nbt.insert("ForcedAge", self.forced_age());
        nbt.insert("AgeLocked", i8::from(self.is_age_locked()));
    }

    /// Loads vanilla ageable mob fields.
    fn load_ageable_mob(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.set_age(nbt.int("Age").unwrap_or(0));
        self.set_forced_age(nbt.int("ForcedAge").unwrap_or(0));
        self.set_age_locked(nbt.byte("AgeLocked").is_some_and(|value| value != 0));
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Weak;

    use glam::DVec3;
    use steel_registry::entity_type::EntityTypeRef;
    use steel_registry::{test_support::init_test_registry, vanilla_entities};

    use super::*;
    use crate::entity::{EntityBase, LivingEntity, LivingEntityBase, MobBase};

    struct TestAgeableMob {
        base: EntityBase,
        living_base: LivingEntityBase,
        mob_base: MobBase,
        ageable_base: AgeableMobBase,
        mob_flags: SyncMutex<i8>,
        health: SyncMutex<f32>,
        baby: SyncMutex<bool>,
        age_locked: SyncMutex<bool>,
    }

    impl TestAgeableMob {
        fn new() -> Self {
            init_test_registry();
            Self {
                base: EntityBase::new(
                    1,
                    DVec3::ZERO,
                    vanilla_entities::PIG.dimensions,
                    Weak::new(),
                ),
                living_base: LivingEntityBase::new(&vanilla_entities::PIG),
                mob_base: MobBase::new(),
                ageable_base: AgeableMobBase::new(),
                mob_flags: SyncMutex::new(0),
                health: SyncMutex::new(10.0),
                baby: SyncMutex::new(false),
                age_locked: SyncMutex::new(false),
            }
        }
    }

    impl Entity for TestAgeableMob {
        fn base(&self) -> &EntityBase {
            &self.base
        }

        fn entity_type(&self) -> EntityTypeRef {
            &vanilla_entities::PIG
        }
    }

    impl LivingEntity for TestAgeableMob {
        fn living_base(&self) -> &LivingEntityBase {
            &self.living_base
        }

        fn get_health(&self) -> f32 {
            *self.health.lock()
        }

        fn set_health(&self, health: f32) {
            *self.health.lock() = health;
        }
    }

    impl Mob for TestAgeableMob {
        fn mob_base(&self) -> &MobBase {
            &self.mob_base
        }

        fn mob_flags(&self) -> i8 {
            *self.mob_flags.lock()
        }

        fn set_mob_flags(&self, flags: i8) {
            *self.mob_flags.lock() = flags;
        }
    }

    impl AgeableMob for TestAgeableMob {
        fn ageable_base(&self) -> &AgeableMobBase {
            &self.ageable_base
        }

        fn is_age_locked(&self) -> bool {
            *self.age_locked.lock()
        }

        fn set_age_locked(&self, age_locked: bool) {
            *self.age_locked.lock() = age_locked;
        }

        fn set_synced_baby(&self, baby: bool) {
            *self.baby.lock() = baby;
        }
    }

    #[test]
    fn age_boundary_updates_synced_baby_flag() {
        let mob = TestAgeableMob::new();

        mob.set_age(-1);
        assert!(AgeableMob::is_baby(&mob));
        assert!(*mob.baby.lock());

        mob.set_age(0);
        assert!(!AgeableMob::is_baby(&mob));
        assert!(!*mob.baby.lock());
    }

    #[test]
    fn age_tick_grows_babies_and_reduces_positive_forced_age() {
        let mob = TestAgeableMob::new();

        mob.set_age(-2);
        mob.tick_ageable_mob();
        assert_eq!(mob.get_age(), -1);

        mob.set_age(2);
        mob.tick_ageable_mob();
        assert_eq!(mob.get_age(), 1);
    }

    #[test]
    fn age_lock_blocks_baby_growth() {
        let mob = TestAgeableMob::new();

        mob.set_age(-2);
        mob.set_age_locked(true);
        mob.tick_ageable_mob();

        assert_eq!(mob.get_age(), -2);
    }

    #[test]
    fn age_up_adds_forced_age_and_timer() {
        let mob = TestAgeableMob::new();

        mob.set_age(-100);
        mob.age_up(1, true);

        assert_eq!(mob.get_age(), -80);
        assert_eq!(mob.forced_age(), 20);
        assert_eq!(mob.forced_age_timer(), 40);
    }

    #[test]
    fn age_up_applies_forced_age_when_reaching_adulthood() {
        let mob = TestAgeableMob::new();

        mob.set_age(-10);
        mob.age_up(1, true);

        assert_eq!(mob.get_age(), 10);
        assert_eq!(mob.forced_age(), 10);
    }

    #[test]
    fn feeding_speed_up_seconds_matches_vanilla_integer_order() {
        assert_eq!(
            TestAgeableMob::get_speed_up_seconds_when_feeding(24_000),
            120
        );
        assert_eq!(TestAgeableMob::get_speed_up_seconds_when_feeding(199), 0);
    }
}
