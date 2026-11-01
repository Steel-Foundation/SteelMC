# Contract: Reference Ports (commands, permissions, config)

Command ids give permissions `<ns>.command.<cmd>`. "default" = everyone unless denied; others
require an explicit grant (operators have `*`).

## Vault (`vault`)
| Command | Access | Purpose |
|---|---|---|
| `/vault-info` | op | list providers for each Vault service |
Providers: `vault:permission/v1` and `vault:chat/v1` backed by Steel permission groups
(metadata keys `vault:prefix`, `vault:suffix`), priority 0.

## Essentials (`essentials`, soft-depends `vault`)
| Command | Access | Notes |
|---|---|---|
| `/home [name]`, `/sethome [name]`, `/delhome <name>`, `/homes` | default | limit: metadata `essentials:homes` or config `default_home_limit` |
| `/warp <name>`, `/warps` | default | |
| `/setwarp <name>`, `/delwarp <name>` | op | |
| `/spawn` | default; `/setspawn` op | |
| `/tpa <player>`, `/tpahere <player>`, `/tpaccept`, `/tpdeny` | default | expiry `tpa_timeout_seconds` |
| `/back` | default | last teleport origin or death location |
| `/msg <player> <message>` (`/tell`,`/w` aliases `/m`), `/r <message>` | default | |
| `/kit [name]` | default; per-kit sub-permission | cooldown per kit |
| `/balance [player]` (`/bal`), `/pay <player> <amount>`, `/baltop` | default | |
| `/eco <give\|take\|set> <player> <amount>` | op | |
| `/god [player]`, `/fly [player]`, `/heal [player]`, `/feed [player]` | op | |
| `/afk` | default | auto after `afk_timeout_seconds` |
| `/nick <name\|off>` | op | supports `&` colors |
| `/broadcast <message>` (`/bc`) | op | |
| `/etp <player> [target]` (alias `/tpto`), `/tphere <player>` | op | vanilla `/tp` stays unchanged |
| `/gmc`, `/gms`, `/gma`, `/gmsp` | op | |
Config keys: `chat_format` (`{prefix}{displayname}{suffix}&7: &f{message}`), `join_message`,
`quit_message`, `starting_balance`, `max_money`, `currency_symbol`, `default_home_limit`,
`tpa_timeout_seconds`, `afk_timeout_seconds`, `kits.<name>.items = ["minecraft:bread 16"]`,
`kits.<name>.cooldown_seconds`.

## WorldEdit (`worldedit`)
Commands are registered as `worldedit:/<name>` (typed `//<name>`) with an explicit root
permission `worldedit.<name>`, because `/name` is not a valid permission segment. Access: op.
`//wand`, `//pos1`, `//pos2`, `//set <pattern>`, `//replace [mask] <pattern>`,
`//walls <pattern>`, `//count <mask>`, `//size`, `//expand <n> [dir]`, `//expand vert`,
`//contract <n> [dir]`, `//copy`, `//cut`, `//paste`, `//undo [n]`, `//redo [n]`, `//sel`
(clear). `//hpos1`/`//hpos2` are deferred until a raycast API exists. Config: `wand_item = "minecraft:wooden_axe"`, `max_blocks = 1000000`,
`history_size = 15`.

## WorldGuard (`worldguard`, depends `worldedit`)
`/rg define <id>`, `/rg redefine <id>`, `/rg remove <id>`, `/rg list`, `/rg info [id]`,
`/rg addowner|removeowner|addmember|removemember <id> <player>`, `/rg flag <id> <flag>
[value]`, `/rg setpriority <id> <n>`, `/rg setparent <id> [parent]`. Access: op; `/rg info`
and `/rg list` default. Bypass permission: `worldguard.region.bypass`.
Flags: `build`, `block-break`, `block-place`, `interact`, `chest-access`, `pvp`, `mob-damage`,
`invincible`, `entry`, `exit` (state) and `greeting`, `farewell` (string).
