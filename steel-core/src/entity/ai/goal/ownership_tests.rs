use std::mem;
use std::sync::Arc;

use glam::DVec3;
use steel_registry::{vanilla_damage_types, vanilla_dimension_types, vanilla_entities};
use steel_utils::{ChunkPos, Identifier, types::Difficulty};

use super::interact::InteractGoal;
use super::selector::Goal;
use super::{GoalSelector, HurtByTargetGoal, LookAtPlayerGoal};
use crate::entity::damage::{DamageHistory, DamageSource};
use crate::entity::entities::{FishingHookEntity, PigEntity};
use crate::entity::{
    Entity, EntityArc, EntityGeneration, EntityWeak, LivingEntity, Mob, Projectile, RemovalReason,
    SharedEntity,
};
use crate::level_data::GameTimeSource;
use crate::player::Player;
use crate::test_support::{
    TestPlayerBuilder, create_test_world_with_damage_history, insert_ready_full_chunk,
};
use crate::world::World;

struct Fixture {
    history: Arc<DamageHistory>,
    world: Arc<World>,
    victim: EntityArc<Player>,
    attacker: EntityArc<PigEntity>,
}

impl Fixture {
    fn new() -> Self {
        let history = Arc::new(DamageHistory::default());
        let world = create_test_world_with_damage_history(
            Identifier::vanilla_static("goal_ownership"),
            Difficulty::Normal,
            &vanilla_dimension_types::OVERWORLD,
            GameTimeSource::Primary,
            &history,
        );
        insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
        let victim = TestPlayerBuilder::new(Arc::clone(&world), "Victim", 1).build();
        victim.set_client_loaded(true);
        victim
            .try_set_position(DVec3::new(8.0, 64.0, 8.0))
            .expect("victim position");
        assert!(world.players.insert(victim.clone()));
        let attacker = EntityArc::new(PigEntity::new(
            &vanilla_entities::PIG,
            2,
            DVec3::new(9.0, 64.0, 8.0),
            Arc::downgrade(&world),
        ));
        world
            .try_add_entity(attacker.clone())
            .expect("register attacker");
        *attacker.mob_base().goal_selector().lock() = GoalSelector::new();
        Self {
            history,
            world,
            victim,
            attacker,
        }
    }

    fn look_goal(&self) -> LookAtPlayerGoal {
        let mut goal = LookAtPlayerGoal::new_with_probability(6.0, 1.0);
        assert!(goal.can_use(self.attacker.as_ref()));
        goal
    }

    fn watch_victim(&self) {
        let mut goals = self.attacker.mob_base().goal_selector().lock();
        goals.add_goal(0, LookAtPlayerGoal::new_with_probability(6.0, 1.0));
        let entity: SharedEntity = self.attacker.clone();
        goals.tick(self.attacker.as_ref(), &entity);
    }

    fn damage_victim(&self) {
        assert!(self.victim.hurt(
            &self.world,
            &DamageSource::direct(&vanilla_damage_types::GENERIC, self.attacker.clone()),
            1.0,
        ));
    }

    fn remove(self) -> Removed {
        let victim = EntityArc::downgrade(&self.victim);
        let attacker = EntityArc::downgrade(&self.attacker);
        let generation = self.victim.generation();
        self.attacker.set_removed(RemovalReason::Discarded);
        self.victim.set_removed(RemovalReason::Discarded);
        assert!(
            self.world
                .players
                .remove_player_sync(&self.victim)
                .is_some()
        );
        Removed {
            history: self.history,
            world: self.world,
            victim,
            attacker,
            generation,
        }
    }
}

struct Removed {
    history: Arc<DamageHistory>,
    world: Arc<World>,
    victim: EntityWeak<Player>,
    attacker: EntityWeak<PigEntity>,
    generation: EntityGeneration,
}

impl Removed {
    fn assert_collected(&self) {
        let time = self.world.game_time();
        self.history.collect_unreachable();
        assert_eq!(self.world.game_time(), time);
        assert!(self.history.last_damage_source(self.generation).is_none());
        assert_eq!(self.victim.strong_count(), 0, "victim must be destroyed");
        assert_eq!(
            self.attacker.strong_count(),
            0,
            "attacker must be destroyed"
        );
    }

    fn assert_retained(&self) {
        self.history.collect_unreachable();
        assert!(self.history.last_damage_source(self.generation).is_some());
        assert!(self.victim.upgrade().is_some());
        assert!(self.attacker.upgrade().is_some());
    }
}

