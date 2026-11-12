use super::*;
use crate::entity::WeakSelf;
use steel_registry::{entity_type::EntityTypeRef, init_vanilla_registry, vanilla_entities};

use steel_registry::blocks::properties::{BlockStateProperties, Tilt};
use steel_registry::vanilla_blocks;
use steel_utils::types::UpdateFlags;
use steel_utils::{ChunkPos, Direction};

use crate::behavior::init_behaviors;
use crate::block_entity::init_block_entities;
use crate::entity::entities::FireworkRocketEntity;
use crate::entity::{EntityDefaults, EntityHooks, EntityProps};
use crate::test_support::{TestPlayerBuilder, world_border_projectile_test_world};
use crate::{entity::EntityBase, test_support::test_world};

struct OwnerCollisionProjectile {
    base: EntityBase,
    projectile_base: ProjectileBase,
}

impl OwnerCollisionProjectile {
    const PROJECTILE: ProjectileProps<Self> = ProjectileProps::new();

    fn new(id: i32, position: DVec3) -> Self {
        Self {
            base: EntityBase::new(
                id,
                position,
                vanilla_entities::ENDER_PEARL.dimensions,
                Arc::downgrade(test_world()),
                WeakSelf::detached(),
            ),
            projectile_base: ProjectileBase::new(&Self::PROJECTILE),
        }
    }
}

crate::entity::impl_test_downcast_type!(OwnerCollisionProjectile);

impl Has<ProjectileBase> for OwnerCollisionProjectile {
    fn component(&self) -> &ProjectileBase {
        &self.projectile_base
    }
}

impl Entity for OwnerCollisionProjectile {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        &vanilla_entities::ENDER_PEARL
    }

    fn entity_hooks(&self) -> &'static dyn EntityHooks {
        &EntityDefaults
    }
}

struct OwnerCollisionTestEntity {
    base: EntityBase,
    pickable: bool,
    entity_type: EntityTypeRef,
}

impl OwnerCollisionTestEntity {
    fn shared(id: i32, position: DVec3, pickable: bool) -> SharedEntity {
        Self::shared_with_type(id, position, pickable, &vanilla_entities::PIG)
    }

    fn shared_with_type(
        id: i32,
        position: DVec3,
        pickable: bool,
        entity_type: EntityTypeRef,
    ) -> SharedEntity {
        Arc::new(Self {
            base: EntityBase::new(
                id,
                position,
                entity_type.dimensions,
                Arc::downgrade(test_world()),
                WeakSelf::detached(),
            ),
            pickable,
            entity_type,
        })
    }
}

crate::entity::impl_test_downcast_type!(OwnerCollisionTestEntity);

impl OwnerCollisionTestEntity {
    const ENTITY: EntityProps<Self> = EntityProps {
        is_pickable: |entity| entity.pickable && !entity.is_removed(),
        ..EntityProps::new()
    };
}

impl Entity for OwnerCollisionTestEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn entity_hooks(&self) -> &'static dyn EntityHooks {
        &Self::ENTITY
    }
}

#[test]
fn compute_margin_ramps_from_zero_to_cap() {
    assert!((compute_margin(2) - 0.0).abs() < 1.0e-9);
    assert!((compute_margin(7) - 0.25).abs() < 1.0e-9);
    assert!((compute_margin(100) - 0.3).abs() < 1.0e-6);
}

#[test]
fn compute_margin_matches_vanilla_float_widening() {
    assert_eq!(compute_margin(3), f64::from(0.05_f32));
    assert_eq!(compute_margin(100), f64::from(0.3_f32));
    assert_ne!(compute_margin(100), 0.3);
}

#[test]
fn clip_segment_hits_box_in_path() {
    let aabb = WorldAabb::new(4.0, -0.5, -0.5, 5.0, 0.5, 0.5);
    let hit = clip_segment(aabb, DVec3::ZERO, DVec3::new(10.0, 0.0, 0.0))
        .expect("ray along +x should enter the box");
    assert!((hit.x - 4.0).abs() < 1.0e-6);
}

