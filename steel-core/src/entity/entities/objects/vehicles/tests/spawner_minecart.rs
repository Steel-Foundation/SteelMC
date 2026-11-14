use std::io::Cursor;

use simdnbt::borrow::read_compound;
use steel_registry::{vanilla_entities, vanilla_game_rules};
use steel_utils::{ChunkPos, Downcast as _};

use super::*;
use crate::block_entity::entities::BaseSpawnerConfig;
use crate::bootstrap::init_globals;
use crate::entity::entities::objects::vehicles::test_support::test_world;
use crate::entity::{ENTITIES, next_entity_id};
use crate::player::ResetReason;
use crate::test_support::TestPlayerBuilder;

#[test]
fn factory_cart_round_trips_minecart_and_spawner_data_together() {
    init_globals();
    let original = SpawnerMinecartEntity::new(
        &vanilla_entities::SPAWNER_MINECART,
        next_entity_id(),
        DVec3::ZERO,
        Weak::new(),
    );
    original.set_custom_display_block(Some(vanilla_blocks::STONE.default_state()));
    original.set_display_offset(-7);
    original.minecart_base().set_flipped(true);
    original.set_first_tick(false);
    original.spawner().set_entity_id(&vanilla_entities::PIG);
    original.spawner().set_spawn_delay(37);
    original.spawner().set_config(BaseSpawnerConfig {
        min_spawn_delay: 13,
        max_spawn_delay: 29,
        spawn_count: 2,
        ..BaseSpawnerConfig::default()
    });

    let mut saved = NbtCompound::new();
    original.save_additional(&mut saved);
    let mut bytes = Vec::new();
    saved.write(&mut bytes);
    let borrowed = read_compound(&mut Cursor::new(bytes.as_slice()))
        .expect("saved spawner minecart NBT should be readable");
    let loaded = ENTITIES
        .create(
            &vanilla_entities::SPAWNER_MINECART,
            next_entity_id(),
            DVec3::ZERO,
            Weak::new(),
        )
        .expect("spawner minecart factory should be registered");
    loaded.load_additional((&borrowed).into());
    let loaded = loaded
        .downcast_ref::<SpawnerMinecartEntity>()
        .expect("factory should create a spawner minecart");

    assert_eq!(
        loaded.custom_display_block(),
        original.custom_display_block()
    );
    assert_eq!(loaded.display_offset(), -7);
    assert!(loaded.is_flipped());
    assert!(!loaded.is_first_tick());
    assert_eq!(loaded.spawner().spawn_delay(), 37);
    assert_eq!(loaded.spawner().config(), original.spawner().config());
    assert_eq!(
        loaded.spawner().next_spawn_data(),
        original.spawner().next_spawn_data()
    );
}

#[test]
fn spawner_uses_the_post_movement_block_position_and_obeys_the_game_rule() {
    let fixture = test_world("spawner_minecart_post_movement", ChunkPos::new(0, 0));
    let world = &fixture.world;
    let player =
        TestPlayerBuilder::new(Arc::clone(world), "SpawnerPlayer", next_entity_id()).build();
    player.snap_to(DVec3::new(9.6, 65.2, 8.5), 0.0, 0.0);
    assert!(world.add_player(player, ResetReason::InitialJoin));
    let cart = Arc::new(SpawnerMinecartEntity::new(
        &vanilla_entities::SPAWNER_MINECART,
        next_entity_id(),
        DVec3::new(8.9, 64.0, 8.5),
        Arc::downgrade(world),
    ));
    world
        .try_add_entity(Arc::clone(&cart).into_shared_entity())
        .expect("spawner minecart should enter the loaded chunk");
    cart.set_no_gravity(true);
    cart.set_velocity(DVec3::new(0.4, 0.0, 0.0));
    cart.spawner().set_config(BaseSpawnerConfig {
        required_player_range: 1,
        ..BaseSpawnerConfig::default()
    });
    cart.spawner().set_spawn_delay(2);

    Arc::clone(&cart).tick();

    assert_eq!(cart.block_position().x(), 9);
    assert_eq!(cart.spawner().spawn_delay(), 1);

    assert!(world.set_game_rule(&vanilla_game_rules::SPAWNER_BLOCKS_WORK, false));
    cart.set_velocity(DVec3::ZERO);
    Arc::clone(&cart).tick();
    assert_eq!(cart.spawner().spawn_delay(), 1);
}
