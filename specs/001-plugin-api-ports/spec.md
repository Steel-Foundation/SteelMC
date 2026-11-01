# Feature Specification: Plugin API Foundations and Reference Plugin Ports

**Feature Branch**: `another-plugin-experiment`

**Created**: 2026-10-31

**Status**: Draft

**Input**: User description: "Extend the SteelMC native plugin API so that real-world server plugins
can be written against it, then validate it by porting the core feature sets of four Bukkit
plugins as native plugins: Vault, EssentialsX, WorldEdit and WorldGuard. The API must cover what
plugins and mods need: cancellable gameplay events, full typed commands, permission queries and
declarations, player/world/entity/inventory/item access, text components, scheduler, per-plugin
data directories and config, and a cross-plugin service registry."

## Actors

- **Server administrator**: installs plugins, edits their configuration, grants permissions.
- **Player**: uses plugin commands and experiences plugin rules in game.
- **Plugin author**: writes a plugin against the public plugin API without modifying Steel.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Plugin authors can react to and veto gameplay (Priority: P1)

A plugin author subscribes to gameplay events (player join, quit, chat, command preprocess,
block break, block place, block/item interaction, entity damage, player death, movement between
blocks, teleport) and, for actions a player initiates, can cancel the action or change its
outcome (for example rewrite a chat message, change a join message, change a teleport target).
When no plugin intervenes, the game behaves exactly as unmodified Steel.

**Why this priority**: Every one of the four target plugins depends on events; without them,
protection, chat formatting, join messages, `/back`, AFK and wand selection are impossible.

**Independent Test**: A test plugin cancels breaking stone and prefixes chat; a player can no
longer break stone, other blocks still break, and chat shows the prefix.

**Acceptance Scenarios**:

1. **Given** a plugin that cancels block break events for one block type, **When** a player
   breaks that block, **Then** the block remains, the client is resynchronized, and no drops or
   experience appear.
2. **Given** two plugins subscribed to the same event at different priorities, **When** the
   event fires, **Then** they run in priority order, later handlers see earlier modifications and
   cancellation, and handlers that asked to ignore cancelled events are skipped.
3. **Given** a plugin that changes the chat message format, **When** a player chats, **Then**
   every recipient sees the formatted message and the console log records it.
4. **Given** a plugin that cancels damage to a player, **When** the player is hit or falls,
   **Then** no health is lost and no hurt effects are applied.
5. **Given** no plugins are installed, **When** players play normally, **Then** all gameplay
   outcomes are identical to Steel without the event system.

---

### User Story 2 - Plugin authors can offer rich commands (Priority: P1)

A plugin author declares commands with typed arguments (players, numbers, words, greedy text,
block states, positions, worlds), tab completion, aliases, and per-node permissions. Players
and the console run these commands; the plugin reads parsed arguments and replies with
formatted text.

**Why this priority**: All four ports are command-driven (`/home`, `//set`, `/rg define`,
`/balance`).

**Independent Test**: A test plugin registers `/greet <player> [message...]`; tab completion
suggests online players; running it delivers a formatted message; players without permission
cannot see or run it.

**Acceptance Scenarios**:

1. **Given** a command requiring a permission, **When** a player without it types the command,
   **Then** it is hidden from completion and rejected with the standard unknown-command response.
2. **Given** a command with an optional trailing text argument, **When** the player omits it,
   **Then** the plugin is invoked with the argument absent rather than receiving an error.
3. **Given** a console sender, **When** it runs a player-only command, **Then** it receives a
   clear error instead of the server failing.

---

### User Story 3 - Plugins can share services (Vault) (Priority: P1)

The Vault port defines named service contracts for economy, permissions and chat metadata. Any
plugin can provide an implementation (for example Essentials provides economy; Steel's built-in
permission groups provide permissions and chat prefixes), and any plugin can look up the
active provider by name and call it, without knowing which plugin supplies it.

**Why this priority**: Economy integration between Essentials and other plugins is the core
purpose of Vault, and the service registry is a foundation for all cross-plugin cooperation.

**Independent Test**: With Vault and Essentials installed, a third plugin queries the economy
service, deposits money to a player, and `/balance` in Essentials shows the new amount.

**Acceptance Scenarios**:

1. **Given** two plugins providing the same service, **When** a consumer looks it up, **Then** it
   receives the provider with the highest declared priority and can list all providers.
2. **Given** no economy provider is installed, **When** a consumer looks it up, **Then** it
   receives a clear "no provider" result rather than a crash.
