# Contract: Events

## Subscription (pre-init only)

```rust
hooks.on_chat(EventPriority::Normal, |event: &mut ChatEvent<'_>| { ... })?;
hooks.on_block_break_with(EventPriority::High, /*ignore_cancelled*/ true, |event| { ... })?;
```

- Native form: `Hooks::subscribe_<event>(priority, ignore_cancelled, extern "C" fn(Event<'_>, u64), token: u64)`.
- Subscriptions after pre-init return an error (registries frozen).
- Handlers run on the thread processing the action (packet worker or world thread). They may run
  concurrently for different players; they never outlive the call.

## Common handle methods

| Method | Notes |
|---|---|
| `is_cancelled() -> bool` / `set_cancelled(bool)` | cancellable events only |
| `server() -> Server<'_>` | borrowed for the callback |
| `player() -> PlayerId` | player events |
| `world() -> World<'_>` | block, interact, move, death events |

## Event-specific

- `PreLoginEvent`: `uuid() -> [u8;16]`, `name()`, `deny(reason_json)`.
- `JoinEvent` / `QuitEvent` / `DeathEvent`: `message() -> Option<String JSON>`,
  `set_message(Option<json>)`.
- `ChatEvent`: `message()`, `set_message(str)`, `set_rendered(json)` — when set, every recipient
  receives that component as system chat and the signed message is not relayed (documented
  deviation from vanilla signed chat, same as Paper chat renderers).
- `CommandEvent`: `command()` (without leading `/`), `set_command(str)`.
- `BlockBreakEvent`: `pos()`, `state()`.
- `BlockPlaceEvent`: `pos()`, `state()`, `against()`.
- `InteractEvent`: `action() -> InteractAction {LeftClickBlock, RightClickBlock, RightClickAir}`,
  `hand() -> Hand`, `pos() -> Option<BlockPos>`, `face()`, `item() -> String` (held item key).
- `DamageEvent`: `victim() -> EntityInfo`, `attacker() -> Option<EntityInfo>`, `cause() -> String`
  (damage type key), `amount()`, `set_amount(f32)`.
- `MoveEvent`: `from()`, `to()` (Position).
- `TeleportEvent`: `from() -> Location`, `to() -> Location`, `set_to(Location)`, `cause()`.

## Guarantees

1. Priority order Lowest→Monitor; equal priority in subscription order.
2. Cancelled flag is visible to later handlers; `ignore_cancelled` handlers are skipped while set.
3. Monitor handlers cannot change the outcome (setters are no-ops, logged once at debug).
4. With no subscribers, behavior is identical to unmodified Steel.
5. A cancelled player action is resynchronized to the client (blocks, inventory, position).