#[test]
fn history_collects_removed_attacker_and_its_goal_target_without_advancing_time() {
    let fixture = Fixture::new();
    fixture.watch_victim();
    fixture.damage_victim();
    let removed = fixture.remove();
    assert!(
        removed.victim.upgrade().is_some(),
        "the goal still owns its target"
    );
    removed.assert_collected();
}

#[test]
fn independent_handles_preserve_history_reachable_through_goals() {
    for retain_attacker in [false, true] {
        let fixture = Fixture::new();
        fixture.watch_victim();
        fixture.damage_victim();
        let outside: SharedEntity = if retain_attacker {
            fixture.attacker.clone()
        } else {
            fixture.victim.clone()
        };
        let removed = fixture.remove();
        removed.assert_retained();
        drop(outside);
        removed.assert_collected();
    }
}

#[test]
fn standalone_goal_is_an_independent_owner() {
    let fixture = Fixture::new();
    let goal = fixture.look_goal();
    fixture.damage_victim();
    let removed = fixture.remove();
    removed.assert_retained();
    drop(goal);
    removed.assert_collected();
}

#[test]
fn extracted_selector_keeps_its_targets_independently() {
    let fixture = Fixture::new();
    fixture.watch_victim();
    fixture.damage_victim();
    let extracted = mem::take(&mut *fixture.attacker.mob_base().goal_selector().lock());
    let removed = fixture.remove();
    removed.assert_retained();
    drop(extracted);
    removed.assert_collected();
}

#[test]
fn duplicate_goal_targets_are_counted_separately() {
    let fixture = Fixture::new();
    let first = fixture.look_goal();
    let second = fixture.look_goal();
    {
        let mut goals = fixture.attacker.mob_base().goal_selector().lock();
        goals.add_goal(0, first);
        goals.add_goal(1, second);
    }
    fixture.damage_victim();
    let removed = fixture.remove();
    assert!(removed.victim.upgrade().is_some());
    removed.assert_collected();
}

#[test]
fn nested_goals_expose_their_entity_references() {
    for use_interact in [false, true] {
        let fixture = Fixture::new();
        if use_interact {
            let mut goal = InteractGoal::new_player(6.0, 1.0);
            assert!(goal.can_use(fixture.attacker.as_ref()));
            fixture
                .attacker
                .mob_base()
                .goal_selector()
                .lock()
                .add_goal(0, goal);
        } else {
            let victim: SharedEntity = fixture.victim.clone();
            fixture.attacker.set_last_hurt_by_mob(Some(&victim));
            let mut goal = HurtByTargetGoal::new();
            goal.start(fixture.attacker.as_ref());
            fixture
                .attacker
                .mob_base()
                .target_selector()
                .lock()
                .add_goal(0, goal);
        }
        fixture.damage_victim();
        let removed = fixture.remove();
        assert!(
            removed.victim.upgrade().is_some(),
            "nested goal must retain its target"
        );
        removed.assert_collected();
    }
}

#[test]
fn history_traces_an_intermediate_goal_owner_without_its_own_history() {
    let fixture = Fixture::new();
    let intermediate = EntityArc::new(PigEntity::new(
        &vanilla_entities::PIG,
        3,
        DVec3::new(10.0, 64.0, 8.0),
        Arc::downgrade(&fixture.world),
    ));
    fixture
        .world
        .try_add_entity(intermediate.clone())
        .expect("register intermediate");
    let intermediate_uuid = intermediate.uuid();
    let mut first_goal = LookAtPlayerGoal::new_for_living_entities(6.0, 1.0, move |living, _| {
        living.uuid() == intermediate_uuid
    });
    assert!(first_goal.can_use(fixture.attacker.as_ref()));
    fixture
        .attacker
        .mob_base()
        .goal_selector()
        .lock()
        .add_goal(0, first_goal);
    let mut second_goal = LookAtPlayerGoal::new_with_probability(6.0, 1.0);
    assert!(second_goal.can_use(intermediate.as_ref()));
    intermediate
        .mob_base()
        .goal_selector()
        .lock()
        .add_goal(0, second_goal);
    fixture.damage_victim();
    assert!(intermediate.last_damage_source().is_none());
    let weak = EntityArc::downgrade(&intermediate);
    intermediate.set_removed(RemovalReason::Discarded);
    drop(intermediate);
    let removed = fixture.remove();
    assert!(removed.victim.upgrade().is_some());
    removed.assert_collected();
    assert_eq!(weak.strong_count(), 0);
}

