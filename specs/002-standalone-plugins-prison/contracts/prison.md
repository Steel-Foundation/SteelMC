# Contract: OP Prison bundle

## prison-mines (`prison-mines`, depends worldedit)
| Command | Permission |
|---|---|
| `/mine create <name>` (from WorldEdit selection) | `prison.mines.admin` |
| `/mine set <name> <mix>` e.g. `60%stone,30%coal_ore,10%diamond_ore` | admin |
| `/mine interval <name> <seconds>`, `/mine threshold <name> <percent>` | admin |
| `/mine reset <name>` | admin |
| `/mine delete <name>`, `/mine list`, `/mine info [name]`, `/mine tp <name>` | admin / everyone / everyone / everyone |
Reset message: `Mine <name> reset: <n> blocks in <ms> ms (<blocks/s> blocks/s)`.
Service `prison:mines/v1`: `mine_at(world, pos) -> Option<MineBounds>`, `reset(server, name) ->
ResetReport`, `add_mined(name, count)`, `seconds_until_reset(name)`.

## prison-enchants (`prison-enchants`, depends prison-mines, soft-depends vault)
| Command | Permission |
|---|---|
| `/pickaxe` | everyone |
| `/enchants` (also right-click with the pickaxe) | everyone |
| `/penchant <enchant> <level> [player]` | `prison.enchants.admin` |
Enchants: jackhammer (max 100), explosive (max 50), nuke (max 5), soundwave (max 50), haste
(max 3), speed (max 3), fortune (max 10). Config: `[enchants.<id>]` costs/chances,
`[values]` block values, `default_value`.
