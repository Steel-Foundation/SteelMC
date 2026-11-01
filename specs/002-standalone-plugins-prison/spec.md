# Feature Specification: Standalone Plugins, Full API Coverage and OP Prison Demo

**Feature Branch**: `another-plugin-experiment`

**Created**: 2026-10-31

**Status**: Draft

**Input**: User description: "Iteration 2 of the SteelMC plugin platform: (1) standalone
distribution - the server auto-loads every native plugin library dropped into its plugins/
folder, plugin projects live outside the server workspace and depend on the API by git, with a
plugin template; (2) maximal API and event coverage of everything Steel's core can support
today; (3) a fast bulk block-fill path like Paper/FAWE prison plugins use; (4) an OP Prison demo
bundle: prison mines with timed and manual resets, a custom pickaxe with custom enchants
(Jackhammer, Explosive, Nuke, Soundwave, Haste/Speed/Fortune) upgraded through a chest menu,
mined blocks auto-sold into the Vault economy."

## Actors

- **Server administrator**: drops plugin files into the server, configures and runs them.
- **Plugin author**: builds a plugin in their own project, without the server's source tree.
- **Player**: plays on the OP Prison demo server.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Drop-in plugins (Priority: P1)

An administrator copies plugin library files into the server's `plugins/` folder and starts the
server. Every plugin found is loaded, ordered by its dependencies, and given
`plugins/<name>/config.toml` (created empty on first start) and `plugins/<name>/` for data.
`/plugins` lists loaded plugins with versions; `/plugins <name>` shows one plugin's details.
A plugin author builds a plugin in an independent project created from a template and gets a
single library file to distribute.

**Why this priority**: The user's first requirement; every other story ships as drop-in files.

**Independent Test**: Build the template plugin outside the server workspace, copy its file into
an empty server's `plugins/` folder, start the server: it loads, `/plugins` lists it, and its
config file appears.

**Acceptance Scenarios**:

1. **Given** five plugin files in `plugins/` in any order, **When** the server starts, **Then** all
   load with dependencies first, and the log names each plugin and version.
2. **Given** a file in `plugins/` that is not a Steel plugin or was built for an incompatible
   API, **When** the server starts, **Then** that file is skipped with a clear message and the
   other plugins still load (unless they depend on it).
3. **Given** a plugin with a missing hard dependency, **When** the server starts, **Then** that
   plugin and plugins depending on it are skipped with a message naming the missing plugin.
4. **Given** two files declaring the same plugin name, **When** the server starts, **Then** both
   are refused with a message naming the files.
5. **Given** the plugin template, **When** an author builds it in a directory outside the server
   repository, **Then** the build succeeds using only a git dependency on the API.

---

### User Story 2 - Complete gameplay events (Priority: P1)

Plugin authors can observe and, where the action is player- or entity-initiated, cancel or modify
every gameplay occurrence Steel implements today, beyond the iteration-1 set: respawn, world
change, game mode change, held-slot change, sneak/sprint/flight toggles, hand swap, item drop,
item pickup, entity interaction, inventory open/click/close, sign edit, experience and food level
change, entity spawn, living-entity death, explosions (blocks destroyed, cancellable or
filterable), piston extend/retract, chunk load/unload, world load/unload, weather change,
console commands, server list ping (MOTD, player counts), and custom plugin-channel messages.

**Why this priority**: The user asked for maximal coverage; protection and prison plugins need
explosions, item pickup/drop and inventory clicks.

**Independent Test**: A test plugin subscribes to every event and logs it; a scripted session
(join, sneak, drop, pick up, open chest, click, explode TNT, die, respawn, change world) produces
the expected log, and cancelling drop/pickup/click/explosion keeps the world unchanged.

**Acceptance Scenarios**:

1. **Given** a plugin cancelling item drops, **When** a player presses Q, **Then** the item stays
   in the inventory and the client is resynchronized.
2. **Given** a plugin removing blocks from an explosion's block list, **When** TNT explodes, **Then**
   only the remaining blocks are destroyed.
