use std::iter::once;
use std::sync::{
    Arc, Weak,
    atomic::{AtomicUsize, Ordering},
};

use glam::DVec3;
use steel_registry::{
    entity_type::EntityTypeRef, item_stack::ItemStack, vanilla_entities, vanilla_items,
};
use steel_utils::{BlockPos, ChunkPos};

use crate::bootstrap::init_globals;
use crate::entity::entities::{ChickenEntity, ExperienceOrbEntity, ItemEntity};
use crate::entity::{Entity, EntityBase, SharedEntity, next_entity_id};
use crate::entity::{EntityHooks, EntityProps};
use crate::test_support::{
    TestPlayerBuilder, TestWorld, fresh_test_world, insert_ready_full_chunk,
};
use crate::world::World;

pub(super) type Factory = fn(&Arc<World>, i32, DVec3) -> SharedEntity;

/// Leading `DEFAULT_ENTITIES` entries that are living mobs. The removed
/// species' slots use chickens until those entities are ported.
pub(super) const LIVING_FIXTURES: usize = 6;

pub(super) const DEFAULT_ENTITIES: [(&str, Factory); 9] = [
    ("chicken in cow slot", |world, id, pos| {
        Arc::new(ChickenEntity::new(
            &vanilla_entities::CHICKEN,
            id,
            pos,
            Arc::downgrade(world),
        ))
    }),
    ("chicken in pig slot", |world, id, pos| {
        Arc::new(ChickenEntity::new(
            &vanilla_entities::CHICKEN,
            id,
            pos,
            Arc::downgrade(world),
        ))
    }),
    ("chicken in sheep slot", |world, id, pos| {
        Arc::new(ChickenEntity::new(
            &vanilla_entities::CHICKEN,
            id,
            pos,
            Arc::downgrade(world),
        ))
    }),
    ("chicken", |world, id, pos| {
        Arc::new(ChickenEntity::new(
            &vanilla_entities::CHICKEN,
            id,
            pos,
            Arc::downgrade(world),
        ))
    }),
    ("chicken in mooshroom slot", |world, id, pos| {
        Arc::new(ChickenEntity::new(
            &vanilla_entities::CHICKEN,
            id,
            pos,
            Arc::downgrade(world),
        ))
    }),
    ("chicken in endermite slot", |world, id, pos| {
        Arc::new(ChickenEntity::new(
            &vanilla_entities::CHICKEN,
            id,
            pos,
            Arc::downgrade(world),
        ))
    }),
    ("chicken in bat slot", |world, id, pos| {
        Arc::new(ChickenEntity::new(
            &vanilla_entities::CHICKEN,
            id,
            pos,
            Arc::downgrade(world),
        ))
    }),
    ("item", |world, id, pos| {
        Arc::new(ItemEntity::with_item_and_velocity(
            &vanilla_entities::ITEM,
            id,
            pos,
            ItemStack::new(&vanilla_items::WHEAT),
            DVec3::ZERO,
            Arc::downgrade(world),
        ))
    }),
    ("experience_orb", |world, id, pos| {
        Arc::new(ExperienceOrbEntity::new(
            &vanilla_entities::EXPERIENCE_ORB,
            id,
            pos,
            Arc::downgrade(world),
        ))
    }),
];

pub(super) fn player(world: &Arc<World>, id: i32, position: DVec3) -> SharedEntity {
    let player = TestPlayerBuilder::new(Arc::clone(world), "PushPlayer", id).build();
    player
        .try_set_position(position)
        .expect("player fixture position");

    player
}

pub(super) struct CustomContact {
    base: EntityBase,
    contacts: AtomicUsize,
}

impl CustomContact {
    pub(super) fn new(world: Weak<World>) -> Arc<Self> {
        Arc::new(Self {
            base: EntityBase::new(
                next_entity_id(),
                DVec3::new(1.65, 64.0, 1.65),
                vanilla_entities::ITEM.dimensions,
                world,
            ),
            contacts: AtomicUsize::new(0),
        })
    }
}

crate::entity::impl_test_downcast_type!(CustomContact);

impl CustomContact {
    const ENTITY: EntityProps<Self> = EntityProps {
        is_pushable: Self::entity_is_pushable,
        push_entity: Self::entity_push_entity,
        ..EntityProps::new()
    };

    fn entity_is_pushable(&self) -> bool {
        let world = self.level().expect("custom contact world");
        assert!(world.entity_manager().get_by_id(self.id()).is_some());

        true
    }

    fn entity_push_entity(&self, target: &dyn Entity) {
        self.contacts.fetch_add(1, Ordering::Relaxed);
        target
            .try_set_position(DVec3::new(-0.5, 64.0, 0.75))
            .expect("contact may move the target");
        target
            .as_living_entity()
            .expect("living push target")
            .set_health(0.0);
        target.push_impulse(DVec3::new(0.25, 0.0, -0.125));
    }
}

