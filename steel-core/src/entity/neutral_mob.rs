//! Persistent anger shared by neutral mobs (vanilla `NeutralMob`).

use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_registry::vanilla_game_rules::{FORGIVE_DEAD_PLAYERS, UNIVERSAL_ANGER};
use steel_utils::TICKS_PER_SECOND;
use steel_utils::locks::SyncMutex;
use steel_utils::nbt::NbtValueInput as _;
use steel_utils::types::Difficulty;

use crate::entity::{Entity, EntityReference, LivingEntity, Mob, Player};
use crate::world::World;

const NO_ANGER_END_TIME: i64 = -1;
const MIN_PERSISTENT_ANGER_TICKS: i64 = 20 * TICKS_PER_SECOND as i64;
const MAX_PERSISTENT_ANGER_TICKS: i64 = 39 * TICKS_PER_SECOND as i64;
const LEGACY_ANGER_TIME_KEY: &str = "AngerTime";
const ANGER_END_TIME_KEY: &str = "anger_end_time";
const ANGRY_AT_KEY: &str = "angry_at";

/// Stored persistent-anger state for a neutral mob.
#[derive(Debug)]
pub struct NeutralMobBase {
    persistent_anger_end_time: SyncMutex<i64>,
    persistent_anger_target: SyncMutex<Option<EntityReference>>,
}

impl NeutralMobBase {
    /// Creates anger state for a newly spawned neutral mob.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            persistent_anger_end_time: SyncMutex::new(NO_ANGER_END_TIME),
            persistent_anger_target: SyncMutex::new(None),
        }
    }

    /// Returns the game time at which anger ends.
    #[must_use]
    pub fn persistent_anger_end_time(&self) -> i64 {
        *self.persistent_anger_end_time.lock()
    }

    /// Sets the game time at which anger ends.
    pub fn set_persistent_anger_end_time(&self, end_time: i64) {
        *self.persistent_anger_end_time.lock() = end_time;
    }

    /// Returns the entity this mob remains angry at.
    #[must_use]
    pub fn persistent_anger_target(&self) -> Option<EntityReference> {
        self.persistent_anger_target.lock().clone()
    }

    /// Sets the entity this mob remains angry at.
    pub fn set_persistent_anger_target(&self, target: Option<EntityReference>) {
        *self.persistent_anger_target.lock() = target;
    }
}

impl Default for NeutralMobBase {
    fn default() -> Self {
        Self::new()
    }
}

/// Anger that persists across target changes and saves.
pub trait NeutralMob: Mob {
    /// Returns the mob's stored anger state.
    fn neutral_mob_base(&self) -> &NeutralMobBase;

    /// Returns the game time at which anger ends.
    fn persistent_anger_end_time(&self) -> i64 {
        self.neutral_mob_base().persistent_anger_end_time()
    }

    /// Sets the game time at which anger ends.
    fn set_persistent_anger_end_time(&self, end_time: i64) {
        self.neutral_mob_base()
            .set_persistent_anger_end_time(end_time);
    }

    /// Returns the entity this mob remains angry at.
    fn persistent_anger_target(&self) -> Option<EntityReference> {
        self.neutral_mob_base().persistent_anger_target()
    }

    /// Sets the entity this mob remains angry at.
    fn set_persistent_anger_target(&self, target: Option<EntityReference>) {
        self.neutral_mob_base().set_persistent_anger_target(target);
    }

    /// Sets anger to expire after `remaining_time` ticks.
    fn set_time_to_remain_angry(&self, remaining_time: i64) {
        let Some(world) = self.level() else {
            return;
        };
        self.set_persistent_anger_end_time(world.game_time() + remaining_time);
    }

    /// Randomizes the anger duration to 20–39 seconds.
    fn start_persistent_anger_timer(&self) {
        self.set_time_to_remain_angry(rand::random_range(
            MIN_PERSISTENT_ANGER_TICKS..=MAX_PERSISTENT_ANGER_TICKS,
        ));
    }

    /// Writes persistent anger to NBT.
    fn add_persistent_anger_save_data(&self, nbt: &mut NbtCompound) {
        nbt.insert(ANGER_END_TIME_KEY, self.persistent_anger_end_time());
        if let Some(target) = self.persistent_anger_target() {
            target.store(nbt, ANGRY_AT_KEY);
        }
    }