3. **Given** a plugin cancelling a click in a custom menu, **When** the player clicks, **Then** no
   item moves and the cursor is restored.
4. **Given** a plugin changing the server list MOTD, **When** a client refreshes, **Then** the new
   text and player count appear.
5. **Given** no plugins, **When** players play, **Then** gameplay is unchanged.

---

### User Story 3 - Complete game API (Priority: P1)

Plugin authors can: build item stacks with name, lore, enchantment glint, unbreakable flag,
vanilla enchantments and private plugin data; read and replace any inventory slot; open custom
chest menus (1–6 rows) whose clicks reach the plugin; spawn particles and play sounds at any
position or for one player; apply potion effects; change experience; spawn and remove basic
entities (items, lightning, fireworks); show a per-player sidebar scoreboard and boss bars; set
tab list header/footer; change world time and weather; send plugin-channel messages; and look up
loaded worlds by key to read and write blocks outside event callbacks.

**Why this priority**: Required by the OP Prison demo (custom pickaxe data, enchant menu,
particles, sidebar, boss bar) and by "everything that could be added now".

**Independent Test**: A test plugin's `/apitest` command gives a named, glowing item with lore and
private data, opens a 3-row menu, shows a sidebar and boss bar, spawns particles, plays a sound,
applies Haste and shows a lightning flash; each effect is visible in a client.

**Acceptance Scenarios**:

1. **Given** an item with private plugin data, **When** the player relogs or the server restarts,
   **Then** the plugin reads the same data back from the item.
2. **Given** a custom menu, **When** the player shift-clicks an item into it, **Then** the plugin
   receives the click and can refuse it.
3. **Given** a sidebar with 10 lines, **When** values change every second, **Then** the client shows
   the updates without flicker or stale lines.

---

### User Story 4 - Fast bulk block replacement (Priority: P1)

Plugins can replace large cuboids (hundreds of thousands of blocks) in one call, the way fast
Paper/FAWE-based prison plugins do: blocks are written without physics or neighbor updates,
nearby players receive the changed sections in batches, and lighting is recalculated afterwards.
The call reports how many blocks changed and how long it took.

**Why this priority**: Mine regeneration speed is the demo's showcase.

**Independent Test**: Fill a 100×100×100 region with a mix of ores twice in a loaded area; both
complete, a nearby client shows the result, and the reported time is below the target.

**Acceptance Scenarios**:

1. **Given** a loaded 1,000,000-block cuboid, **When** a plugin fills it, **Then** every block holds
   a state from the requested mix and the server keeps ticking.
2. **Given** players standing inside the area, **When** it is filled, **Then** they see the new blocks
   without relogging and are not stuck in blocks if the plugin moves them out first.
3. **Given** a cuboid touching unloaded chunks, **When** filled, **Then** the call fails without
   changing anything.

---

### User Story 5 - Prison mines (Priority: P2)

Administrators define mines from a WorldEdit selection (`/mine create <name>`), set a weighted
block mix (`/mine set <name> 60%stone,30%coal_ore,10%diamond_ore`), a reset interval and a reset
threshold (percent mined), and reset them on demand (`/mine reset <name>`). Before a reset,
players inside are lifted to the top of the mine. Every reset announces its duration in
milliseconds and its rate in blocks per second. A boss bar near the mine counts down to the next
reset. Mines persist across restarts.

**Why this priority**: The second core of the demo the user asked for.

**Independent Test**: Create a 50×50×50 mine, mine part of it, run `/mine reset`: it refills in
place, players inside end up on top, and chat shows the time and blocks per second.

**Acceptance Scenarios**:

1. **Given** a mine with a 300-second interval, **When** the interval passes, **Then** it resets
   automatically and announces the reset to nearby players.
2. **Given** a 70% threshold, **When** players mine 70% of its blocks, **Then** it resets early.
3. **Given** a server restart, **When** it starts, **Then** mines, mixes and timers are restored.

---

### User Story 6 - Custom pickaxe and enchants (Priority: P2)