#[test]
fn clip_segment_misses_when_starting_inside() {
    let aabb = WorldAabb::new(-1.0, -1.0, -1.0, 1.0, 1.0, 1.0);
    assert_eq!(
        clip_segment(aabb, DVec3::ZERO, DVec3::new(0.0, 5.0, 0.0)),
        None
    );
}

#[test]
fn owner_collision_range_checks_root_vehicle_passengers() {
    init_vanilla_registry();

    let projectile = OwnerCollisionProjectile::new(1, DVec3::ZERO);
    let owner = OwnerCollisionTestEntity::shared(2, DVec3::new(10.0, 0.0, 0.0), true);
    let vehicle = OwnerCollisionTestEntity::shared(3, DVec3::new(10.0, 0.0, 0.0), true);
    let passenger = OwnerCollisionTestEntity::shared(4, DVec3::ZERO, true);
    EntityBase::restore_passenger_relationship(&vehicle, &owner);
    EntityBase::restore_passenger_relationship(&vehicle, &passenger);

    projectile.projectile().set_owner(Some(&owner));

    assert!(!projectile.projectile().is_outside_owner_collision_range());
}

#[test]
fn owner_collision_range_ignores_non_pickable_root_vehicle_passengers() {
    init_vanilla_registry();

    let projectile = OwnerCollisionProjectile::new(1, DVec3::ZERO);
    let owner = OwnerCollisionTestEntity::shared(2, DVec3::new(10.0, 0.0, 0.0), true);
    let vehicle = OwnerCollisionTestEntity::shared(3, DVec3::new(10.0, 0.0, 0.0), true);
    let passenger = OwnerCollisionTestEntity::shared(4, DVec3::ZERO, false);
    EntityBase::restore_passenger_relationship(&vehicle, &owner);
    EntityBase::restore_passenger_relationship(&vehicle, &passenger);

    projectile.projectile().set_owner(Some(&owner));

    assert!(projectile.projectile().is_outside_owner_collision_range());
}

#[test]
fn may_break_requires_impact_projectile_tag() {
    init_vanilla_registry();

    let firework = FireworkRocketEntity::new(
        &vanilla_entities::FIREWORK_ROCKET,
        1,
        DVec3::ZERO,
        Weak::new(),
    );
    let ender_pearl = OwnerCollisionProjectile::new(2, DVec3::ZERO);

    assert!(firework.projectile().may_break(test_world()));
    assert!(!ender_pearl.projectile().may_break(test_world()));
}

#[test]
fn move_vector_synthesizes_world_border_block_hit() {
    init_vanilla_registry();
    init_behaviors();

    let world = Arc::clone(world_border_projectile_test_world());
    let firework = FireworkRocketEntity::new(
        &vanilla_entities::FIREWORK_ROCKET,
        3,
        DVec3::new(4.5, 64.0, 0.0),
        Arc::downgrade(&world),
    );
    firework.set_velocity(DVec3::X);

    let Some(ProjectileHit::Block { location, hit }) =
        firework.projectile().hit_result_on_move_vector()
    else {
        panic!("firework should hit the world border");
    };

    assert!(hit.world_border_hit);
    assert!(!hit.inside);
    assert_eq!(hit.direction, Direction::East);
    assert_eq!(hit.block_pos, BlockPos::new(4, 64, 0));
    assert_eq!(location, hit.location);
    assert!(location.x < 5.0 && location.x > 4.999);
}

