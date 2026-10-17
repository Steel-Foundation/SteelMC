use std::sync::Arc;

use glam::DVec3;
use steel_registry::data_components::vanilla_components::CUSTOM_NAME;
use steel_registry::vanilla_game_rules::ENTITY_DROPS;
use steel_registry::{vanilla_damage_types, vanilla_entities, vanilla_items};
use steel_utils::{ChunkPos, Downcast as _, WorldAabb};
use text_components::TextComponent;

use super::VehicleEntity;
use crate::bootstrap::init_globals;
use crate::entity::entities::{ItemEntity, MinecartEntity};
use crate::entity::{
    DamageSource, Entity, EntityEventSource, RemovalReason, SharedEntity, next_entity_id,
};
use crate::test_support::{TestWorld, fresh_test_world, insert_ready_full_chunk};
use crate::world::World;

fn test_world(key: &'static str) -> TestWorld {
    init_globals();

    let fixture = fresh_test_world(key);
    insert_ready_full_chunk(&fixture.world, ChunkPos::new(0, 0));
    assert!(fixture.world.set_game_rule(&ENTITY_DROPS, true));

    fixture
}

fn test_cart(world: &Arc<World>) -> Arc<MinecartEntity> {
    let cart = Arc::new(MinecartEntity::new(
        &vanilla_entities::MINECART,
        next_entity_id(),
        DVec3::new(8.5, 64.0, 8.5),
        Arc::downgrade(world),
    ));

    world
        .try_add_entity(Arc::clone(&cart).into_shared_entity())
        .expect("test minecart should enter the loaded chunk");

    cart
}

fn dropped_items(world: &World, position: DVec3) -> Vec<SharedEntity> {
    world.get_entities_in_aabb_matching(&WorldAabb::of_size(position, 4.0, 4.0, 4.0), |entity| {
        entity.entity_type() == &vanilla_entities::ITEM
    })
}

#[test]
fn damage_accumulates_and_destruction_requires_more_than_forty() {
    let fixture = test_world("vehicle_damage_threshold");
    let world = &fixture.world;
    let cart = test_cart(world);
    let source = DamageSource::environment(&vanilla_damage_types::GENERIC);
    let initial_direction = cart.hurt_dir();

    assert!(cart.hurt(world, &source, 2.0));
    assert_eq!(cart.damage(), 20.0);
    assert_eq!(cart.hurt_dir(), -initial_direction);
    assert_eq!(cart.hurt_time(), 10);
    assert!(!cart.is_removed());

    assert!(cart.hurt(world, &source, 2.0));
    assert_eq!(cart.damage(), 40.0);
    assert_eq!(cart.hurt_dir(), initial_direction);
    assert!(!cart.is_removed());
    assert!(dropped_items(world, cart.position()).is_empty());

    assert!(cart.hurt(world, &source, 1.0));
    assert_eq!(cart.damage(), 50.0);
    assert_eq!(cart.removal_reason(), Some(RemovalReason::Killed));
    assert_eq!(dropped_items(world, cart.position()).len(), 1);

    let removed_state = (cart.hurt_time(), cart.hurt_dir(), cart.damage());

    assert!(cart.hurt(world, &source, 100.0));
    assert_eq!(
        (cart.hurt_time(), cart.hurt_dir(), cart.damage()),
        removed_state,
    );
    assert_eq!(dropped_items(world, cart.position()).len(), 1);
}

#[test]
fn destruction_respects_entity_drops_and_preserves_the_custom_name() {
    for entity_drops in [false, true] {
        let fixture = test_world("vehicle_named_drop");
        let world = &fixture.world;
        assert!(world.set_game_rule(&ENTITY_DROPS, entity_drops));

        let cart = test_cart(world);
        let name = TextComponent::plain("Rail Runner");
        cart.set_custom_name(Some(name.clone()));

        let source = DamageSource::environment(&vanilla_damage_types::GENERIC);

        assert!(cart.hurt(world, &source, 5.0));
        assert_eq!(cart.removal_reason(), Some(RemovalReason::Killed));

        let drops = dropped_items(world, cart.position());
        assert_eq!(drops.len(), usize::from(entity_drops));

        if !entity_drops {
            continue;
        }

        let item_entity = drops[0]
            .downcast_ref::<ItemEntity>()
            .expect("the vehicle drop should be an item entity");
        let stack = item_entity.get_item();

        assert!(stack.is(&vanilla_items::MINECART));
        assert_eq!(stack.count(), 1);
        assert_eq!(stack.get(CUSTOM_NAME), Some(&name));
    }
}
