# Contract: Events added in iteration 2

All follow the iteration-1 pattern (`hooks.on_<event>[_with](priority, ..., closure)`).
✱ = cancellable.

| Handle | Accessors / setters |
|---|---|
| `RespawnEvent` | player, location, `set_location` |
| `WorldChangeEvent` | player, from world, to world |
| `GameModeChangeEvent`✱ | player, new mode |
| `HeldSlotEvent`✱ | player, previous, new slot |
| `ToggleSneakEvent`✱ / `ToggleSprintEvent`✱ / `ToggleFlightEvent`✱ | player, new state |
| `SwapHandsEvent`✱ | player |
| `DropItemEvent`✱ | player, item spec |
| `PickupItemEvent`✱ | player, item spec, entity id |
| `EntityInteractEvent`✱ | player, target entity info, hand |
| `InventoryOpenEvent`✱ | player, title, menu id (plugin menus) |
| `InventoryClickEvent`✱ | player, menu id, slot, button, click type, item in slot |
| `InventoryCloseEvent` | player, menu id |
| `SignChangeEvent`✱ | player, pos, front side, lines, `set_line` |
| `ExperienceChangeEvent` | player, amount, `set_amount` |
| `FoodLevelChangeEvent`✱ | player, new level |
| `EntitySpawnEvent`✱ | entity info, world |
| `EntityDeathEvent` | entity info, `set_drop_experience`, `clear_drops` |
| `ExplosionEvent`✱ | world, center, radius, source, blocks, `retain_blocks(filter)` |
| `PistonEvent`✱ | world, pos, direction, extending, moved blocks |
| `ChunkLoadEvent` / `ChunkUnloadEvent` | world, chunk x/z |
| `WorldLoadEvent` / `WorldUnloadEvent` | world key |
| `WeatherChangeEvent`✱ | world, raining, thundering |
| `ServerCommandEvent`✱ | sender kind, command |
| `ServerListPingEvent` | motd, max players, online count, setters |
| `PluginMessageEvent` | player, channel, bytes |
| `BlockBreakEvent` (extended) | `set_drop_items(bool)`, `set_exp_to_drop(i32)` |
