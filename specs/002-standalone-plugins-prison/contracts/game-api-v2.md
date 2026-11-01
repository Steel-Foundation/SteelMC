# Contract: Game API added in iteration 2

- Items: `items::ItemSpec` builder; `Server::{held_item, slot, set_slot, give, remove_item,
  set_held_slot, cursor}`; `ItemSpec::data(key)`/`with_data`.
- Menus: `menus::Menu::new(title, rows)`, `.slot(i, spec)`, `.open(server, player, on_click)`;
  `MenuHandle::{update_slot, close}`; click closure gets `MenuClick { player, slot, button, kind }`.
- Effects: `World::{particles, particle_for, play_sound, sound_for}` with identifier keys and dust
  color helpers; `Server::{add_effect, remove_effect, has_effect, give_experience, set_level,
  set_walk_speed, set_fly_speed}`.
- Entities: `World::{drop_item(spec, pos), lightning_flash(pos), remove_entity(id)}`.
- HUD: `Server::{set_sidebar(player, title, lines), clear_sidebar, set_tab_list(player, header,
  footer)}`; `BossBar::{create, set_title, set_progress, set_color, set_style, add_viewer,
  remove_viewer, remove}`.
- Worlds: `Server::with_world(key, |world| ..)`; `World::{time, set_time, weather, set_weather}`.
- Bulk: `World::fill(FillRequest) -> FillReport` (see data-model).
- Messaging: `Hooks::register_channel(name)`; `Server::send_plugin_message(player, channel,
  bytes)`; `PluginMessageEvent`.
