//! Records the name of every event it receives in `<data>/events.log` and cancels a few, so
//! integration tests can check that each binding reaches the extension. Not meant for servers.
#![recursion_limit = "256"]

use std::{
    io::Write as _,
    sync::{Mutex, OnceLock, PoisonError},
};
use steel_extension_api::{
    BlockPos, Hooks, ExtensionContext, Server,
    effects::{Effect, Particle, SoundCategory},
    events::EventPriority,
    fill::{FillMode, FillRequest, PatternEntry},
    hud::{BarColor, BossBar},
    items::ItemSpec,
    menus::{self, MenuHandler},
    players::Uuid,
    scheduler,
};

static LOG: OnceLock<Mutex<std::fs::File>> = OnceLock::new();
static MENU: OnceLock<MenuHandler> = OnceLock::new();

/// Exercises the game API on the first player; triggered by the console line `probe hud`.
fn probe_hud(server: &Server<'_>) -> Result<(), String> {
    let players = server.players();
    let target = players.first().ok_or("no player")?;
    let (id, world, at) = (target.id, target.world.to_string(), target.position);
    let ok =
        |result: steel_extension_api::ApiResult<()>| Result::from(result).map_err(|e| e.to_string());
    ok(server.set_sidebar(
        id,
        "Probe".into(),
        ["line a", "line b"].into_iter().map(Into::into).collect(),
    ))?;
    ok(server.set_boss_bar(
        id,
        Uuid { bytes: [7; 16] },
        BossBar::new("Bar", 0.5, BarColor::Yellow),
    ))?;
    ok(server.set_tab_list(id, "Header".into(), "Footer".into()))?;
    Result::from(server.add_effect(id, Effect::new("minecraft:haste", 200, 1)))
        .map_err(|e| e.to_string())?;
    ok(server.set_walk_speed(id, 0.2))?;
    ok(server.play_sound_to(
        id,
        "minecraft:block.note_block.harp".into(),
        SoundCategory::Master,
        1.0,
        1.0,
    ))?;
    let dropped = server.with_world(&world, |world| -> Result<bool, String> {
        Result::from(world.spawn_particle(Particle::note(at, 5))).map_err(|e| e.to_string())?;
        Result::from(
            world.spawn_particle(Particle::new("minecraft:dust", at, 10).dust(0xFF_AA00, 1.5)),
        )
        .map_err(|e| e.to_string())?;
        ok(world.set_day_time(1000))?;
        let _ = world.strike_lightning_effect(at);
        let gold = Result::from(steel_extension_api::default_block_state(
            "minecraft:gold_block".into(),
        ))
        .map_err(|e| e.to_string())?;
        let air = Result::from(steel_extension_api::default_block_state(
            "minecraft:air".into(),
        ))
        .map_err(|e| e.to_string())?;
        let (a, b) = (
            BlockPos {
                x: -4,
                y: 120,
                z: -4,
            },
            BlockPos { x: 4, y: 120, z: 4 },
        );
        let entry = |state| PatternEntry { state, weight: 1 };
        Result::from(world.fill(FillRequest::cuboid(a, b, &[entry(gold)])))
            .map_err(|e| e.to_string())?;
        let mined = Result::from(
            world.fill(FillRequest::cuboid(a, b, &[entry(air)]).mode(FillMode::NonAir)),
        )
        .map_err(|e| e.to_string())?;
        record(&format!("fill:{}", mined.replaced_count(gold)));
        let entity = Result::from(world.drop_item(at, ItemSpec::new("minecraft:diamond")))
            .map_err(|e| e.to_string())?;
        Result::from(world.remove_entity(entity)).map_err(|e| e.to_string())
    })??;
    let handler = *MENU.get().ok_or("menu handler missing")?;
    server.open_menu_with(
        id,
        "Probe menu",
        1,
        [ItemSpec::new("minecraft:diamond")],
        handler,
    )?;
    record(&format!("probe_hud:{dropped}"));
    Ok(())
}

fn record(name: &str) {
    if let Some(file) = LOG.get() {
        let mut file = file.lock().unwrap_or_else(PoisonError::into_inner);
        let _ = writeln!(file, "{name}");
    }
}

fn pre_init(mut hooks: Hooks<'_>) -> Result<(), String> {
    hooks.enable_scheduler();
    let _ = MENU.set(menus::handler(|server, click| {
        record(&format!("menu_click:{}:{:?}", click.slot, click.kind));
        let _ = server.set_menu_item(click.player, 1, ItemSpec::new("minecraft:emerald"));
    }));
    let p = EventPriority::Normal;
    hooks.on_item_held(p, |event| {
        record("item_held");
        event.set_cancelled(true);
    });
    hooks.on_toggle(p, |event| {
        record(&format!("toggle:{:?}:{}", event.kind(), event.enabled()));
    });
    hooks.on_game_mode_change(p, |event| {
        record("game_mode_change");
        event.set_cancelled(true);
    });
    hooks.on_swap_hands(p, |event| {
        record("swap_hands");
        event.set_cancelled(true);
    });
    hooks.on_drop_item(p, |event| {
        record(&format!("drop_item:{}", event.item().item));
        event.set_cancelled(true);
    });
    hooks.on_explosion(p, |event| {
        record(&format!("explosion:{}", event.blocks().len()));
        event.set_cancelled(true);
    });
    hooks.on_entity_spawn(p, |event| {
        record(&format!("entity_spawn:{}", event.entity().kind));
    });
    hooks.on_world_load(p, |event| {
        record(&format!("world_load:{}", event.world().key()));
    });
    hooks.on_chunk_load(p, |_| record("chunk_load"));
    hooks.on_server_command(p, |event| {
        let command = event.command();
        record(&format!("server_command:{command}"));
        if command.as_str() == "probe hud" {
            event.set_cancelled(true);
            scheduler::run_later(1, |server| {
                if let Err(error) = probe_hud(server) {
                    record(&format!("probe_hud_error:{error}"));
                }
            });
        }
    });
    hooks.on_server_list_ping(p, |event| event.set_motd("probe motd".into()));
    hooks.on_inventory_open(p, |_| record("inventory_open"));
    hooks.on_inventory_close(p, |_| record("inventory_close"));
    hooks.on_exp_change(p, |event| {
        record(&format!("exp_change:{}", event.amount()));
        event.set_amount(event.amount() * 2);
    });
    hooks.on_block_break(p, |event| {
        record("block_break");
        event.set_drop_items(false);
    });
    Ok(())
}

fn init(context: ExtensionContext<'_>) -> Result<(), String> {
    let path = std::path::Path::new(context.data_directory().as_str()).join("events.log");
    let file = std::fs::File::create(path).map_err(|error| error.to_string())?;
    LOG.set(Mutex::new(file))
        .map_err(|_| "probe initialized twice".to_owned())
}

steel_extension_api::export_extension!("event-probe", pre_init, init);
