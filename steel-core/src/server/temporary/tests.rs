use super::*;
use crate::player::Player;
use crate::{
    bootstrap::init_globals,
    config::ResolvedDomainConfig,
    entity::{Entity, LivingEntity, SharedEntity},
    permission::PermissionSubjectIndex,
    player::{PlayerInput, bot::BotInput},
    server::test_server_with_worlds_and_config,
    test_support::{fresh_test_world, test_runtime_config},
    world::LevelReader,
};
use glam::DVec3;
use std::slice::from_ref;
use std::time::{Duration, Instant};
use steel_registry::item_stack::ItemStack;
use steel_registry::{vanilla_blocks, vanilla_items};
use steel_utils::types::UpdateFlags;
use tokio::{
    runtime::Builder,
    task::JoinSet,
    time::{sleep, timeout},
};

async fn finish(server: &Arc<Server>, id: WorldOperationId) -> Result<Identifier, String> {
    let deadline = Instant::now() + Duration::from_secs(10);
    for tick in 1.. {
        server.tick_jobs(tick, true);
        if let Some(result) = server.take_world_operation(id)? {
            return result;
        }
        if Instant::now() >= deadline {
            return Err("world operation did not finish".into());
        }
        sleep(Duration::from_millis(1)).await;
    }
    Err("tick identity exhausted".into())
}
#[test]
#[expect(
    clippy::too_many_lines,
    reason = "one lifecycle verifies movement, mining, falling, occupied retirement, and resource release"
)]
fn temporary_worlds_run_real_bot_physics_and_retire_without_retaining_worlds() -> Result<(), String>
{
    init_globals();
    let fixture = fresh_test_world("temporary_lifecycle_home");
    let home = &fixture.world;
    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    runtime.block_on(async {
        let domain = ResolvedDomainConfig {
            name: home.domain().to_owned(),
            default_world: home.key.clone(),
            worlds: vec![home.key.clone()],
        };
        let server = test_server_with_worlds_and_config(
            domain.name.clone(),
            from_ref(&domain),
            from_ref(home),
            PermissionSubjectIndex::new(),
            test_runtime_config(16),
            None,
        )
        .await?;
        let request = TemporaryWorldRequest {
            key: Identifier::new_static("temporary_test", "lobby"),
            primary: true,
            generator: Identifier::from_steel("empty"),
            config: toml::from_str("dimension_type='minecraft:overworld'")
                .map_err(|e| e.to_string())?,
            seed: 17,
            spawn: BlockPos::new(0, 65, 0),
            default_gamemode: GameType::Adventure,
            difficulty: Difficulty::Peaceful,
        };
        let id = server.create_temporary_world(request)?;
        assert!(
            !server.worlds.has_domain("temporary_test"),
            "creation does not publish until its job stage"
        );
        let key = finish(&server, id).await?;
        let world = server.worlds.get(&key).ok_or("world was not published")?;
        let weak = Arc::downgrade(&world);
        assert_eq!(server.worlds.len(), 2);
        assert!(world.level_data.read().owns_game_time());
        for x in -6..=6 {
            for z in -6..=6 {
                assert!(world.set_block(
                    BlockPos::new(x, 64, z),
                    vanilla_blocks::SNOW_BLOCK.default_state(),
                    UpdateFlags::UPDATE_ALL
                ));
            }
        }
        let player = server.spawn_bot(&key, "PhysicsBot", DVec3::new(0.5, 65.0, 0.5))?;
        assert!(player.has_client_loaded());
        assert!(player.has_joined_world());
        assert!(!player.is_world_change_pending());
        player.set_game_mode(GameType::Survival);
        player
            .inventory
            .lock()
            .set_selected_item(ItemStack::new(&vanilla_items::DIAMOND_SHOVEL));
        let entity: SharedEntity = Arc::<Player>::clone(&player);
        for _ in 0..8 {
            player.control_bot(BotInput {
                yaw: -90.0,
                pitch: 0.0,
                movement: PlayerInput::from_controls(
                    true, false, false, false, false, false, false,
                ),
                dig: None,
            })?;
            player.tick_living_entity(&entity);
        }
        assert!(
            player.position().x > 0.8,
            "server travel must commit bot movement"
        );
        let dig = BlockPos::new(player.position().x.floor() as i32 + 1, 64, 0);
        player.control_bot(BotInput {
            dig: Some(dig),
            ..Default::default()
        })?;
        player.tick_living_entity(&entity);
        assert_ne!(
            world.get_block_state(dig),
            vanilla_blocks::SNOW_BLOCK.default_state(),
            "bot mining uses the equipped shovel"
        );
        let below = BlockPos::new(
            player.position().x.floor() as i32,
            64,
            player.position().z.floor() as i32,
        );
        for x in below.x() - 1..=below.x() + 1 {
            for z in below.z() - 1..=below.z() + 1 {
                world.set_block(
                    BlockPos::new(x, 64, z),
                    vanilla_blocks::AIR.default_state(),
                    UpdateFlags::UPDATE_ALL,
                );
            }
        }
        for _ in 0..16 {
            player.tick_living_entity(&entity);
        }
        assert!(
            player.position().y < 64.0,
            "bots must fall under normal gravity"
        );
        let destroy = server.destroy_temporary_domain("temporary_test")?;
        assert!(
            server
                .queue_domain_switch(Arc::clone(&player), "temporary_test".into())
                .is_err()
        );
        server.tick_jobs(100, true);
        assert!(
            server.worlds.has_domain("temporary_test"),
            "occupied worlds cannot retire"
        );
        server.despawn_bot(&player)?;
        let mut saves = JoinSet::new();
        server.start_player_disconnect_saves(&mut saves);
        while let Some(result) = saves.join_next().await {
            result.map_err(|e| e.to_string())?;
        }
        drop(entity);
        drop(player);
        drop(world);
        finish(&server, destroy).await?;
        assert!(!server.worlds.has_domain("temporary_test"));
        assert!(
            weak.upgrade().is_none(),
            "retirement must release the actual world and chunks"
        );
        assert_eq!(server.worlds.len(), 1);
        server.jobs.cancel_all();
        Ok(())
    })
}