3. **Given** the Vault port, **When** an administrator runs `/vault-info`, **Then** it lists the
   active economy, permission and chat providers.

---

### User Story 4 - Server essentials (EssentialsX core) (Priority: P2)

Players use homes (`/sethome`, `/home`, `/delhome`, `/homes` with a per-permission limit), warps
(`/setwarp`, `/warp`, `/delwarp`, `/warps`), `/spawn` and `/setspawn`, teleport requests
(`/tpa`, `/tpahere`, `/tpaccept`, `/tpdeny` with expiry), `/back` (last location before
teleport or death), private messages (`/msg`, `/r`), kits with cooldowns (`/kit`), economy
(`/balance`, `/pay`, `/baltop`, `/eco give|take|set`), and staff utilities (`/god`, `/fly`,
`/heal`, `/feed`, `/afk` with automatic idle detection, `/nick`, `/broadcast`, `/tp`, `/tphere`,
`/gamemode` shortcuts `/gmc` `/gms`). Chat uses a configurable format with group prefixes, and
join/quit messages are configurable. All player data persists across restarts.

**Why this priority**: The most widely installed Bukkit plugin; it exercises nearly every API
area (events, commands, persistence, teleport, inventory, services).

**Independent Test**: A player sets a home, restarts the server, rejoins and `/home` teleports
them back; `/pay` moves money between two players and `/baltop` reflects it.

**Acceptance Scenarios**:

1. **Given** a player with a home limit of 3 and 3 homes, **When** they `/sethome fourth`,
   **Then** they are told the limit is reached and no home is created.
2. **Given** a pending `/tpa` request older than the configured expiry, **When** the target runs
   `/tpaccept`, **Then** they are told no request is pending.
3. **Given** a player teleported by any means or who died, **When** they run `/back`, **Then**
   they return to the prior location.
4. **Given** a player in god mode, **When** they take damage of any kind, **Then** health does
   not decrease.
5. **Given** a player idle for longer than the configured AFK timeout, **When** the timeout
   passes, **Then** they are marked AFK and a broadcast announces it; moving or chatting clears it.
6. **Given** insufficient balance, **When** a player runs `/pay`, **Then** no money moves and the
   player is told why.

---

### User Story 5 - In-world editing (WorldEdit core) (Priority: P2)

Builders select a cuboid with a wand item (left click = position 1, right click = position 2)
or `//pos1` / `//pos2`, then run `//set <pattern>`, `//replace <mask> <pattern>`, `//walls`,
`//count <mask>`, `//expand`, `//contract`, `//copy`, `//cut`, `//paste`, `//undo`, `//redo`,
`//size`. Patterns accept block states and weighted mixes (e.g. `50%stone,50%dirt`). Every
edit is recorded so it can be undone and redone per player.

**Why this priority**: WorldEdit is the second canonical plugin and supplies the selection
WorldGuard builds on; it stresses bulk world access.

**Independent Test**: Select a 10×10×10 region, `//set stone`, `//undo` restores the previous
blocks exactly, `//redo` reapplies it.

**Acceptance Scenarios**:

1. **Given** a wand in hand, **When** the player left-clicks a block, **Then** position 1 is set,
   the block is not broken, and the player is told the coordinates and selection size.
2. **Given** a selection containing unloaded chunks, **When** an edit runs, **Then** the edit
   reports the problem and does not leave a partially applied, unrecorded change.
3. **Given** an edit exceeding the configured block limit, **When** it is requested, **Then** it is
   refused with the limit shown.
4. **Given** a copied clipboard, **When** the player moves and runs `//paste`, **Then** the
   structure appears relative to the player exactly as it was relative to the copy origin.

---

### User Story 6 - Region protection (WorldGuard core) (Priority: P2)

Administrators define cuboid regions from the WorldEdit selection (`/rg define <id>`), add
owners and members (`/rg addowner`, `/rg addmember`, `/rg removeowner`, `/rg removemember`),
set priorities and parents (`/rg setpriority`, `/rg setparent`), set flags (`/rg flag <id>
<flag> <value>`), inspect (`/rg info`, `/rg list`) and delete regions (`/rg remove`). Flags
supported: build, block-break, block-place, interact, chest-access, pvp, mob-damage,
invincible, entry, exit, greeting, farewell. A global region per world supplies defaults.
Non-members cannot build in protected regions; higher-priority regions override lower ones;
children inherit unset flags from parents. Bypass is granted by permission.

**Why this priority**: Exercises cancellable events across many action kinds and cross-plugin
dependency on WorldEdit selections.

