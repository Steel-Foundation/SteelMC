use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_entities;
use uuid::Uuid;

use super::*;
use crate::behavior::init_behaviors;
use crate::test_support::{TestPlayerBuilder, fresh_loaded_test_world, fresh_test_world};

fn test_hook(world: &Arc<World>, id: i32) -> Arc<FishingHookEntity> {
    Arc::new(FishingHookEntity::new(
        &vanilla_entities::FISHING_BOBBER,
        id,
        DVec3::ZERO,
        Arc::downgrade(world),
    ))
}

#[test]
fn spawn_data_identifies_the_owning_player() {
    let world_fixture = fresh_test_world("fishing_hook_spawn_data");
    let world = &world_fixture.world;
    let player = TestPlayerBuilder::new(Arc::clone(world), "Player", 37)
        .uuid(Uuid::from_u128(1))
        .build();
    let owner: SharedEntity = player;
    let hook = test_hook(world, 38);
    hook.projectile().set_owner(Some(&owner));

    assert_eq!(hook.spawn_data(), owner.id());
}

#[test]
fn removal_only_clears_the_matching_active_hook() {
    let world_fixture = fresh_test_world("fishing_hook_owner_lifecycle");
    let world = &world_fixture.world;
    let player = TestPlayerBuilder::new(Arc::clone(world), "Player", 40)
        .uuid(Uuid::from_u128(2))
        .build();
    let player_owner = Arc::clone(&player);
    let owner: SharedEntity = player_owner;
    let first = test_hook(world, 41);
    let second = test_hook(world, 42);

    first.set_owner(&owner);
    assert!(
        player
            .fishing_hook()
            .is_some_and(|active| Arc::ptr_eq(&active, &first))
    );

    second.set_owner(&owner);
    first.set_removed(RemovalReason::Discarded);
    assert!(
        player
            .fishing_hook()
            .is_some_and(|active| Arc::ptr_eq(&active, &second))
    );

    second.set_removed(RemovalReason::Discarded);
    assert!(player.fishing_hook().is_none());
}

#[test]
fn retrieving_discards_the_active_hook() {
    let world_fixture = fresh_test_world("fishing_hook_retrieve_lifecycle");
    let world = &world_fixture.world;
    let player = TestPlayerBuilder::new(Arc::clone(world), "Player", 50)
        .uuid(Uuid::from_u128(3))
        .build();
    player
        .inventory
        .lock()
        .set_selected_item(ItemStack::new(&vanilla_items::FISHING_ROD));
    let player_owner = Arc::clone(&player);
    let owner: SharedEntity = player_owner;
    let hook = test_hook(world, 51);
    hook.set_owner(&owner);
    let rod = ItemStack::new(&vanilla_items::FISHING_ROD);

    assert_eq!(hook.retrieve(&rod), 0);
    assert!(hook.is_removed());
    assert!(player.fishing_hook().is_none());
}

#[test]
fn shoot_from_player_respects_pitch_and_yaw_signs() {
    let world_fixture = fresh_test_world("fishing_hook_shoot_signs");
    let world = &world_fixture.world;

    // Straight down: pitch = 90.0, yaw = 0.0 -> Y velocity must be negative (downwards)
    let player_down = TestPlayerBuilder::new(Arc::clone(world), "Player", 100)
        .uuid(Uuid::from_u128(10))
        .build();
    player_down.set_rotation((0.0, 90.0));
    let hook_down = test_hook(world, 101);
    hook_down.shoot_from_player(&player_down, 0, 0);
    assert!(
        hook_down.velocity().y < -2.0,
        "Looking straight down must throw downwards, got y={}",
        hook_down.velocity().y
    );

    // Straight up: pitch = -90.0, yaw = 0.0 -> Y velocity must be positive (upwards)
    let player_up = TestPlayerBuilder::new(Arc::clone(world), "Player", 110)
        .uuid(Uuid::from_u128(11))
        .build();
    player_up.set_rotation((0.0, -90.0));
    let hook_up = test_hook(world, 111);
    hook_up.shoot_from_player(&player_up, 0, 0);
    assert!(
        hook_up.velocity().y > 2.0,
        "Looking straight up must throw upwards, got y={}",
        hook_up.velocity().y
    );

    // West: yaw = 90.0, pitch = 0.0 -> X velocity must be negative (-X is West)
    let player_west = TestPlayerBuilder::new(Arc::clone(world), "Player", 120)
        .uuid(Uuid::from_u128(12))
        .build();
    player_west.set_rotation((90.0, 0.0));
    let hook_west = test_hook(world, 121);
    hook_west.shoot_from_player(&player_west, 0, 0);
    assert!(
        hook_west.velocity().x < -0.8,
        "Looking West must throw in -X direction, got x={}",
        hook_west.velocity().x
    );

    // East: yaw = -90.0, pitch = 0.0 -> X velocity must be positive (+X is East)
    let player_east = TestPlayerBuilder::new(Arc::clone(world), "Player", 130)
        .uuid(Uuid::from_u128(13))
        .build();
    player_east.set_rotation((-90.0, 0.0));
    let hook_east = test_hook(world, 131);
    hook_east.shoot_from_player(&player_east, 0, 0);
    assert!(
        hook_east.velocity().x > 0.8,
        "Looking East must throw in +X direction, got x={}",
        hook_east.velocity().x
    );
}