#[test]
fn an_existing_goal_cycle_does_not_root_history_or_get_retired_with_it() {
    let fixture = Fixture::new();
    let partner = EntityArc::new(PigEntity::new(
        &vanilla_entities::PIG,
        3,
        DVec3::new(10.0, 64.0, 8.0),
        Arc::downgrade(&fixture.world),
    ));
    fixture
        .world
        .try_add_entity(partner.clone())
        .expect("register partner");
    for (owner, target) in [(&fixture.attacker, &partner), (&partner, &fixture.attacker)] {
        let target_uuid = target.uuid();
        let mut goal = LookAtPlayerGoal::new_for_living_entities(6.0, 1.0, move |living, _| {
            living.uuid() == target_uuid
        });
        assert!(goal.can_use(owner.as_ref()));
        owner.mob_base().goal_selector().lock().add_goal(0, goal);
    }
    fixture
        .attacker
        .record_last_damage_source(&DamageSource::direct(
            &vanilla_damage_types::GENERIC,
            fixture.victim.clone(),
        ));
    let generation = fixture.attacker.generation();
    let weak_partner = EntityArc::downgrade(&partner);
    partner.set_removed(RemovalReason::Discarded);
    drop(partner);
    let removed = fixture.remove();
    assert!(removed.history.last_damage_source(generation).is_some());
    assert!(removed.victim.upgrade().is_some());

    let time = removed.world.game_time();
    removed.history.collect_unreachable();
    assert_eq!(removed.world.game_time(), time);
    assert!(removed.history.last_damage_source(generation).is_none());
    assert_eq!(removed.victim.strong_count(), 0);

    let attacker = removed.attacker.upgrade().expect("unretired cycle member");
    let partner = weak_partner.upgrade().expect("unretired cycle member");
    *attacker.mob_base().goal_selector().lock() = GoalSelector::new();
    *partner.mob_base().goal_selector().lock() = GoalSelector::new();
    drop((attacker, partner));
    assert_eq!(removed.attacker.strong_count(), 0);
    assert_eq!(weak_partner.strong_count(), 0);
}

#[test]
fn fishing_hook_target_does_not_root_its_own_damage_history() {
    let fixture = Fixture::new();
    let hook = EntityArc::new(FishingHookEntity::new(
        &vanilla_entities::FISHING_BOBBER,
        3,
        fixture.victim.position(),
        Arc::downgrade(&fixture.world),
    ));
    fixture
        .world
        .try_add_entity(hook.clone())
        .expect("register hook");
    let victim: SharedEntity = fixture.victim.clone();
    EntityArc::clone(&hook).on_hit_entity(&victim, victim.position());
    fixture
        .victim
        .record_last_damage_source(&DamageSource::direct(
            &vanilla_damage_types::GENERIC,
            hook.clone(),
        ));
    drop(victim);
    let weak = EntityArc::downgrade(&hook);
    hook.set_removed(RemovalReason::Discarded);
    drop(hook);
    let removed = fixture.remove();
    assert!(removed.victim.upgrade().is_some());
    removed.assert_collected();
    assert_eq!(weak.strong_count(), 0);
}

#[test]
fn moving_a_selector_changes_its_containing_entity() {
    let fixture = Fixture::new();
    fixture.watch_victim();
    fixture.damage_victim();
    let replacement = EntityArc::new(PigEntity::new(
        &vanilla_entities::PIG,
        3,
        DVec3::new(10.0, 64.0, 8.0),
        Arc::downgrade(&fixture.world),
    ));
    let selector = mem::take(&mut *fixture.attacker.mob_base().goal_selector().lock());
    *replacement.mob_base().goal_selector().lock() = selector;
    let removed = fixture.remove();
    removed.assert_retained();
    drop(replacement);
    removed.assert_collected();
}

#[test]
fn replacing_a_hook_target_removes_the_old_ownership_edge() {
    let fixture = Fixture::new();
    let hook = EntityArc::new(FishingHookEntity::new(
        &vanilla_entities::FISHING_BOBBER,
        3,
        fixture.victim.position(),
        Arc::downgrade(&fixture.world),
    ));
    let victim: SharedEntity = fixture.victim.clone();
    EntityArc::clone(&hook).on_hit_entity(&victim, victim.position());
    fixture
        .victim
        .record_last_damage_source(&DamageSource::direct(
            &vanilla_damage_types::GENERIC,
            hook.clone(),
        ));
    let replacement: SharedEntity = fixture.attacker.clone();
    EntityArc::clone(&hook).on_hit_entity(&replacement, replacement.position());
    drop((victim, replacement));
    let weak_hook = EntityArc::downgrade(&hook);
    drop(hook);
    let removed = fixture.remove();
    assert_eq!(
        removed.victim.strong_count(),
        0,
        "the previous target was released"
    );
    removed.assert_collected();
    assert_eq!(weak_hook.strong_count(), 0);
}