**Independent Test**: Define a region, deny build; a non-member cannot break or place blocks
there but can elsewhere; a member can build; `greeting` shows on entry.

**Acceptance Scenarios**:

1. **Given** overlapping regions with priorities 0 (build deny) and 10 (build allow), **When** a
   non-member builds in the overlap, **Then** the action is allowed.
2. **Given** `entry deny`, **When** a non-member walks into the region, **Then** they are pushed
   back to their previous position and told they may not enter.
3. **Given** `pvp deny`, **When** a player attacks another inside, **Then** no damage is dealt.
4. **Given** a server restart, **When** it comes back, **Then** all regions, members, flags and
   priorities are unchanged.

---

### User Story 7 - Plugin data and lifecycle hygiene (Priority: P3)

Each plugin receives its own data directory, its configuration from the server config, a
logger tagged with its name, a scheduler for delayed and repeating tasks on the server tick,
and a shutdown notification so it can save data before the server exits. Plugins declare
dependencies (WorldGuard depends on WorldEdit; Essentials soft-depends on Vault) and are
initialized after their dependencies.

**Why this priority**: Needed for correct persistence and ordering, but the ports can start
with simpler stand-ins.

**Independent Test**: A plugin schedules a task 40 ticks out, writes a file to its data
directory on shutdown, and the file exists after a clean stop.

**Acceptance Scenarios**:

1. **Given** a plugin depending on another that is not installed, **When** the server starts,
   **Then** startup fails with a message naming the missing dependency.
2. **Given** a soft dependency that is missing, **When** the server starts, **Then** the plugin
   still loads.
3. **Given** a clean stop, **When** the server shuts down, **Then** each plugin's shutdown runs
   once, in reverse dependency order, before worlds are saved.

### Edge Cases

- A player disconnects while a teleport request, AFK timer, or WorldEdit operation targets them:
  requests are dropped and no stale handle is used.
- A plugin handler returns an error: the error is logged with the plugin name and the event
  continues as if that handler had not run; the server does not crash.
- An event handler tries to keep a borrowed player or world beyond the callback: it is rejected
  at compile time; long-lived references use identifiers that are re-checked on use.
- Concurrent edits on different worlds: each runs on its own world's thread; no cross-world
  borrow is allowed.
- Corrupt or missing plugin data files: the plugin reports the problem and starts with empty
  data instead of crashing (Essentials) or refuses to overwrite and keeps the file (WorldGuard).
- Economy amounts: negative, non-finite or overflowing values are rejected.
- Pasting or setting into locations outside world height limits: those positions are skipped and
  counted, not errors that abort the whole edit.

## Requirements *(mandatory)*

### Functional Requirements

**Events**

- **FR-001**: The API MUST let plugins subscribe to: player pre-login (deny with reason), join,
  quit, chat, command preprocess, block break, block place, interact (block and item, left/right),
  entity damage (with attacker when present), player death, move (block-position change),
  teleport, and server shutdown.
- **FR-002**: Player-initiated actions (pre-login, chat, command preprocess, block break, block
  place, interact, damage, move, teleport) MUST be cancellable; when cancelled, the client view
  MUST be resynchronized.
- **FR-003**: Events MUST expose modifiable outcomes where the action has one: chat message and
  recipients' formatted text, join/quit/death messages, damage amount, teleport destination.
- **FR-004**: Handlers MUST run in a declared priority order (lowest to monitor) with an option to
  skip already-cancelled events; monitor handlers MUST NOT modify outcomes.
- **FR-005**: With no handlers subscribed, event firing MUST NOT change any gameplay outcome.

**Commands**

- **FR-006**: Plugins MUST be able to register commands composed of literal and typed argument
  nodes, with aliases, per-node permission requirements, optional arguments, and custom
  completion suggestions.
- **FR-007**: Typed arguments MUST include at least: boolean, integer/long/double with bounds,
  word, quoted string, greedy text, single/multiple players, block position, block state, world,
  and item.
- **FR-008**: Command callbacks MUST know their sender kind (player, console, other) and be able
  to reply with formatted text.

**Permissions**

- **FR-009**: Plugins MUST be able to declare permission nodes with a default (everyone, operators,
  nobody) and check whether a player holds a node.
- **FR-010**: Plugins MUST be able to read player permission groups and metadata (prefix, suffix,
  numeric options such as home limits) from Steel's permission system.

**Game access**

- **FR-011**: Plugins MUST be able to look up online players by name or identity, read name,
  display name, world, position, rotation, health, food, game mode and flight state, and change
  display name, health, food, flight permission, flying state, game mode, invulnerability.
