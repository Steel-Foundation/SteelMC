//! The OP Prison bundle (`prison-mines`, `prison-enchants`) with `WorldEdit`, Vault and Essentials,
//! loaded from their built libraries and driven through commands, breaks and menu clicks.
#![cfg(feature = "host")]
#![recursion_limit = "256"]
mod common;

use common::{
    Fixture, break_block, command, extension_data_root, grants, load_extensions, on_tick, start,
    stop, teleport, until,
};
use glam::DVec3;
use rustc_hash::FxHashMap;
use std::{fs, path::Path, sync::Arc};
use steel_core::{player::Player, world::World};
use steel_protocol::packets::game::{ClickType, HashedStack, SContainerClick};
use steel_registry::{
    data_components::vanilla_components::CUSTOM_DATA, vanilla_blocks, vanilla_items,
};
use steel_utils::{BlockPos, types::GameType};
use uuid::Uuid;

const MINES: &str = r#"
[mines.a]
world = "home:overworld"
min = [2, 60, 2]
max = [13, 63, 13]
interval = 0
threshold = 0

[[mines.a.mix]]
block = "minecraft:stone"
weight = 3

[[mines.a.mix]]
block = "minecraft:coal_ore"
weight = 1
"#;

/// Jackhammer always procs and costs 1 per level, so the test is deterministic.
const ENCHANTS: &str = "
[enchants.jackhammer]
max = 100
base_cost = 1.0
growth = 1.0
base_chance = 1.0
chance_per_level = 0.0
";

fn mine_layer_is(home: &Arc<World>, y: i32, check: impl Fn(bool) -> bool) -> bool {
    let air = vanilla_blocks::AIR.default_state();
    (2..=13).all(|x| (2..=13).all(|z| check(home.get_block_state(BlockPos::new(x, y, z)) == air)))
}

fn balance_cents(data_root: &Path, uuid: Uuid) -> i64 {
    let file = data_root
        .join("essentials/userdata")
        .join(format!("{uuid}.toml"));
    fs::read_to_string(file)
        .ok()
        .and_then(|text| {
            text.lines()
                .find_map(|line| line.strip_prefix("money = ").map(str::to_owned))
        })
        .and_then(|money| money.trim().parse().ok())
        .unwrap_or(0)
}

fn held_jackhammer(player: &Player) -> Option<String> {
    let inventory = player.inventory.lock();
    let item = inventory.get_selected_item();
    let data = item.get(CUSTOM_DATA)?;
    let entries = data.as_compound().compound("steel_extensions")?;
    entries
        .string("prison-enchants:jackhammer")
        .map(|value| value.to_str().into_owned())
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "one server session walks the whole demo in order"
)]
#[ignore = "requires cargo build --manifest-path extension-projects/Cargo.toml"]
fn prison_bundle_resets_mines_jackhammers_layers_and_sells_into_vault() -> Result<(), String> {
    let data_root = extension_data_root();
    fs::create_dir_all(data_root.join("prison-mines")).map_err(|e| e.to_string())?;
    fs::write(data_root.join("prison-mines/mines.toml"), MINES).map_err(|e| e.to_string())?;
    let loaded = load_extensions(
        &[
            ("prison_enchants", ENCHANTS),
            ("prison_mines", ""),
            ("worldedit", ""),
            ("vault", ""),
            (
                "essentials",
                "starting_balance = 0.0\nafk_timeout_seconds = 0\n",
            ),
        ],
        &data_root,
    )?;
    let index = |name: &str| loaded.order.iter().position(|loaded| *loaded == name);
    assert!(
        index("prison-mines") < index("prison-enchants"),
        "dependencies load first: {:?}",
        loaded.order
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
            grants(uuid, &["prison.*", "essentials.*"])?,
            uuid,
            "Miner",
        )
        .await?;
        let (server, player) = (Arc::clone(&running.server), Arc::clone(&running.player));
        let actor = Arc::clone(&player);
        on_tick(&server, move || {
            actor.set_game_mode(GameType::Creative);
            Ok(())
        })
        .await?;

        // The mine refills from its mix.
        command(&server, &player, "mine reset a").await?;
        until(&player, "mine reset", || {
            (60..=63).all(|y| mine_layer_is(home, y, |air| !air))
        })
        .await?;
        let coal = vanilla_blocks::COAL_ORE.default_state();
        let has_coal = (60..=63).any(|y| {
            (2..=13).any(|x| (2..=13).any(|z| home.get_block_state(BlockPos::new(x, y, z)) == coal))
        });
        assert!(has_coal, "a 3:1 mix over 576 blocks contains coal ore");

        // The pickaxe lands in the selected hotbar slot.
        command(&server, &player, "pickaxe").await?;
        until(&player, "pickaxe", || {
            player
                .inventory
                .lock()
                .get_selected_item()
                .is(&vanilla_items::DIAMOND_PICKAXE)
        })
        .await?;
        assert_eq!(held_jackhammer(&player).as_deref(), Some("1"));

        // Breaking one block procs Jackhammer: the whole top layer goes, the rest stays,
        // and everything is sold into the Essentials economy.
        teleport(&server, &player, DVec3::new(8.5, 65.0, 8.5)).await?;
        break_block(&server, &player, BlockPos::new(6, 63, 6)).await?;
        until(&player, "jackhammer layer", || {
            mine_layer_is(home, 63, |air| air)
        })
        .await?;
        assert!(
            mine_layer_is(home, 62, |air| !air),
            "only the broken layer is cleared"
        );
        until(&player, "auto-sell payout", || {
            balance_cents(&data_root, uuid) > 0
        })
        .await?;

        // The upgrade menu buys Jackhammer II with that money.
        command(&server, &player, "enchants").await?;
        until(&player, "enchant menu", || {
            player.open_container_id().is_some()
        })
        .await?;
        let container_id = i32::from(player.open_container_id().ok_or("menu closed")?);
        let clicker = Arc::clone(&player);
        on_tick(&server, move || {
            clicker.handle_container_click(SContainerClick {
                container_id,
                state_id: 0,
                slot_num: 10,
                button_num: 0,
                click_type: ClickType::Pickup,
                changed_slots: FxHashMap::default(),
                carried_item: HashedStack::Empty,
            });
            Ok(())
        })
        .await?;
        until(&player, "jackhammer upgrade", || {
            held_jackhammer(&player).as_deref() == Some("2")
        })
        .await?;

        stop(running, &initialized, home).await?;
        Ok(())
    })
}
