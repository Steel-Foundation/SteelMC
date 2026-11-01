# Contract: Commands

## Plugin-side builder

```rust
hooks.register_commands(|commands| {
    commands.register(
        CommandBuilder::new("essentials:home")
            .alias("homes")
            .default_access()                       // else operators/explicit grant only
            .executes(|ctx| home(ctx, None))
            .then(argument("name", Argument::word())
                .suggests(home_names)
                .executes(|ctx| home(ctx, Some(ctx.word("name")?)))),
    )
})
```

- Permission for every node derives from the id: `essentials.command.home`; `.permission("x")`
  on a literal adds a sub-permission `essentials.command.home.x` (Steel `subcommand_permission`).
- Argument kinds: `boolean`, `integer(min,max)`, `long`, `double(min,max)`, `word`, `string`,
  `greedy`, `player`, `players`, `block_pos`, `block_state`, `world`, `item`.

## Invocation (`CommandInvocation<'_>`)

| Method | Result |
|---|---|
| `sender() -> Sender {Player(PlayerId), Console, Other}` | |
| `player() -> ApiResult<PlayerId>` | error for non-players |
| `world() -> World<'_>` | execution world |
| `position() -> Position`, `rotation() -> (f32,f32)` | execution anchor |
| `boolean/integer/long/double/word/string(name)` | typed reads |
| `players(name) -> Vec<PlayerId>`, `player(name)` | selectors resolved by host |
| `block_pos(name) -> BlockPos` | relative/local coords resolved |
| `block_state(name) -> BlockState`, `item(name) -> String` | |
| `has(name) -> bool` | whether an optional argument was supplied |
| `reply(json)`, `reply_plain(str)`, `fail(json)` | feedback |

Callback returns `ApiResult<i32>`; `Err(msg)` becomes red command feedback (no crash).

## Suggestions

`.suggests(fn(&SuggestionCtx) -> Vec<String>)` — the context exposes `sender()`, `remaining()`
and previously parsed arguments where available.
