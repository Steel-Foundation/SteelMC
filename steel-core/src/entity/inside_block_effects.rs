use std::array;

use crate::entity::Entity;

/// Callback queued around an inside-block effect.
pub type InsideBlockEffectCallback = Box<dyn Fn(&dyn Entity) + Send + Sync + 'static>;

const NO_STEP: i32 = -1;
const EFFECT_TYPE_COUNT: usize = 5;
const APPLY_ORDER: [InsideBlockEffectType; EFFECT_TYPE_COUNT] = [
    InsideBlockEffectType::Freeze,
    InsideBlockEffectType::ClearFreeze,
    InsideBlockEffectType::FireIgnite,
    InsideBlockEffectType::LavaIgnite,
    InsideBlockEffectType::Extinguish,
];

/// Vanilla inside-block effect kinds applied after entity/block intersections.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsideBlockEffectType {
    /// Powder-snow freezing.
    Freeze,
    /// Clears accumulated freezing.
    ClearFreeze,
    /// Fire-block ignition.
    FireIgnite,
    /// Lava ignition.
    LavaIgnite,
    /// Clears fire.
    Extinguish,
}

impl InsideBlockEffectType {
    const fn index(self) -> usize {
        match self {
            Self::Freeze => 0,
            Self::ClearFreeze => 1,
            Self::FireIgnite => 2,
            Self::LavaIgnite => 3,
            Self::Extinguish => 4,
        }
    }
}

/// Step-scoped collector for vanilla inside-block side effects.
///
/// Vanilla lets blocks enqueue effects while the movement trace is still
/// scanning intersections, then flushes those effects in a stable per-step
/// order after the scan.
pub struct InsideBlockEffectCollector {
    effects_in_step: [bool; EFFECT_TYPE_COUNT],
    before_effects_in_step: [Vec<InsideBlockEffectCallback>; EFFECT_TYPE_COUNT],
    after_effects_in_step: [Vec<InsideBlockEffectCallback>; EFFECT_TYPE_COUNT],
    final_effects: Vec<InsideBlockEffectCallback>,
    last_step: i32,
}

impl InsideBlockEffectCollector {
    /// Creates an empty collector.
    #[must_use]
    pub fn new() -> Self {
        Self {
            effects_in_step: [false; EFFECT_TYPE_COUNT],
            before_effects_in_step: array::from_fn(|_| Vec::new()),
            after_effects_in_step: array::from_fn(|_| Vec::new()),
            final_effects: Vec::new(),
            last_step: NO_STEP,
        }
    }

    /// Advances to a new block-trace step, flushing the previous step.
    pub fn advance_step(&mut self, step: i32) {
        if self.last_step == step {
            return;
        }

        self.last_step = step;
        self.flush_step();
    }

    /// Queues a vanilla inside-block effect kind for the current step.
    pub const fn apply(&mut self, effect_type: InsideBlockEffectType) {
        self.effects_in_step[effect_type.index()] = true;
    }

    /// Queues a callback to run before this effect kind in the current step.
    pub fn run_before(
        &mut self,
        effect_type: InsideBlockEffectType,
        effect: InsideBlockEffectCallback,
    ) {
        self.before_effects_in_step[effect_type.index()].push(effect);
    }

    /// Queues a callback to run after this effect kind in the current step.
    pub fn run_after(
        &mut self,
        effect_type: InsideBlockEffectType,
        effect: InsideBlockEffectCallback,
    ) {
        self.after_effects_in_step[effect_type.index()].push(effect);
    }

    /// Applies queued effects and resets the collector for the next scan.
    pub fn apply_and_clear(&mut self, entity: &dyn Entity) {
        self.flush_step();

        for effect in self.final_effects.drain(..) {
            if !entity.is_alive() {
                break;
            }
            effect(entity);
        }

        self.last_step = NO_STEP;
    }

    fn flush_step(&mut self) {
        for effect_type in APPLY_ORDER {
            let index = effect_type.index();
            self.final_effects
                .append(&mut self.before_effects_in_step[index]);
            if self.effects_in_step[index] {
                self.effects_in_step[index] = false;
                self.final_effects.push(Box::new(move |entity| {
                    entity.apply_inside_block_effect(effect_type);
                }));
            }
            self.final_effects
                .append(&mut self.after_effects_in_step[index]);
        }
    }
}