- **FR-012**: Plugins MUST be able to teleport players across worlds with rotation.
- **FR-013**: Plugins MUST be able to read and modify player inventories (slot contents, give
  items with overflow handling, held item) and create item stacks with counts and custom names.
- **FR-014**: Plugins MUST be able to read and write blocks (with full block state properties),
  query loaded chunks and world height bounds, and parse/format block states as text.
- **FR-015**: Plugins MUST be able to send messages, action bars and titles using styled text
  (colors, bold/italic, legacy `&` color codes, click and hover actions) and broadcast to all.

**Plugin services**

- **FR-016**: Plugins MUST be able to register a named service with a priority and look it up from
  any plugin, receiving the highest-priority provider; providers are reachable only through
  stable, versioned contracts.
- **FR-017**: Plugins MUST receive a private data directory, their configuration, a scheduler for
  delayed and repeating tasks on the server thread, and a shutdown notification.
- **FR-018**: Plugins MUST be able to declare hard and soft dependencies; the server MUST initialize
  plugins in dependency order and refuse to start on missing hard dependencies or cycles.

**Reference ports**

- **FR-019**: A Vault port MUST define economy, permission and chat service contracts, provide a
  permission and chat provider backed by Steel permission groups, and expose `/vault-info`.
- **FR-020**: An Essentials port MUST implement the commands, behaviors, and persistence listed in
  User Story 4, register as the economy provider when Vault is present, and use configurable
  messages.
- **FR-021**: A WorldEdit port MUST implement the commands, wand, patterns, masks, clipboard, block
  limit, and per-player undo/redo listed in User Story 5, and expose player selections as a
  service for other plugins.
- **FR-022**: A WorldGuard port MUST implement the commands, flags, priority and inheritance rules,
  bypass permission, global region, and persistence listed in User Story 6, using the WorldEdit
  selection service.
- **FR-023**: Each port MUST ship a README with installation, configuration and a manual test
  script an administrator can follow in a live server.

### Key Entities

- **Event**: a gameplay occurrence with a kind, mutable outcome fields, and a cancelled flag.
- **Event handler**: a plugin subscription with priority and ignore-cancelled setting.
- **Command tree**: literal/argument nodes with permissions, suggestions and an action.
- **Permission node**: dotted name with default access; player state resolved through groups.
- **Service**: named contract (e.g. `vault:economy`), provider plugin, priority.
- **Plugin**: name, version, dependencies, configuration, data directory, lifecycle state.
- **Player identity**: stable unique id plus session, re-validated on every use.
- **Home / Warp / Kit / Account**: Essentials data owned per player or globally.
- **Selection / Clipboard / Edit history**: WorldEdit per-player data.
- **Region**: id, world, bounds, priority, parent, owners, members, flags.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: All four port plugins load together in one server and every manual test step in
  their READMEs passes in a live client session.
- **SC-002**: The four ports are built using only the public plugin API; zero changes to Steel are
  needed for any port beyond what this feature adds to the API.
- **SC-003**: With no plugins installed, the existing Steel test suite passes unchanged.
- **SC-004**: A WorldEdit `//set` of 100,000 blocks completes and is undoable without the server
  dropping below 20 ticks per second for more than one second.
- **SC-005**: Region protection checks add no perceptible delay; a player breaking blocks in a
  world with 1,000 regions sees results within the same tick.
- **SC-006**: All plugin data (homes, warps, balances, regions) survives 10 consecutive
  restart cycles without loss or corruption.
- **SC-007**: A plugin author can write and run a "hello world" plugin with a command and an event
  handler in under 50 lines, following the API documentation.

## Assumptions

- Ports are native Rust re-implementations of each plugin's core feature set, not loaders for the
  original Java plugins; advanced extras (Essentials signs/jails/mail, WorldEdit brushes,
  schematics and expressions, WorldGuard polygon regions and the full flag list) are out of scope
  and recorded as follow-up work.
- WorldEdit and WorldGuard are two separate plugins; WorldGuard hard-depends on WorldEdit.
- Plugins are trusted, loaded at startup, and never unloaded; hot reload is out of scope.
- Plugin data uses simple human-readable files in each plugin's data directory; databases are out
  of scope.
- Steel's existing permission groups are the permission backend; Vault's permission service wraps
  them rather than introducing another permission system.
- Bulk edits run on the owning world's thread within tick budget limits; asynchronous chunk loading
  for edits is out of scope (unloaded chunks are reported, not loaded).
- Economy amounts use a fixed decimal precision of two digits.
