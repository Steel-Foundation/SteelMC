//! shared push stuff, contacts stay in order

use std::cell::RefCell;

use steel_registry::vanilla_game_rules::MAX_ENTITY_CRAMMING;

use super::selector::EntitySelector;
use super::{Entity, LivingEntity, SharedEntity};
use crate::world::World;

pub use snapshot::{DefaultEntityPush, EntityPushSnapshot};

mod impulse;
mod snapshot;

thread_local! {
    static SELECTED_PUSH_STATES: RefCell<Vec<Option<EntityPushSnapshot>>> = const { RefCell::new(Vec::new()) };
}

pub(crate) struct EntityPush;

impl EntityPush {
    pub(crate) fn push_entities<E: LivingEntity + ?Sized>(pusher: &E) {
        let Some(world) = pusher.level() else {
            return;
        };

        if !world.tick_runs_normally() {
            return;
        }

        let target = pusher.as_entity_event_source();
        let candidates = if let Some(default_push) = target.as_default_entity_push() {
            let max_cramming = world.get_game_rule(&MAX_ENTITY_CRAMMING);
            let target_id = target.id();
            let borrow_limit = usize::try_from(max_cramming)
                .ok()
                .filter(|&limit| limit > 0);
            let fallback = world.entity_manager().try_with_entities_in_aabb(
                &target.bounding_box(),
                borrow_limit,
                |entities| {
                    let below_cramming_limit = max_cramming <= 0
                        || entities.len() < max_cramming as usize
                        || (entities.len() == max_cramming as usize
                            && entities.iter().any(|entity| entity.id() == target_id));

                    below_cramming_limit
                        && entities
                            .iter()
                            .all(|entity| entity.as_default_entity_push().is_some())
                        && Self::push_selected_entities(default_push, entities)
                },
            );

            let Some(candidates) = fallback else {
                return;
            };

            candidates
        } else {
            world.get_entities_in_aabb(&target.bounding_box())
        };

        let entities = World::filter_pushable_entities(target, candidates);

        if entities.is_empty() {
            return;
        }

        pusher.apply_entity_cramming_damage(&world, &entities);
        pusher.push_entities_in_order(&entities);
    }

    fn push_selected_entities(target: &dyn DefaultEntityPush, entities: &[&SharedEntity]) -> bool {
        SELECTED_PUSH_STATES.with(|states| {
            let mut states = states.borrow_mut();
            states.clear();
            let predicate = EntitySelector::pushable_by(target);

            for entity in entities {
                let Some(source) = entity.as_default_entity_push() else {
                    return false;
                };

                if !predicate(entity.as_ref()) {
                    states.push(None);
                    continue;
                }

                let Some(state) = source.push_snapshot(true) else {
                    return false;
                };

                states.push(Some(state));
            }

            if states.iter().all(Option::is_none) {
                return true;
            }

            let Some(target_state) = target.push_snapshot(false) else {
                return false;
            };

            // select + check first, then impulses
            // default contacts under lock, custom callbacks after unlock
            for (entity, source) in entities.iter().zip(states.iter()) {
                if let Some(source) = source {
                    Self::apply_contact(entity.as_ref(), *source, target, target_state);
                }
            }

            true
        })
    }

    pub(crate) fn push_entities_in_order(target: &dyn Entity, entities: &[SharedEntity]) {
        let Some(default_target) = target.as_default_entity_push() else {
            for entity in entities {
                entity.push_entity(target);
            }

            return;
        };

        let mut cached = None;

        for entity in entities {
            if let Some(source) = entity.as_default_entity_push()
                && let Some(target_state) =
                    *cached.get_or_insert_with(|| default_target.push_snapshot(false))
                && let Some(source_state) = source.push_snapshot(false)
            {
                if entity.id() != target.id() {
                    Self::apply_contact(entity.as_ref(), source_state, target, target_state);
                }

                continue;
            }

            cached = None;
            entity.push_entity(target);
        }
    }
}

#[cfg(test)]
mod test_fixtures;
#[cfg(test)]
mod tests;
