//! The four extension ports loaded together into a real server, driven through commands and
//! player actions of a controlled test peer.
#![cfg(feature = "host")]
#![recursion_limit = "256"]
mod common;

use common::{
    Fixture, break_block, command, extension_data_root, grants, load_extensions, on_tick, start,
    stop, teleport, until,
};
use glam::DVec3;
use std::time::Duration;
use std::{fs, sync::Arc};
use steel_core::{entity::Entity, server::Server};
use steel_registry::vanilla_blocks;
use steel_utils::{
    BlockPos,
    types::{GameType, UpdateFlags},
};
use tokio::time::sleep;
use uuid::Uuid;

/// Explicit grants instead of `*`, so the `WorldGuard` bypass stays off.
const GRANTS: [&str; 5] = [
    "essentials.*",
    "worldedit.*",
    "worldguard.region.define",
    "worldguard.region.removeowner",
    "worldguard.region.flag",
];

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "the ports share one server lifecycle before process-global teardown"
)]
#[ignore = "requires cargo build --manifest-path extension-projects/Cargo.toml"]
fn ports_protect_edit_teleport_and_persist_through_real_commands() -> Result<(), String> {
    let data_root = extension_data_root();
    let loaded = load_extensions(
        &[
            ("worldguard", ""),
            (
                "essentials",
                "starting_balance = 250.0\nafk_timeout_seconds = 0\n",
            ),
            ("worldedit", ""),
            ("vault", ""),
        ],
        &data_root,
    )?;
    assert_eq!(
        loaded.order,
        ["worldedit", "worldguard", "vault", "essentials"]
    );
    let initialized = loaded.initialized;
    let fixture = Fixture::new()?;
    let runtime = Arc::clone(&fixture.runtime);
    runtime.block_on(async {
        let home = &fixture.world;
        let uuid = Uuid::new_v4();
        let running = start(
            home,
            (loaded.extensions, loaded.commands),
            grants(uuid, &GRANTS)?,
            uuid,
            "PortTester",
        )
        .await?;
        let (server, player) = (Arc::clone(&running.server), Arc::clone(&running.player));
        let actor = Arc::clone(&player);
        on_tick(&server, move || {
            actor.set_game_mode(GameType::Creative);
            Ok(())
        })
        .await?;

        // Essentials: a home survives teleporting away.
        teleport(&server, &player, DVec3::new(8.5, 65.0, 8.5)).await?;
        command(&server, &player, "sethome base").await?;
        teleport(&server, &player, DVec3::new(3.5, 65.0, 3.5)).await?;
        command(&server, &player, "home base").await?;
        until(&player, "home teleport", || {
            (player.position() - DVec3::new(8.5, 65.0, 8.5)).length_squared() < 0.01
        })
        .await?;
        // `/back` returns to where `/home` came from.
        command(&server, &player, "back").await?;
        until(&player, "back teleport", || {
            (player.position() - DVec3::new(3.5, 65.0, 3.5)).length_squared() < 0.01
        })
        .await?;
        command(&server, &player, "eco give PortTester 5.25").await?;

        // WorldEdit: select 4x2x4 above the floor, fill, undo.
        teleport(&server, &player, DVec3::new(2.5, 65.0, 2.5)).await?;
        command(&server, &player, "/pos1").await?;
        teleport(&server, &player, DVec3::new(5.5, 66.0, 5.5)).await?;
        command(&server, &player, "/pos2").await?;
        command(&server, &player, "/set minecraft:glass").await?;
        let glass = vanilla_blocks::GLASS.default_state();
        let air = vanilla_blocks::AIR.default_state();
        let filled = || {
            (2..=5).all(|x| {
                (65..=66)
                    .all(|y| (2..=5).all(|z| home.get_block_state(BlockPos::new(x, y, z)) == glass))
            })
        };
        until(&player, "//set", filled).await?;
        command(&server, &player, "/undo").await?;
        until(&player, "//undo", || {
            home.get_block_state(BlockPos::new(3, 65, 3)) == air
                && home.get_block_state(BlockPos::new(5, 66, 5)) == air
        })
        .await?;

        // WorldGuard: the owner can build; after removing ownership the break is refused,
        // and an explicit allow flag permits it again.
        command(&server, &player, "rg define spawn").await?;
        let target = BlockPos::new(3, 65, 3);
        let place_stone = |server: Arc<Server>| {
            let world = Arc::clone(home);
            async move {
                on_tick(&server, move || {
                    world.set_block(
                        target,
                        vanilla_blocks::STONE.default_state(),
                        UpdateFlags::UPDATE_ALL,
                    );
                    Ok(())
                })
                .await
            }
        };
        place_stone(Arc::clone(&server)).await?;
        break_block(&server, &player, target).await?;
        until(&player, "owner break", || {
            home.get_block_state(target) == air
        })
        .await?;
        command(&server, &player, "rg removeowner spawn PortTester").await?;
        place_stone(Arc::clone(&server)).await?;
        break_block(&server, &player, target).await?;
        sleep(Duration::from_millis(100)).await;
        assert_eq!(
            home.get_block_state(target),
            vanilla_blocks::STONE.default_state(),
            "non-members must not break blocks in a region"
        );
        command(&server, &player, "rg flag spawn build allow").await?;
        break_block(&server, &player, target).await?;
        until(&player, "flagged break", || {
            home.get_block_state(target) == air
        })
        .await?;

        // Persistence: Essentials saves within 100 ticks; WorldGuard on every change.
        let user_file = data_root
            .join("essentials/userdata")
            .join(format!("{uuid}.toml"));
        until(&player, "essentials save", || {
            fs::read_to_string(&user_file)
                .is_ok_and(|text| text.contains("money = 25525") && text.contains("[homes.base]"))
        })
        .await?;
        let regions = fs::read_to_string(data_root.join("worldguard/regions/home_overworld.toml"))
            .map_err(|e| e.to_string())?;
        assert!(regions.contains("[regions.spawn]") && regions.contains("build = \"allow\""));

        stop(running, &initialized, home).await?;
        Ok(())
    })
}