impl Entity for CustomContact {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        &vanilla_entities::ITEM
    }

    fn entity_hooks(&self) -> &'static dyn EntityHooks {
        &Self::ENTITY
    }
}

pub(super) struct Fixture {
    pub(super) entities: Vec<SharedEntity>,
    custom: Option<Arc<CustomContact>>,
    pub(super) world: TestWorld,
}

impl Fixture {
    pub(super) fn new(pusher: Factory, custom: bool) -> Self {
        init_globals();
        let world = fresh_test_world("shared_entity_push");

        for x in -1..=0 {
            for z in -1..=0 {
                insert_ready_full_chunk(&world.world, ChunkPos::new(x, z));
            }
        }

        world.world.set_tick_runs_normally(true);
        let custom = custom.then(|| CustomContact::new(Arc::downgrade(&world.world)));
        let mut entities = Vec::new();
        let factories = once(pusher).chain(DEFAULT_ENTITIES.iter().map(|(_, factory)| *factory));

        for (index, factory) in factories.enumerate() {
            let entity = factory(&world.world, next_entity_id(), Self::position(index));
            world
                .world
                .try_add_entity(Arc::clone(&entity))
                .expect("register push fixture");
            entities.push(entity);

            if index == 1
                && let Some(custom) = &custom
            {
                let entity: SharedEntity = Arc::clone(custom) as SharedEntity;
                world
                    .world
                    .try_add_entity(entity)
                    .expect("register custom contact");
            }
        }

        Self {
            entities,
            custom,
            world,
        }
    }

    fn position(index: usize) -> DVec3 {
        DVec3::new(
            1.6 + (index % 3) as f64 * 0.05,
            64.0,
            1.6 + (index / 3) as f64 * 0.05,
        )
    }

    pub(super) fn reset(&self, case: usize) {
        for entity in &self.entities {
            entity.stop_riding();
        }

        for (index, entity) in self.entities.iter().enumerate() {
            if let Some(living) = entity.as_living_entity() {
                living.clear_sleeping_pos();
                living.set_health(living.max_health());
            }

            entity.set_no_physics(false);
            entity
                .try_set_position(Self::position(index))
                .expect("reset push fixture position");
            entity.set_velocity(DVec3::new(-0.0, -0.0, 0.0));
            entity.clear_velocity_sync();
        }

        let target = &self.entities[0];
        let source = &self.entities[2];
        let vehicle = &self.entities[3];

        match case {
            1 => target.set_no_physics(true),
            2 => source.set_no_physics(true),
            3 => target
                .as_living_entity()
                .expect("target health")
                .set_health(0.0),
            4 => source
                .as_living_entity()
                .expect("source health")
                .set_health(0.0),
            5 => target
                .as_living_entity()
                .expect("target sleeping")
                .set_sleeping_pos(BlockPos::new(1, 64, 1)),
            6 => source
                .as_living_entity()
                .expect("source sleeping")
                .set_sleeping_pos(BlockPos::new(1, 64, 1)),
            7 => EntityBase::restore_passenger_relationship(vehicle, target),
            8 => EntityBase::restore_passenger_relationship(vehicle, source),
            9 => {
                EntityBase::restore_passenger_relationship(vehicle, target);
                EntityBase::restore_passenger_relationship(vehicle, source);
            }
            10 => EntityBase::restore_passenger_relationship(target, vehicle),
            11 => EntityBase::restore_passenger_relationship(source, vehicle),
            _ => {}
        }
    }

    pub(super) fn assert_same_state(&self, reference: &Self, label: &str, case: usize) {
        for (actual, expected) in self.entities.iter().zip(&reference.entities) {
            assert_eq!(
                actual.position().to_array().map(f64::to_bits),
                expected.position().to_array().map(f64::to_bits),
                "{label}/{case}: position"
            );
            assert_eq!(
                actual.velocity().to_array().map(f64::to_bits),
                expected.velocity().to_array().map(f64::to_bits),
                "{label}/{case}: velocity"
            );
            assert_eq!(
                actual
                    .as_living_entity()
                    .map(|living| living.health().to_bits()),
                expected
                    .as_living_entity()
                    .map(|living| living.health().to_bits()),
                "{label}/{case}: health"
            );
            assert_eq!(
                actual.needs_velocity_sync(),
                expected.needs_velocity_sync(),
                "{label}/{case}: sync"
            );
            assert_eq!(
                actual.hurt_marked(),
                expected.hurt_marked(),
                "{label}/{case}: hurt"
            );
        }

        assert_eq!(
            self.custom
                .as_ref()
                .map(|custom| custom.contacts.load(Ordering::Relaxed)),
            reference
                .custom
                .as_ref()
                .map(|custom| custom.contacts.load(Ordering::Relaxed))
        );
    }
}