#[test]
fn cancelling_prepared_unpublished_worlds_drains_their_background_work() -> Result<(), String> {
    init_globals();
    let fixture = fresh_test_world("temporary_cancel_home");
    let home = &fixture.world;
    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    runtime.block_on(async {
        let domain = ResolvedDomainConfig {
            name: home.domain().to_owned(),
            default_world: home.key.clone(),
            worlds: vec![home.key.clone()],
        };
        let server = test_server_with_worlds_and_config(
            domain.name.clone(),
            from_ref(&domain),
            from_ref(home),
            PermissionSubjectIndex::new(),
            test_runtime_config(4),
            None,
        )
        .await?;
        let id = server.create_temporary_world(TemporaryWorldRequest {
            key: Identifier::new_static("cancelled_room", "lobby"),
            primary: true,
            generator: Identifier::from_steel("empty"),
            config: toml::from_str("dimension_type='minecraft:overworld'")
                .map_err(|e| e.to_string())?,
            seed: 0,
            spawn: BlockPos::new(0, 65, 0),
            default_gamemode: GameType::Adventure,
            difficulty: Difficulty::Peaceful,
        })?;
        // Finish real generation, leaving its prepared world inside the unpublished job.
        let deadline = Instant::now() + Duration::from_secs(10);
        while !server.temporary_worlds.cleanup.is_empty() {
            if Instant::now() >= deadline {
                return Err("world preparation did not finish".into());
            }
            sleep(Duration::from_millis(1)).await;
        }
        assert!(!server.worlds.has_domain("cancelled_room"));
        server.jobs.cancel_all();
        server.temporary_worlds.cleanup.close();
        timeout(
            Duration::from_secs(10),
            server.temporary_worlds.cleanup.wait(),
        )
        .await
        .map_err(|e| e.to_string())?;
        let result = server
            .take_world_operation(id)?
            .ok_or("cancelled operation remained pending")?;
        assert!(
            result
                .expect_err("cancelled creation must not publish")
                .contains("cancelled")
        );
        assert!(!server.worlds.has_domain("cancelled_room"));
        Ok(())
    })
}