impl Default for InsideBlockEffectCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Weak};

    use glam::DVec3;
    use steel_registry::entity_type::EntityTypeRef;
    use steel_registry::vanilla_entities;
    use steel_utils::locks::SyncMutex;

    use crate::entity::{Entity, EntityBase};

    use super::{InsideBlockEffectCollector, InsideBlockEffectType};

    struct EffectTestEntity {
        base: EntityBase,
        calls: Arc<SyncMutex<Vec<&'static str>>>,
        alive: Arc<SyncMutex<bool>>,
    }

    impl EffectTestEntity {
        fn new() -> Arc<Self> {
            let calls = Arc::new(SyncMutex::new(Vec::new()));
            let alive = Arc::new(SyncMutex::new(true));

            Arc::new(Self {
                base: EntityBase::new(
                    1,
                    DVec3::ZERO,
                    vanilla_entities::ITEM.dimensions,
                    Weak::new(),
                ),
                calls,
                alive,
            })
        }
    }

    crate::entity::impl_test_downcast_type!(EffectTestEntity);

    impl Entity for EffectTestEntity {
        fn base(&self) -> &EntityBase {
            &self.base
        }

        fn entity_type(&self) -> EntityTypeRef {
            &vanilla_entities::ITEM
        }

        fn is_alive(&self) -> bool {
            !self.is_removed() && *self.alive.lock()
        }

        fn apply_inside_block_effect(&self, effect_type: InsideBlockEffectType) {
            self.calls.lock().push(match effect_type {
                InsideBlockEffectType::Freeze => "freeze",
                InsideBlockEffectType::ClearFreeze => "clear_freeze",
                InsideBlockEffectType::FireIgnite => "fire_ignite",
                InsideBlockEffectType::LavaIgnite => "lava_ignite",
                InsideBlockEffectType::Extinguish => "extinguish",
            });
        }
    }

    #[test]
    fn collector_flushes_effects_in_vanilla_type_order_per_step() {
        let entity = EffectTestEntity::new();
        let mut collector = InsideBlockEffectCollector::new();

        collector.advance_step(0);
        collector.apply(InsideBlockEffectType::Extinguish);
        collector.apply(InsideBlockEffectType::Freeze);
        collector.advance_step(1);
        collector.apply(InsideBlockEffectType::LavaIgnite);
        collector.apply_and_clear(entity.as_ref());

        assert_eq!(
            *entity.calls.lock(),
            vec!["freeze", "extinguish", "lava_ignite"]
        );
    }

    #[test]
    fn collector_runs_before_and_after_callbacks_around_default_effect() {
        let entity = EffectTestEntity::new();
        let mut collector = InsideBlockEffectCollector::new();

        collector.advance_step(0);
        {
            let calls = entity.calls.clone();
            collector.run_before(
                InsideBlockEffectType::FireIgnite,
                Box::new(move |_| calls.lock().push("before")),
            );
        }
        collector.apply(InsideBlockEffectType::FireIgnite);
        {
            let calls = entity.calls.clone();
            collector.run_after(
                InsideBlockEffectType::FireIgnite,
                Box::new(move |_| calls.lock().push("after")),
            );
        }
        collector.apply_and_clear(entity.as_ref());

        assert_eq!(*entity.calls.lock(), vec!["before", "fire_ignite", "after"]);
    }

    #[test]
    fn collector_stops_after_effect_makes_entity_not_alive() {
        let entity = EffectTestEntity::new();
        let mut collector = InsideBlockEffectCollector::new();

        collector.advance_step(0);
        collector.apply(InsideBlockEffectType::FireIgnite);
        {
            let calls = Arc::clone(&entity.calls);
            let alive = Arc::clone(&entity.alive);
            collector.run_after(
                InsideBlockEffectType::FireIgnite,
                Box::new(move |_| {
                    calls.lock().push("kill");
                    *alive.lock() = false;
                }),
            );
        }
        collector.apply(InsideBlockEffectType::LavaIgnite);
        collector.apply_and_clear(entity.as_ref());

        assert_eq!(*entity.calls.lock(), vec!["fire_ignite", "kill"]);
    }
}