Players get a prison pickaxe (`/pickaxe`) whose enchant levels are stored on the item and shown
in its lore. Right-clicking with it (or `/enchants`) opens a chest menu listing each enchant with
level, effect and upgrade cost; clicking buys a level with Vault money. Enchants:

- **Jackhammer**: chance per block to break the entire mine layer at that height.
- **Explosive**: chance to break a sphere of mine blocks around the block.
- **Nuke**: rare chance to clear the whole mine and reset it, with a server-wide title.
- **Soundwave**: chance to send an expanding ring outward from the block that breaks mine blocks
  as it travels over a few ticks, with note particles on the wavefront.
- **Haste** and **Speed**: potion effects while holding the pickaxe.
- **Fortune**: multiplies the sell value of mined blocks.

Every proc plays sounds and lavish particles. Enchant effects only break blocks inside mines.

**Why this priority**: The custom content of the demo.

**Independent Test**: Give the pickaxe, upgrade Jackhammer to max in the menu, mine in a mine:
whole layers vanish with particles and money increases accordingly.

**Acceptance Scenarios**:

1. **Given** insufficient money, **When** a player clicks an enchant upgrade, **Then** nothing is
   bought and the menu explains why.
2. **Given** an enchant at max level, **When** clicked, **Then** the menu shows it is maxed.
3. **Given** a player mining outside any mine, **When** an enchant procs, **Then** no blocks outside
   mines are broken.

---

### User Story 7 - Auto-sell and prison HUD (Priority: P3)

Blocks broken in mines (by hand or by enchants) are not dropped; their configured value (times
Fortune) is deposited into the player's Vault balance. A sidebar shows balance, blocks mined,
current mine and time to its next reset.

**Why this priority**: Completes the OP-prison loop; depends on the earlier stories.

**Independent Test**: Mine 100 stone worth 1 each with Fortune 2: balance rises by 200 and the
sidebar counters update.

**Acceptance Scenarios**:

1. **Given** Vault and Essentials installed, **When** a Jackhammer proc breaks 2,500 blocks, **Then**
   the balance increases by their total value in one deposit.
2. **Given** no economy provider, **When** the server starts, **Then** the prison plugin reports
   that selling is disabled and mines still work.

### Edge Cases

- A library in `plugins/` that crashes during pre-init: logged with its file name; the server stops
  startup like any plugin init failure (native code cannot be safely isolated).
- Two mines overlapping: refused at creation.
- A reset while a Soundwave is still travelling: the wave stops at the reset.
- A player leaves while their menu is open: the menu is discarded without item loss.
- A pickaxe moved to a chest or dropped: enchants stay on the item; potion effects stop.
- Bulk fill of a cuboid larger than the configured limit: refused.

## Requirements *(mandatory)*

### Functional Requirements

**Distribution**

- **FR-001**: The server MUST load every plugin library file in its `plugins/` folder at startup
  and MUST keep supporting explicitly configured plugin paths.
- **FR-002**: Each plugin MUST receive configuration from `plugins/<name>/config.toml` (created
  empty when missing); explicit server-config values MUST override it.
- **FR-003**: Unloadable, incompatible or duplicate files and plugins with missing dependencies
  MUST be skipped with diagnostics without preventing unrelated plugins from loading.
- **FR-004**: `/plugins` MUST list loaded plugins with versions and `/plugins <name>` MUST show a
  plugin's version, API version, dependencies and file (vanilla `/version` stays unchanged).
- **FR-005**: A plugin template and the plugin projects MUST build outside the server workspace,
  depending on the API through a git source.

**Events** (cancellable where marked ✱)

- **FR-006**: Player: respawn, world change, game mode change✱, held slot change✱, sneak/sprint
  toggle✱, flight toggle✱, hand swap✱, item drop✱, item pickup✱, entity interact✱, inventory
  open✱, inventory click✱, inventory close, sign change✱ (lines editable), experience change
  (amount editable), food level change✱.