#[test]
fn firework_deflects_without_exploding_on_deflecting_entity() {
    init_vanilla_registry();

    let world = Arc::clone(test_world());
    let owner = OwnerCollisionTestEntity::shared(6, DVec3::ZERO, true);
    let replacement: SharedEntity = TestPlayerBuilder::new(Arc::clone(&world), "Replacement", 7)
        .uuid(owner.uuid())
        .build();
    for cached in [false, true] {
        let firework = FireworkRocketEntity::new(
            &vanilla_entities::FIREWORK_ROCKET,
            4,
            DVec3::ZERO,
            Arc::downgrade(&world),
        );
        if cached {
            firework.projectile().set_owner(Some(&owner));
        } else {
            firework.projectile().set_owner_uuid(Some(owner.uuid()));
        }
        let reference = firework
            .get::<ProjectileBase>()
            .state
            .lock()
            .owner
            .clone()
            .expect("stored owner reference");
        firework.set_velocity(DVec3::X);
        let deflector = OwnerCollisionTestEntity::shared_with_type(
            5,
            DVec3::X,
            true,
            &vanilla_entities::BREEZE,
        );

        let deflection = firework
            .projectile()
            .hit_target_or_deflect_self(&ProjectileHit::Entity(EntityHitResult {
                entity: deflector,
                location: DVec3::X,
            }));

        assert_eq!(deflection, ProjectileDeflection::Reverse);
        assert_eq!(firework.velocity(), DVec3::new(-0.5, 0.0, 0.0));
        assert!(firework.needs_velocity_sync());
        assert!(!firework.is_removed());
        assert_eq!(firework.projectile().owner_uuid(), Some(owner.uuid()));
        if cached {
            assert!(Arc::ptr_eq(
                &firework.projectile().owner().expect("cached owner"),
                &owner
            ));
        } else {
            assert!(firework.projectile().owner().is_none());
        }
        reference.cache_entity(&replacement);
        assert!(Arc::ptr_eq(
            &firework
                .projectile()
                .owner()
                .expect("shared owner resolution"),
            &replacement
        ));
    }
}

#[test]
fn base_block_hit_dispatches_vanilla_block_callbacks() {
    init_vanilla_registry();
    init_block_entities();
    init_behaviors();

    let world = Arc::clone(test_world());
    let chunk_map = Arc::clone(&world.chunk_map);
    let pos = BlockPos::new(1_136, 64, 1_136);
    let block_callback_test =
        chunk_map.with_full_chunks_in_radius(ChunkPos::from_block_pos(pos), 0, || {
            let unlit = vanilla_blocks::CANDLE
                .default_state()
                .set_value(&BlockStateProperties::LIT, false);
            assert!(world.set_block(pos, unlit, UpdateFlags::UPDATE_ALL));

            let firework = FireworkRocketEntity::new(
                &vanilla_entities::FIREWORK_ROCKET,
                3,
                DVec3::new(1_136.5, 64.5, 1_136.5),
                Arc::downgrade(&world),
            );
            let hit_block = |block_pos| {
                firework.projectile().on_hit_block_base(&ClipHitResult {
                    location: firework.position(),
                    direction: Direction::Up,
                    block_pos,
                    miss: false,
                    inside: false,
                    world_border_hit: false,
                });
            };
            firework.set_remaining_fire_ticks(1);
            hit_block(pos);

            assert!(
                world
                    .get_block_state(pos)
                    .get_value(&BlockStateProperties::LIT)
            );

            let campfire_pos = pos.offset(1, 0, 0);
            let unlit_campfire = vanilla_blocks::CAMPFIRE
                .default_state()
                .set_value(&BlockStateProperties::LIT, false)
                .set_value(&BlockStateProperties::WATERLOGGED, false);
            assert!(world.set_block(campfire_pos, unlit_campfire, UpdateFlags::UPDATE_CLIENTS,));
            hit_block(campfire_pos);
            assert!(
                world
                    .get_block_state(campfire_pos)
                    .get_value(&BlockStateProperties::LIT)
            );

            let dripleaf_pos = pos.offset(2, 0, 0);
            assert!(world.set_block(
                dripleaf_pos,
                vanilla_blocks::BIG_DRIPLEAF.default_state(),
                UpdateFlags::UPDATE_CLIENTS,
            ));
            hit_block(dripleaf_pos);
            assert_eq!(
                world
                    .get_block_state(dripleaf_pos)
                    .get_value(&BlockStateProperties::TILT),
                Tilt::Full
            );

            let chorus_pos = pos.offset(3, 0, 0);
            assert!(world.set_block(
                chorus_pos,
                vanilla_blocks::CHORUS_FLOWER.default_state(),
                UpdateFlags::UPDATE_CLIENTS,
            ));
            hit_block(chorus_pos);
            assert!(world.get_block_state(chorus_pos).is_air());
        });
    let result = chunk_map.chunk_runtime.block_on(block_callback_test);

    assert_eq!(result, Some(()));
}