#[test]
fn grounded_hook_does_not_hook_owner_when_player_stands_on_it() {
    steel_registry::init_vanilla_registry();
    init_behaviors();

    let world_fixture = fresh_test_world("fishing_hook_grounded_owner");
    let world = &world_fixture.world;
    let player = TestPlayerBuilder::new(Arc::clone(world), "Player", 200)
        .uuid(Uuid::from_u128(20))
        .build();
    player
        .inventory
        .lock()
        .set_selected_item(ItemStack::new(&vanilla_items::FISHING_ROD));
    let player_owner = Arc::clone(&player);
    let owner: SharedEntity = player_owner;
    let hook = test_hook(world, 201);
    hook.set_owner(&owner);
    hook.set_on_ground(true);
    hook.set_velocity(DVec3::ZERO);

    // Position hook inside player's bounding box
    hook.try_set_position(player.position())
        .expect("should position hook");

    Arc::clone(&hook).tick();

    let hooked_entity = hook.hook_state.lock().hooked_entity.clone();
    assert!(
        hooked_entity.is_none(),
        "Grounded stationary hook must not hook player standing on it"
    );
}

#[test]
fn submerged_hook_in_water_experiences_upward_buoyancy() {
    use steel_utils::types::UpdateFlags;

    steel_registry::init_vanilla_registry();
    init_behaviors();

    let world_fixture = fresh_loaded_test_world("fishing_hook_buoyancy");
    let world = &world_fixture.world;

    let water = vanilla_blocks::WATER.default_state();
    let pos_submerged = BlockPos::new(0, 60, 0);
    let pos_above = BlockPos::new(0, 61, 0);
    world.set_block(pos_submerged, water, UpdateFlags::UPDATE_NONE);
    world.set_block(pos_above, water, UpdateFlags::UPDATE_NONE);

    let player = TestPlayerBuilder::new(Arc::clone(world), "Player", 300)
        .uuid(Uuid::from_u128(30))
        .build();
    player
        .try_set_position(DVec3::new(0.5, 61.0, 0.5))
        .expect("should position player near water");
    player
        .inventory
        .lock()
        .set_selected_item(ItemStack::new(&vanilla_items::FISHING_ROD));
    let player_owner = Arc::clone(&player);
    let owner: SharedEntity = player_owner;

    let hook = test_hook(world, 301);
    hook.set_owner(&owner);
    hook.try_set_position(DVec3::new(0.5, 60.5, 0.5))
        .expect("should position hook");
    hook.set_velocity(DVec3::ZERO);
    hook.hook_state.lock().bobber_state = BobberState::Bobbing;

    Arc::clone(&hook).tick();

    assert!(
        hook.velocity().y > 0.0,
        "Submerged hook must accelerate upwards towards the water surface, got velocity.y={}",
        hook.velocity().y
    );
}

#[test]
fn open_water_calculation_identifies_open_lake_and_shallow_puddle() {
    use steel_utils::types::UpdateFlags;

    steel_registry::init_vanilla_registry();
    init_behaviors();

    let world_fixture = fresh_loaded_test_world("fishing_hook_open_water");
    let world = &world_fixture.world;

    let water = vanilla_blocks::WATER.default_state();
    let air = vanilla_blocks::AIR.default_state();
    let center = BlockPos::new(8, 64, 8);

    // Build 5x5 open water area:
    // y in -1..=0: water
    // y in 1..=2: air
    for y in -1..=0 {
        for dx in -2..=2 {
            for dz in -2..=2 {
                world.set_block(
                    BlockPos::new(center.x() + dx, center.y() + y, center.z() + dz),
                    water,
                    UpdateFlags::UPDATE_NONE,
                );
            }
        }
    }
    for y in 1..=2 {
        for dx in -2..=2 {
            for dz in -2..=2 {
                world.set_block(
                    BlockPos::new(center.x() + dx, center.y() + y, center.z() + dz),
                    air,
                    UpdateFlags::UPDATE_NONE,
                );
            }
        }
    }

    let hook = test_hook(world, 401);
    assert!(
        hook.calculate_open_water(center),
        "5x5 open water lake must be considered open water"
    );

    // Place a solid block in the water layer -> should no longer be open water
    world.set_block(
        BlockPos::new(center.x() + 1, center.y(), center.z()),
        vanilla_blocks::STONE.default_state(),
        UpdateFlags::UPDATE_NONE,
    );
    assert!(
        !hook.calculate_open_water(center),
        "Obstructed water area must not be considered open water"
    );
}