- **FR-007**: World and entity: entity spawn✱, living entity death (drops and experience editable),
  explosion✱ (affected block list editable), piston extend/retract✱, chunk load/unload, world
  load/unload, weather change✱.
- **FR-008**: Server: console/remote command✱, server list ping (MOTD, max players, visible count
  editable), plugin-channel message received.

**Game API**

- **FR-009**: Item stacks with item key, count, custom name, lore, enchantment glint, unbreakable,
  vanilla enchantments and per-plugin private data that persists with the item.
- **FR-010**: Read/write player inventory slots, held slot and cursor; give and remove items.
- **FR-011**: Custom chest menus of 1–6 rows with plugin-supplied items, per-click callbacks
  (slot, click type), refresh and close.
- **FR-012**: Particles (any vanilla particle, count, spread, speed, optional color/item data),
  sounds (any vanilla sound, volume, pitch, source) at a position or for one player.
- **FR-013**: Potion effects (add, remove, query), experience points and levels, walk/fly speed.
- **FR-014**: Spawn dropped items and visual lightning (Steel has no lightning entity yet); remove spawned entities by id.
- **FR-015**: Per-player sidebar (title and up to 15 lines), boss bars (title, progress, color,
  style, viewers), tab list header/footer.
- **FR-016**: World time and weather; world lookup by key from server callbacks for block reads,
  writes and bulk fills.
- **FR-017**: Plugin-channel messages to a player and registration of channels.

**Bulk fill**

- **FR-018**: Fill a cuboid with a weighted state mix or an explicit per-block function result,
  without physics or neighbor updates, batching client updates per section and recalculating light,
  returning changed count and elapsed time; refuses unloaded chunks and oversize regions.

**OP Prison demo**

- **FR-019**: A mines plugin implementing User Story 5 and exposing mines as a service.
- **FR-020**: An enchants plugin implementing User Story 6, with the chest-menu upgrade flow.
- **FR-021**: Auto-sell and HUD implementing User Story 7, using the Vault economy service.
- **FR-022**: A README with a scripted demo: build, drop files into `plugins/`, configure, play.

### Key Entities

- **Plugin file**: library in `plugins/`, with descriptor (name, version, dependencies, API version).
- **Item data**: name, lore, glint, enchantments, plugin data namespace → values.
- **Menu**: title, rows, slots, owner plugin, click handler.
- **Sidebar / Boss bar**: per-player displays owned by a plugin.
- **Mine**: name, world, cuboid, block mix, reset interval, threshold, statistics, last reset.
- **Prison enchant**: id, max level, cost curve, proc chance curve, effect.
- **Pickaxe**: item with enchant levels in plugin data.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: An administrator installs the whole demo by copying files into `plugins/` and
  starting the server, with zero server configuration edits.
- **SC-002**: A plugin built from the template outside the repository loads unchanged.
- **SC-003**: Resetting a 50×50×50 mine (125,000 blocks) completes in under 100 ms of server time
  and a 100×100×100 region (1,000,000 blocks) in under 1 second, measured on the demo machine.
- **SC-004**: The server holds 20 ticks per second while one player procs Jackhammer every second
  in a 50×50 mine.
- **SC-005**: Every event in FR-006–FR-008 fires in an automated test; with no plugins installed,
  the existing core test suite passes unchanged.
- **SC-006**: A player can go from joining to a max-level Jackhammer pickaxe purely through the
  menu flow in under 5 minutes of play with the demo's default prices.

## Assumptions

- Plugins remain trusted native libraries loaded once at startup; no hot reload or sandboxing.
- "Outside the server workspace" means separate Cargo workspaces (here under `plugin-projects/`)
  that use the git URL https://github.com/coco875/SteelMC (branch `another-plugin-experiment`);
  local builds redirect that source to this checkout so nothing must be pushed.
- Prison mines are cuboids; block values are configured per block key.
- Events only cover behavior Steel implements; vanilla features missing from Steel (e.g. item
  consumption, fishing loot, crop growth hooks) are out of scope and recorded as TODOs.
- The demo targets one world and a handful of players; mine sizes up to 100×100×100.