    /// Restores persistent anger from NBT and reattaches its target.
    fn read_persistent_anger_save_data(&self, nbt: &BorrowedNbtCompoundView<'_, '_>) {
        if let Some(end_time) = nbt.long_value(ANGER_END_TIME_KEY) {
            self.set_persistent_anger_end_time(end_time);
        } else if let Some(anger_time) = nbt.int_value(LEGACY_ANGER_TIME_KEY) {
            self.set_time_to_remain_angry(i64::from(anger_time));
        } else {
            self.set_persistent_anger_end_time(NO_ANGER_END_TIME);
        }

        let target = EntityReference::read(nbt, ANGRY_AT_KEY);
        if let Some(world) = self.level() {
            self.set_target(
                target
                    .as_ref()
                    .and_then(|reference| reference.get_entity(world.as_ref()))
                    .as_ref(),
            );
        }
        self.set_persistent_anger_target(target);
    }

    /// Refreshes anger from the current and remembered targets.
    fn update_persistent_anger(&self, world: &World, stay_angry_if_target_present: bool) {
        let previous_target = self.target_unchecked();
        let persistent_target = self.persistent_anger_target();

        if let Some(previous) = &previous_target
            && previous
                .as_living_entity()
                .is_some_and(LivingEntity::is_dead_or_dying)
            && persistent_target
                .as_ref()
                .is_some_and(|reference| reference.matches(previous.as_ref()))
            && previous.as_mob().is_some()
        {
            self.stop_being_angry();
            return;
        }

        let target = self.target();
        if let Some(target) = &target {
            let new_target = persistent_target
                .as_ref()
                .is_none_or(|reference| !reference.matches(target.as_ref()));
            if new_target {
                self.set_persistent_anger_target(Some(EntityReference::from_entity(target)));
            }
            if new_target || stay_angry_if_target_present {
                self.start_persistent_anger_timer();
            }
        }

        if persistent_target.is_some()
            && !self.is_angry()
            && (target.is_none()
                || !target
                    .as_ref()
                    .and_then(|entity| entity.as_living_entity())
                    .is_some_and(is_valid_player_target)
                || !stay_angry_if_target_present)
        {
            self.stop_being_angry();
        }

        let player_target = persistent_target.and_then(|reference| reference.get_entity(world));
        if let Some(entity) = player_target
            && entity.as_player().is_some_and(|player| {
                player.has_infinite_materials()
                    || player.is_spectator()
                    || world.difficulty() == Difficulty::Peaceful
            })
        {
            self.stop_being_angry();
        }
    }

    /// Returns whether this mob is angry at `entity`.
    fn is_angry_at(&self, entity: &dyn LivingEntity, world: &World) -> bool {
        if !Mob::can_attack(self, entity) {
            return false;
        }

        if is_valid_player_target(entity) && self.is_angry_at_all_players(world) {
            return true;
        }

        self.persistent_anger_target()
            .is_some_and(|reference| reference.matches(entity.as_entity_event_source()))
    }

    /// Returns whether universal anger makes this mob angry at every player.
    fn is_angry_at_all_players(&self, world: &World) -> bool {
        world.get_game_rule(&UNIVERSAL_ANGER)
            && self.is_angry()
            && self.persistent_anger_target().is_none()
    }

    /// Returns whether the persistent anger timer is still running.
    fn is_angry(&self) -> bool {
        let end_time = self.persistent_anger_end_time();
        if end_time > 0
            && let Some(world) = self.level()
        {
            return end_time - world.game_time() > 0;
        }
        false
    }

    /// Forgives a player on death when `forgiveDeadPlayers` is enabled.
    fn player_died(&self, world: &World, player: &Player) {
        if world.get_game_rule(&FORGIVE_DEAD_PLAYERS)
            && self
                .persistent_anger_target()
                .is_some_and(|reference| reference.matches(player))
        {
            self.stop_being_angry();
        }
    }

    /// Clears the target and refreshes universal anger.
    fn forget_current_target_and_refresh_universal_anger(&self) {
        self.stop_being_angry();
        self.start_persistent_anger_timer();
    }

    /// Clears all anger state and the current target.
    fn stop_being_angry(&self) {
        self.set_last_hurt_by_mob(None);
        self.set_persistent_anger_target(None);
        self.set_target(None);
        self.set_persistent_anger_end_time(NO_ANGER_END_TIME);
    }
}

fn is_valid_player_target(target: &dyn LivingEntity) -> bool {
    target.as_player().is_some_and(|player| {
        !player.has_infinite_materials()
            && !player.is_spectator()
            && player
                .level()
                .is_some_and(|world| world.difficulty() != Difficulty::Peaceful)
    })
}
